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

/// Per-node state that layout does not need.
#[derive(Clone, Debug, Default)]
pub struct UiNode {
    /// Paint properties.
    pub props: Props,
    /// The text run, for text and button-label nodes.
    pub text: Option<TextId>,
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
                self.nodes.push(UiNode {
                    props: Props {
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
                });
                node
            }
            ViewKind::Spacer(_) => {
                let node = self.tree.add_child(parent, view.style.clone());
                self.nodes.push(UiNode {
                    props: view.props.clone(),
                    text: None,
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
    pub fn paint(&mut self, canvas: &mut Canvas) {
        canvas.clear(self.theme.background);

        let ids: Vec<NodeId> = self.tree.preorder(self.tree.root()).collect();
        for id in ids {
            let rect = self.layout.rect(id);
            if rect.w <= 0.0 || rect.h <= 0.0 {
                continue;
            }
            let node = &self.nodes[id.index()];

            if let Some(background) = node.props.background {
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
                let size = self
                    .texts
                    .style(text_id)
                    .map_or(self.theme.font_size, |style| style.size);
                let color = with_opacity(
                    node.props.color.unwrap_or(self.theme.text),
                    node.props.opacity,
                );
                let shaped = self.texts.shape(text_id, rect.w);
                canvas.draw_text(
                    self.texts.engine(),
                    &shaped,
                    size,
                    Point::new(rect.x, rect.y),
                    color,
                );
            }
        }
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
