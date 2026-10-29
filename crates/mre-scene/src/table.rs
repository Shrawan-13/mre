//! The reference workload: a virtualised, data-dense table.
//!
//! This is not a demo. It is the permanent benchmark scene, and every engine in
//! `baselines/` renders the same thing from `perf/scenes/table.toml`.
//!
//! The important property is **row recycling**: a fixed pool of row nodes is
//! reused for whichever rows are visible. Reconciling the pool is O(visible),
//! never O(rows), and after warm-up it performs **zero allocations** — which is
//! exactly the property the allocation gate exists to protect.

use crate::dirty::Dirty;
use crate::display::{Command, DisplayList};
use crate::node::{Node, NodeKind};
use crate::scene::Scene;
use mre_core::{Id, Rect, Rgba8};

/// Row height in logical pixels.
pub const ROW_HEIGHT: f32 = 32.0;
/// Sticky header height in logical pixels.
pub const HEADER_HEIGHT: f32 = 40.0;
/// Column width in logical pixels.
pub const COLUMN_WIDTH: f32 = 240.0;
/// Number of columns rendered per row.
pub const COLUMNS: usize = 6;
/// Rows rendered above and below the viewport to hide recycling.
pub const OVERSCAN: usize = 2;
/// Sentinel marking a pool node that currently represents no row.
const UNUSED: u32 = u32::MAX;

const BACKGROUND: Rgba8 = Rgba8::from_rgba(0x14, 0x14, 0x14, 0xFF);
const ROW_EVEN: Rgba8 = Rgba8::from_rgba(0x1c, 0x1c, 0x1c, 0xFF);
const ROW_ODD: Rgba8 = Rgba8::from_rgba(0x20, 0x20, 0x20, 0xFF);
const HEADER_BG: Rgba8 = Rgba8::from_rgba(0x2a, 0x2a, 0x2a, 0xFF);
const TEXT: Rgba8 = Rgba8::from_rgba(0xE6, 0xE6, 0xE6, 0xFF);

/// Scroll state of a virtualised table.
#[derive(Clone, Copy, Debug)]
pub struct TableModel {
    /// Total rows in the data set.
    pub rows: usize,
    /// Scroll offset in logical pixels.
    pub scroll_y: f32,
    /// Visible body area, excluding the header.
    pub viewport: Rect,
}

impl TableModel {
    #[must_use]
    pub fn new(rows: usize, viewport: Rect) -> Self {
        Self {
            rows,
            scroll_y: 0.0,
            viewport,
        }
    }

    #[must_use]
    pub fn total_height(&self) -> f32 {
        self.rows as f32 * ROW_HEIGHT
    }

    #[must_use]
    pub fn max_scroll(&self) -> f32 {
        (self.total_height() - self.viewport.h).max(0.0)
    }

    /// Rows that intersect the viewport, before overscan.
    #[must_use]
    pub fn visible_rows(&self) -> usize {
        (self.viewport.h / ROW_HEIGHT).ceil() as usize + 1
    }

    /// Index of the first row intersecting the viewport.
    #[must_use]
    pub fn first_visible(&self) -> usize {
        (self.scroll_y / ROW_HEIGHT).floor().max(0.0) as usize
    }

    /// Scrolls by `dy` pixels, clamped to the data range. Allocation-free.
    #[inline]
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, self.max_scroll());
    }
}

/// What one frame did. Used by the benchmark to prove work was actually avoided.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    /// Region that changed, or [`Rect::EMPTY`] when nothing did.
    pub damage: Rect,
    /// True when the display list was rebuilt this frame.
    pub repainted: bool,
    /// Commands in the display list after this frame.
    pub commands: usize,
}

/// A virtualised table bound to a scene.
pub struct TableView {
    pub model: TableModel,
    scene: Scene,
    header: Id,
    body: Id,
    /// Fixed pool of row nodes, reused every frame.
    pool: Vec<Id>,
    damage: Rect,
}

