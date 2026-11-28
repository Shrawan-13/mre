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
use mre_anim::{Easing, Spring, SpringConfig, Tween};
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

/// One animation driving one signal or paint property. Springs carry velocity
/// and can be retargeted; tweens are time-based with an easing curve.
enum Animation {
    Spring(Spring),
    Tween {
        tween: Tween,
        duration: f32,
        easing: Easing,
    },
}

impl Animation {
    fn settled(&self) -> bool {
        match self {
            Self::Spring(spring) => spring.settled(),
            Self::Tween { tween, .. } => tween.finished(),
        }
    }

    fn step(&mut self, dt: f32) -> f32 {
        match self {
            Self::Spring(spring) => spring.step(dt),
            Self::Tween { tween, .. } => tween.step(dt),
        }
    }

    fn value(&self) -> f32 {
        match self {
            Self::Spring(spring) => spring.value(),
            Self::Tween { tween, .. } => tween.value(),
        }
    }

    /// Retargets smoothly: a spring keeps velocity, a tween restarts from its
    /// current value with the same duration and easing.
    fn retarget(&mut self, target: f32) {
        match self {
            Self::Spring(spring) => spring.set_target(target),
            Self::Tween {
                tween,
                duration,
                easing,
            } => {
                let from = tween.value();
                *tween = Tween::new(from, target, *duration, *easing);
            }
        }
    }
}

/// A one-shot callback fired by the engine clock, in seconds.
struct Timer {
    remaining: f32,
    callback: Function,
}

struct AnimationSlot {
    animation: Animation,
    /// Optional signal the animation writes into. Bound properties need no Lua
    /// call per frame: the runtime reads `value()` and writes them directly.
    signal: Option<Function>,
    id: u64,
}

