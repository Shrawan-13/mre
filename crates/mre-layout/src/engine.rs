//! The layout engine: measure, then arrange.
//!
//! Two passes, one cache.
//!
//! * **Measure** answers "how big would this be, given at most this much room?"
//!   It is recursive, memoised by `(node, constraints)`, and is where intrinsic
//!   sizing lives.
//! * **Arrange** answers "where exactly does everything go?" given a definite
//!   rectangle. It re-measures children against their final slots; the cache
//!   makes that nearly free.
//!
//! The cache is the reason a 50k-row table does not re-measure 50k rows. It is
//! keyed on quantised constraints (1/64 logical px), so float noise does not
//! silently destroy the hit rate.

use crate::style::{Align, Axis, Display, Edges, Justify, Position, Track};
use crate::tree::{Content, NodeId, TextId, Tree};
use mre_core::{Rect, Size};
use std::collections::HashMap;

/// Box constraints: a range for each axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraints {
    /// Minimum width.
    pub min_w: f32,
    /// Maximum width. May be infinite.
    pub max_w: f32,
    /// Minimum height.
    pub min_h: f32,
    /// Maximum height. May be infinite.
    pub max_h: f32,
}

impl Constraints {
    /// Unbounded in both axes.
    pub const UNBOUNDED: Self = Self {
        min_w: 0.0,
        max_w: f32::INFINITY,
        min_h: 0.0,
        max_h: f32::INFINITY,
    };

    /// A fixed size on both axes.
    #[must_use]
    pub const fn tight(size: Size) -> Self {
        Self {
            min_w: size.w,
            max_w: size.w,
            min_h: size.h,
            max_h: size.h,
        }
    }

    /// At most this size, at least zero.
    #[must_use]
    pub const fn loose(size: Size) -> Self {
        Self {
            min_w: 0.0,
            max_w: size.w,
            min_h: 0.0,
            max_h: size.h,
        }
    }

    /// Clamps a size into the constraint range.
    #[must_use]
    pub fn constrain(self, size: Size) -> Size {
        Size::new(
            size.w.clamp(self.min_w, self.max_w.max(self.min_w)),
            size.h.clamp(self.min_h, self.max_h.max(self.min_h)),
        )
    }

    /// Shrinks the constraints by padding.
    #[must_use]
    pub fn deflate(self, padding: Edges) -> Self {
        Self {
            min_w: (self.min_w - padding.horizontal()).max(0.0),
            max_w: (self.max_w - padding.horizontal()).max(0.0),
            min_h: (self.min_h - padding.vertical()).max(0.0),
            max_h: (self.max_h - padding.vertical()).max(0.0),
        }
    }
}

/// Measures a text run.
///
/// Implemented by `mre-text`. Keeping it a trait means the layout crate has no
/// dependency on font shaping, and tests can supply exact numbers.
pub trait TextMetrics {
    /// Measures `text` wrapped to at most `max_width` logical pixels.
    fn measure(&mut self, text: TextId, max_width: f32) -> Size;
}

/// Counters for one or more layout passes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Measure calls that missed the cache.
    pub measures: u64,
    /// Measure calls served from the cache.
    pub cache_hits: u64,
    /// Completed layout passes.
    pub layouts: u64,
}

impl Stats {
    /// Fraction of measure calls served from cache, in `0.0..=1.0`.
    #[must_use]
    pub fn hit_rate(&self) -> f32 {
        let total = self.measures + self.cache_hits;
        if total == 0 {
            0.0
        } else {
            self.cache_hits as f32 / total as f32
        }
    }
}

/// Geometry produced by a layout pass.
#[derive(Clone, Debug)]
pub struct Layout {
    rects: Vec<Rect>,
    size: Size,
}

