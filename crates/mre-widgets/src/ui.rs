//! Turning views into geometry and pixels.
//!
//! [`Ui`] owns the layout tree, the text store, the layout cache and the theme.
//! Its three operations are deliberately separate:
//!
//! * [`Ui::set_view`] rebuilds the node tree from a declarative [`View`],
//! * [`Ui::layout`] computes geometry for a viewport,
//! * [`Ui::paint`] draws into a backend.
//!
//! A resize only calls `layout`; a theme change only calls `paint`. Nothing does
//! work it was not asked to do.

use crate::input::InputState;
use crate::theme::Theme;
use crate::view::{Props, TextInput, View, ViewKind};
use mre_core::{Point, Rect, Size, Viewport};
use mre_layout::{Align, Axis, Dim, Edges, Engine, Justify, Layout, NodeId, Style, TextId, Tree};
use mre_paint::{Backend, Paint};
use mre_text::{TextStore, TextStyle, Wrap};
use std::collections::HashMap;

/// Repaint bands are clipped slightly wider than the damaged node bounds.
/// Antialiased borders, rounded corners and glyph edges extend about half a
/// pixel past those bounds; clipping at the exact bounds cuts them.
const AA_PAD: f32 = 1.5;

// Edge physics. Flutter's BouncingScrollSimulation uses this overdamped spring
// to return from an overscroll; `damping_ratio > 1` is deliberate so the edge
// never oscillates back past the boundary.
const SPRING_MASS: f32 = 0.5;
const SPRING_STIFFNESS: f32 = 100.0;
const SPRING_DAMPING_RATIO: f32 = 1.1;
/// iOS rubber-band constant. Flutter's equivalent gain starts at 0.52.
const RUBBER_BAND_C: f32 = 0.55;
/// Flutter's velocity tolerance at dpr 1, in px/s.
const SETTLE_VELOCITY_TOLERANCE: f32 = 20.0;
const SETTLE_DISTANCE_TOLERANCE: f32 = 0.1;
/// Flutter's `maxSpringTransferVelocity`.
const MAX_SPRING_TRANSFER_VELOCITY: f32 = 5000.0;

/// A square node with a radius of half its side is a circle. It must use the
/// circle primitives: a rounded rectangle approximates the corners with
/// quadratics and is visibly square-ish at small knob sizes.
pub(crate) fn is_circle(rect: Rect, radius: f32) -> bool {
    let side = rect.w.min(rect.h);
    side > 0.0
        && (rect.w - rect.h).abs() < 0.01
        && radius > 0.0
        && (radius - side * 0.5).abs() < 0.01
}

/// One step of a paint walk.
enum PaintOp {
    /// Draw a node's own background, border and text, with the displacement and
    /// opacity inherited from animated ancestors.
    Node {
        /// The node.
        id: NodeId,
        /// Horizontal displacement.
        dx: f32,
        /// Vertical displacement.
        dy: f32,
        /// Opacity multiplier.
        opacity: f32,
    },
    /// Start clipping to a rect.
    PushClip(Rect),
    /// End the most recent clip.
    PopClip,
}

/// Per-node state that layout does not need.
#[derive(Clone, Debug, Default)]
pub struct UiNode {
    /// Paint properties.
    pub props: Props,
    /// The text run, for text and button-label nodes.
    pub text: Option<TextId>,
    /// Stable key, used to route events back to the code that declared them.
    pub key: Option<String>,
    /// Vector icon, when this node is one.
    pub icon: Option<crate::view::IconKind>,
    /// Text field configuration, when this node is one.
    pub input: Option<TextInput>,
}

/// One node of the accessibility tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Semantics {
    /// What the node is.
    pub role: crate::view::Role,
    /// Human-readable label.
    pub label: String,
    /// Editable or displayable value, for fields and sliders.
    pub value: Option<String>,
    /// Bounds in logical pixels.
    pub rect: Rect,
    /// Stable key, when the node declared one.
    pub key: Option<String>,
    /// Whether the node can take keyboard focus.
    pub focusable: bool,
    /// Whether the node currently has focus.
    pub focused: bool,
}

/// A property an animation can drive without rebuilding the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimProp {
    /// Multiplied into the node's opacity.
    Opacity,
    /// Horizontal offset, logical pixels.
    X,
    /// Vertical offset, logical pixels.
    Y,
}

/// Paint-time values supplied by animations, keyed by node key.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Animated {
    /// Opacity multiplier, `0.0..=1.0`.
    pub opacity: Option<f32>,
    /// Horizontal displacement.
    pub dx: f32,
    /// Vertical displacement.
    pub dy: f32,
}

/// Engine-owned scroll state for one keyed scroll container.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollState {
    /// Clamped position within the content, in logical pixels.
    pub offset: f32,
    /// Elastic displacement beyond either end, in logical pixels.
    pub overscroll: f32,
    /// True from touch-down to touch-up. The spring never fights the finger.
    pub dragging: bool,
    /// True while the overscroll is springing back to rest.
    pub settling: bool,
    /// Overscroll at the instant the spring took over.
    pub spring_x0: f32,
    /// Velocity handed to the spring at that instant, in px/s.
    pub spring_v0: f32,
    /// Seconds since the spring took over. Absolute time avoids dt drift.
    pub spring_elapsed: f32,
}

/// An edit a text field can perform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOp {
    /// Delete one character backwards.
    Backspace,
    /// Delete one character forwards.
    Delete,
    /// Move the caret left.
    Left,
    /// Move the caret right.
    Right,
    /// Move the caret to the start.
    Home,
    /// Move the caret to the end.
    End,
    /// Move the caret to the start of the previous word.
    WordLeft,
    /// Move the caret to the start of the next word.
    WordRight,
    /// Delete the word before the caret.
    DeleteWordBackward,
    /// Delete the word after the caret.
    DeleteWordForward,
    /// Select the whole field.
    SelectAll,
    /// Remove everything.
    Clear,
}

/// A laid-out, paintable interface.
pub struct Ui {
    tree: Tree,
    nodes: Vec<UiNode>,
    texts: TextStore,
    engine: Engine,
    layout: Layout,
    theme: Theme,
    viewport: Rect,
    hovered: Option<String>,
    pressed: Option<String>,
    focused: Option<String>,
    /// True only when focus was reached with the keyboard. Pointer focus must
    /// not paint a focus ring — that is the "weird border for a split second"
    /// people see when they click a checkbox.
    focus_visible: bool,
    inputs: HashMap<String, InputState>,
    animated: HashMap<String, Animated>,
    /// Engine-owned scroll offsets, keyed by the scroll node's stable key.
    scrolls: HashMap<String, ScrollState>,
    /// Full window rect used for overlays on the last layout pass.
    full: Rect,
    /// Regions that need repainting, in logical pixels.
    damage: Vec<Rect>,
    /// Whether the whole surface needs repainting.
    full_damage: bool,
    /// A pure translation detected between the previous and current layouts:
    /// the region and the delta. The paint pass copies the previous pixels
    /// instead of re-rasterising them, which is what makes scrolling cost the
    /// exposed band rather than the whole scrolled view.
    scroll: Option<(Rect, f32, f32)>,
}

impl Ui {
    /// Creates an empty UI around a text store.
    #[must_use]
    pub fn new(texts: TextStore) -> Self {
        Self {
            tree: Tree::new(Style::column()),
            nodes: vec![UiNode::default()],
            texts,
            engine: Engine::new(),
            layout: Layout::empty(),
            theme: Theme::light(),
            viewport: Rect::EMPTY,
            hovered: None,
            pressed: None,
            focused: None,
            focus_visible: false,
            inputs: HashMap::new(),
            animated: HashMap::new(),
            scrolls: HashMap::new(),
            full: Rect::EMPTY,
            damage: Vec::new(),
            full_damage: true,
            scroll: None,
        }
    }

