//! Lua host for MRE.
//!
//! This crate owns exactly one thing: the **reactive primitive graph**. Signals,
//! computations, dependency tracking and flushing live here, in Rust, because
//! this is the code that must be correct and allocation-free on the hot path.
//!
//! Everything compositional — node constructors, `mount`, sugar — lives in
//! `prelude.lua`, because it is pure Lua and cheaper to iterate on there.
//!
//! # Model
//!
//! * `signal(v)` returns a callable. Call it with no arguments to read, with one
//!   argument to write.
//! * `effect(fn)` runs immediately and subscribes to every signal read during
//!   that run.
//! * Writing a signal marks subscribers dirty. Nothing runs until `flush`.
//! * `flush` runs every dirty computation **once**, in a loop until quiescent,
//!   with a hard tick limit so a dependency cycle is an error instead of a hang.
//!
//! That last property is what makes the engine cheap: a frame in which nothing
//! changed runs zero Lua code.

use mlua::{Function, Lua, MultiValue, Table, Value as LuaValue};
use std::cell::RefCell;
use std::rc::Rc;

/// A signal value.
///
/// Scalars only. Signals are for state, not for blobs: if you need to store an
/// object, store a table in Lua and keep a scalar revision counter as the signal.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Nil,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
}

impl Val {
    fn from_lua(value: &LuaValue) -> Option<Self> {
        match value {
            LuaValue::Nil => Some(Self::Nil),
            LuaValue::Boolean(b) => Some(Self::Bool(*b)),
            LuaValue::Integer(i) => Some(Self::Int(*i)),
            LuaValue::Number(n) => Some(Self::Num(*n)),
            LuaValue::String(s) => Some(Self::Str(s.to_string_lossy().to_string())),
            _ => None,
        }
    }

    fn to_lua(&self, lua: &Lua) -> mlua::Result<LuaValue> {
        Ok(match self {
            Self::Nil => LuaValue::Nil,
            Self::Bool(b) => LuaValue::Boolean(*b),
            Self::Int(i) => LuaValue::Integer(*i),
            Self::Num(n) => LuaValue::Number(*n),
            Self::Str(s) => LuaValue::String(lua.create_string(s)?),
        })
    }

    /// Numeric view, for Lua code that does arithmetic on signal values.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(i) => Some(*i as f64),
            Self::Num(n) => Some(*n),
            Self::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            Self::Nil => Some(false),
            _ => None,
        }
    }
}

struct SignalSlot {
    value: Val,
    subscribers: Vec<usize>,
}

struct Computation {
    func: Function,
    deps: Vec<usize>,
    runs: u32,
}

#[derive(Default)]
struct State {
    signals: Vec<SignalSlot>,
    computations: Vec<Computation>,
    /// The computation currently executing, if any. Reads subscribe to it.
    current: Option<usize>,
    dirty: Vec<usize>,
    last_flush_runs: u32,
    flush_count: u64,
    total_runs: u64,
}

/// A snapshot of engine counters, exposed to Lua as `mre.stats()`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Number of signals ever created.
    pub signals: usize,
    /// Number of computations (effects/mounts) ever created.
    pub computations: usize,
    /// Computations currently queued for the next flush.
    pub dirty: usize,
    /// Computations that ran during the last flush. Zero means the frame was free.
    pub last_flush_runs: u32,
    /// Number of flushes performed.
    pub flush_count: u64,
    /// Total computations run since start.
    pub total_runs: u64,
}

/// The Lua engine instance.
///
/// One `Runtime` is one app on one thread. It is deliberately not `Send`: the
/// scene it drives is single-threaded by design (see `docs/ARCHITECTURE.md`).
pub struct Runtime {
    lua: Lua,
    state: Rc<RefCell<State>>,
}

impl Runtime {
    /// Creates a runtime with the MRE prelude loaded.
    ///
    /// # Errors
    ///
    /// Fails if the vendored Lua VM cannot start or the prelude does not load,
    /// which indicates a build problem rather than a user error.
    pub fn new() -> mlua::Result<Self> {
        let lua = Lua::new();
        let state = Rc::new(RefCell::new(State::default()));
        let mre = lua.create_table()?;

        install_signal(&lua, &mre, &state)?;
        install_effect(&lua, &mre, &state)?;
        install_flush(&lua, &mre, &state)?;
        install_untracked(&lua, &mre, &state)?;
        install_stats(&lua, &mre, &state)?;

        lua.globals().set("mre", &mre)?;
        lua.load(include_str!("prelude.lua"))
            .set_name("mre/prelude")
            .call::<()>(&mre)?;

        Ok(Self { lua, state })
    }