impl Layout {
    /// An empty layout with no rects. Useful as the initial state of a UI that
    /// has not been laid out yet.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            rects: Vec::new(),
            size: Size::ZERO,
        }
    }

    /// The rect assigned to a node, in viewport coordinates.
    #[must_use]
    pub fn rect(&self, id: NodeId) -> Rect {
        self.rects.get(id.index()).copied().unwrap_or(Rect::EMPTY)
    }

    /// The rect assigned to a node, or `None` if it was never visited.
    #[must_use]
    pub fn get(&self, id: NodeId) -> Option<Rect> {
        self.rects.get(id.index()).copied()
    }

    /// The laid-out size of the root.
    #[must_use]
    pub const fn size(&self) -> Size {
        self.size
    }

    /// All rects, indexed by `NodeId::index`.
    #[must_use]
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }

    /// The deepest node whose bounds contain `point`, in paint order.
    ///
    /// Later siblings are painted on top, so the search runs backwards. This is
    /// the primitive every pointer event needs.
    #[must_use]
    pub fn hit_test(&self, tree: &Tree, point: mre_core::Point) -> Option<NodeId> {
        self.hit_test_from(tree, tree.root(), point)
    }

    fn hit_test_from(&self, tree: &Tree, id: NodeId, point: mre_core::Point) -> Option<NodeId> {
        let rect = self.get(id)?;
        if !contains(rect, point) {
            return None;
        }
        let node = tree.node(id);
        if node.style.display == Display::None {
            return None;
        }
        for child in node.children.iter().rev() {
            if let Some(hit) = self.hit_test_from(tree, *child, point) {
                return Some(hit);
            }
        }
        Some(id)
    }
}

fn contains(rect: Rect, point: mre_core::Point) -> bool {
    point.x >= rect.x && point.y >= rect.y && point.x < rect.right() && point.y < rect.bottom()
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    node: u32,
    min_w: u32,
    max_w: u32,
    min_h: u32,
    max_h: u32,
}

fn quantize(value: f32) -> u32 {
    if !value.is_finite() {
        u32::MAX
    } else {
        (value.max(0.0) * 64.0).round().min(u32::MAX as f32) as u32
    }
}

impl CacheKey {
    fn new(node: NodeId, constraints: Constraints) -> Self {
        Self {
            node: node.raw(),
            min_w: quantize(constraints.min_w),
            max_w: quantize(constraints.max_w),
            min_h: quantize(constraints.min_h),
            max_h: quantize(constraints.max_h),
        }
    }
}

/// The layout engine.
///
/// Reusable across frames and across trees. Call [`Engine::invalidate`] when a
/// node's style or text changes; call [`Engine::clear`] when the tree structure
/// changes materially.
#[derive(Default)]
pub struct Engine {
    cache: HashMap<CacheKey, Size>,
    stats: Stats,
}

impl Engine {
    /// A fresh engine with an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops every cached measurement.
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Drops the cached measurement for one subtree.
    ///
    /// This is the incremental path: changing one node's text should not force a
    /// full-tree re-measure.
    pub fn invalidate(&mut self, tree: &Tree, id: NodeId) {
        for node in tree.preorder(id) {
            self.cache.retain(|key, _| key.node != node.raw());
        }
    }

    /// Counters since the engine was created.
    #[must_use]
    pub const fn stats(&self) -> Stats {
        self.stats
    }

    /// Lays out `tree` inside `viewport`.
    ///
    /// The root fills the viewport. Rotation, resize and DPI changes are all
    /// expressed by calling this again with a different `viewport`.
    pub fn layout(&mut self, tree: &Tree, viewport: Rect, text: &mut dyn TextMetrics) -> Layout {
        let constraints = Constraints::tight(viewport.size());
        let size = self.measure(tree, tree.root(), constraints, text);
        let root_rect = Rect::new(viewport.x, viewport.y, size.w, size.h);

        let mut rects = vec![Rect::EMPTY; tree.len()];
        self.arrange(tree, tree.root(), root_rect, text, &mut rects);
        self.stats.layouts += 1;

        Layout { rects, size }
    }

