# Feature parity with egui

Parity target: **behaviour, not checkboxes.** A feature counts as done when it
has a golden-image test, a keyboard path, an accessibility role, and a frame-time
cost inside the reference budget.

Status: `✅` shipped · `🔜 Mx` planned in milestone x · `➖` deliberate non-goal

## Core widgets (M5)

| Feature | egui | MRE |
|---|---|---|
| Button (text/icon/disabled/loading) | ✅ | 🔜 M5 |
| Icon button / segmented button | ✅ | 🔜 M5 |
| Toggle / checkbox | ✅ | 🔜 M5 |
| Radio group | ✅ | 🔜 M5 |
| Slider (range, stepped, log) | ✅ | 🔜 M5 |
| Drag value / numeric field | ✅ | 🔜 M5 |
| Text edit (single/multi, password, mask) | ✅ | 🔜 M5 |
| Combo box / select | ✅ | 🔜 M5 |
| Menu bar, menus, submenus | ✅ | 🔜 M5 |
| Context menu | ✅ | 🔜 M5 |
| Tooltip (hover, long-press, keyboard) | ✅ | 🔜 M5 |
| Modal / dialog | ✅ | 🔜 M5 |
| Tabs | egui_extras | 🔜 M5 |
| Table (virtualised, resizable, sortable) | egui_extras | 🔜 M5 |
| Tree view / collapsible | ✅ | 🔜 M5 |
| Progress bar / spinner | ✅ | 🔜 M5 |
| Color picker | ✅ | 🔜 M5 |
| Date/time picker | egui_extras | 🔜 M5 |
| Toast / snackbar | ❌ | 🔜 M5 |
| Breadcrumb, pagination | ❌ | 🔜 M5 |
| Split pane / resizable panels | ✅ | 🔜 M5 |
| Dockable layout | ❌ | 🔜 M6 |

## Containers and layout

| Feature | egui | MRE |
|---|---|---|
| Flex row/column with gap/align/justify | partial | 🔜 M1 |
| Grid layout | ✅ | 🔜 M1 |
| Absolute/stack positioning | ✅ | 🔜 M1 |
| Scroll area (both axes) | ✅ | 🔜 M1 |
| Virtualised list/grid | ✅ | ✅ M0 |
| Collapsing/accordion | ✅ | 🔜 M5 |
| Multi-window | ✅ | 🔜 M2 |
| Panels (side/top/bottom) | ✅ | 🔜 M2 |
| Container queries / breakpoints | ❌ | 🔜 M1 |
| RTL / bidi mirroring | partial | 🔜 M1 |

## Text

| Feature | egui | MRE |
|---|---|---|
| Shaping (complex scripts, ligatures) | ✅ | 🔜 M1 |
| Bidi (Arabic/Hebrew) | partial | 🔜 M1 |
| Rich text (inline style runs) | ✅ | 🔜 M1 |
| Font fallback + emoji | ✅ | 🔜 M1 |
| Subpixel positioning | ✅ | 🔜 M1 |
| Line breaking / hyphenation | ✅ | 🔜 M1 |
| Editable text with IME | ✅ | 🔜 M3 |
| Undo/redo, selection | ✅ | 🔜 M3 |
| Accessibility text protocol | ✅ | 🔜 M3 |

## Painting

| Feature | egui | MRE |
|---|---|---|
| Rect, rounded rect, circle, line, path | ✅ | 🔜 M1 |
| Analytic anti-aliasing | ✅ | 🔜 M1 |
| Gradients (linear/radial/conic) | partial | 🔜 M1 |
| Images (fit modes, nine-slice) | ✅ | 🔜 M2 |
| Shadows | ✅ | 🔜 M2 |
| Blur / backdrop blur | ❌ | 🔜 M2 |
| Clipping (rect, rounded) | ✅ | 🔜 M2 |
| Layers / composited subtrees | partial | 🔜 M2 |
| SDF icons | ❌ | 🔜 M2 |
| Custom canvas / painter API | ✅ | 🔜 M1 |