    /// The active theme.
    #[must_use]
    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Replaces the theme. The next [`Ui::paint`] uses the new colours.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        // Every colour on screen comes from the theme, so all of it changed.
        self.damage_all();
    }

    /// The layout tree.
    #[must_use]
    pub const fn tree(&self) -> &Tree {
        &self.tree
    }

    /// The last layout result.
    #[must_use]
    pub const fn layout_result(&self) -> &Layout {
        &self.layout
    }

    /// The text store.
    #[must_use]
    pub const fn texts(&self) -> &TextStore {
        &self.texts
    }

    /// The text store, mutably.
    pub fn texts_mut(&mut self) -> &mut TextStore {
        &mut self.texts
    }

    /// The viewport the last layout ran in.
    #[must_use]
    pub const fn viewport(&self) -> Rect {
        self.viewport
    }

    /// Every text run currently in the tree, one per line.
    ///
    /// This is the observable surface for tests and for accessibility tooling
    /// until the a11y tree lands.
    #[must_use]
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        for node in &self.nodes {
            if let Some(id) = node.text {
                out.push_str(self.texts.text(id));
                out.push('\n');
            }
            if node.input.is_some() {
                let key = node.key.as_deref().unwrap_or_default();
                if let Some(state) = self.inputs.get(key) {
                    out.push_str(&state.text);
                    out.push('\n');
                }
            }
        }
        out
    }

    /// Number of layout nodes, including the root.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.tree.len()
    }

    /// Rebuilds the tree from a view.
    ///
    /// The text store keeps its shaping engine and cache, so unchanged text is
    /// not re-shaped after a rebuild.
    pub fn set_view(&mut self, view: &View) {
        self.tree = Tree::new(Style::column());
        self.nodes.clear();
        self.nodes.push(UiNode::default());
        self.texts.clear();
        // The tree was rebuilt, so node ids now mean different things. The
        // measure cache is keyed by node id, so it has to go: keeping it would
        // size a new node with an old node's measurement.
        self.engine.clear();

        let root = self.tree.root();
        // The app's root view is the window: it fills the viewport, so an
        // overlay anchored to it covers the whole screen and a root column can
        // centre its content.
        let mut view = view.clone();
        if view.style.height == Dim::Auto {
            view.style.height = Dim::Percent(100.0);
        }
        self.build(root, &view);

        // Editing state belongs to keys, not node indices, so it survives a
        // rebuild. Drop state for fields that no longer exist.
        self.inputs.retain(|key, _| {
            self.nodes
                .iter()
                .any(|node| node.key.as_deref() == Some(key))
        });
        if let Some(focused) = &self.focused {
            if !self.inputs.contains_key(focused) {
                self.focused = None;
            }
        }
        self.animated.retain(|key, _| {
            self.nodes
                .iter()
                .any(|node| node.key.as_deref() == Some(key))
        });
        self.scrolls.retain(|key, _| {
            self.nodes
                .iter()
                .any(|node| node.key.as_deref() == Some(key))
        });
        // Scroll offsets belong to keys, not node ids, so a rebuild keeps them.
        let restore: Vec<(NodeId, f32)> = self
            .scrolls
            .iter()
            .filter_map(|(key, state)| {
                self.node_by_key(key)
                    .map(|id| (id, state.offset + state.overscroll))
            })
            .collect();
        for (id, displacement) in restore {
            let mut style = self.tree.node(id).style.clone();
            style.scroll_offset = displacement;
            self.tree.set_style(id, style);
        }
    }

    /// The accessibility tree, in paint order.
    ///
    /// Roles come from the widget kind unless a view overrides them, and labels
    /// are drawn from text runs so a screen reader never sees an unnamed
    /// control. This is the surface a platform accessibility bridge consumes.
    #[must_use]
    /// The text runs with their rects and their visual baselines.
    ///
    /// A diagnostic: it says where the letters of every run actually sit.
    pub fn text_baselines(&self) -> Vec<(String, Rect, f32)> {
        let mut out = Vec::new();
        for (index, node) in self.nodes.iter().enumerate().skip(1) {
            let Some(text_id) = node.text else { continue };
            let id = NodeId::from_raw(index as u32);
            let rect = self.layout.rect(id);
            let content = self.texts.text(text_id).to_string();
            let baseline = self.texts.style(text_id).map_or(0.0, |style| {
                self.texts.engine().visual_baseline(&content, style)
            });
            out.push((content, rect, baseline));
        }
        out
    }

    pub fn semantics(&self) -> Vec<Semantics> {
        use crate::view::Role;
        let mut out = Vec::new();
        for (index, node) in self.nodes.iter().enumerate().skip(1) {
            let id = NodeId::from_raw(index as u32);
            let rect = self.layout.rect(id);
            if rect.w <= 0.0 || rect.h <= 0.0 {
                continue;
            }
            let role = node.props.role.unwrap_or_else(|| {
                if node.input.is_some() {
                    Role::TextField
                } else if node.icon.is_some() {
                    Role::Image
                } else if node.text.is_some() {
                    Role::Text
                } else if node.props.clip {
                    Role::ScrollView
                } else if node.props.interactive {
                    Role::Button
                } else {
                    Role::Group
                }
            });

            let key = node.key.as_deref();
            let value = node.input.as_ref().and_then(|input| {
                key.and_then(|key| self.inputs.get(key))
                    .map(|state| state.text.clone())
                    .or_else(|| Some(input.value.clone()))
            });

            let label = if let Some(label) = &node.props.label {
                label.clone()
            } else if let Some(text) = node.text {
                self.texts.text(text).to_string()
            } else if let Some(input) = &node.input {
                match &value {
                    Some(value) if !value.is_empty() => value.clone(),
                    _ => input.placeholder.clone(),
                }
            } else {
                self.first_text(id).unwrap_or_default()
            };

            out.push(Semantics {
                role,
                label,
                value,
                rect,
                key: node.key.clone(),
                focusable: node.props.focusable,
                focused: node.key.is_some() && node.key == self.focused,
            });
        }
        out
    }

    fn first_text(&self, id: NodeId) -> Option<String> {
        for child in self.tree.children(id) {
            if let Some(node) = self.nodes.get(child.index()) {
                if let Some(text) = node.text {
                    return Some(self.texts.text(text).to_string());
                }
            }
            if let Some(text) = self.first_text(*child) {
                return Some(text);
            }
        }
        None
    }

    /// Sets an animated paint property for a node, without rebuilding anything.
    ///
    /// Returns `true` when the value that painting uses actually changed.
    /// Marks a rectangle as needing a repaint.
    ///
    /// The paint pass only touches the damaged region, so the cost of a frame
    /// is proportional to what changed rather than to the size of the window.
    /// This is the same idea as a backing store: keep what is on screen and
    /// repaint only the part that is stale.
    pub fn damage(&mut self, rect: Rect) {
        if rect.w > 0.0 && rect.h > 0.0 {
            self.damage.push(rect);
        }
    }

    /// Marks the whole surface as needing a repaint.
    pub fn damage_all(&mut self) {
        self.full_damage = true;
    }

    /// The regions that need repainting, in logical pixels.
    ///
    /// Empty means nothing changed since the last paint, and the paint pass
    /// does no work at all. A full repaint covers the whole canvas, not just
    /// the viewport: the safe-area insets live outside the viewport and have
    /// to be painted too.
    fn take_damage(&mut self, backend: &dyn Backend) -> Vec<Rect> {
        if self.full_damage {
            self.full_damage = false;
            self.damage.clear();
            self.scroll = None;
            let scale = backend.scale();
            return vec![Rect::new(
                0.0,
                0.0,
                backend.width() as f32 / scale,
                backend.height() as f32 / scale,
            )];
        }
        std::mem::take(&mut self.damage)
    }

    /// Damages the node with a key, if it has one.
    fn damage_key(&mut self, key: Option<&str>) {
        let Some(key) = key else {
            return;
        };
        if let Some(id) = self.node_by_key(key) {
            let rect = self.layout.rect(id);
            self.damage(rect);
        }
    }

    pub fn set_animated(&mut self, key: &str, property: AnimProp, value: f32) -> bool {
        let before = self.animated.get(key).copied().unwrap_or_default();
        let mut after = before;
        match property {
            AnimProp::Opacity => after.opacity = Some(value.clamp(0.0, 1.0)),
            AnimProp::X => after.dx = value,
            AnimProp::Y => after.dy = value,
        }
        if after == before {
            return false;
        }
        // An animated property moves or fades a node, so both the old and the
        // new position have to be repainted.
        if let Some(id) = self.node_by_key(key) {
            let rect = self.layout.rect(id);
            self.damage(translate_rect(rect, before.dx, before.dy));
            self.damage(translate_rect(rect, after.dx, after.dy));
        }
        self.animated.insert(key.to_string(), after);
        true
    }

    /// The animated properties currently applied to a key.
    #[must_use]
    pub fn animated(&self, key: &str) -> Option<Animated> {
        self.animated.get(key).copied()
    }

    fn build(&mut self, parent: NodeId, view: &View) -> NodeId {
        match &view.kind {
            ViewKind::Container => {
                let node = self.tree.add_child(parent, view.style.clone());
                self.nodes.push(UiNode {
                    props: view.props.clone(),
                    text: None,
                    key: view.key.clone(),
                    icon: None,
                    input: None,
                });
                for child in &view.children {
                    self.build(node, child);
                }
                node
            }
            ViewKind::Text(text, style) => {
                let text_id = self.texts.add(text.clone(), style.clone());
                let node = self.tree.add_text(parent, view.style.clone(), text_id);
                self.nodes.push(UiNode {
                    props: view.props.clone(),
                    text: Some(text_id),
                    key: view.key.clone(),
                    icon: None,
                    input: None,
                });
                node
            }
            ViewKind::Button(label) => {
                let theme = self.theme.clone();
                let mut style = view.style.clone();
                style.display = mre_layout::Display::Flex;
                style.direction = mre_layout::Direction::Row;
                style.align_items = Align::Center;
                style.justify_content = Justify::Center;
                style.padding = Edges::symmetric(theme.spacing * 0.75, theme.spacing * 2.0);
                style.min_height = Dim::Px(theme.control_height);

                let node = self.tree.add_child(parent, style);
                let key = view
                    .key
                    .clone()
                    .or_else(|| Some(format!("@btn{}", self.nodes.len())));
                self.nodes.push(UiNode {
                    props: Props {
                        interactive: true,
                        background: Some(view.props.background.unwrap_or(theme.accent)),
                        radius: if view.props.radius > 0.0 {
                            view.props.radius
                        } else {
                            theme.radius
                        },
                        opacity: view.props.opacity,
                        focusable: true,
                        ..Props::default()
                    },
                    text: None,
                    key,
                    icon: None,
                    input: None,
                });

                let label_style = TextStyle::new(theme.font_size.max(1.0)).weight(500);
                let text_id = self.texts.add(label.clone(), label_style);
                self.tree.add_text(node, Style::default(), text_id);
                self.nodes.push(UiNode {
                    props: Props {
                        color: Some(view.props.color.unwrap_or(theme.on_accent)),
                        opacity: view.props.opacity,
                        ..Props::default()
                    },
                    text: Some(text_id),
                    key: None,
                    icon: None,
                    input: None,
                });
                node
            }
            ViewKind::Icon(kind) => {
                let node = self.tree.add_child(parent, view.style.clone());
                self.nodes.push(UiNode {
                    props: view.props.clone(),
                    text: None,
                    key: view.key.clone(),
                    icon: Some(*kind),
                    input: None,
                });
                node
            }
            ViewKind::TextInput(input) => {
                let mut style = view.style.clone();
                if style.height == Dim::Auto {
                    style.height = Dim::Px(32.0);
                }
                let node = self.tree.add_child(parent, style);
                let key = view
                    .key
                    .clone()
                    .or_else(|| Some(format!("@input{}", self.nodes.len())));
                if let Some(key) = &key {
                    match self.inputs.get_mut(key) {
                        Some(state) => {
                            // A view that supplies a new value from outside wins
                            // while the field is not focused. While it is, the
                            // editor is the source of truth.
                            if self.focused.as_deref() != Some(key.as_str())
                                && state.text != input.value
                            {
                                state.set_text(input.value.clone());
                            }
                        }
                        None => {
                            self.inputs
                                .insert(key.clone(), InputState::new(input.value.clone()));
                        }
                    }
                }
                self.nodes.push(UiNode {
                    props: view.props.clone(),
                    text: None,
                    key,
                    icon: None,
                    input: Some(input.clone()),
                });
                node
            }
            ViewKind::Spacer(_) => {
                let node = self.tree.add_child(parent, view.style.clone());
                self.nodes.push(UiNode {
                    props: view.props.clone(),
                    text: None,
                    key: view.key.clone(),
                    icon: None,
                    input: None,
                });
                node
            }
        }
    }

    /// Lays out the current tree inside `viewport`.
    ///
    /// Call this on every resize, rotation and DPI change. The measure cache
    /// makes an unchanged layout a cache hit and a changed one proportional to
    /// the changed subtree.
    pub fn layout(&mut self, viewport: Rect) {
        self.layout_with_full(viewport, viewport);
    }

    /// Lays out against a content rect while overlays anchor to `full`.
    pub fn layout_with_full(&mut self, viewport: Rect, full: Rect) {
        self.viewport = viewport;
        self.full = full;
        let Self {
            tree,
            texts,
            engine,
            layout,
            ..
        } = self;
        engine.set_full_rect(full);
        *layout = engine.layout(tree, viewport, texts);
    }

    /// Lays out against a [`Viewport`], which is the platform-facing entry
    /// point: it applies safe-area insets and density for you.
    pub fn layout_viewport(&mut self, viewport: &Viewport) {
        let previous = self.signatures();
        self.layout_with_full(viewport.content(), viewport.full());
        self.damage_from_diff(previous);
    }

    /// Re-lays out the current tree after a scroll offset changed.
    ///
    /// This never calls `set_view`: the Lua tree, text runs and measure cache
    /// are all preserved. Only geometry changes, and `damage_from_diff` lets
    /// the paint pass blit the scrolled region instead of re-rasterising it.
    fn relayout(&mut self) {
        let previous = self.signatures();
        let (viewport, full) = (self.viewport, self.full);
        self.layout_with_full(viewport, full);
        self.damage_from_diff(previous);
    }

    /// All keyed engine-owned scroll containers.
    #[must_use]
    pub fn scroll_keys(&self) -> Vec<String> {
        self.scrolls.keys().cloned().collect()
    }

    /// The state of an engine-owned scroll container, if the key names one.
    #[must_use]
    pub fn scroll_state(&self, key: &str) -> Option<ScrollState> {
        let id = self.node_by_key(key)?;
        if !self.tree.node(id).style.scroll {
            return None;
        }
        Some(self.scrolls.get(key).copied().unwrap_or_default())
    }

    /// Whether a scroll container moves along the vertical axis.
    #[must_use]
    pub fn scroll_is_vertical(&self, key: &str) -> bool {
        self.node_by_key(key)
            .is_some_and(|id| self.tree.node(id).style.direction.axis() == Axis::Vertical)
    }

    /// The clamped position of a scroll container, in logical pixels.
    #[must_use]
    pub fn scroll_offset(&self, key: &str) -> f32 {
        self.scroll_state(key).map_or(0.0, |state| state.offset)
    }

    /// True when a delta would push a scroll container past one of its ends.
    #[must_use]
    pub fn scroll_outward(&self, key: &str, dx: f32, dy: f32) -> bool {
        let Some(state) = self.scroll_state(key) else {
            return false;
        };
        let Some(id) = self.node_by_key(key) else {
            return false;
        };
        let vertical = self.tree.node(id).style.direction.axis() == Axis::Vertical;
        let delta = if vertical { dy } else { dx };
        let max = self.max_scroll(key);
        (state.offset <= 0.0 && delta < 0.0) || (state.offset >= max - 0.01 && delta > 0.0)
    }

    /// True when the scroll container is at (or past) either end.
    #[must_use]
    pub fn scroll_at_bound(&self, key: &str) -> bool {
        let Some(state) = self.scroll_state(key) else {
            return false;
        };
        state.offset <= 0.0 || state.offset >= self.max_scroll(key) - 0.01
    }

    /// The largest valid offset for a scroll container, measured from the
    /// laid-out content extent rather than guessed by the app.
    #[must_use]
    pub fn max_scroll(&self, key: &str) -> f32 {
        let Some(id) = self.node_by_key(key) else {
            return 0.0;
        };
        let style = &self.tree.node(id).style;
        if !style.scroll {
            return 0.0;
        }
        let rect = self.layout.rect(id);
        let extent = self.layout.content_extent(id);
        if style.direction.axis() == Axis::Vertical {
            (extent.h - (rect.h - style.padding.vertical()).max(0.0)).max(0.0)
        } else {
            (extent.w - (rect.w - style.padding.horizontal()).max(0.0)).max(0.0)
        }
    }

    fn viewport_extent(&self, key: &str) -> f32 {
        let Some(id) = self.node_by_key(key) else {
            return 1.0;
        };
        let style = &self.tree.node(id).style;
        let rect = self.layout.rect(id);
        if style.direction.axis() == Axis::Vertical {
            (rect.h - style.padding.vertical()).max(1.0)
        } else {
            (rect.w - style.padding.horizontal()).max(1.0)
        }
    }

    /// iOS rubber-band closed form: `d * (1 - 1/(x*c/d + 1))`, c = 0.55.
    /// Flutter's gain formulation integrates to the same curve with c = 0.52.
    /// Stateless, so reversing the finger retraces it instead of fighting a
    /// spring: that is what removes the shiver at the ends.
    fn rubber_band(travel: f32, viewport: f32) -> f32 {
        if travel == 0.0 {
            return 0.0;
        }
        let x = travel.abs().min(viewport * 4.0);
        let displaced = viewport * (1.0 - 1.0 / (x * RUBBER_BAND_C / viewport + 1.0));
        displaced.copysign(travel)
    }

    /// Closed-form overdamped spring used by Flutter's `BouncingScrollSimulation`:
    /// mass 0.5, stiffness 100, damping ratio 1.1.
    fn spring_at(x0: f32, v0: f32, t: f32) -> (f32, f32) {
        let c = 2.0 * SPRING_DAMPING_RATIO * (SPRING_STIFFNESS * SPRING_MASS).sqrt();
        let disc = (c * c - 4.0 * SPRING_MASS * SPRING_STIFFNESS)
            .max(1.0e-6)
            .sqrt();
        let r1 = (-c + disc) / (2.0 * SPRING_MASS);
        let r2 = (-c - disc) / (2.0 * SPRING_MASS);
        let a = (v0 - r2 * x0) / (r1 - r2);
        let b = (r1 * x0 - v0) / (r1 - r2);
        let e1 = (r1 * t).exp();
        let e2 = (r2 * t).exp();
        (a * e1 + b * e2, a * r1 * e1 + b * r2 * e2)
    }

    fn apply_scroll_style(&mut self, key: &str, id: NodeId, state: ScrollState) {
        let mut style = self.tree.node(id).style.clone();
        style.scroll_offset = state.offset + state.overscroll;
        self.tree.set_style(id, style);
        self.scrolls.insert(key.to_string(), state);
    }

    /// Marks a scroll container as actively dragged or released. No spring runs
    /// while the finger is down; that alone removes the end-of-list shiver.
    pub fn set_scroll_dragging(&mut self, key: &str, dragging: bool) {
        let Some(id) = self.node_by_key(key) else {
            return;
        };
        let mut state = self.scrolls.get(key).copied().unwrap_or_default();
        if state.dragging == dragging {
            return;
        }
        state.dragging = dragging;
        if dragging {
            // A finger that grabs a rebounding edge owns it again; stop asking
            // the host for settle frames until it is released.
            state.settling = false;
        }
        if !dragging && state.overscroll != 0.0 && !state.settling {
            self.begin_scroll_settle(key, 0.0);
            return;
        }
        self.apply_scroll_style(key, id, state);
    }

    /// Hands a release velocity to the edge spring, in offset-space px/s.
    pub fn begin_scroll_settle(&mut self, key: &str, velocity: f32) {
        let Some(id) = self.node_by_key(key) else {
            return;
        };
        let mut state = self.scrolls.get(key).copied().unwrap_or_default();
        state.dragging = false;
        state.settling = true;
        state.spring_x0 = state.overscroll;
        state.spring_v0 =
            velocity.clamp(-MAX_SPRING_TRANSFER_VELOCITY, MAX_SPRING_TRANSFER_VELOCITY);
        state.spring_elapsed = 0.0;
        self.apply_scroll_style(key, id, state);
    }

    /// Scrolls a keyed container by a delta. Positive `dy` scrolls down.
    pub fn scroll_by(&mut self, key: &str, dx: f32, dy: f32) -> bool {
        self.scroll_by_inner(key, dx, dy, false)
    }

    /// Touch-driven scroll: the drag flag suppresses settling while the finger
    /// is still in control of the content.
    pub fn drag_by(&mut self, key: &str, dx: f32, dy: f32) -> bool {
        self.scroll_by_inner(key, dx, dy, true)
    }

    fn scroll_by_inner(&mut self, key: &str, dx: f32, dy: f32, dragging: bool) -> bool {
        let Some(id) = self.node_by_key(key) else {
            return false;
        };
        if !self.tree.node(id).style.scroll {
            return false;
        }
        let vertical = self.tree.node(id).style.direction.axis() == Axis::Vertical;
        let delta = if vertical { dy } else { dx };
        if delta == 0.0 {
            return false;
        }

        let max = self.max_scroll(key);
        let viewport = self.viewport_extent(key);
        let mut state = self.scrolls.get(key).copied().unwrap_or_default();
        state.dragging = dragging;
        state.settling = false;

        // Virtual position = legal offset + damped overscroll. Working in one
        // number makes the rubber band exactly reversible when the finger turns.
        let proposed = state.offset + state.overscroll + delta;
        let (offset, overscroll) = if proposed < 0.0 {
            (0.0, Self::rubber_band(proposed, viewport))
        } else if proposed > max {
            (max, Self::rubber_band(proposed - max, viewport))
        } else {
            (proposed, 0.0)
        };
        if (state.offset - offset).abs() < 1.0e-4 && (state.overscroll - overscroll).abs() < 1.0e-4
        {
            return false;
        }
        state.offset = offset;
        state.overscroll = overscroll;
        self.apply_scroll_style(key, id, state);
        self.relayout();
        true
    }

    /// Moves a scroll container to an absolute position. The engine clamps it.
    pub fn scroll_to(&mut self, key: &str, target: f32) -> bool {
        let Some(id) = self.node_by_key(key) else {
            return false;
        };
        if !self.tree.node(id).style.scroll {
            return false;
        }
        let mut state = self.scrolls.get(key).copied().unwrap_or_default();
        let next = target.clamp(0.0, self.max_scroll(key));
        if (state.offset - next).abs() < 0.01 && state.overscroll == 0.0 {
            return false;
        }
        state.offset = next;
        state.overscroll = 0.0;
        state.settling = false;
        state.dragging = false;
        self.apply_scroll_style(key, id, state);
        self.relayout();
        true
    }

    /// True while an elastic overscroll still needs frames.
    #[must_use]
    pub fn any_scroll_settling(&self) -> bool {
        self.scrolls.values().any(|state| state.settling)
    }

    /// Keys whose overscroll is still springing back.
    #[must_use]
    pub fn settling_scroll_keys(&self) -> Vec<String> {
        self.scrolls
            .iter()
            .filter(|(_, state)| state.settling)
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Advances the edge spring. Returns `true` while it is still moving.
    ///
    /// The position is evaluated from absolute elapsed time, so 60 Hz and
    /// 120 Hz devices land at the same place after the same duration.
    pub fn settle_scroll(&mut self, key: &str, dt: f32) -> bool {
        let Some(id) = self.node_by_key(key) else {
            return false;
        };
        let mut state = self.scrolls.get(key).copied().unwrap_or_default();
        if !state.settling || state.dragging {
            return false;
        }
        state.spring_elapsed += dt.max(0.0);
        let (x, v) = Self::spring_at(state.spring_x0, state.spring_v0, state.spring_elapsed);
        if x.abs() < SETTLE_DISTANCE_TOLERANCE && v.abs() < SETTLE_VELOCITY_TOLERANCE {
            state.overscroll = 0.0;
            state.settling = false;
        } else {
            state.overscroll = x;
        }
        self.apply_scroll_style(key, id, state);
        self.relayout();
        state.settling
    }

    /// The keyed engine-owned scroll containers under a point, innermost first.
    ///
    /// Nested scrollers need the full chain so a vertical drag over a
    /// horizontal shelf can fall through to the vertical page behind it.
    #[must_use]
    pub fn scroll_keys_at(&self, point: Point) -> Vec<String> {
        let Some(mut id) = self.layout.hit_test(&self.tree, point) else {
            return Vec::new();
        };
        let mut keys = Vec::new();
        loop {
            let style = &self.tree.node(id).style;
            if style.scroll {
                if let Some(key) = &self.nodes[id.index()].key {
                    keys.push(key.clone());
                }
            }
            let Some(parent) = self.tree.parent(id) else {
                return keys;
            };
            id = parent;
        }
    }

    /// The keyed engine-owned scroll container under a point, if any.
    #[must_use]
    pub fn scroll_key_at(&self, point: Point) -> Option<String> {
        self.scroll_keys_at(point).into_iter().next()
    }

    /// Replaces the view, lays it out, and damages only what changed.
    ///
    /// The signatures of the old tree have to be taken *before* the tree is
    /// replaced: `set_view` clears the nodes and the text runs, so comparing
    /// afterwards would compare the new tree with itself and find no change —
    /// leaving the old pixels on screen under the new content.
    pub fn set_view_and_layout(&mut self, view: &View, viewport: &Viewport) {
        let previous = self.signatures();
        self.set_view(view);
        self.layout_with_full(viewport.content(), viewport.full());
        self.damage_from_diff(previous);
    }

    /// The layout rect and content signature of every node, in tree order.
    ///
    /// Comparing these before and after a rebuild says exactly which parts of
    /// the screen changed, which is what lets the paint pass touch only those.
    fn signatures(&self) -> Vec<(Rect, u64)> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let rect = self.layout.rect(NodeId::from_raw(index as u32));
                (rect, node_signature(node, &self.texts))
            })
            .collect()
    }

    /// Damages the parts of the screen that a rebuild changed.
    fn damage_from_diff(&mut self, previous: Vec<(Rect, u64)>) {
        let current = self.signatures();
        let mut damage: Vec<Rect> = Vec::new();
        let mut moved_damage: Vec<Rect> = Vec::new();
        let mut moved: Vec<(usize, Rect, Rect)> = Vec::new();
        let mut changed = false;

        let count = current.len().max(previous.len());
        for index in 0..count {
            match (previous.get(index), current.get(index)) {
                (Some((old_rect, old_sig)), Some((rect, sig)))
                    if old_rect == rect && old_sig == sig => {}
                (Some((old_rect, old_sig)), Some((rect, sig))) => {
                    if old_sig == sig {
                        // Same content, moved: a candidate for the scroll blit.
                        moved.push((index, *old_rect, *rect));
                        moved_damage.push(*old_rect);
                        moved_damage.push(*rect);
                    } else {
                        changed = true;
                        damage.push(*old_rect);
                        damage.push(*rect);
                    }
                }
                (Some((rect, _)), None) => {
                    changed = true;
                    damage.push(*rect);
                }
                (None, Some((rect, _))) => {
                    changed = true;
                    damage.push(*rect);
                }
                (None, None) => {}
            }
        }
        for (rect, _) in previous.iter().skip(current.len()) {
            changed = true;
            damage.push(*rect);
        }

        if !changed && self.try_scroll(&current, &moved) {
            // A pure scroll: the paint pass translates the region and
            // repaints only the two bands, so the moved nodes' rects are not
            // damage at all.
            return;
        }
        self.damage.extend(damage);
        self.damage.extend(moved_damage);
    }

    /// Attempts scroll detection; `true` when a blittable scroll was found.
    fn try_scroll(&mut self, current: &[(Rect, u64)], moved: &[(usize, Rect, Rect)]) -> bool {
        self.scroll = None;
        self.detect_scroll(current, moved);
        self.scroll.is_some()
    }

    /// Detects a pure translation between the previous and current layouts.
    ///
    /// When every node whose rect changed kept its content and moved by the
    /// same integral offset — a scroll — the paint pass can move the previous
    /// frame's pixels instead of re-rasterising them, and only the exposed and
    /// vacated bands need repainting. Anything else (gaps between the moved
    /// nodes, another node crossing the region, a fractional offset at this
    /// density) falls back to repainting, which is always correct.
    fn detect_scroll(&mut self, current: &[(Rect, u64)], moved: &[(usize, Rect, Rect)]) {
        if moved.len() < 2 {
            return;
        }
        let (_, first_old, first_new) = moved[0];
        let dx = first_new.x - first_old.x;
        let dy = first_new.y - first_old.y;
        if dx.abs() < 1.0e-3 && dy.abs() < 1.0e-3 {
            return;
        }
        // Only axis-aligned translations have rectangle-shaped damage.
        if dx.abs() > 1.0e-3 && dy.abs() > 1.0e-3 {
            return;
        }
        for (_, old_rect, new_rect) in moved {
            if (new_rect.x - old_rect.x - dx).abs() > 1.0e-3
                || (new_rect.y - old_rect.y - dy).abs() > 1.0e-3
            {
                return;
            }
        }
        // The offset has to be a whole number of physical pixels or the copy
        // would blur the previous frame.
        // (The scale is not known here; paint converts it. A fractional
        // logical offset is still rejected by the integral check there.)
        // Two facts bound what a node actually painted last frame: its
        // clipping ancestors (a row under a scrolling container never painted
        // over the header above it) and the surface (overscan rows hang off
        // the canvas). Only the painted part of a node is translatable, so
        // the region and the tiling check work on those intersections.
        let surface = Rect::new(0.0, 0.0, self.surface_width(), self.surface_height());
        let clipped: Vec<Rect> = moved
            .iter()
            .map(|(index, old, _)| old.intersect(surface).intersect(self.clip_of(*index)))
            .filter(|rect| !rect.is_empty())
            .collect();
        if clipped.len() < 2 {
            return;
        }
        let region = clipped
            .iter()
            .copied()
            .reduce(union_rect)
            .expect("at least two clipped rects");
        // The top-level moved nodes must tile the region: no gaps, no
        // overlaps. A node contained in another moved node (a text cell
        // inside its row) does not count against tiling, because its pixels
        // live inside the containing node's band and move with it.
        let area: f32 = clipped
            .iter()
            .enumerate()
            .filter(|(i, old)| {
                !clipped.iter().enumerate().any(|(j, other)| {
                    j != *i
                        && other.contains_rect(**old)
                        && (other.w * other.h > old.w * old.h || j < *i)
                })
            })
            .map(|(_, old)| old.w * old.h)
            .sum();
        if (area - region.w * region.h).abs() > 0.5 {
            // The moved nodes leave gaps or overlap: translating the region
            // would drag the wrong pixels along.
            return;
        }
        let new_region = region.translated(dx, dy);
        if new_region.x < 0.0 || new_region.y < 0.0 {
            return;
        }
        // No other paintable node may cross the region, or its pixels would
        // be dragged along with the scroll. A node that fully contains the
        // region is an ancestor whose fill lies underneath, and translating
        // the region is exactly what it expects.
        let moved_indices: Vec<usize> = moved.iter().map(|(index, _, _)| *index).collect();
        for (index, (rect, _)) in current.iter().enumerate() {
            if moved_indices.contains(&index) || rect.is_empty() {
                continue;
            }
            let node = &self.nodes[index];
            let paints = node.props.background.is_some()
                || node.text.is_some()
                || node.icon.is_some()
                || node.input.is_some();
            if paints && rect.intersects(region) && !rect.contains_rect(region) {
                return;
            }
        }

        // The scroll entry carries the bands; the paint pass clears and
        // repaints exactly them, and the region in between is translated.
        self.scroll = Some((region, dx, dy));
    }

    /// The two bands a scroll repaints: the vacated band loses its content,
    /// the exposed band gains the content that scrolled into view.
    fn scroll_bands(region: Rect, dx: f32, dy: f32) -> Vec<Rect> {
        let bands = if dy.abs() > 1.0e-3 {
            let amount = dy.abs();
            let (vacated_y, exposed_y) = if dy > 0.0 {
                (region.y, region.y + region.h)
            } else {
                (region.y + region.h + dy, region.y + dy)
            };
            vec![
                Rect::new(region.x, vacated_y, region.w, amount),
                Rect::new(region.x, exposed_y, region.w, amount),
            ]
        } else {
            let amount = dx.abs();
            let (vacated_x, exposed_x) = if dx > 0.0 {
                (region.x, region.x + region.w)
            } else {
                (region.x + region.w + dx, region.x + dx)
            };
            vec![
                Rect::new(vacated_x, region.y, amount, region.h),
                Rect::new(exposed_x, region.y, amount, region.h),
            ]
        };
        bands
            .into_iter()
            .filter(|rect| rect.w > 0.0 && rect.h > 0.0)
            .collect()
    }

    /// The effective clip of a node: the intersection of the layout rects of
    /// its clipping ancestors, bounded by the surface.
    fn clip_of(&self, index: usize) -> Rect {
        let mut clip = Rect::new(0.0, 0.0, self.surface_width(), self.surface_height());
        let mut node = mre_layout::NodeId::from_raw(index as u32);
        while let Some(parent) = self.tree.parent(node) {
            if self.nodes[parent.index()].props.clip {
                clip = clip.intersect(self.layout.rect(parent));
            }
            node = parent;
        }
        clip
    }

    /// The canvas width in logical pixels, for scroll-detection bounds.
    fn surface_width(&self) -> f32 {
        self.layout
            .rect(mre_layout::NodeId::from_raw(1))
            .right()
            .max(1.0)
    }

    /// The canvas height in logical pixels, for scroll-detection bounds.
    fn surface_height(&self) -> f32 {
        self.layout
            .rect(mre_layout::NodeId::from_raw(1))
            .bottom()
            .max(1.0)
    }

    /// Draws the current layout into `backend`.
    ///
    /// Painting is a single preorder walk. Nodes that clip push a clip rectangle
    /// before their children and pop it after, which is what makes a scrolling
    /// list unable to paint over its own header.
    ///
    /// Only the damaged region is touched, and the region that was painted is
    /// returned so the host can copy just that much to the screen.
    /// The backend may be the software reference rasteriser (golden images,
    /// headless rendering, machines without a GPU) or the wgpu renderer. The
    /// damage walk, clipping and scroll blit are identical for both, because
    /// they are properties of the scene, not of the device.
    pub fn paint(&mut self, backend: &mut dyn Backend) -> Option<Vec<Rect>> {
        // Only the damaged region is repainted. The rest of the canvas still
        // holds the previous frame, which is what makes a hover or a keystroke
        // cost what it changes rather than what the window is.
        let mut bands = self.take_damage(backend);

        // Clearing the whole canvas and then painting an opaque background over
        // it is two full-surface writes. When the app's root covers the whole
        // canvas with an opaque colour, the clear is the one that is redundant.
        //
        // "The whole canvas" means the physical surface, not the viewport: the
        // safe-area insets live outside the viewport and still have to be
        // painted.
        let scale = backend.scale();
        let app_root = mre_layout::NodeId::from_raw(1);
        let covered = self.nodes.get(1).is_some_and(|node| {
            node.props.background.is_some_and(|color| color.a() == 255)
                && node.props.radius <= 0.0
                && node.props.opacity >= 1.0
        }) && {
            let rect = self.layout.rect(app_root);
            rect.x <= 0.0
                && rect.y <= 0.0
                && rect.w * scale >= backend.width() as f32
                && rect.h * scale >= backend.height() as f32
        };

        // A detected scroll translates the previous frame's pixels instead of
        // re-rasterising them. The check needs the device scale, so it happens
        // here; if the offset is not a whole number of physical pixels, the
        // bands grow to cover the whole scrolled region and repaint it.
        if let Some((region, dx, dy)) = self.scroll.take() {
            let px = dx * scale;
            let py = dy * scale;
            let integral = (px - px.round()).abs() < 0.05 && (py - py.round()).abs() < 0.05;
            if integral && backend.copy_region(region, px.round() as i32, py.round() as i32) {
                // Blitted: only the bands need pixels.
                bands.extend(Self::scroll_bands(region, dx, dy));
            } else {
                // Not translatable: repaint the whole scrolled region once.
                bands.push(union_rect(region, region.translated(dx, dy)));
            }
        }
        if bands.is_empty() {
            return None;
        }

        // Clear only the true damage, but clip and repaint a slightly larger
        // band so antialiasing is not cut at the edge of the region.
        let paint_bands: Vec<Rect> = bands.iter().map(|band| band.expanded(AA_PAD)).collect();

        if !covered {
            for band in &bands {
                backend.clear_rect(*band, self.theme.background);
            }
        }

        let ops = self.paint_ops();
        let Self {
            tree,
            nodes,
            texts,
            layout,
            theme,
            hovered,
            pressed,
            focused,
            focus_visible,
            inputs,
            ..
        } = self;
        let hovered_key = hovered.as_deref();
        let pressed_key = pressed.as_deref();
        let focused_key = focused.as_deref();

        // One pass per band: the clip keeps every draw inside the band, and
        // nodes outside it are skipped before they cost anything.
        let mut painted = Vec::with_capacity(paint_bands.len());
        for band in &paint_bands {
            painted.push(*band);
            backend.push_clip(*band);
            for op in &ops {
                match op {
                    PaintOp::Node {
                        id,
                        dx,
                        dy,
                        opacity,
                    } => {
                        let mut rect = layout.rect(*id);
                        if rect.w <= 0.0 || rect.h <= 0.0 {
                            continue;
                        }
                        let node = &nodes[id.index()];
                        rect.x += *dx;
                        rect.y += *dy;

                        // A node outside the band paints nothing, and skipping
                        // early keeps its shaping and property work off the frame
                        // too. The clip would produce the same result later, but
                        // this is the cheap check.
                        if !rect.intersects(*band) {
                            continue;
                        }

                        if let Some(background) = node.props.background {
                            let hovered = node.props.interactive
                                && node.props.tint
                                && node
                                    .key
                                    .as_deref()
                                    .is_some_and(|key| Some(key) == hovered_key);
                            let pressed = node.props.interactive
                                && node.props.tint
                                && node
                                    .key
                                    .as_deref()
                                    .is_some_and(|key| Some(key) == pressed_key);
                            let background = interaction_tint(background, hovered, pressed);
                            let paint =
                                Paint::fill(with_opacity(background, node.props.opacity * opacity));
                            if is_circle(rect, node.props.radius) {
                                backend.fill_circle(
                                    Point::new(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5),
                                    rect.w * 0.5,
                                    paint,
                                );
                            } else {
                                backend.fill_rounded_rect(rect, node.props.radius, paint);
                            }
                        }

                        if let Some((border, width)) = node.props.border {
                            // The border follows the node's shape: a rounded
                            // card with a square outline shows its corners
                            // past the card's.
                            let paint =
                                Paint::fill(with_opacity(border, node.props.opacity * opacity));
                            if is_circle(rect, node.props.radius) {
                                backend.stroke_circle(
                                    Point::new(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5),
                                    rect.w * 0.5,
                                    width,
                                    paint,
                                );
                            } else {
                                backend.stroke_rounded_rect(rect, node.props.radius, width, paint);
                            }
                        }

                        if let Some(kind) = node.icon {
                            paint_icon(backend, rect, kind, node.props.color.unwrap_or(theme.text));
                        }

                        if let Some(input) = &node.input {
                            let key = node.key.as_deref();
                            let state = key.and_then(|key| inputs.get(key));
                            let focused = key.is_some() && key == focused.as_deref();
                            paint_input(
                                backend,
                                texts,
                                theme,
                                rect,
                                input,
                                state,
                                focused,
                                node.props.color,
                                node.props.radius,
                            );
                        } else if node.props.focusable
                            && *focus_visible
                            && node.key.as_deref().is_some()
                            && node.key.as_deref() == focused_key
                        {
                            // The focus ring follows the shape too.
                            backend.stroke_rounded_rect(
                                Rect::new(
                                    rect.x + 1.0,
                                    rect.y + 1.0,
                                    (rect.w - 2.0).max(0.0),
                                    (rect.h - 2.0).max(0.0),
                                ),
                                (node.props.radius - 1.0).max(0.0),
                                2.0,
                                Paint::fill(theme.accent),
                            );
                        }

                        if let Some(text_id) = node.text {
                            let size = texts
                                .style(text_id)
                                .map_or(theme.font_size, |style| style.size);
                            let color = with_opacity(
                                node.props.color.unwrap_or(theme.text),
                                node.props.opacity * opacity,
                            );
                            let padding = tree.node(*id).style.padding;
                            let width = (rect.w - padding.horizontal()).max(0.0);
                            let shaped = texts.shape(text_id, width);
                            // The line is optically centred: the line box is
                            // taller than the letters, so centring the box
                            // leaves them sitting high.
                            // The shape already carries the optical inset:
                            // the box is tall enough to hold the letters
                            // where they are drawn.
                            let inset = 0.0;
                            // The baseline is left where the layout put it.
                            // The rasteriser puts it on a whole device
                            // pixel, which is where the hinter fitted the
                            // letters' heights; two runs that share a
                            // baseline therefore share it exactly, and the
                            // letters of a word end on the same rows.
                            let origin_y = rect.y + padding.top + inset;
                            backend.draw_text(
                                texts.engine(),
                                &shaped,
                                size,
                                Point::new(rect.x + padding.left, origin_y),
                                color,
                            );
                        }
                    }
                    PaintOp::PushClip(rect) => backend.push_clip(*rect),
                    PaintOp::PopClip => backend.pop_clip(),
                }
            }
            backend.pop_clip();
        }
        Some(painted)
    }

    fn paint_ops(&self) -> Vec<PaintOp> {
        let mut ops = Vec::with_capacity(self.tree.len() * 2);
        self.walk_paint(self.tree.root(), &mut ops, 0.0, 0.0, 1.0);
        ops
    }

    fn walk_paint(&self, id: NodeId, ops: &mut Vec<PaintOp>, dx: f32, dy: f32, opacity: f32) {
        let node = self.nodes.get(id.index());
        let motion = node
            .and_then(|node| node.key.as_deref())
            .and_then(|key| self.animated.get(key))
            .copied()
            .unwrap_or_default();
        let dx = dx + motion.dx;
        let dy = dy + motion.dy;
        let opacity = opacity * motion.opacity.unwrap_or(1.0);

        ops.push(PaintOp::Node {
            id,
            dx,
            dy,
            opacity,
        });

        let clips = node.is_some_and(|node| node.props.clip);
        let rect = self.layout.rect(id);
        let clips = clips && rect.w > 0.0 && rect.h > 0.0;
        if clips {
            ops.push(PaintOp::PushClip(Rect::new(
                rect.x + dx,
                rect.y + dy,
                rect.w,
                rect.h,
            )));
        }
        for child in self.tree.children(id) {
            self.walk_paint(*child, ops, dx, dy, opacity);
        }
        if clips {
            ops.push(PaintOp::PopClip);
        }
    }

    /// Moves the pointer, updating hover state.
    ///
    /// Returns `true` when the hovered node changed, which means the next paint
    /// differs and a frame is due. This never re-renders a view.
    pub fn pointer_move(&mut self, point: Point) -> bool {
        let key = self.interactive_key_at(point);
        if key == self.hovered {
            return false;
        }
        let previous = self.hovered.clone();
        self.hovered = key;
        self.damage_key(previous.as_deref());
        let current = self.hovered.clone();
        self.damage_key(current.as_deref());
        true
    }

    /// Presses at a point. Returns `true` when the pressed node changed.
    pub fn pointer_down(&mut self, point: Point) -> bool {
        let key = self.interactive_key_at(point);
        let changed = key != self.pressed;
        let previous = self.pressed.clone();
        self.pressed = key;
        if changed {
            self.damage_key(previous.as_deref());
            let current = self.pressed.clone();
            self.damage_key(current.as_deref());
        }
        changed
    }

    /// Releases the pointer.
    ///
    /// Returns the key of the interactive node under the pointer, but only when
    /// the release happened over the same node that was pressed. Dragging off a
    /// button therefore cancels it, as it should.
    pub fn pointer_up(&mut self) -> Option<String> {
        let pressed = self.pressed.take();
        self.damage_key(pressed.as_deref());
        match (pressed, self.hovered.clone()) {
            (Some(pressed), Some(hovered)) if pressed == hovered => Some(pressed),
            _ => None,
        }
    }

    /// The key captured by the last press, regardless of where it was pressed.
    ///
    /// This is what a drag session follows: a slider keeps receiving movement
    /// even after the pointer leaves its bounds.
    #[must_use]
    pub fn pointer_up_target(&self) -> Option<String> {
        self.pressed.clone()
    }

    /// Gives keyboard focus to the focusable node at a point, if any.
    ///
    /// Returns `true` when the focus target changed. Clicking empty space
    /// blurs the current field.
    pub fn focus_at(&mut self, point: Point) -> bool {
        // Pointer focus is not keyboard focus: no focus ring after a click.
        // Repaint the old target if it had a keyboard ring.
        let was_visible = self.focus_visible;
        self.focus_visible = false;
        if was_visible {
            let current = self.focused.clone();
            self.damage_key(current.as_deref());
        }
        let key = self.focusable_key_at(point);
        if key == self.focused {
            return false;
        }
        let previous = self.focused.clone();
        self.focused = key;
        self.damage_key(previous.as_deref());
        let current = self.focused.clone();
        self.damage_key(current.as_deref());
        true
    }

    /// Focuses a node by key.
    pub fn focus(&mut self, key: &str) -> bool {
        if self.focused.as_deref() == Some(key) {
            return false;
        }
        let previous = self.focused.clone();
        self.focused = Some(key.to_string());
        self.damage_key(previous.as_deref());
        let current = self.focused.clone();
        self.damage_key(current.as_deref());
        true
    }

    /// Removes focus.
    pub fn clear_focus(&mut self) -> bool {
        let previous = self.focused.take();
        if previous.is_none() {
            return false;
        }
        self.damage_key(previous.as_deref());
        true
    }

    /// The focused key.
    #[must_use]
    pub fn focused(&self) -> Option<&str> {
        self.focused.as_deref()
    }

    /// True when the focus ring should be painted (keyboard navigation), as
    /// opposed to a pointer click that merely moved focus.
    #[must_use]
    pub const fn focus_visible(&self) -> bool {
        self.focus_visible
    }

    /// Advances the caret of the focused field by `dt` seconds.
    ///
    /// Returns `true` when the caret appeared or disappeared, which is the
    /// only thing that has to be repainted.
    pub fn tick_blink(&mut self, dt: f32) -> bool {
        let Some(key) = self.focused.clone() else {
            return false;
        };
        let Some(state) = self.inputs.get_mut(&key) else {
            return false;
        };
        if !state.tick_blink(dt) {
            return false;
        }
        // The caret changed; the field has to be painted again.
        if let Some(id) = self.node_by_key(&key) {
            let rect = self.layout.rect(id);
            self.damage(rect);
        }
        true
    }

    /// True when a field is focused, so the caret is blinking.
    #[must_use]
    pub fn has_focused_input(&self) -> bool {
        self.focused
            .as_deref()
            .is_some_and(|key| self.inputs.contains_key(key))
    }

    /// Brings the caret of the focused field back into view.
    pub fn restart_blink(&mut self) {
        let Some(key) = self.focused.clone() else {
            return;
        };
        if let Some(state) = self.inputs.get_mut(&key) {
            state.restart_blink();
        }
    }

    /// Whether the caret of a field is in its visible phase.
    #[must_use]
    pub fn caret_visible(&self, key: &str) -> bool {
        self.inputs
            .get(key)
            .is_none_or(|state| state.caret_visible())
    }

    /// Moves focus to the next (or previous) focusable node, in tree order.
    pub fn focus_next(&mut self, forward: bool) -> bool {
        self.focus_visible = true;
        let keys: Vec<&str> = self
            .nodes
            .iter()
            .filter(|node| node.props.focusable)
            .filter_map(|node| node.key.as_deref())
            .collect();
        if keys.is_empty() {
            return false;
        }
        let current = self
            .focused
            .as_deref()
            .and_then(|focused| keys.iter().position(|key| *key == focused));
        let next = match (current, forward) {
            (Some(index), true) => (index + 1) % keys.len(),
            (Some(index), false) => (index + keys.len() - 1) % keys.len(),
            (None, true) => 0,
            (None, false) => keys.len() - 1,
        };
        let key = keys[next].to_string();
        if self.focused.as_deref() == Some(key.as_str()) {
            return false;
        }
        self.focused = Some(key);
        true
    }

    /// The current contents of a field.
    #[must_use]
    pub fn input_text(&self, key: &str) -> Option<&str> {
        self.inputs.get(key).map(|state| state.text.as_str())
    }

    /// The caret of a field, in characters from the start.
    #[must_use]
    pub fn input_caret(&self, key: &str) -> Option<usize> {
        self.inputs.get(key).map(|state| state.caret)
    }

    /// The selected character range of a field, if anything is selected.
    #[must_use]
    pub fn input_selection(&self, key: &str) -> Option<(usize, usize)> {
        self.inputs.get(key).and_then(|state| state.selection())
    }

    /// Replaces the focused field's whole editor state.
    ///
    /// An input method owns the full text and selection; it sends them back as
    /// one unit on every change. This keeps the engine in step with a real
    /// `EditText` without trying to replay individual key events.
    pub fn replace_input(
        &mut self,
        text: &str,
        caret: usize,
        anchor: usize,
        preedit: &str,
    ) -> bool {
        let Some(key) = self.focused.clone() else {
            return false;
        };
        let Some(state) = self.inputs.get_mut(&key) else {
            return false;
        };
        let len = text.chars().count();
        let caret = caret.min(len);
        let anchor = anchor.min(len);
        if state.text == text
            && state.caret == caret
            && state.anchor == anchor
            && state.preedit == preedit
        {
            return false;
        }
        state.set_text(text);
        state.set_caret(caret, false);
        state.anchor = anchor;
        state.preedit = preedit.to_string();
        self.damage_key(Some(&key));
        true
    }

    /// Inserts text into the focused field.
    ///
    /// Returns `true` when the contents changed.
    pub fn insert_text(&mut self, text: &str) -> bool {
        let Some(key) = self.focused.clone() else {
            return false;
        };
        let Some(state) = self.inputs.get_mut(&key) else {
            return false;
        };
        let mut changed = state.insert(text);
        if !state.preedit.is_empty() {
            // Committed text replaces whatever was being composed.
            state.preedit.clear();
            changed = true;
        }
        if changed {
            self.damage_key(Some(&key));
        }
        changed
    }

    /// Applies an editing operation to the focused field.
    ///
    /// `extend` keeps the selection anchor, which is what Shift+arrow does.
    /// Returns `true` when the contents, the caret or the selection changed.
    pub fn edit(&mut self, op: EditOp, extend: bool) -> bool {
        let Some(key) = self.focused.clone() else {
            return false;
        };
        let Some(state) = self.inputs.get_mut(&key) else {
            return false;
        };
        let changed = match op {
            EditOp::Backspace => state.backspace(),
            EditOp::Delete => state.delete(),
            EditOp::Left => {
                if extend {
                    state.left_extend()
                } else {
                    state.left()
                }
            }
            EditOp::Right => {
                if extend {
                    state.right_extend()
                } else {
                    state.right()
                }
            }
            EditOp::Home => {
                if extend {
                    state.home_extend()
                } else {
                    state.home()
                }
            }
            EditOp::WordLeft => {
                if extend {
                    state.word_left_extend()
                } else {
                    state.word_left()
                }
            }
            EditOp::WordRight => {
                if extend {
                    state.word_right_extend()
                } else {
                    state.word_right()
                }
            }
            EditOp::DeleteWordBackward => state.delete_word_backward(),
            EditOp::DeleteWordForward => state.delete_word_forward(),
            EditOp::SelectAll => state.select_all(),
            EditOp::End => {
                if extend {
                    state.end_extend()
                } else {
                    state.end()
                }
            }
            EditOp::Clear => state.clear(),
        };
        if changed {
            self.damage_key(Some(&key));
        }
        changed
    }

    /// The key of the editable field at a point, if any.
    #[must_use]
    pub fn input_at(&self, point: Point) -> Option<&str> {
        let mut node = self.hit_test(point)?;
        loop {
            let entry = self.nodes.get(node.index())?;
            if entry.input.is_some() {
                // Only a node that says so can be selected from.
                if entry.props.selectable {
                    if let Some(key) = &entry.key {
                        return Some(key);
                    }
                }
            }
            node = self.tree.parent(node)?;
        }
    }

    /// The selected text in the focused field, if any.
    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        let key = self.focused.as_deref()?;
        let state = self.inputs.get(key)?;
        let selected = state.selected();
        (!selected.is_empty()).then_some(selected)
    }

    /// Selects the whole focused field.
    pub fn select_all(&mut self) -> bool {
        let Some(key) = self.focused.clone() else {
            return false;
        };
        let changed = self
            .inputs
            .get_mut(&key)
            .is_some_and(|state| state.select_all());
        if changed {
            self.damage_key(Some(&key));
        }
        changed
    }

    /// Undoes the last edit in the focused field.
    ///
    /// Returns `true` when the contents changed.
    pub fn undo(&mut self) -> bool {
        let Some(key) = self.focused.clone() else {
            return false;
        };
        self.inputs.get_mut(&key).is_some_and(|state| state.undo())
    }

    /// Sets the text an input method is composing in the focused field.
    ///
    /// The composition is shown at the caret and is not part of the value until
    /// the input method commits it.
    ///
    /// Returns `true` when the composition changed.
    pub fn set_preedit(&mut self, text: &str) -> bool {
        let Some(key) = self.focused.clone() else {
            return false;
        };
        let Some(state) = self.inputs.get_mut(&key) else {
            return false;
        };
        if state.preedit == text {
            return false;
        }
        state.preedit = text.to_string();
        self.damage_key(Some(&key));
        true
    }

    /// Redoes the last undone edit in the focused field.
    ///
    /// Returns `true` when the contents changed.
    pub fn redo(&mut self) -> bool {
        let Some(key) = self.focused.clone() else {
            return false;
        };
        self.inputs.get_mut(&key).is_some_and(|state| state.redo())
    }

    /// Replaces the selection in the focused field.
    ///
    /// This is what paste calls; the engine does not own the platform clipboard.
    /// Returns `true` when the contents changed.
    pub fn paste(&mut self, text: &str) -> bool {
        self.insert_text(text)
    }

    /// Cuts the selection: returns it and removes it.
    pub fn cut(&mut self) -> Option<String> {
        let key = self.focused.clone()?;
        let state = self.inputs.get_mut(&key)?;
        let selected = state.selected();
        if selected.is_empty() {
            return None;
        }
        state.backspace();
        let key = self.focused.clone();
        self.damage_key(key.as_deref());
        Some(selected)
    }

    /// Places the caret at the nearest character boundary to a point.
    ///
    /// Points outside the field horizontally clamp to the first or last
    /// character, which is what makes drag-selection past the edge work.
    pub fn place_caret(&mut self, point: Point, extend: bool) -> bool {
        let Some(key) = self.focused.clone() else {
            return false;
        };
        let Some(index) = self
            .nodes
            .iter()
            .position(|node| node.key.as_deref() == Some(key.as_str()) && node.input.is_some())
        else {
            return false;
        };
        let rect = self.layout.rect(NodeId::from_raw(index as u32));
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return false;
        }
        let input = self.nodes[index].input.clone().expect("checked above");
        let (text, caret) = {
            let Some(state) = self.inputs.get(&key) else {
                return false;
            };
            (state.text.clone(), state.caret)
        };

        let display = if input.password {
            "\u{2022}".repeat(text.chars().count())
        } else {
            text
        };
        let style = TextStyle {
            size: input.size,
            wrap: Wrap::None,
            ..TextStyle::default()
        };
        let shaped = self.texts.engine_mut().shape(&display, &style, 1.0e6);
        let positions = caret_positions(&display, &shaped);

        let pad = 8.0;
        let available = (rect.w - pad * 2.0).max(1.0);
        let before = positions.get(caret).copied().unwrap_or(0.0);
        let offset = (before - available).max(0.0);
        let local = (point.x - (rect.x + pad) + offset).max(0.0);

        let mut best = 0;
        let mut best_distance = f32::INFINITY;
        for (index, x) in positions.iter().enumerate() {
            let distance = (x - local).abs();
            if distance < best_distance {
                best_distance = distance;
                best = index;
            }
        }

        let Some(state) = self.inputs.get_mut(&key) else {
            return false;
        };
        let changed = state.set_caret(best, extend);
        if changed {
            // The caret and the selection are painted inside the field.
            self.damage_key(Some(&key));
        }
        changed
    }

    /// Selects the word under the pointer in the focused field.
    ///
    /// This is what a double click does.
    pub fn select_word_at(&mut self, point: Point) -> bool {
        let Some(key) = self.focused.clone() else {
            return false;
        };
        let Some(index) = self.caret_index_at(&key, point) else {
            return false;
        };
        let Some(state) = self.inputs.get_mut(&key) else {
            return false;
        };
        let changed = state.select_word_at(index);
        if changed {
            self.damage_key(Some(&key));
        }
        changed
    }

    /// The character index under a point in a field.
    fn caret_index_at(&mut self, key: &str, point: Point) -> Option<usize> {
        let index = self
            .nodes
            .iter()
            .position(|node| node.key.as_deref() == Some(key) && node.input.is_some())?;
        let rect = self.layout.rect(NodeId::from_raw(index as u32));
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return None;
        }
        let input = self.nodes[index].input.clone()?;
        let (text, _) = {
            let state = self.inputs.get(key)?;
            (state.text.clone(), state.caret)
        };
        let display = if input.password {
            "\u{2022}".repeat(text.chars().count())
        } else {
            text
        };
        let style = TextStyle {
            size: input.size,
            wrap: Wrap::None,
            ..TextStyle::default()
        };
        let shaped = self.texts.engine_mut().shape(&display, &style, 1.0e6);
        let positions = caret_positions(&display, &shaped);
        let pad = 8.0;
        let available = (rect.w - pad * 2.0).max(1.0);
        let caret = self.inputs.get(key).map(|s| s.caret).unwrap_or(0);
        let before = positions.get(caret).copied().unwrap_or(0.0);
        let offset = (before - available).max(0.0);
        let local = (point.x - (rect.x + pad) + offset).max(0.0);

        let mut best = 0;
        let mut best_distance = f32::INFINITY;
        for (index, x) in positions.iter().enumerate() {
            let distance = (x - local).abs();
            if distance < best_distance {
                best_distance = distance;
                best = index;
            }
        }
        Some(best)
    }

    fn focusable_key_at(&self, point: Point) -> Option<String> {
        let mut node = self.hit_test(point)?;
        loop {
            let entry = self.nodes.get(node.index())?;
            if entry.props.focusable {
                if let Some(key) = &entry.key {
                    return Some(key.clone());
                }
            }
            node = self.tree.parent(node)?;
        }
    }

    /// The currently hovered key.
    #[must_use]
    pub fn hovered(&self) -> Option<&str> {
        self.hovered.as_deref()
    }

    /// The currently pressed key.
    #[must_use]
    pub fn pressed(&self) -> Option<&str> {
        self.pressed.as_deref()
    }

    fn interactive_key_at(&self, point: Point) -> Option<String> {
        let mut node = self.hit_test(point)?;
        loop {
            let entry = self.nodes.get(node.index())?;
            if entry.props.interactive {
                if let Some(key) = &entry.key {
                    return Some(key.clone());
                }
            }
            node = self.tree.parent(node)?;
        }
    }

    /// The first node with the given key.
    /// Re-applies a field's declared value to its editor state.
    ///
    /// While a field is focused the editor owns the buffer, which is what
    /// keeps typing and IME composition stable. Submitting is the moment the
    /// application takes control back — a form that clears itself on submit
    /// has to win — so the runtime calls this after a submit handler returns.
    /// It is a no-op when the declared value matches the buffer.
    pub fn apply_external_value(&mut self, key: &str) {
        let Some(value) = self
            .node_by_key(key)
            .and_then(|id| self.nodes[id.index()].input.as_ref())
            .map(|input| input.value.clone())
        else {
            return;
        };
        let Some(state) = self.inputs.get_mut(key) else {
            return;
        };
        if state.text == value {
            return;
        }
        state.set_text(value);
    }

    /// The first node with the given key.
    #[must_use]
    pub fn node_by_key(&self, key: &str) -> Option<NodeId> {
        self.nodes
            .iter()
            .position(|node| node.key.as_deref() == Some(key))
            .map(|index| NodeId::from_raw(index as u32))
    }

    /// The rect of the node with the given key, if laid out.
    #[must_use]
    pub fn rect_of_key(&self, key: &str) -> Option<Rect> {
        let id = self.node_by_key(key)?;
        let rect = self.layout.rect(id);
        (rect.w > 0.0 && rect.h > 0.0).then_some(rect)
    }

    /// The key of a node, if it declared one.
    #[must_use]
    pub fn key(&self, id: NodeId) -> Option<&str> {
        self.nodes
            .get(id.index())
            .and_then(|node| node.key.as_deref())
    }

    /// The deepest node under a point.
    #[must_use]
    pub fn hit_test(&self, point: Point) -> Option<NodeId> {
        self.layout.hit_test(&self.tree, point)
    }

    /// The visible size of the UI.
    #[must_use]
    pub fn size(&self) -> Size {
        self.layout.size()
    }
}