    /// Runs a Lua chunk in the app's global environment.
    ///
    /// # Errors
    ///
    /// Returns any Lua error raised by the chunk.
    pub fn exec(&self, source: &str) -> mlua::Result<()> {
        self.lua.load(source).set_name("app").exec()
    }

    /// Runs a Lua chunk and returns its single return value.
    ///
    /// # Errors
    ///
    /// Returns any Lua error raised by the chunk.
    pub fn eval<R: mlua::FromLuaMulti>(&self, source: &str) -> mlua::Result<R> {
        self.lua.load(source).set_name("app").eval()
    }

    /// Processes every dirty computation.
    ///
    /// # Errors
    ///
    /// Returns any Lua error raised by a computation, or a cycle error if the
    /// reactive graph does not settle.
    pub fn flush(&self) -> mlua::Result<()> {
        flush_state(&self.state)
    }

    /// Engine counters.
    #[must_use]
    pub fn stats(&self) -> Stats {
        let st = self.state.borrow();
        Stats {
            signals: st.signals.len(),
            computations: st.computations.len(),
            dirty: st.dirty.len(),
            last_flush_runs: st.last_flush_runs,
            flush_count: st.flush_count,
            total_runs: st.total_runs,
        }
    }

    /// Renders the current vnode tree as an S-expression, for tests and tooling.
    ///
    /// Returns an empty string when nothing is mounted.
    #[must_use]
    pub fn tree(&self) -> String {
        let Ok(mre) = self.lua.globals().get::<Table>("mre") else {
            return String::new();
        };
        let Ok(tree) = mre.get::<LuaValue>("__tree") else {
            return String::new();
        };
        let mut out = String::new();
        dump(&tree, &mut out, 0);
        out
    }

    /// Access to the underlying Lua state, for host integrations.
    #[must_use]
    pub fn lua(&self) -> &Lua {
        &self.lua
    }
}

fn install_signal(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let create = lua.create_function(move |lua, initial: LuaValue| {
        let value = Val::from_lua(&initial).ok_or_else(|| {
            mlua::Error::runtime("signal value must be nil, boolean, number or string")
        })?;

        let id = {
            let mut st = state.borrow_mut();
            st.signals.push(SignalSlot {
                value,
                subscribers: Vec::new(),
            });
            st.signals.len() - 1
        };

        let state = Rc::clone(&state);
        lua.create_function(move |lua, args: MultiValue| {
            if args.is_empty() {
                read_signal(lua, &state, id)
            } else {
                let value = Val::from_lua(&args[0]).ok_or_else(|| {
                    mlua::Error::runtime("signal value must be nil, boolean, number or string")
                })?;
                write_signal(&state, id, value);
                Ok(LuaValue::Nil)
            }
        })
    })?;

    mre.set("signal", create)
}

fn read_signal(lua: &Lua, state: &Rc<RefCell<State>>, id: usize) -> mlua::Result<LuaValue> {
    let value = {
        let mut st = state.borrow_mut();
        let value = st.signals[id].value.clone();

        if let Some(current) = st.current {
            let already = st.computations[current].deps.contains(&id);
            if !already {
                let State {
                    signals,
                    computations,
                    ..
                } = &mut *st;
                computations[current].deps.push(id);
                signals[id].subscribers.push(current);
            }
        }
        value
    };
    value.to_lua(lua)
}

fn write_signal(state: &Rc<RefCell<State>>, id: usize, value: Val) {
    let mut st = state.borrow_mut();
    if st.signals[id].value == value {
        return;
    }
    st.signals[id].value = value;

    let subscribers = st.signals[id].subscribers.clone();
    let State { dirty, .. } = &mut *st;
    for computation in subscribers {
        if !dirty.contains(&computation) {
            dirty.push(computation);
        }
    }
}

fn install_effect(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let effect = lua.create_function(move |_lua, func: Function| {
        let id = {
            let mut st = state.borrow_mut();
            st.computations.push(Computation {
                func,
                deps: Vec::new(),
                runs: 0,
            });
            st.computations.len() - 1
        };
        {
            let mut st = state.borrow_mut();
            st.dirty.push(id);
        }
        flush_state(&state)
    })?;
    mre.set("effect", effect)
}

fn install_flush(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let flush = lua.create_function(move |_lua, ()| flush_state(&state))?;
    mre.set("flush", flush)
}

fn install_untracked(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let untracked = lua.create_function(move |_lua, func: Function| {
        let saved = {
            let mut st = state.borrow_mut();
            st.current.take()
        };
        let result: mlua::Result<LuaValue> = func.call(());
        {
            let mut st = state.borrow_mut();
            st.current = saved;
        }
        result
    })?;
    mre.set("untracked", untracked)
}

