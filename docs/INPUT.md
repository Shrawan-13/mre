# Input, focus and accessibility

Input is where "state of the art" is easiest to claim and hardest to earn. A UI
engine with bad IME, no gesture disambiguation and a 6-pixel focus ring is not
state of the art regardless of frame times.

## 0. What is implemented today

| Surface | State | Where |
|---|---|---|
| Pointer move/down/up, hover and press feedback | shipped | `Ui::pointer_*`, `App::pointer_*` |
| Press capture, drag outside the node, cancel on release | shipped | `App::pointer_drag` |
| Wheel scrolling with clamping, virtualised lists | shipped | `mre.scroll`, `mre.list` |
| Scrollbar thumb (content-height driven) | shipped | `mre.scroll` with `content`/`offset` |
| Click-to-focus, Tab / Shift-Tab order, Escape blur | shipped | `Ui::focus_at`, `Ui::focus_next` |
| Single-line text editing: insert, backspace, delete, caret moves | shipped | `mre-widgets::InputState` |
| Character-counted carets (no split UTF-8, emoji-safe) | shipped | `InputState` |
| Password fields (bullets, value never painted) | shipped | `View::text_input` |
| `on_change` / `on_submit`, Enter, keyboard button activation | shipped | `App::key` |
| IME commit | shipped (winit `Ime::Commit`) | `mre-window` |
| Click-to-place caret, drag-select, Shift+arrows | shipped | `Ui::place_caret`, `InputState` |
| Copy / cut / paste / select all | shipped (engine) | `App::copied_text/cut/paste/select_all` |
| Platform clipboard + Ctrl+C/X/V/A | shipped (desktop host) | `mre-window`, `arboard` |
| Undo / redo with coalescing | next | — |
| IME pre-edit display | next | — |
| Gesture arena (tap/double/long/pan/pinch) | next | — |
| Fling / rubber-banding | next | — |
| Semantic tree (roles, names, values, focus) | shipped | `Ui::semantics` |
| Platform a11y bridge (AccessKit) | next | — |
| Contrast and hit-target audits | next | — |

The sections below are the contract these implementations are held to. Where a
row says "next", the section is a specification, not a description.

## 1. One pointer abstraction

```rust
pub struct PointerEvent {
    pub id: PointerId,          // stable per contact
    pub kind: PointerKind,      // Mouse | Touch | Pen
    pub phase: Phase,           // Down | Move | Up | Cancel
    pub pos: Point,             // logical px
    pub delta: Size,
    pub pressure: f32,          // 0..1, 0.5 for mouse
    pub tilt: Size,             // pen only
    pub buttons: Buttons,
    pub modifiers: Modifiers,
    pub timestamp: Duration,
}
```

Rules:

- **No `mouse_*` / `touch_*` split in the public API.** Widgets handle one event
  type. Platform quirks are normalised at the boundary.
- **Contact ids are stable** across a gesture, so multi-touch works without
  guesswork.
- **Synthetic compatibility events** (mouse events generated from a touch) are
  marked as synthetic and are never delivered twice.
- **Palm rejection**: contacts whose size exceeds a threshold, or that arrive
  while a pen contact is active, are dropped.

## 2. Gesture recognition

Recognised in the engine, delivered as `GestureEvent`:

| Gesture | Default threshold | Notes |
|---|---|---|
| tap | ≤ 10 px, ≤ 300 ms | |
| double tap | ≤ 300 ms apart, ≤ 24 px | |
| long press | ≥ 500 ms, ≤ 10 px | fires once, cancels on move |
| pan | > 10 px | single pointer |
| fling | velocity ≥ 800 px/s at release | emits velocity for momentum |
| pinch | 2 pointers, scale change > 3% | focal point preserved |
| rotate | 2 pointers, angle change > 8° | opt-in per widget |
| swipe | ≥ 25% of container, ≥ 800 px/s | directional, opt-in |

Design rules:

- Gestures in the same arena are **disambiguated** before delivery: a pan that
  starts inside a scrollable consumes the scroll; a long-press cancels a tap.
- **Velocity is computed from a fitted window**, not last-frame delta, so flings
  do not depend on frame rate.
- Thresholds are theme-level constants, not magic numbers in widgets.

## 3. Scroll and momentum

- Wheel, trackpad and touch scroll normalise into one `ScrollEvent` with
  `delta`, `unit` (`Pixel` | `Line` | `Page`) and `momentum` (`None` | `Begin` |
  `Update` | `End`).
- Trackpad momentum is **not** re-simulated; the platform's momentum is used and
  marked, so we never fight the OS.
- Touch scroll uses engine-side rubber-banding and fling decay with the same
  spring integrator as animation.