/// Lightens a dark fill and darkens a light one, so hover reads correctly on
/// every theme without widgets declaring a second colour.
fn interaction_tint(color: mre_core::Rgba8, hovered: bool, pressed: bool) -> mre_core::Rgba8 {
    let amount = if pressed {
        0.16
    } else if hovered {
        0.07
    } else {
        return color;
    };
    let luminance =
        0.299 * f32::from(color.r()) + 0.587 * f32::from(color.g()) + 0.114 * f32::from(color.b());
    let target = if luminance > 140.0 {
        mre_core::Rgba8::BLACK
    } else {
        mre_core::Rgba8::WHITE
    };
    let mix = |a: u8, b: u8| -> u8 {
        (f32::from(a) * (1.0 - amount) + f32::from(b) * amount).round() as u8
    };
    mre_core::Rgba8::from_rgba(
        mix(color.r(), target.r()),
        mix(color.g(), target.g()),
        mix(color.b(), target.b()),
        color.a(),
    )
}

#[allow(clippy::too_many_arguments)]
fn paint_input(
    backend: &mut dyn Backend,
    texts: &mut TextStore,
    theme: &Theme,
    rect: Rect,
    input: &TextInput,
    state: Option<&InputState>,
    focused: bool,
    colour: Option<mre_core::Rgba8>,
    radius: f32,
) {
    let text = state.map_or(input.value.as_str(), |state| state.text.as_str());
    let caret = state.map_or(text.chars().count(), |state| state.caret);
    let selection = state.and_then(InputState::selection);
    let preedit = state.map_or("", |state| state.preedit.as_str());

    if focused {
        // The focus ring follows the field's shape, like every other border.
        backend.stroke_rounded_rect(
            Rect::new(
                rect.x + 1.0,
                rect.y + 1.0,
                (rect.w - 2.0).max(0.0),
                (rect.h - 2.0).max(0.0),
            ),
            (radius - 1.0).max(0.0),
            2.0,
            Paint::fill(theme.accent),
        );
    }

    let style = TextStyle {
        size: input.size,
        wrap: Wrap::None,
        ..TextStyle::default()
    };

    // Password fields shape the bullets, never the value, so the real text
    // never reaches the glyph cache. A composition is shown as typed, because
    // the user is in the middle of writing it.
    let mut display = String::new();
    let mut preedit_start = 0_usize;
    let mut preedit_end = 0_usize;
    if preedit.is_empty() {
        if input.password {
            display.push_str(&"\u{2022}".repeat(text.chars().count()));
        } else {
            display.push_str(text);
        }
    } else {
        let chars: Vec<char> = text.chars().collect();
        let at = caret.min(chars.len());
        preedit_start = at;
        preedit_end = at + preedit.chars().count();
        if input.password {
            display.push_str(&"\u{2022}".repeat(at));
        } else {
            display.extend(chars[..at].iter());
        }
        display.push_str(preedit);
        if input.password {
            display.push_str(&"\u{2022}".repeat(chars.len() - at));
        } else {
            display.extend(chars[at..].iter());
        }
    }
    let pad = 8.0;
    let available = (rect.w - pad * 2.0).max(1.0);

    // The text and the placeholder are shaped separately. The caret's
    // position comes from the text, so an empty field shows the caret at the
    // start of the placeholder and not at its end.
    let shaped = if display.is_empty() {
        None
    } else {
        Some(texts.engine_mut().shape(&display, &style, 1.0e6))
    };
    let placeholder = if display.is_empty() && !input.placeholder.is_empty() {
        Some(texts.engine_mut().shape(&input.placeholder, &style, 1.0e6))
    } else {
        None
    };
    let (content, color) = if display.is_empty() {
        (input.placeholder.clone(), theme.text_muted)
    } else {
        (display.clone(), colour.unwrap_or(theme.text))
    };

    // The line is centred on the height the text engine gave it, not on an
    // assumed one: a font's line box is taller than its size, and centring
    // on the size leaves the text sitting high in the field.
    let line_height = shaped
        .as_ref()
        .or(placeholder.as_ref())
        .map_or(input.size * 1.3, |shaped| shaped.height);
    let mut top = rect.y + (rect.h - line_height) / 2.0;

    // The line is optically centred, like every other text in the engine.
    if let Some(shaped) = shaped.as_ref().or(placeholder.as_ref()) {
        let baseline = shaped.lines.first().map_or(0.0, |line| line.baseline);
        top += texts
            .engine()
            .optical_offset(&content, &style, shaped.height, baseline);
    }

    let positions = shaped
        .as_ref()
        .map(|shaped| caret_positions(&display, shaped))
        .unwrap_or_else(|| vec![0.0; display.chars().count() + 1]);

    let prefix_width = positions.get(caret).copied().unwrap_or(0.0);
    let offset = if focused {
        (prefix_width - available).max(0.0)
    } else {
        0.0
    };

    if !content.is_empty() {
        backend.push_clip(Rect::new(
            rect.x + pad,
            rect.y + 1.0,
            available,
            (rect.h - 2.0).max(1.0),
        ));

        if let Some((start, end)) = selection {
            let x0 = positions.get(start).copied().unwrap_or(0.0);
            let x1 = positions.get(end).copied().unwrap_or(x0);
            backend.fill_rect(
                Rect::new(
                    rect.x + pad + x0 - offset,
                    top,
                    (x1 - x0).max(0.0),
                    line_height,
                ),
                Paint::fill(with_opacity(theme.accent, 0.28)),
            );
        }

        // The text is drawn; when there is none, the placeholder takes its
        // place, in the muted colour.
        if let Some(shaped) = shaped.as_ref().or(placeholder.as_ref()) {
            backend.draw_text(
                texts.engine(),
                shaped,
                input.size,
                Point::new(rect.x + pad - offset, top),
                color,
            );
        }

        // A composition is underlined, so it is clear that it is not part of
        // the value yet.
        if focused && !preedit.is_empty() {
            let x0 = positions.get(preedit_start).copied().unwrap_or(0.0);
            let x1 = positions.get(preedit_end).copied().unwrap_or(x0);
            backend.fill_rect(
                Rect::new(
                    rect.x + pad + x0 - offset,
                    top + line_height - 2.0,
                    (x1 - x0).max(1.0),
                    1.0,
                ),
                Paint::fill(theme.text),
            );
        }
        backend.pop_clip();
    }

    // The caret blinks: it is only drawn in the visible half of its cycle.
    let caret_visible = state.is_none_or(|state| state.caret_visible());
    if focused && caret_visible {
        // While composing, the caret sits after the composition.
        let caret_index = if preedit.is_empty() {
            caret
        } else {
            preedit_end
        };
        let caret_x = rect.x + pad + positions.get(caret_index).copied().unwrap_or(0.0) - offset;
        // The caret is drawn in the field's own colour: the theme's text
        // colour is a light-theme black, which is invisible on a dark field.
        backend.fill_rect(
            Rect::new(caret_x, rect.y + rect.h * 0.25, 1.5, rect.h * 0.5),
            Paint::fill(colour.unwrap_or(theme.text)),
        );
    }
}