    fn measure(
        &mut self,
        tree: &Tree,
        id: NodeId,
        constraints: Constraints,
        text: &mut dyn TextMetrics,
    ) -> Size {
        let key = CacheKey::new(id, constraints);
        if let Some(size) = self.cache.get(&key) {
            self.stats.cache_hits += 1;
            return *size;
        }
        self.stats.measures += 1;

        let node = tree.node(id);
        let style = &node.style;

        if style.display == Display::None {
            self.cache.insert(key, Size::ZERO);
            return Size::ZERO;
        }

        let mut width = style.width.resolve(constraints.max_w);
        let mut height = style.height.resolve(constraints.max_h);

        if let Some(ratio) = style.aspect_ratio {
            if ratio > 0.0 {
                if let (Some(w), None) = (width, height) {
                    height = Some(w / ratio);
                } else if let (None, Some(h)) = (width, height) {
                    width = Some(h * ratio);
                }
            }
        }

        let inner = Constraints {
            min_w: 0.0,
            max_w: (width.unwrap_or(constraints.max_w) - style.padding.horizontal()).max(0.0),
            min_h: 0.0,
            max_h: (height.unwrap_or(constraints.max_h) - style.padding.vertical()).max(0.0),
        };

        let content = match node.content {
            Content::Text(text_id) => text.measure(text_id, inner.max_w),
            Content::None => match style.display {
                Display::Flex => self.measure_flex(tree, id, inner, text),
                Display::Grid => self.measure_grid(tree, id, inner, text),
                Display::Stack => self.measure_stack(tree, id, inner, text),
                Display::None => Size::ZERO,
            },
        };

        let mut size = Size::new(
            width.unwrap_or(content.w + style.padding.horizontal()),
            height.unwrap_or(content.h + style.padding.vertical()),
        );

        let min_w = style.min_width.resolve(constraints.max_w).unwrap_or(0.0);
        let min_h = style.min_height.resolve(constraints.max_h).unwrap_or(0.0);
        let max_w = style
            .max_width
            .resolve(constraints.max_w)
            .unwrap_or(f32::INFINITY);
        let max_h = style
            .max_height
            .resolve(constraints.max_h)
            .unwrap_or(f32::INFINITY);
        size.w = size.w.clamp(min_w.min(max_w), max_w.max(min_w));
        size.h = size.h.clamp(min_h.min(max_h), max_h.max(min_h));

        size = constraints.constrain(size);
        self.cache.insert(key, size);
        size
    }

    fn measure_flex(
        &mut self,
        tree: &Tree,
        id: NodeId,
        inner: Constraints,
        text: &mut dyn TextMetrics,
    ) -> Size {
        let node = tree.node(id);
        let horizontal = node.style.direction.axis() == Axis::Horizontal;
        let gap = node.style.gap;
        let main_max = if horizontal { inner.max_w } else { inner.max_h };
        let cross_max = if horizontal { inner.max_h } else { inner.max_w };

        let mut main = 0.0_f32;
        let mut cross = 0.0_f32;
        let mut count = 0_usize;

        for &child in &node.children {
            let child_style = &tree.node(child).style;
            if child_style.position == Position::Absolute || child_style.display == Display::None {
                continue;
            }
            let basis = child_style.flex_basis.resolve(main_max);
            let constraints = match (basis, horizontal) {
                (Some(basis), true) => Constraints {
                    min_w: basis,
                    max_w: basis,
                    min_h: 0.0,
                    max_h: cross_max,
                },
                (Some(basis), false) => Constraints {
                    min_w: 0.0,
                    max_w: cross_max,
                    min_h: basis,
                    max_h: basis,
                },
                (None, true) => Constraints {
                    min_w: 0.0,
                    max_w: main_max,
                    min_h: 0.0,
                    max_h: cross_max,
                },
                (None, false) => Constraints {
                    min_w: 0.0,
                    max_w: cross_max,
                    min_h: 0.0,
                    max_h: main_max,
                },
            };
            let size = self.measure(tree, child, constraints, text);
            let (child_main, child_cross) = if horizontal {
                (size.w, size.h)
            } else {
                (size.h, size.w)
            };
            main += child_main + child_style.margin.main(horizontal);
            cross = cross.max(child_cross + child_style.margin.cross(horizontal));
            count += 1;
        }

        if count > 1 {
            main += gap * (count - 1) as f32;
        }

        if horizontal {
            Size::new(main, cross)
        } else {
            Size::new(cross, main)
        }
    }

    fn measure_stack(
        &mut self,
        tree: &Tree,
        id: NodeId,
        inner: Constraints,
        text: &mut dyn TextMetrics,
    ) -> Size {
        let node = tree.node(id);
        let mut width = 0.0_f32;
        let mut height = 0.0_f32;
        for &child in &node.children {
            let child_style = &tree.node(child).style;
            if child_style.display == Display::None {
                continue;
            }
            let constraints = Constraints {
                min_w: 0.0,
                max_w: inner.max_w,
                min_h: 0.0,
                max_h: inner.max_h,
            };
            let size = self.measure(tree, child, constraints, text);
            width = width.max(size.w + child_style.margin.horizontal());
            height = height.max(size.h + child_style.margin.vertical());
        }
        Size::new(width, height)
    }