## 4. Keyboard

Implemented: Tab and Shift-Tab cycle focusable nodes in tree order, Escape
blurs, Enter submits an editable field or activates a focused button, Space
activates a focused button, and Backspace/Delete/arrow/Home/End edit the focused
field. Focus rings are painted by the engine, not by each widget.

- **Scoped shortcuts**: `mre.shortcut("ctrl+s", handler, { scope = "editor" })`.
- **Conflict detection** at registration; duplicates warn with both call sites.
- **Spatial navigation** (arrow keys / D-pad / remote) as a first-class focus
  strategy, not just tab order.
- **Focus**: one focused node, visible ring, trap regions for modals, restoration
  on close, and `on_focus`/`on_blur`.
- Key repeat is distinguishable from discrete presses.

## 5. Text input

The hardest input surface, specified up front.

### What the engine owns

Editing state is keyed by the **node key**, not by the node index, and lives in
`Ui`. A field is seeded from the `value` the view supplies. While it is focused
the editor is the source of truth, so a stale view can never roll back a
keystroke; when it is unfocused an external value wins. `on_change` receives the
new string and is the only place an app needs to write the value back.

```lua
local name = mre.signal("")

mre.textfield {
  key = "name",
  value = name(),
  placeholder = "Full name",
  width = 320,
  on_change = function(value) name(value) end,
  on_submit = function(value) submit(value) end,
}
```

A keystroke is one character insertion and one repaint. It does not rebuild the
view unless `on_change` writes a signal the view reads, and even then only that
subtree changes.

Selection is a caret plus an anchor. Shift+arrows, Home/End and dragging inside
a field move the caret while keeping the anchor; any movement without Shift
collapses the selection. Typing, Backspace and Delete replace the selection
first. Copy and cut return the selected string to the host, which owns the
platform clipboard: `arboard` on the desktop, the platform text-input protocol
on mobile. Ctrl/Cmd+A, C, X and V are wired in the window host.

| Concern | Requirement |
|---|---|
| IME | Composition events with preedit string and cursor, committed separately |
| Dead keys | Composed at the platform layer, delivered as text |
| Grapheme clusters | Cursor movement never splits a cluster (emoji, combining marks) |
| Bidi | Logical index model; visual navigation via a bidi map |
| Selection | Anchor + focus, shift-extend, drag-select; double-click word and triple-click line are next |
| Editing | Cut/copy/paste and drag-select are shipped; undo/redo with coalescing and autoscroll are next |
| Security | Password mode, no clipboard leak, no IME logs in release |
| Virtual keyboard | Insets reported as safe-area changes, never as a resize flash |

## 6. Drag and drop

- Internal: typed payloads, hover targets, drop indicators, cancel on Escape.
- External: files, text, images — with a capability gate before any read.
- Auto-scroll near container edges during a drag.

## 7. Accessibility

Implemented: `Ui::semantics()` derives a tree from the laid-out node list, with
roles from the widget kind (`Text`, `Button`, `TextField`, `CheckBox`, `Switch`,
`Slider`, `ProgressBar`, `ScrollView`, `List`, `Image`, `Group`) and names from
text runs, field placeholders and explicit `label` props. Composers such as
`mre.checkbox` set the role and forward `label`, so a composed widget is not an
unnamed box. `App::semantics()` exposes the tree and
`cargo run -p mre-runtime --example semantics -- examples/form.lua` prints it.

The platform bridge is AccessKit, derived from the scene every time the scene
changes:

- Roles, names, descriptions, values, states (checked/expanded/selected/disabled).
- Focus order matching visual order unless overridden.
- Live regions for toasts and async results.
- Text-input protocol for screen readers.
- **Contrast and hit-target audits** in debug builds: a target smaller than 44×44
  logical px warns.

Accessibility is gated: a widget without a role and a name fails a test. The
current gate is `the_accessibility_tree_names_the_controls` in
`crates/mre-runtime/tests/app.rs`.

## 8. Platform mapping

| Platform | Pointer | Text | Gesture extras |
|---|---|---|---|
| Linux/X11+Wayland | mouse, touch, pen (libinput) | IME via text-input-v3 | touchpad kinetic scroll |
| Windows | mouse, touch, pen (WM_POINTER) | TSF IME | precision touchpad |
| macOS | mouse, trackpad, sidecar pen | NSTextInputClient | trackpad momentum |
| Web | Pointer Events | composition events | touch + wheel |
| Android | MotionEvent | InputConnection | back gesture, haptics |
| iOS | UITouch | UITextInput | safe areas, edge swipe |

The normalisation layer lives in `mre-window`; nothing above it knows which
platform it is on.