/// Mixes a value into a hash.
fn mix(hash: &mut u64, value: u64) {
    *hash ^= value;
    *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
}

/// Mixes bytes into a hash.
fn mix_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        mix(hash, u64::from(*byte));
    }
}

/// A hash of everything about a node that affects what it looks like.
///
/// Two nodes with the same signature paint the same pixels in the same place,
/// so a change of signature is a change of pixels.
fn node_signature(node: &UiNode, texts: &TextStore) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    if let Some(color) = node.props.background {
        mix(&mut hash, u64::from(color.0));
    }
    if let Some((color, width)) = node.props.border {
        mix(&mut hash, u64::from(color.0));
        mix(&mut hash, width.to_bits() as u64);
    }
    mix(&mut hash, node.props.radius.to_bits() as u64);
    if let Some(color) = node.props.color {
        mix(&mut hash, u64::from(color.0));
    }
    mix(&mut hash, node.props.opacity.to_bits() as u64);
    mix(&mut hash, u64::from(node.props.clip));
    mix(&mut hash, u64::from(node.props.interactive));
    if let Some(text) = node.text {
        mix_bytes(&mut hash, texts.text(text).as_bytes());
    }
    if let Some(input) = &node.input {
        mix_bytes(&mut hash, input.value.as_bytes());
        mix_bytes(&mut hash, input.placeholder.as_bytes());
        mix(&mut hash, u64::from(input.password));
    }
    if let Some(icon) = node.icon {
        mix(&mut hash, icon as u64);
    }
    hash
}