## Animation

| Feature | egui | MRE |
|---|---|---|
| Tween with easing | ✅ | 🔜 M3 |
| Spring physics | ❌ | 🔜 M3 |
| Keyframe timelines | ❌ | 🔜 M3 |
| Interruptible / velocity-preserving | ❌ | 🔜 M3 |
| Layout animation | ❌ | 🔜 M3 |
| Shared-element transitions | ❌ | 🔜 M3 |
| Reduced-motion support | ❌ | 🔜 M3 |
| Deterministic integration | ❌ | 🔜 M3 |

Animation is where MRE is intended to be a **superset**, not at parity.

## Input

| Feature | egui | MRE |
|---|---|---|
| Mouse + touch + pen unified | partial | 🔜 M3 |
| Pressure / tilt | ❌ | 🔜 M3 |
| Gestures (tap/double/long/pan/fling/pinch) | partial | 🔜 M3 |
| Momentum scroll | ✅ | 🔜 M3 |
| Keyboard shortcuts + conflicts | ✅ | 🔜 M3 |
| Spatial navigation (TV/remote) | ❌ | 🔜 M3 |
| Focus trap / restoration | partial | 🔜 M3 |
| Drag & drop (internal + files) | ✅ | 🔜 M3 |
| IME composition | ✅ | 🔜 M3 |
| Gamepad | ❌ | 🔜 M7 |

## Accessibility

| Feature | egui | MRE |
|---|---|---|
| AccessKit tree | ✅ | 🔜 M3 |
| Roles/names/values/states | ✅ | 🔜 M3 |
| Live regions | ❌ | 🔜 M3 |
| Screen-reader text protocol | partial | 🔜 M3 |
| Contrast / hit-target audit | ❌ | 🔜 M3 |
| Reduced motion | ❌ | 🔜 M3 |

## Platform and shell

| Feature | egui | MRE |
|---|---|---|
| Linux / Windows / macOS | ✅ | 🔜 M2 |
| Web (wasm) | ✅ | 🔜 M7 |
| Android / iOS | ✅ (eframe) | 🔜 M7 |
| Multi-window | ✅ | 🔜 M2 |
| Native file dialogs | via rfd | 🔜 M5 |
| Native menus / tray | via muda | 🔜 M6 |
| Clipboard (rich) | ✅ | 🔜 M3 |
| Persistence | ✅ | 🔜 M4 |
| Open URL / share | ✅ | 🔜 M5 |
| DPI / fractional scaling | ✅ | 🔜 M2 |
| Safe areas / notches | ❌ | 🔜 M7 |

## Styling

| Feature | egui | MRE |
|---|---|---|
| Light/dark themes | ✅ | 🔜 M2 |
| Design tokens | partial | 🔜 M2 |
| Per-widget styling | ✅ | 🔜 M1 |
| Global zoom | ✅ | 🔜 M2 |
| Custom fonts | ✅ | 🔜 M1 |
| High-contrast mode | ❌ | 🔜 M5 |

## Scripting (superset, not in egui)

| Feature | MRE |
|---|---|
| Apps in Lua | 🔜 M4 |
| Fine-grained reactivity | 🔜 M4 |
| Hot reload with state preservation | 🔜 M4 |
| Capability sandbox | 🔜 M4 |
| C ABI for Swift/Kotlin/C#/Python | 🔜 M6 |

## Score

| Category | egui | MRE today | MRE target |
|---|---|---|---|
| Widgets | 21 | 0 | 23 |
| Containers | 8 | 1 | 9 |
| Text | 6 | 0 | 6 |
| Painting | 10 | 0 | 10 |
| Animation | 2 | 0 | 8 |
| Input | 7 | 0 | 10 |
| Accessibility | 3 | 0 | 6 |
| Platform | 7 | 0 | 11 |
| Styling | 5 | 0 | 6 |
| **Total** | **69** | **1** | **89** |

The honest reading: at M0 we have one feature (virtualised lists) and a
performance harness. Everything else is a plan with an exit criterion attached.