impl TableView {
    /// Builds the tree and the row pool. This is the cold path.
    #[must_use]
    pub fn new(rows: usize, viewport: Rect) -> Self {
        let model = TableModel::new(rows, viewport);
        let pool_len = (model.visible_rows() + OVERSCAN * 2).min(rows);

        // Everything is sized up front so no frame can ever grow the arena.
        let mut scene = Scene::with_capacity(4 + pool_len);
        let root = scene.root();
        let header = scene.append(root, Node::new(NodeKind::Header));
        let body = scene.append(root, Node::new(NodeKind::Body));

        let mut pool = Vec::with_capacity(pool_len);
        for _ in 0..pool_len {
            pool.push(scene.append(body, Node::new(NodeKind::Row)));
        }

        scene[header].bounds = Rect::new(
            viewport.x,
            viewport.y - HEADER_HEIGHT,
            viewport.w,
            HEADER_HEIGHT,
        );
        scene[body].bounds = viewport;

        Self {
            model,
            scene,
            header,
            body,
            pool,
            damage: Rect::EMPTY,
        }
    }

    #[must_use]
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Id of the sticky header node.
    #[must_use]
    pub fn header(&self) -> Id {
        self.header
    }

    /// Id of the scrolling body node that owns the row pool.
    #[must_use]
    pub fn body(&self) -> Id {
        self.body
    }

    /// Number of row nodes in the recycling pool.
    #[must_use]
    pub fn pool_len(&self) -> usize {
        self.pool.len()
    }