/// The smallest rectangle containing both.
fn union_rect(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = (a.x + a.w).max(b.x + b.w);
    let bottom = (a.y + a.h).max(b.y + b.h);
    Rect::new(x, y, right - x, bottom - y)
}

/// A rectangle moved by an offset.
fn translate_rect(rect: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(rect.x + dx, rect.y + dy, rect.w, rect.h)
}

/// Caret x positions by character index, including the end position.
fn caret_positions(text: &str, shaped: &mre_text::ShapedText) -> Vec<f32> {
    let len = text.chars().count();
    let mut positions: Vec<Option<f32>> = vec![None; len + 1];
    for glyph in &shaped.glyphs {
        let cluster = glyph.cluster as usize;
        if cluster > text.len() {
            continue;
        }
        let index = text[..cluster].chars().count();
        if index <= len && positions[index].is_none() {
            positions[index] = Some(glyph.x);
        }
    }
    positions[len] = Some(shaped.width);

    let mut out = Vec::with_capacity(len + 1);
    let mut last = 0.0;
    for position in positions {
        let x = position.unwrap_or(last);
        out.push(x);
        last = x;
    }
    out
}

/// Normalised line segments for an icon, with their stroke width.
///
/// Kept separate from painting so the geometry itself can be tested: every
/// point must be inside the icon box by at least half a stroke, otherwise the
/// antialiased ink gets clipped and the icon looks cut off.
pub(crate) fn icon_segments(kind: crate::view::IconKind, rect: Rect) -> (f32, Vec<(Point, Point)>) {
    use crate::view::IconKind;
    let size = rect.w.min(rect.h);
    let stroke = (size * 0.14).max(1.0);
    let half = stroke * 0.5;
    let span = (size - stroke).max(0.0);
    let point = |x: f32, y: f32| Point::new(rect.x + half + x * span, rect.y + half + y * span);
    let segments = match kind {
        IconKind::Check => vec![
            (point(0.12, 0.52), point(0.42, 0.80)),
            (point(0.42, 0.80), point(0.88, 0.20)),
        ],
        IconKind::Close => vec![
            (point(0.18, 0.18), point(0.82, 0.82)),
            (point(0.82, 0.18), point(0.18, 0.82)),
        ],
        IconKind::ChevronDown => vec![
            (point(0.18, 0.36), point(0.5, 0.66)),
            (point(0.5, 0.66), point(0.82, 0.36)),
        ],
        IconKind::ChevronRight => vec![
            (point(0.36, 0.18), point(0.66, 0.5)),
            (point(0.66, 0.5), point(0.36, 0.82)),
        ],
    };
    (stroke, segments)
}

fn paint_icon(
    backend: &mut dyn Backend,
    rect: Rect,
    kind: crate::view::IconKind,
    color: mre_core::Rgba8,
) {
    let (stroke, segments) = icon_segments(kind, rect);
    let paint = Paint::fill(color);
    for (from, to) in segments {
        backend.draw_line(from, to, stroke, paint);
    }
}

fn with_opacity(color: mre_core::Rgba8, opacity: f32) -> mre_core::Rgba8 {
    if (opacity - 1.0).abs() < f32::EPSILON {
        return color;
    }
    let alpha = (f32::from(color.a()) * opacity.clamp(0.0, 1.0)).round();
    mre_core::Rgba8::from_rgba(
        color.r(),
        color.g(),
        color.b(),
        u8::try_from(alpha as i32).unwrap_or(0),
    )
}