#[derive(Default)]
struct State {
    signals: Vec<SignalSlot>,
    /// Hover signals, by node key.
    hover: std::collections::HashMap<String, usize>,
    animations: Vec<Rc<RefCell<AnimationSlot>>>,
    /// Animations bound to a signal by `mre.animate(signal)`. A Lua write to
    /// that signal retargets them instead of being overwritten on the next tick.
    signal_animations: std::collections::HashMap<usize, Vec<Rc<RefCell<AnimationSlot>>>>,
    /// Lua closure pointer → signal id. C functions have no environment, so
    /// this is how a bound spring recognises the signal it was given.
    signal_ptrs: std::collections::HashMap<usize, usize>,
    /// True while the runtime writes a bound signal from an animation step;
    /// those writes must not retarget the animation that produced them.
    animation_write: bool,
    timers: Vec<Timer>,
    next_animation_id: u64,
    computations: Vec<Computation>,
    /// The computation currently executing, if any. Reads subscribe to it.
    current: Option<usize>,
    dirty: Vec<usize>,
    last_flush_runs: u32,
    flush_count: u64,
    total_runs: u64,
    /// The signal that carries the viewport size, created on first read.
    viewport: Option<usize>,
    /// The signal that carries safe-area insets, created on first read.
    ///
    /// Insets are reactive: a view that reads `mre.safe_area()` rebuilds when
    /// the keyboard or the system bars change them.
    safe_area: Option<usize>,
    /// Last insets published by the host, used to initialise the signal.
    insets: [f32; 4],
    /// Accumulated engine time in seconds, advanced only by `Runtime::tick`.
    /// This is the deterministic clock Lua apps should use instead of `os.time`.
    clock: f64,
    /// Non-fatal problems found in the last view conversion.
    diagnostics: Vec<String>,
    /// The palette name last requested from Lua.
    theme: String,
    /// Shared with the host: a theme change is applied at the start of a frame,
    /// never while a view is being built.
    theme_request: Rc<RefCell<Option<String>>>,
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
        Self::with_loader(|_| None)
    }

    /// Creates a runtime whose Lua framework modules can be loaded at runtime.
    ///
    /// The embedded prelude provides the stable base API; the loader is asked
    /// for `framework/init` and for any `mre.load(name)` call. Returning `None`
    /// simply means the module is absent, which is not an error: the app still
    /// has the built-in API.
    pub fn with_loader<F>(loader: F) -> mlua::Result<Self>
    where
        F: Fn(&str) -> Option<String> + 'static,
    {
        let lua = Lua::new();
        let state = Rc::new(RefCell::new(State {
            theme: "light".to_string(),
            ..State::default()
        }));
        let mre = lua.create_table()?;

        install_signal(&lua, &mre, &state)?;
        install_effect(&lua, &mre, &state)?;
        install_flush(&lua, &mre, &state)?;
        install_untracked(&lua, &mre, &state)?;
        install_stats(&lua, &mre, &state)?;
        install_clock(&lua, &mre, &state)?;
        install_diagnostics(&lua, &mre, &state)?;
        install_log(&lua, &mre)?;
        install_theme(&lua, &mre, &state)?;
        install_animations(&lua, &mre, &state)?;
        install_tween(&lua, &mre, &state)?;
        install_after(&lua, &mre, &state)?;
        install_safe_area(&lua, &mre, &state)?;
        install_hover(&lua, &mre, &state)?;
        install_viewport(&lua, &mre, &state)?;

        lua.globals().set("mre", &mre)?;

        let loader = Rc::new(loader);
        let load = {
            let loader = Rc::clone(&loader);
            let mre = mre.clone();
            lua.create_function(move |lua, name: String| {
                let Some(source) = loader(&name) else {
                    return Ok(false);
                };
                lua.load(source).set_name(name.clone()).call::<()>(&mre)?;
                Ok(true)
            })?
        };
        mre.set("load", load)?;

        lua.load(include_str!("prelude.lua"))
            .set_name("mre/prelude")
            .call::<()>(&mre)?;

        // The framework overlay is optional. When present it can redefine
        // themes, styles and animation helpers without rebuilding the host.
        let load: Function = mre.get("load")?;
        let _: bool = load.call("framework/init")?;

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

    /// Publishes the viewport size to Lua, in logical pixels.
    ///
    /// `mre.viewport()` reads this signal, so a view that reads it is
    /// rebuilt when the window changes size — a rotation or a DPI change
    /// alters the layout, not just the pixels.
    pub fn set_viewport(&mut self, width: f32, height: f32, scale: f32) {
        let id = {
            let mut st = self.state.borrow_mut();
            if st.viewport.is_none() {
                st.signals.push(SignalSlot {
                    value: Val::Str("0|0|1".to_string()),
                    subscribers: Vec::new(),
                });
                st.viewport = Some(st.signals.len() - 1);
            }
            st.viewport.expect("just created")
        };
        let value = Val::Str(format!("{width}|{height}|{scale}"));
        write_signal(&self.state, id, value);
    }

    /// Publishes the safe-area insets to Lua, in logical pixels.
    ///
    /// `mre.safe_area()` reads a signal, so a view that uses it rebuilds when
    /// the keyboard or the system bars change. Layout itself applies insets at
    /// the root; Lua only needs them for subtree-level adjustments.
    pub fn set_safe_area(&mut self, top: f32, right: f32, bottom: f32, left: f32) {
        let id = {
            let mut st = self.state.borrow_mut();
            st.insets = [top, right, bottom, left];
            if st.safe_area.is_none() {
                st.signals.push(SignalSlot {
                    value: Val::Str("0|0|0|0".to_string()),
                    subscribers: Vec::new(),
                });
                st.safe_area = Some(st.signals.len() - 1);
            }
            st.safe_area.expect("just created")
        };
        write_signal(
            &self.state,
            id,
            Val::Str(format!("{top}|{right}|{bottom}|{left}")),
        );
    }

    /// The accumulated engine clock in seconds.
    ///
    /// Unlike `os.time()`, this only advances when the host calls `tick`, so
    /// animations and app logic are reproducible from an event log.
    #[must_use]
    pub fn now(&self) -> f64 {
        self.state.borrow().clock
    }

    /// The pending theme request, shared with the host.
    #[must_use]
    pub fn theme_request(&self) -> Rc<RefCell<Option<String>>> {
        Rc::clone(&self.state.borrow().theme_request)
    }

    /// Publishes non-fatal conversion problems.
    ///
    /// New problems are logged once and can also be read from Lua with
    /// `mre.diagnostics()`, so a typo is visible instead of silently changing
    /// a pixel.
    pub fn set_diagnostics(&self, items: Vec<String>) {
        let mut st = self.state.borrow_mut();
        if st.diagnostics == items {
            return;
        }
        for item in &items {
            eprintln!("mre: diagnostic: {item}");
        }
        st.diagnostics = items;
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

    /// Advances every animation and timer by `dt` seconds, writing driven
    /// signals and firing due callbacks.
    ///
    /// Returns `true` while any animation is still moving. The platform loop
    /// uses that to decide whether the next frame is due; an idle app with no
    /// running animation is never woken.
    ///
    /// # Errors
    ///
    /// Returns an error raised by a driven signal, for example a signal that was
    /// replaced by a non-numeric value.
    pub fn tick(&self, dt: f32) -> mlua::Result<bool> {
        {
            let mut st = self.state.borrow_mut();
            if dt.is_finite() && dt > 0.0 {
                st.clock += f64::from(dt);
            }
        }
        let slots: Vec<Rc<RefCell<AnimationSlot>>> = self.state.borrow().animations.clone();
        let mut active = false;

        for slot in slots {
            let (value, signal) = {
                let mut slot = slot.borrow_mut();
                if slot.animation.settled() {
                    continue;
                }
                let value = slot.animation.step(dt);
                (value, slot.signal.clone())
            };
            if let Some(signal) = signal {
                self.state.borrow_mut().animation_write = true;
                let written = signal.call::<()>(f64::from(value));
                self.state.borrow_mut().animation_write = false;
                written?;
            }
            active = true;
        }

        let due = {
            let mut st = self.state.borrow_mut();
            let mut due = Vec::new();
            for timer in &mut st.timers {
                timer.remaining -= dt;
                if timer.remaining <= 0.0 {
                    due.push(timer.callback.clone());
                }
            }
            st.timers.retain(|timer| timer.remaining > 0.0);
            due
        };
        let timer_ran = !due.is_empty();
        for callback in due {
            callback.call::<()>(())?;
        }
        let pending = !self.state.borrow().timers.is_empty();
        if active || timer_ran || pending {
            self.flush()?;
        }
        Ok(active || pending)
    }

    /// True while any animation or timer is still pending.
    #[must_use]
    pub fn animating(&self) -> bool {
        let state = self.state.borrow();
        !state.timers.is_empty()
            || state
                .animations
                .iter()
                .any(|slot| !slot.borrow().animation.settled())
    }

    /// Number of registered animations.
    #[must_use]
    pub fn animation_count(&self) -> usize {
        self.state.borrow().animations.len()
    }

    /// Every animation's current value, by id.
    ///
    /// This is how a bound property is updated without calling into Lua: the
    /// engine asks for the numbers it needs and writes them straight into the
    /// paint override table.
    #[must_use]
    pub fn springs(&self) -> Vec<(u64, f32)> {
        self.state
            .borrow()
            .animations
            .iter()
            .map(|slot| {
                let slot = slot.borrow();
                (slot.id, slot.animation.value())
            })
            .collect()
    }

    /// One animation's current value.
    #[must_use]
    pub fn spring(&self, id: u64) -> Option<f32> {
        self.state
            .borrow()
            .animations
            .iter()
            .find(|slot| slot.borrow().id == id)
            .map(|slot| slot.borrow().animation.value())
    }

    /// Writes the hover state of a node key into its hover signal.
    ///
    /// Returns `true` when the value changed. Keys that never asked for a
    /// hover signal are ignored, so this costs nothing for the common case.
    pub fn set_hover(&self, key: &str, hovered: bool) -> bool {
        let id = {
            let st = self.state.borrow();
            match st.hover.get(key) {
                Some(id) => *id,
                None => return false,
            }
        };
        let current = {
            let st = self.state.borrow();
            matches!(st.signals[id].value, Val::Bool(true))
        };
        if current == hovered {
            return false;
        }
        write_signal(&self.state, id, Val::Bool(hovered));
        true
    }

    /// The current view tree, as set by `mre.mount`.
    ///
    /// `None` before the first render or after a view function returned nil.
    pub fn view(&self) -> mlua::Result<Option<Table>> {
        let mre: Table = self.lua.globals().get("mre")?;
        match mre.get::<LuaValue>("__tree")? {
            LuaValue::Table(table) => Ok(Some(table)),
            _ => Ok(None),
        }
    }

    /// Access to the underlying Lua state, for host integrations.
    #[must_use]
    pub fn lua(&self) -> &Lua {
        &self.lua
    }
}

/// Installs `mre.__hover(key)`, which returns a reader for the hover state of
/// one node.
///
/// The engine writes the value; Lua only reads it, so a hover change can
/// re-render exactly the part of the tree that cares.
fn install_hover(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let create = lua.create_function(move |lua, key: String| {
        let id = {
            let mut st = state.borrow_mut();
            match st.hover.get(&key) {
                Some(id) => *id,
                None => {
                    st.signals.push(SignalSlot {
                        value: Val::Bool(false),
                        subscribers: Vec::new(),
                    });
                    let id = st.signals.len() - 1;
                    st.hover.insert(key, id);
                    id
                }
            }
        };

        let state = Rc::clone(&state);
        lua.create_function(move |lua, _args: MultiValue| read_signal(lua, &state, id))
    })?;
    mre.set("__hover", create)
}

fn install_animations(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let create = lua.create_function(move |lua, (source, config): (LuaValue, Option<Table>)| {
        let spring_config = read_spring_config(config.as_ref())?;
        let (start, signal, signal_id) = match source {
            LuaValue::Function(signal) => {
                let id = state
                    .borrow()
                    .signal_ptrs
                    .get(&(signal.to_pointer() as usize))
                    .copied();
                (signal.call::<f64>(())? as f32, Some(signal), id)
            }
            LuaValue::Integer(value) => (value as f32, None, None),
            LuaValue::Number(value) => (value as f32, None, None),
            LuaValue::Nil => (0.0, None, None),
            _ => {
                return Err(mlua::Error::runtime(
                    "mre.spring expects a signal, a number or nil",
                ));
            }
        };
        let id = {
            let mut state = state.borrow_mut();
            let id = state.next_animation_id;
            state.next_animation_id += 1;
            id
        };
        let slot = Rc::new(RefCell::new(AnimationSlot {
            animation: Animation::Spring(Spring::with_config(start, spring_config)),
            signal,
            id,
        }));
        state.borrow_mut().animations.push(Rc::clone(&slot));
        if let Some(signal_id) = signal_id {
            state
                .borrow_mut()
                .signal_animations
                .entry(signal_id)
                .or_default()
                .push(Rc::clone(&slot));
        }

        let handle = lua.create_table()?;
        handle.set("id", id)?;
        let target = {
            let slot = Rc::clone(&slot);
            lua.create_function(move |_, value: f64| {
                slot.borrow_mut().animation.retarget(value as f32);
                Ok(())
            })?
        };
        handle.set("to", target.clone())?;
        let current = {
            let slot = Rc::clone(&slot);
            lua.create_function(move |_, ()| Ok(f64::from(slot.borrow().animation.value())))?
        };
        handle.set("value", current)?;

        let meta = lua.create_table()?;
        meta.set(
            "__call",
            lua.create_function(move |_, (_handle, value): (Table, f64)| {
                slot.borrow_mut().animation.retarget(value as f32);
                Ok(())
            })?,
        )?;
        handle.set_metatable(Some(meta))?;
        Ok(handle)
    })?;
    mre.set("__spring", create)?;
    Ok(())
}

fn read_spring_config(config: Option<&Table>) -> mlua::Result<SpringConfig> {
    let mut out = SpringConfig::default();
    let Some(config) = config else {
        return Ok(out);
    };
    let number = |key: &str| -> mlua::Result<Option<f32>> {
        Ok(config.get::<Option<f64>>(key)?.map(|value| value as f32))
    };
    if let Some(stiffness) = number("stiffness")? {
        out.stiffness = stiffness.max(0.0);
    }
    if let Some(damping) = number("damping")? {
        out.damping = damping.max(0.0);
    }
    if let Some(mass) = number("mass")? {
        out.mass = mass.max(1.0e-6);
    }
    Ok(out)
}

/// Exposes `mre.__tween(from, to, config)` — a real time-based animation with
/// an easing curve, stepped by the engine like a spring.
fn install_tween(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let create =
        lua.create_function(move |lua, (from, to, config): (f64, f64, Option<Table>)| {
            let duration = (config
                .as_ref()
                .and_then(|table| table.get::<Option<f64>>("duration").ok().flatten())
                .unwrap_or(300.0)
                .max(0.0)
                / 1000.0) as f32;
            let easing = config
                .as_ref()
                .and_then(|table| table.get::<Option<String>>("easing").ok().flatten())
                .map_or(Easing::EaseOut, |name| Easing::parse(&name));
            let id = {
                let mut st = state.borrow_mut();
                let id = st.next_animation_id;
                st.next_animation_id += 1;
                id
            };
            let slot = Rc::new(RefCell::new(AnimationSlot {
                animation: Animation::Tween {
                    tween: Tween::new(from as f32, to as f32, duration, easing),
                    duration,
                    easing,
                },
                signal: None,
                id,
            }));
            state.borrow_mut().animations.push(Rc::clone(&slot));

            let handle = lua.create_table()?;
            handle.set("id", id)?;
            let current = {
                let slot = Rc::clone(&slot);
                lua.create_function(move |_, ()| Ok(f64::from(slot.borrow().animation.value())))?
            };
            handle.set("value", current)?;
            let target = {
                let slot = Rc::clone(&slot);
                lua.create_function(move |_, value: f64| {
                    slot.borrow_mut().animation.retarget(value as f32);
                    Ok(())
                })?
            };
            handle.set("to", target.clone())?;
            let meta = lua.create_table()?;
            meta.set(
                "__call",
                lua.create_function(move |_, (_handle, value): (Table, f64)| {
                    slot.borrow_mut().animation.retarget(value as f32);
                    Ok(())
                })?,
            )?;
            handle.set_metatable(Some(meta))?;
            Ok(handle)
        })?;
    mre.set("__tween", create)?;
    Ok(())
}

/// A one-shot callback fired on the engine clock, in milliseconds. Lua uses
/// it to sequence animations without doing work on idle frames.
fn install_after(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let create = lua.create_function(move |_, (ms, callback): (f64, Function)| {
        state.borrow_mut().timers.push(Timer {
            remaining: (ms.max(0.0) / 1000.0) as f32,
            callback,
        });
        Ok(())
    })?;
    mre.set("__after", create)?;
    Ok(())
}

fn install_safe_area(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let read = lua.create_function(move |lua, ()| {
        let id = {
            let mut st = state.borrow_mut();
            if st.safe_area.is_none() {
                let [top, right, bottom, left] = st.insets;
                st.signals.push(SignalSlot {
                    value: Val::Str(format!("{top}|{right}|{bottom}|{left}")),
                    subscribers: Vec::new(),
                });
                st.safe_area = Some(st.signals.len() - 1);
            }
            st.safe_area.expect("just created")
        };
        // Subscribing makes insets reactive: a keyboard change rebuilds any
        // view that reads safe_area(), just like a viewport change does.
        let raw = read_signal(lua, &state, id)?;
        let text = raw
            .as_string()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "0|0|0|0".to_string());
        let mut parts = text.split('|');
        let mut number = |fallback: f64| -> f64 {
            parts
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(fallback)
        };
        let table = lua.create_table()?;
        table.set("top", number(0.0))?;
        table.set("right", number(0.0))?;
        table.set("bottom", number(0.0))?;
        table.set("left", number(0.0))?;
        Ok(LuaValue::Table(table))
    })?;
    mre.set("__safe_area", read)?;
    Ok(())
}

