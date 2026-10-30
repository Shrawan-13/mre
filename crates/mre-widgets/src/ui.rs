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

use crate::theme::Theme;
use crate::view::{Props, View, ViewKind};
use mre_core::{Point, Rect, Size, Viewport};
use mre_layout::{Align, Dim, Edges, Engine, Justify, Layout, NodeId, Style, TextId, Tree};
use mre_raster::{Canvas, Paint};
use mre_text::{TextStore, TextStyle};

/// One step of a paint walk.
enum PaintOp {
    /// Draw a node's own background, border and text.
    Node(NodeId),
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

        let root = self.tree.root();
        self.build(root, view);
    }

    fn build(&mut self, parent: NodeId, view: &View) -> NodeId {
        match &view.kind {
            ViewKind::Container => {
                let node = self.tree.add_child(parent, view.style.clone());
                self.nodes.push(UiNode {
                    props: view.props.clone(),
                    text: None,
                    key: view.key.clone(),
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
                        ..Props::default()
                    },
                    text: None,
                    key,
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
                });
                node
            }
            ViewKind::Spacer(_) => {
                let node = self.tree.add_child(parent, view.style.clone());
                self.nodes.push(UiNode {
                    props: view.props.clone(),
                    text: None,
                    key: view.key.clone(),
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
        self.layout(viewport.content());
    }

    /// Draws the current layout into `canvas`.
    ///
    /// Painting is a single preorder walk. Nodes that clip push a clip rectangle
    /// before their children and pop it after, which is what makes a scrolling
    /// list unable to paint over its own header.
    pub fn paint(&mut self, canvas: &mut Canvas) {
        canvas.clear(self.theme.background);

        let ops = self.paint_ops();
        let Self {
            nodes,
            texts,
            layout,
            theme,
            hovered,
            pressed,
            ..
        } = self;
        let hovered_key = hovered.as_deref();
        let pressed_key = pressed.as_deref();

        for op in ops {
            match op {
                PaintOp::Node(id) => {
                    let rect = layout.rect(id);
                    if rect.w <= 0.0 || rect.h <= 0.0 {
                        continue;
                    }
                    let node = &nodes[id.index()];

                    if let Some(background) = node.props.background {
                        let hovered = node.props.interactive
                            && node
                                .key
                                .as_deref()
                                .is_some_and(|key| Some(key) == hovered_key);
                        let pressed = node.props.interactive
                            && node
                                .key
                                .as_deref()
                                .is_some_and(|key| Some(key) == pressed_key);
                        let background = interaction_tint(background, hovered, pressed);
                        canvas.fill_rounded_rect(
                            rect,
                            node.props.radius,
                            Paint::fill(with_opacity(background, node.props.opacity)),
                        );
                    }

                    if let Some((border, width)) = node.props.border {
                        canvas.stroke_rect(
                            rect,
                            width,
                            Paint::fill(with_opacity(border, node.props.opacity)),
                        );
                    }

                    if let Some(text_id) = node.text {
                        let size = texts
                            .style(text_id)
                            .map_or(theme.font_size, |style| style.size);
                        let color = with_opacity(
                            node.props.color.unwrap_or(theme.text),
                            node.props.opacity,
                        );
                        let shaped = texts.shape(text_id, rect.w);
                        canvas.draw_text(
                            texts.engine(),
                            &shaped,
                            size,
                            Point::new(rect.x, rect.y),
                            color,
                        );
                    }
                }
                PaintOp::PushClip(rect) => canvas.push_clip(rect),
                PaintOp::PopClip => canvas.pop_clip(),
            }
        }
    }

    fn paint_ops(&self) -> Vec<PaintOp> {
        let mut ops = Vec::with_capacity(self.tree.len() * 2);
        self.walk_paint(self.tree.root(), &mut ops);
        ops
    }

    fn walk_paint(&self, id: NodeId, ops: &mut Vec<PaintOp>) {
        ops.push(PaintOp::Node(id));
        let clips = self
            .nodes
            .get(id.index())
            .is_some_and(|node| node.props.clip);
        let rect = self.layout.rect(id);
        let clips = clips && rect.w > 0.0 && rect.h > 0.0;
        if clips {
            ops.push(PaintOp::PushClip(rect));
        }
        for child in self.tree.children(id) {
            self.walk_paint(*child, ops);
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
        self.hovered = key;
        true
    }

    /// Presses at a point. Returns `true` when the pressed node changed.
    pub fn pointer_down(&mut self, point: Point) -> bool {
        let key = self.interactive_key_at(point);
        let changed = key != self.pressed;
        self.pressed = key;
        changed
    }

    /// Releases the pointer.
    ///
    /// Returns the key of the interactive node under the pointer, but only when
    /// the release happened over the same node that was pressed. Dragging off a
    /// button therefore cancels it, as it should.
    pub fn pointer_up(&mut self) -> Option<String> {
        let pressed = self.pressed.take();
        match (pressed, self.hovered.clone()) {
            (Some(pressed), Some(hovered)) if pressed == hovered => Some(pressed),
            _ => None,
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