    fn measure_grid(
        &mut self,
        tree: &Tree,
        id: NodeId,
        inner: Constraints,
        text: &mut dyn TextMetrics,
    ) -> Size {
        let (columns, rows) = self.grid_tracks(tree, id, inner.max_w, text);
        let gap = tree.node(id).style.gap;
        let width = columns.iter().sum::<f32>() + gap * columns.len().saturating_sub(1) as f32;
        let height = rows.iter().sum::<f32>() + gap * rows.len().saturating_sub(1) as f32;
        Size::new(width, height)
    }

    /// Resolves column widths and row heights for a grid.
    ///
    /// `Fr` rows are treated as `Auto`: a grid inside an auto-height container
    /// has no height to divide, and guessing produces worse layouts than
    /// content sizing.
    fn grid_tracks(
        &mut self,
        tree: &Tree,
        id: NodeId,
        available_width: f32,
        text: &mut dyn TextMetrics,
    ) -> (Vec<f32>, Vec<f32>) {
        let node = tree.node(id);
        let gap = node.style.gap;

        let flow: Vec<NodeId> = node
            .children
            .iter()
            .copied()
            .filter(|child| {
                let style = &tree.node(*child).style;
                style.position != Position::Absolute && style.display != Display::None
            })
            .collect();

        let column_count = if node.style.columns.is_empty() {
            flow.len().max(1)
        } else {
            node.style.columns.len()
        };
        let row_count = flow.len().div_ceil(column_count).max(1);

        let mut fixed = vec![None; column_count];
        let mut fraction = vec![0.0_f32; column_count];
        for (index, track) in node.style.columns.iter().enumerate().take(column_count) {
            match *track {
                Track::Px(value) => fixed[index] = Some(value),
                Track::Fr(value) => fraction[index] = value,
                Track::Auto => {}
            }
        }

        let mut content_width = vec![0.0_f32; column_count];
        let mut row_heights = vec![0.0_f32; row_count];

        for (index, &child) in flow.iter().enumerate() {
            let column = index % column_count;
            let row = index / column_count;
            if row >= row_count {
                break;
            }
            let child_style = &tree.node(child).style;
            let max_width = fixed[column].unwrap_or(f32::INFINITY);
            let size = self.measure(
                tree,
                child,
                Constraints {
                    min_w: 0.0,
                    max_w: max_width,
                    min_h: 0.0,
                    max_h: f32::INFINITY,
                },
                text,
            );
            content_width[column] =
                content_width[column].max(size.w + child_style.margin.horizontal());
            row_heights[row] = row_heights[row].max(size.h + child_style.margin.vertical());
        }

        let mut columns = vec![0.0_f32; column_count];
        let mut fixed_total = 0.0_f32;
        let mut fraction_total = 0.0_f32;
        for column in 0..column_count {
            if let Some(value) = fixed[column] {
                columns[column] = value;
                fixed_total += value;
            } else if fraction[column] > 0.0 {
                fraction_total += fraction[column];
            } else {
                columns[column] = content_width[column];
                fixed_total += content_width[column];
            }
        }

        let gaps = gap * column_count.saturating_sub(1) as f32;
        let leftover = (available_width - fixed_total - gaps).max(0.0);

        for column in 0..column_count {
            if fixed[column].is_none() && fraction[column] > 0.0 {
                columns[column] = if fraction_total > 0.0 && available_width.is_finite() {
                    leftover * fraction[column] / fraction_total
                } else {
                    content_width[column]
                };
            }
        }

        for (row, track) in node.style.rows.iter().enumerate().take(row_count) {
            if let Track::Px(value) = *track {
                row_heights[row] = value;
            }
        }

        (columns, row_heights)
    }