fn install_stats(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let stats = lua.create_function(move |lua, ()| {
        let st = state.borrow();
        let table = lua.create_table()?;
        table.set("signals", st.signals.len())?;
        table.set("computations", st.computations.len())?;
        table.set("dirty", st.dirty.len())?;
        table.set("last_flush_runs", st.last_flush_runs)?;
        table.set("flush_count", st.flush_count)?;
        table.set("total_runs", st.total_runs)?;
        Ok(table)
    })?;
    mre.set("stats", stats)
}

/// Runs every dirty computation until the graph is quiescent.
///
/// Each computation runs at most once per pass; writes made *during* a pass are
/// picked up by the next pass. The tick limit turns a dependency cycle into a
/// clear error instead of a frozen app.
fn flush_state(state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    {
        let mut st = state.borrow_mut();
        st.last_flush_runs = 0;
    }

    let mut ticks = 0u32;
    loop {
        let batch = {
            let mut st = state.borrow_mut();
            if st.dirty.is_empty() {
                break;
            }
            std::mem::take(&mut st.dirty)
        };

        for id in batch {
            let func = {
                let st = state.borrow();
                st.computations[id].func.clone()
            };

            // Unsubscribe from the previous run's dependencies before re-running,
            // otherwise subscribers accumulate forever and every flush gets slower.
            {
                let mut st = state.borrow_mut();
                let old = std::mem::take(&mut st.computations[id].deps);
                let State {
                    signals,
                    computations,
                    ..
                } = &mut *st;
                for dep in old {
                    signals[dep].subscribers.retain(|c| *c != id);
                }
                computations[id].runs += 1;
                st.current = Some(id);
                st.last_flush_runs += 1;
                st.total_runs += 1;
            }

            let result: mlua::Result<()> = func.call(());

            {
                let mut st = state.borrow_mut();
                st.current = None;
            }

            result?;

            ticks += 1;
            if ticks > 10_000 {
                return Err(mlua::Error::runtime(
                    "reactive flush did not settle after 10000 computations: dependency cycle",
                ));
            }
        }
    }

    state.borrow_mut().flush_count += 1;
    Ok(())
}

fn dump(value: &LuaValue, out: &mut String, depth: usize) {
    if depth > 64 {
        out.push_str("...");
        return;
    }

    match value {
        LuaValue::Table(table) => {
            let kind: Option<String> = table.get("__mre").ok();
            match kind {
                Some(kind) => {
                    out.push('(');
                    out.push_str(&kind);

                    if kind == "text" {
                        let text: Option<String> = table.get("text").unwrap_or(None);
                        if let Some(text) = text {
                            out.push(' ');
                            out.push_str(&format!("{text:?}"));
                        }
                    }

                    if let Ok(children) = table.get::<Table>("children") {
                        for child in children.sequence_values::<LuaValue>().flatten() {
                            out.push(' ');
                            dump(&child, out, depth + 1);
                        }
                    }
                    out.push(')');
                }
                None => {
                    for child in table.sequence_values::<LuaValue>().flatten() {
                        dump(&child, out, depth + 1);
                    }
                }
            }
        }
        LuaValue::String(s) => {
            out.push_str(&format!("{:?}", s.to_string_lossy()));
        }
        LuaValue::Integer(i) => out.push_str(&i.to_string()),
        LuaValue::Number(n) => out.push_str(&format!("{n}")),
        LuaValue::Boolean(b) => out.push_str(if *b { "true" } else { "false" }),
        _ => {}
    }
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("stats", &self.stats())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_roundtrips_scalar_types() {
        let rt = Runtime::new().unwrap();
        rt.exec(
            r#"
            mre.__s = mre.signal(41)
            mre.__t = mre.signal("hi")
            mre.__b = mre.signal(true)
            "#,
        )
        .unwrap();

        assert_eq!(rt.eval::<i64>("mre.__s()").unwrap(), 41);
        rt.exec("mre.__s(42)").unwrap();
        assert_eq!(rt.eval::<f64>("mre.__s()").unwrap(), 42.0);
        assert_eq!(rt.eval::<String>("mre.__t()").unwrap(), "hi");
        assert!(rt.eval::<bool>("mre.__b()").unwrap());
    }

    #[test]
    fn rejects_unsupported_signal_values() {
        let rt = Runtime::new().unwrap();
        assert!(rt.exec("mre.signal({})").is_err());
        assert!(rt.exec("mre.signal(function() end)").is_err());
    }
}