fn install_viewport(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let create = lua.create_function(move |lua, _: LuaValue| {
        let id = {
            let mut st = state.borrow_mut();
            if st.viewport.is_none() {
                st.signals.push(SignalSlot {
                    value: Val::Str("0|0|1".to_string()),
                    subscribers: Vec::new(),
                });
                st.viewport = Some(st.signals.len() - 1);
            }
            st.viewport.expect("just created")
        };
        // Reading the signal subscribes the current computation, so a resize
        // rebuilds the view instead of leaving it laid out for the old size.
        let raw = read_signal(lua, &state, id)?;
        let text = raw
            .as_string()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "0|0|1".to_string());
        let mut parts = text.split('|');
        let width: f64 = parts
            .next()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0);
        let height: f64 = parts
            .next()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0);
        let scale: f64 = parts
            .next()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1.0);
        let table = lua.create_table()?;
        table.set("width", width)?;
        table.set("height", height)?;
        table.set("scale", scale)?;
        Ok(LuaValue::Table(table))
    })?;
    mre.set("__viewport", create)
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

        let writer_state = Rc::clone(&state);
        let signal = lua.create_function(move |lua, args: MultiValue| {
            if args.is_empty() {
                read_signal(lua, &writer_state, id)
            } else {
                let value = Val::from_lua(&args[0]).ok_or_else(|| {
                    mlua::Error::runtime("signal value must be nil, boolean, number or string")
                })?;
                write_signal(&writer_state, id, value);
                Ok(LuaValue::Nil)
            }
        })?;
        // Tag the callable so `mre.animate(signal)` can find its id and
        // retarget the spring when Lua writes a new target into the signal.
        // Signal readers are Rust closures, whose Lua environment is
        // immutable, so the stable closure pointer is the key.
        state
            .borrow_mut()
            .signal_ptrs
            .insert(signal.to_pointer() as usize, id);
        Ok(signal)
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
    // A write from app code is a new target for any spring bound to this
    // signal. Writes made by the animation itself are excluded below, or they
    // would cancel the motion on every tick.
    if !st.animation_write {
        if let Some(target) = value.as_f64() {
            if let Some(slots) = st.signal_animations.get(&id) {
                for slot in slots {
                    slot.borrow_mut().animation.retarget(target as f32);
                }
            }
        }
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

fn install_theme(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let set_state = Rc::clone(state);
    let set = lua.create_function(move |_lua, name: String| {
        let name = match name.as_str() {
            "dark" | "light" | "system" => name,
            other => return Err(mlua::Error::RuntimeError(format!("unknown theme: {other}"))),
        };
        let mut st = set_state.borrow_mut();
        st.theme = name.to_string();
        *st.theme_request.borrow_mut() = Some(name.to_string());
        Ok(())
    })?;
    let get_state = Rc::clone(state);
    let get = lua.create_function(move |_lua, ()| Ok(get_state.borrow().theme.clone()))?;
    mre.set("set_theme", set)?;
    mre.set("theme", get)
}

fn install_log(lua: &Lua, mre: &Table) -> mlua::Result<()> {
    let warn = lua.create_function(|_lua, message: String| {
        eprintln!("mre lua: {message}");
        Ok(())
    })?;
    mre.set("warn", warn)
}

fn install_clock(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let now = lua.create_function(move |_lua, ()| Ok(state.borrow().clock))?;
    mre.set("now", now)
}

fn install_diagnostics(lua: &Lua, mre: &Table, state: &Rc<RefCell<State>>) -> mlua::Result<()> {
    let state = Rc::clone(state);
    let read = lua.create_function(move |lua, ()| {
        let table = lua.create_table()?;
        for (index, item) in state.borrow().diagnostics.iter().enumerate() {
            table.set(index + 1, item.as_str())?;
        }
        Ok(LuaValue::Table(table))
    })?;
    mre.set("diagnostics", read)
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