    fn arrange(
        &mut self,
        tree: &Tree,
        id: NodeId,
        rect: Rect,
        text: &mut dyn TextMetrics,
        out: &mut [Rect],
    ) {
        out[id.index()] = rect;
        let node = tree.node(id);
        let style = &node.style;
        if style.display == Display::None {
            return;
        }

        let inner = Rect::new(
            rect.x + style.padding.left,
            rect.y + style.padding.top,
            (rect.w - style.padding.horizontal()).max(0.0),
            (rect.h - style.padding.vertical()).max(0.0),
        );

        match style.display {
            Display::Flex => self.arrange_flex(tree, id, inner, text, out),
            Display::Grid => self.arrange_grid(tree, id, inner, text, out),
            Display::Stack => self.arrange_stack(tree, id, inner, text, out),
            Display::None => {}
        }
    }

    fn arrange_flex(
        &mut self,
        tree: &Tree,
        id: NodeId,
        inner: Rect,
        text: &mut dyn TextMetrics,
        out: &mut [Rect],
    ) {
        let node = tree.node(id);
        let horizontal = node.style.direction.axis() == Axis::Horizontal;
        let reverse = node.style.direction.is_reverse();
        let gap = node.style.gap;
        let align = node.style.align_items;
        let justify = node.style.justify_content;

        let main_inner = if horizontal { inner.w } else { inner.h };
        let cross_inner = if horizontal { inner.h } else { inner.w };

        let flow: Vec<NodeId> = node
            .children
            .iter()
            .copied()
            .filter(|child| {
                let style = &tree.node(*child).style;
                style.position != Position::Absolute && style.display != Display::None
            })
            .collect();

        let mut items: Vec<FlexItem> = Vec::with_capacity(flow.len());
        for &child in &flow {
            let style = &tree.node(child).style;
            let constraints = if horizontal {
                Constraints {
                    min_w: 0.0,
                    max_w: main_inner,
                    min_h: 0.0,
                    max_h: cross_inner,
                }
            } else {
                Constraints {
                    min_w: 0.0,
                    max_w: cross_inner,
                    min_h: 0.0,
                    max_h: main_inner,
                }
            };
            let size = self.measure(tree, child, constraints, text);
            let (main, cross) = if horizontal {
                (size.w, size.h)
            } else {
                (size.h, size.w)
            };
            items.push(FlexItem {
                id: child,
                main,
                cross,
                margin: style.margin,
                grow: style.flex_grow,
                shrink: style.flex_shrink,
            });
        }

        let count = items.len();
        let gap_total = gap * count.saturating_sub(1) as f32;
        let used = items
            .iter()
            .map(|item| item.main + item.margin.main(horizontal))
            .sum::<f32>()
            + gap_total;
        let free = main_inner - used;

        if free > 0.0 {
            let total_grow: f32 = items.iter().map(|item| item.grow).sum();
            if total_grow > 0.0 {
                for item in &mut items {
                    item.main += free * item.grow / total_grow;
                }
            }
        } else if free < 0.0 {
            let total_shrink: f32 = items.iter().map(|item| item.shrink * item.main).sum();
            if total_shrink > 0.0 {
                for item in &mut items {
                    let share = (-free) * (item.shrink * item.main) / total_shrink;
                    item.main = (item.main - share).max(0.0);
                }
            }
        }

        // Re-measure against the final main size so min/max, wrapping and text
        // reflow are honoured. This usually hits the cache.
        //
        // `align: stretch` only applies when the child's cross size is auto, the
        // same rule as CSS: an explicit width must not be overridden by the
        // container's alignment.
        for item in &mut items {
            let style = &tree.node(item.id).style;
            let cross_is_auto = if horizontal {
                style.height.resolve(cross_inner).is_none()
            } else {
                style.width.resolve(cross_inner).is_none()
            };
            let stretch = align == Align::Stretch && cross_is_auto;
            let cross_available = (cross_inner - item.margin.cross(horizontal)).max(0.0);
            let cross_constraint = if stretch {
                (cross_available, cross_available)
            } else {
                (0.0, cross_available)
            };
            let constraints = if horizontal {
                Constraints {
                    min_w: item.main,
                    max_w: item.main,
                    min_h: cross_constraint.0,
                    max_h: cross_constraint.1,
                }
            } else {
                Constraints {
                    min_w: cross_constraint.0,
                    max_w: cross_constraint.1,
                    min_h: item.main,
                    max_h: item.main,
                }
            };
            let size = self.measure(tree, item.id, constraints, text);
            let (main, cross) = if horizontal {
                (size.w, size.h)
            } else {
                (size.h, size.w)
            };
            item.main = main;
            item.cross = if stretch {
                cross_available
            } else {
                cross.min(cross_available)
            };
        }

        let used = items
            .iter()
            .map(|item| item.main + item.margin.main(horizontal))
            .sum::<f32>()
            + gap_total;
        let free = (main_inner - used).max(0.0);

        let (mut cursor, spacing) = match justify {
            Justify::Start => (0.0, gap),
            Justify::Center => (free / 2.0, gap),
            Justify::End => (free, gap),
            Justify::SpaceBetween => {
                if count > 1 {
                    (0.0, gap + free / (count - 1) as f32)
                } else {
                    (0.0, gap)
                }
            }
            Justify::SpaceAround => {
                if count > 0 {
                    let share = free / count as f32;
                    (share / 2.0, gap + share)
                } else {
                    (0.0, gap)
                }
            }
            Justify::SpaceEvenly => {
                if count > 0 {
                    let share = free / (count + 1) as f32;
                    (share, gap + share)
                } else {
                    (0.0, gap)
                }
            }
        };

        let mut positions = Vec::with_capacity(count);
        for item in &items {
            positions.push(cursor + item.margin.main_start(horizontal));
            cursor += item.main + item.margin.main(horizontal) + spacing;
        }

        for (index, item) in items.iter().enumerate() {
            let mut main_position = positions[index];
            if reverse {
                main_position = main_inner - main_position - item.main;
            }

            let cross_offset = match align {
                Align::Start | Align::Stretch => 0.0,
                Align::Center => (cross_inner - item.margin.cross(horizontal) - item.cross) / 2.0,
                Align::End => cross_inner - item.margin.cross(horizontal) - item.cross,
            };

            let cross_position = item.margin.cross_start(horizontal) + cross_offset;

            let (x, y, w, h) = if horizontal {
                (
                    inner.x + main_position,
                    inner.y + cross_position,
                    item.main,
                    item.cross,
                )
            } else {
                (
                    inner.x + cross_position,
                    inner.y + main_position,
                    item.cross,
                    item.main,
                )
            };

            self.arrange(tree, item.id, Rect::new(x, y, w, h), text, out);
        }

        self.arrange_absolute(tree, id, inner, text, out);
    }