    /// Total scene nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.scene.len()
    }

    /// Arena bytes committed for the scene.
    #[must_use]
    pub fn scene_bytes(&self) -> usize {
        self.scene.bytes()
    }

    /// Scrolls and marks rows dirty. Allocation-free.
    #[inline]
    pub fn scroll_by(&mut self, dy: f32) {
        self.model.scroll_by(dy);
    }

    /// Binds pool nodes to rows and positions them.
    ///
    /// Returns the union of every rect whose geometry or row binding changed.
    /// Rows that did not move are marked clean, so downstream stages can skip
    /// them entirely.
    pub fn layout(&mut self) -> Rect {
        let viewport = self.model.viewport;
        let scroll = self.model.scroll_y;
        let rows = self.model.rows;

        let first = self.model.first_visible();
        let start = first.saturating_sub(OVERSCAN);
        let mut damage = Rect::EMPTY;

        for i in 0..self.pool.len() {
            let id = self.pool[i];
            let row = start + i;

            if row >= rows {
                let node = &mut self.scene[id];
                if node.payload != UNUSED {
                    damage = damage.union(node.bounds);
                    node.payload = UNUSED;
                    node.bounds = Rect::EMPTY;
                    node.dirty = Dirty::PAINT;
                } else {
                    node.dirty = Dirty::NONE;
                }
                continue;
            }

            let y = viewport.y + row as f32 * ROW_HEIGHT - scroll;
            let bounds = Rect::new(viewport.x, y, viewport.w, ROW_HEIGHT);

            let node = &mut self.scene[id];
            if node.payload != row as u32 || node.bounds != bounds {
                damage = damage.union(bounds);
                node.payload = row as u32;
                node.bounds = bounds;
                node.dirty = Dirty::PAINT;
            } else {
                node.dirty = Dirty::NONE;
            }
        }

        self.damage = damage;
        damage
    }

    /// Rebuilds the display list from scratch. Allocation-free once the list has
    /// capacity.
    pub fn paint(&mut self, list: &mut DisplayList) {
        list.clear();

        let viewport = self.model.viewport;
        list.push(Command::Rect {
            rect: viewport,
            color: BACKGROUND,
        });

        let header_bounds = Rect::new(
            viewport.x,
            viewport.y - HEADER_HEIGHT,
            viewport.w,
            HEADER_HEIGHT,
        );
        list.push(Command::Rect {
            rect: header_bounds,
            color: HEADER_BG,
        });
        for column in 0..COLUMNS {
            list.push(Command::Text {
                rect: Rect::new(
                    viewport.x + column as f32 * COLUMN_WIDTH,
                    header_bounds.y,
                    COLUMN_WIDTH,
                    HEADER_HEIGHT,
                ),
                run: column as u32,
                color: TEXT,
            });
        }

        for i in 0..self.pool.len() {
            let node = &self.scene[self.pool[i]];
            if node.payload == UNUSED {
                continue;
            }
            let bounds = node.bounds;
            let row = node.payload as usize;
            let color = if row % 2 == 0 { ROW_EVEN } else { ROW_ODD };

            list.push(Command::Rect {
                rect: bounds,
                color,
            });
            for column in 0..COLUMNS {
                list.push(Command::Text {
                    rect: Rect::new(
                        bounds.x + column as f32 * COLUMN_WIDTH,
                        bounds.y,
                        COLUMN_WIDTH,
                        bounds.h,
                    ),
                    run: (row * COLUMNS + column) as u32,
                    color: TEXT,
                });
            }
        }
    }

    /// One frame: scroll, layout, repaint if anything changed.
    ///
    /// The `if` is the whole point. A frame with no damage does no paint work
    /// at all, which is what makes an idle app free instead of a 120 Hz burn.
    pub fn frame(&mut self, scroll_delta: f32, list: &mut DisplayList) -> FrameStats {
        self.scroll_by(scroll_delta);
        let damage = self.layout();
        let repainted = !damage.is_empty() || list.is_empty();
        if repainted {
            self.paint(list);
        }
        FrameStats {
            damage,
            repainted,
            commands: list.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> TableView {
        TableView::new(50_000, Rect::new(0.0, HEADER_HEIGHT, 1440.0, 860.0))
    }

    #[test]
    fn pool_is_bounded_by_the_viewport_not_the_row_count() {
        let v = view();
        assert!(
            v.pool_len() < 40,
            "pool {} should be viewport-sized",
            v.pool_len()
        );
        assert_eq!(
            v.node_count(),
            v.pool_len() + 3,
            "pool + root + header + body"
        );
    }

    #[test]
    fn scrolling_rebinds_rows_without_growing_the_scene() {
        let mut v = view();
        let nodes = v.node_count();
        let bytes = v.scene_bytes();
        let mut list = DisplayList::with_capacity(1024);

        for _ in 0..200 {
            v.frame(7.0, &mut list);
        }

        assert_eq!(v.node_count(), nodes);
        assert_eq!(
            v.scene_bytes(),
            bytes,
            "arena must not grow while scrolling"
        );
    }

    #[test]
    fn idle_frame_produces_no_damage_and_no_repaint() {
        let mut v = view();
        let mut list = DisplayList::with_capacity(1024);

        let first = v.frame(0.0, &mut list);
        assert!(first.repainted, "the first frame must paint");

        let second = v.frame(0.0, &mut list);
        assert!(second.damage.is_empty(), "static frame must have no damage");
        assert!(!second.repainted, "static frame must not repaint");
        assert_eq!(second.commands, first.commands, "display list stays valid");
    }

    #[test]
    fn scroll_produces_damage_and_rows_stay_contiguous() {
        let mut v = view();
        let mut list = DisplayList::with_capacity(1024);
        v.frame(0.0, &mut list);
        let stats = v.frame(ROW_HEIGHT, &mut list);
        assert!(!stats.damage.is_empty());

        let mut ys: Vec<f32> = v
            .scene()
            .iter()
            .filter(|(_, n)| n.kind == NodeKind::Row && n.payload != UNUSED)
            .map(|(_, n)| n.bounds.y)
            .collect();
        ys.sort_by(f32::total_cmp);
        for pair in ys.windows(2) {
            assert!(
                (pair[1] - pair[0] - ROW_HEIGHT).abs() < 0.001,
                "rows must be gap-free"
            );
        }
    }

    #[test]
    fn batching_stays_within_the_draw_call_budget() {
        let mut v = view();
        let mut list = DisplayList::with_capacity(1024);
        v.frame(0.0, &mut list);
        assert!(
            list.batch_count() <= mre_perf_budget_max_draw_calls(),
            "batch count {} exceeded budget",
            list.batch_count()
        );
    }

    fn mre_perf_budget_max_draw_calls() -> usize {
        // Kept as a literal so `mre-scene` does not depend on `mre-perf`.
        // The real gate lives in `mre-bench/tests`.
        64
    }

    #[test]
    fn scroll_clamps_to_the_data_range() {
        let mut v = view();
        v.scroll_by(-1000.0);
        assert_eq!(v.model.scroll_y, 0.0);
        v.scroll_by(1e9);
        assert!((v.model.scroll_y - v.model.max_scroll()).abs() < 0.001);
    }
}
