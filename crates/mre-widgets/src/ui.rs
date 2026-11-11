//! Turning views into geometry and pixels.
//!
//! [`Ui`] owns the layout tree, the text store, the layout cache and the theme.
//! Its three operations are deliberately separate:
//!
//! * [`Ui::set_view`] rebuilds the node tree from a declarative [`View`],
//! * [`Ui::layout`] computes geometry for a viewport,
//! * [`Ui::paint`] draws into a canvas.
//!
//! A resize only calls `layout`; a theme change only calls `paint`. Nothing does
//! work it was not asked to do.

use crate::input::InputState;
use crate::theme::Theme;
use crate::view::{Props, TextInput, View, ViewKind};
use mre_core::{Point, Rect, Size, Viewport};
use mre_layout::{Align, Dim, Edges, Engine, Justify, Layout, NodeId, Style, TextId, Tree};
use mre_raster::{Canvas, Paint};
use mre_text::{TextStore, TextStyle, Wrap};
use std::collections::HashMap;

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
    inputs: HashMap<String, InputState>,
    animated: HashMap<String, Animated>,
    /// Regions that need repainting, in logical pixels.
    damage: Vec<Rect>,
    /// Whether the whole surface needs repainting.
    full_damage: bool,
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
            inputs: HashMap::new(),
            animated: HashMap::new(),
            damage: Vec::new(),
            full_damage: true,
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
    }

    /// The accessibility tree, in paint order.
    ///
    /// Roles come from the widget kind unless a view overrides them, and labels
    /// are drawn from text runs so a screen reader never sees an unnamed
    /// control. This is the surface a platform accessibility bridge consumes.
    #[must_use]
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

    /// The region that needs repainting, in logical pixels.
    ///
    /// `None` means nothing changed since the last paint, and the paint pass
    /// does no work at all. A full repaint covers the whole canvas, not just
    /// the viewport: the safe-area insets live outside the viewport and have
    /// to be painted too.
    fn take_damage(&mut self, canvas: &Canvas) -> Option<Rect> {
        if self.full_damage {
            self.full_damage = false;
            self.damage.clear();
            let scale = canvas.scale();
            return Some(Rect::new(
                0.0,
                0.0,
                canvas.width() as f32 / scale,
                canvas.height() as f32 / scale,
            ));
        }
        let first = self.damage.first().copied()?;
        let mut region = first;
        for rect in &self.damage[1..] {
            region = union_rect(region, *rect);
        }
        self.damage.clear();
        Some(region)
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
        self.viewport = viewport;
        let Self {
            tree,
            texts,
            engine,
            layout,
            ..
        } = self;
        *layout = engine.layout(tree, viewport, texts);
    }

    /// Lays out against a [`Viewport`], which is the platform-facing entry
    /// point: it applies safe-area insets and density for you.
    pub fn layout_viewport(&mut self, viewport: &Viewport) {
        let previous = self.signatures();
        self.layout(viewport.content());
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
        for (index, (rect, signature)) in current.iter().enumerate() {
            match previous.get(index) {
                Some((old_rect, old_signature))
                    if old_rect == rect && old_signature == signature => {}
                Some((old_rect, _)) => {
                    self.damage(*old_rect);
                    self.damage(*rect);
                }
                None => self.damage(*rect),
            }
        }
        for (rect, _) in previous.iter().skip(current.len()) {
            self.damage(*rect);
        }
    }

    /// Draws the current layout into `canvas`.
    ///
    /// Painting is a single preorder walk. Nodes that clip push a clip rectangle
    /// before their children and pop it after, which is what makes a scrolling
    /// list unable to paint over its own header.
    ///
    /// Only the damaged region is touched, and the region that was painted is
    /// returned so the host can copy just that much to the screen.
    pub fn paint(&mut self, canvas: &mut Canvas) -> Option<Rect> {
        // Only the damaged region is repainted. The rest of the canvas still
        // holds the previous frame, which is what makes a hover or a keystroke
        // cost what it changes rather than what the window is.
        let damage = self.take_damage(canvas)?;

        // Clearing the whole canvas and then painting an opaque background over
        // it is two full-surface writes. When the app's root covers the whole
        // canvas with an opaque colour, the clear is the one that is redundant.
        //
        // "The whole canvas" means the physical surface, not the viewport: the
        // safe-area insets live outside the viewport and still have to be
        // painted.
        let scale = canvas.scale();
        let app_root = mre_layout::NodeId::from_raw(1);
        let covered = self.nodes.get(1).is_some_and(|node| {
            node.props.background.is_some_and(|color| color.a() == 255)
                && node.props.radius <= 0.0
                && node.props.opacity >= 1.0
        }) && {
            let rect = self.layout.rect(app_root);
            rect.x <= 0.0
                && rect.y <= 0.0
                && rect.w * scale >= canvas.width() as f32
                && rect.h * scale >= canvas.height() as f32
        };
        if !covered {
            canvas.clear_rect(damage, self.theme.background);
        }
        canvas.push_clip(damage);

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
            inputs,
            ..
        } = self;
        let hovered_key = hovered.as_deref();
        let pressed_key = pressed.as_deref();
        let focused_key = focused.as_deref();

        for op in ops {
            match op {
                PaintOp::Node {
                    id,
                    dx,
                    dy,
                    opacity,
                } => {
                    let mut rect = layout.rect(id);
                    if rect.w <= 0.0 || rect.h <= 0.0 {
                        continue;
                    }
                    let node = &nodes[id.index()];
                    rect.x += dx;
                    rect.y += dy;

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
                        canvas.fill_rounded_rect(
                            rect,
                            node.props.radius,
                            Paint::fill(with_opacity(background, node.props.opacity * opacity)),
                        );
                    }

                    if let Some((border, width)) = node.props.border {
                        canvas.stroke_rect(
                            rect,
                            width,
                            Paint::fill(with_opacity(border, node.props.opacity * opacity)),
                        );
                    }

                    if let Some(kind) = node.icon {
                        paint_icon(canvas, rect, kind, node.props.color.unwrap_or(theme.text));
                    }

                    if let Some(input) = &node.input {
                        let key = node.key.as_deref();
                        let state = key.and_then(|key| inputs.get(key));
                        let focused = key.is_some() && key == focused.as_deref();
                        paint_input(canvas, texts, theme, rect, input, state, focused);
                    } else if node.props.focusable
                        && node.key.as_deref().is_some()
                        && node.key.as_deref() == focused_key
                    {
                        canvas.stroke_rect(
                            Rect::new(
                                rect.x + 1.0,
                                rect.y + 1.0,
                                (rect.w - 2.0).max(0.0),
                                (rect.h - 2.0).max(0.0),
                            ),
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
                        let padding = tree.node(id).style.padding;
                        let width = (rect.w - padding.horizontal()).max(0.0);
                        let shaped = texts.shape(text_id, width);
                        canvas.draw_text(
                            texts.engine(),
                            &shaped,
                            size,
                            Point::new(rect.x + padding.left, rect.y + padding.top),
                            color,
                        );
                    }
                }
                PaintOp::PushClip(rect) => canvas.push_clip(rect),
                PaintOp::PopClip => canvas.pop_clip(),
            }
        }
        canvas.pop_clip();
        Some(damage)
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

    /// Moves focus to the next (or previous) focusable node, in tree order.
    pub fn focus_next(&mut self, forward: bool) -> bool {
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
                if let Some(key) = &entry.key {
                    return Some(key);
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
    canvas: &mut Canvas,
    texts: &mut TextStore,
    theme: &Theme,
    rect: Rect,
    input: &TextInput,
    state: Option<&InputState>,
    focused: bool,
) {
    let text = state.map_or(input.value.as_str(), |state| state.text.as_str());
    let caret = state.map_or(text.chars().count(), |state| state.caret);
    let selection = state.and_then(InputState::selection);
    let preedit = state.map_or("", |state| state.preedit.as_str());

    if focused {
        canvas.stroke_rect(
            Rect::new(
                rect.x + 1.0,
                rect.y + 1.0,
                (rect.w - 2.0).max(0.0),
                (rect.h - 2.0).max(0.0),
            ),
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
    let (content, color) = if display.is_empty() {
        (input.placeholder.clone(), theme.text_muted)
    } else {
        (display.clone(), theme.text)
    };

    let pad = 8.0;
    let available = (rect.w - pad * 2.0).max(1.0);
    let top = rect.y + (rect.h - input.size * 1.3) / 2.0;
    let line_height = input.size * 1.3;

    let shaped = if content.is_empty() {
        None
    } else {
        Some(texts.engine_mut().shape(&content, &style, 1.0e6))
    };
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
        canvas.push_clip(Rect::new(
            rect.x + pad,
            rect.y + 1.0,
            available,
            (rect.h - 2.0).max(1.0),
        ));

        if let Some((start, end)) = selection {
            let x0 = positions.get(start).copied().unwrap_or(0.0);
            let x1 = positions.get(end).copied().unwrap_or(x0);
            canvas.fill_rect(
                Rect::new(
                    rect.x + pad + x0 - offset,
                    top,
                    (x1 - x0).max(0.0),
                    line_height,
                ),
                Paint::fill(with_opacity(theme.accent, 0.28)),
            );
        }

        if let Some(shaped) = &shaped {
            canvas.draw_text(
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
            canvas.fill_rect(
                Rect::new(
                    rect.x + pad + x0 - offset,
                    top + line_height - 2.0,
                    (x1 - x0).max(1.0),
                    1.0,
                ),
                Paint::fill(theme.text),
            );
        }
        canvas.pop_clip();
    }

    if focused {
        // While composing, the caret sits after the composition.
        let caret_index = if preedit.is_empty() {
            caret
        } else {
            preedit_end
        };
        let caret_x = rect.x + pad + positions.get(caret_index).copied().unwrap_or(0.0) - offset;
        canvas.fill_rect(
            Rect::new(caret_x, rect.y + rect.h * 0.25, 1.5, rect.h * 0.5),
            Paint::fill(theme.text),
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

fn paint_icon(
    canvas: &mut Canvas,
    rect: Rect,
    kind: crate::view::IconKind,
    color: mre_core::Rgba8,
) {
    use crate::view::IconKind;
    let size = rect.w.min(rect.h);
    let stroke = (size * 0.16).max(1.0);
    let paint = Paint::fill(color);
    let point = |x: f32, y: f32| Point::new(rect.x + x * size, rect.y + y * size);
    match kind {
        IconKind::Check => {
            canvas.draw_line(point(0.18, 0.52), point(0.42, 0.76), stroke, paint);
            canvas.draw_line(point(0.42, 0.76), point(0.84, 0.26), stroke, paint);
        }
        IconKind::Close => {
            canvas.draw_line(point(0.24, 0.24), point(0.76, 0.76), stroke, paint);
            canvas.draw_line(point(0.76, 0.24), point(0.24, 0.76), stroke, paint);
        }
        IconKind::ChevronDown => {
            canvas.draw_line(point(0.24, 0.38), point(0.5, 0.64), stroke, paint);
            canvas.draw_line(point(0.5, 0.64), point(0.76, 0.38), stroke, paint);
        }
        IconKind::ChevronRight => {
            canvas.draw_line(point(0.38, 0.24), point(0.64, 0.5), stroke, paint);
            canvas.draw_line(point(0.64, 0.5), point(0.38, 0.76), stroke, paint);
        }
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