    fn arrange_stack(
        &mut self,
        tree: &Tree,
        id: NodeId,
        inner: Rect,
        text: &mut dyn TextMetrics,
        out: &mut [Rect],
    ) {
        let node = tree.node(id);
        let align = node.style.align_items;
        let justify = node.style.justify_content;

        for &child in &node.children {
            let style = &tree.node(child).style;
            if style.display == Display::None || style.position == Position::Absolute {
                continue;
            }
            let size = self.measure(
                tree,
                child,
                Constraints::loose(Size::new(inner.w, inner.h)),
                text,
            );

            let (w, x) = place(size.w, inner.x, inner.w, justify);
            let (h, y) = place(size.h, inner.y, inner.h, align);
            let rect = Rect::new(
                x + style.margin.left,
                y + style.margin.top,
                (w - style.margin.horizontal()).max(0.0),
                (h - style.margin.vertical()).max(0.0),
            );
            self.arrange(tree, child, rect, text, out);
        }
    }

    fn arrange_grid(
        &mut self,
        tree: &Tree,
        id: NodeId,
        inner: Rect,
        text: &mut dyn TextMetrics,
        out: &mut [Rect],
    ) {
        let (columns, mut rows) = self.grid_tracks(tree, id, inner.w, text);
        let style = &tree.node(id).style;
        let gap = style.gap;

        // Auto rows stretch to fill a definite height, the same rule as CSS
        // `align-content: stretch`. Without this a grid with a fixed height
        // leaves its items collapsed to their content height.
        let has_fixed_rows = style.rows.iter().any(|track| matches!(track, Track::Px(_)));
        if !has_fixed_rows && !rows.is_empty() {
            let used: f32 = rows.iter().sum::<f32>() + gap * rows.len().saturating_sub(1) as f32;
            let extra = inner.h - used;
            if extra > 0.0 {
                let share = extra / rows.len() as f32;
                for row in &mut rows {
                    *row += share;
                }
            }
        }

        let mut column_offsets = Vec::with_capacity(columns.len());
        let mut x = inner.x;
        for width in &columns {
            column_offsets.push(x);
            x += width + gap;
        }

        let mut row_offsets = Vec::with_capacity(rows.len());
        let mut y = inner.y;
        for height in &rows {
            row_offsets.push(y);
            y += height + gap;
        }

        let flow: Vec<NodeId> = tree
            .node(id)
            .children
            .iter()
            .copied()
            .filter(|child| {
                let style = &tree.node(*child).style;
                style.position != Position::Absolute && style.display != Display::None
            })
            .collect();

        let column_count = columns.len().max(1);
        for (index, child) in flow.iter().enumerate() {
            let column = index % column_count;
            let row = index / column_count;
            if row >= rows.len() {
                break;
            }
            let style = &tree.node(*child).style;
            let rect = Rect::new(
                column_offsets[column] + style.margin.left,
                row_offsets[row] + style.margin.top,
                (columns[column] - style.margin.horizontal()).max(0.0),
                (rows[row] - style.margin.vertical()).max(0.0),
            );
            self.arrange(tree, *child, rect, text, out);
        }

        self.arrange_absolute(tree, id, inner, text, out);
    }

