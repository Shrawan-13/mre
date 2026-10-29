# Input, focus and accessibility

Input is where "state of the art" is easiest to claim and hardest to earn. A UI
engine with bad IME, no gesture disambiguation and a 6-pixel focus ring is not
state of the art regardless of frame times.

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

- **Scoped shortcuts**: `mre.shortcut("ctrl+s", handler, { scope = "editor" })`.
- **Conflict detection** at registration; duplicates warn with both call sites.
- **Spatial navigation** (arrow keys / D-pad / remote) as a first-class focus
  strategy, not just tab order.
- **Focus**: one focused node, visible ring, trap regions for modals, restoration
  on close, and `on_focus`/`on_blur`.
- Key repeat is distinguishable from discrete presses.

## 5. Text input

The hardest input surface, specified up front:

| Concern | Requirement |
|---|---|
| IME | Composition events with preedit string and cursor, committed separately |
| Dead keys | Composed at the platform layer, delivered as text |
| Grapheme clusters | Cursor movement never splits a cluster (emoji, combining marks) |
| Bidi | Logical index model; visual navigation via a bidi map |
| Selection | Anchor + focus, double-click word, triple-click line, shift-extend |
| Editing | Undo/redo with coalescing, cut/copy/paste, drag-select, autoscroll |
| Security | Password mode, no clipboard leak, no IME logs in release |
| Virtual keyboard | Insets reported as safe-area changes, never as a resize flash |

## 6. Drag and drop

- Internal: typed payloads, hover targets, drop indicators, cancel on Escape.
- External: files, text, images — with a capability gate before any read.
- Auto-scroll near container edges during a drag.

## 7. Accessibility

AccessKit tree, derived from the scene every time the scene changes:

- Roles, names, descriptions, values, states (checked/expanded/selected/disabled).
- Focus order matching visual order unless overridden.
- Live regions for toasts and async results.
- Text-input protocol for screen readers.
- **Contrast and hit-target audits** in debug builds: a target smaller than 44×44
  logical px warns.

Accessibility is gated: a widget without a role and a name fails a test.

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