    fn arrange_absolute(
        &mut self,
        tree: &Tree,
        id: NodeId,
        inner: Rect,
        text: &mut dyn TextMetrics,
        out: &mut [Rect],
    ) {
        let node = tree.node(id);
        for &child in &node.children {
            let style = &tree.node(child).style;
            if style.position != Position::Absolute || style.display == Display::None {
                continue;
            }

            let [top, right, bottom, left] = style.inset;

            let width = match (left, right) {
                (Some(l), Some(r)) => Some((inner.w - l - r).max(0.0)),
                _ => style.width.resolve(inner.w),
            };
            let height = match (top, bottom) {
                (Some(t), Some(b)) => Some((inner.h - t - b).max(0.0)),
                _ => style.height.resolve(inner.h),
            };

            let constraints = Constraints {
                min_w: width.unwrap_or(0.0),
                max_w: width.unwrap_or(inner.w),
                min_h: height.unwrap_or(0.0),
                max_h: height.unwrap_or(inner.h),
            };
            let measured = self.measure(tree, child, constraints, text);

            let (w, x) = absolute_axis(left, right, inner.x, inner.w, width, measured.w);
            let (h, y) = absolute_axis(top, bottom, inner.y, inner.h, height, measured.h);

            self.arrange(
                tree,
                child,
                Rect::new(
                    x + style.margin.left,
                    y + style.margin.top,
                    (w - style.margin.horizontal()).max(0.0),
                    (h - style.margin.vertical()).max(0.0),
                ),
                text,
                out,
            );
        }
    }
}

struct FlexItem {
    id: NodeId,
    main: f32,
    cross: f32,
    margin: Edges,
    grow: f32,
    shrink: f32,
}

/// Places one axis of a stack child.
fn place(size: f32, origin: f32, available: f32, mode: impl Into<Align>) -> (f32, f32) {
    let align = mode.into();
    let clamped = size.min(available);
    match align {
        Align::Start => (clamped, origin),
        Align::Center => (clamped, origin + (available - clamped) / 2.0),
        Align::End => (clamped, origin + available - clamped),
        Align::Stretch => (available, origin),
    }
}

impl From<Justify> for Align {
    fn from(value: Justify) -> Self {
        match value {
            Justify::Start
            | Justify::SpaceBetween
            | Justify::SpaceAround
            | Justify::SpaceEvenly => Self::Start,
            Justify::Center => Self::Center,
            Justify::End => Self::End,
        }
    }
}

/// Resolves one axis of an absolutely positioned child.
fn absolute_axis(
    start: Option<f32>,
    end: Option<f32>,
    origin: f32,
    available: f32,
    size: Option<f32>,
    measured: f32,
) -> (f32, f32) {
    let size = size.unwrap_or(measured);
    match (start, end) {
        (Some(start), Some(_)) => (size, origin + start),
        (Some(start), None) => (size, origin + start),
        (None, Some(end)) => (size, origin + available - end - size),
        (None, None) => (size, origin),
    }
}
