//! Layout behaviour tests.
//!
//! Every test states the geometry it expects in absolute pixels, so a change in
//! one algorithm cannot silently move something in another.

use mre_core::{Point, Rect, Size, Viewport};
use mre_layout::{
    Align, Direction, Edges, Engine, Justify, Position, Style, TextId, TextMetrics, Track, Tree,
    pct, px,
};
use std::collections::HashMap;

#[derive(Default)]
struct TestText {
    runs: HashMap<u32, String>,
}

impl TestText {
    fn add(&mut self, text: &str) -> TextId {
        let id = TextId(self.runs.len() as u32);
        self.runs.insert(id.0, text.to_string());
        id
    }
}

impl TextMetrics for TestText {
    fn measure(&mut self, text: TextId, max_width: f32) -> Size {
        let content = self.runs.get(&text.0).map_or("", String::as_str);
        let natural = content.chars().count() as f32 * 8.0;
        let width = natural.min(max_width);
        let lines = if width <= 0.0 {
            1.0
        } else {
            (natural / width.max(1.0)).ceil().max(1.0)
        };
        Size::new(width, 16.0 * lines)
    }
}

fn viewport(w: f32, h: f32) -> Rect {
    Rect::new(0.0, 0.0, w, h)
}

fn approx(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.01,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn root_fills_the_viewport() {
    let tree = Tree::new(Style::column());
    let mut engine = Engine::new();
    let mut text = TestText::default();

    let layout = engine.layout(&tree, viewport(320.0, 240.0), &mut text);

    assert_eq!(layout.size(), Size::new(320.0, 240.0));
    assert_eq!(layout.rect(tree.root()), Rect::new(0.0, 0.0, 320.0, 240.0));
}

#[test]
fn column_stacks_children_with_gaps() {
    let mut tree = Tree::new(Style::column().gap(5.0));
    let root = tree.root();
    let a = tree.add_child(root, Style::default().height(px(10.0)));
    let b = tree.add_child(root, Style::default().height(px(20.0)));
    let c = tree.add_child(root, Style::default().height(px(30.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(100.0, 1000.0), &mut text);

    approx(layout.rect(a).y, 0.0);
    approx(layout.rect(a).h, 10.0);
    approx(layout.rect(b).y, 15.0);
    approx(layout.rect(b).h, 20.0);
    approx(layout.rect(c).y, 40.0);
    approx(layout.rect(c).h, 30.0);
}

#[test]
fn row_grow_distributes_free_space_by_factor() {
    let mut tree = Tree::new(Style::row().width(px(300.0)).height(px(50.0)));
    let root = tree.root();
    let fixed = tree.add_child(root, Style::default().width(px(50.0)));
    let one = tree.add_child(root, Style::default().grow(1.0));
    let two = tree.add_child(root, Style::default().grow(2.0));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(300.0, 50.0), &mut text);

    approx(layout.rect(fixed).w, 50.0);
    approx(layout.rect(one).x, 50.0);
    approx(layout.rect(one).w, 250.0 / 3.0);
    approx(layout.rect(two).x, 50.0 + 250.0 / 3.0);
    approx(layout.rect(two).w, 500.0 / 3.0);
}

#[test]
fn padding_offsets_children_and_shrinks_the_content_box() {
    let mut tree = Tree::new(Style::column().padding(Edges::all(10.0)));
    let root = tree.root();
    let child = tree.add_child(root, Style::default().width(pct(50.0)).height(px(20.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(200.0, 100.0), &mut text);

    // 50% of the *content* width (200 - 20), not of the border box.
    assert_eq!(layout.rect(child).x, 10.0);
    assert_eq!(layout.rect(child).y, 10.0);
    approx(layout.rect(child).w, 90.0);
}

#[test]
fn percentage_sizes_resolve_against_the_parent() {
    let mut tree = Tree::new(Style::row().width(px(200.0)).height(px(100.0)));
    let root = tree.root();
    let half = tree.add_child(root, Style::default().width(pct(50.0)).height(pct(25.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(200.0, 100.0), &mut text);

    approx(layout.rect(half).w, 100.0);
    approx(layout.rect(half).h, 25.0);
}

#[test]
fn align_centers_on_the_cross_axis() {
    let mut tree = Tree::new(
        Style::column()
            .width(px(100.0))
            .height(px(200.0))
            .align(Align::Center),
    );
    let root = tree.root();
    let child = tree.add_child(root, Style::default().width(px(20.0)).height(px(20.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(100.0, 200.0), &mut text);

    approx(layout.rect(child).x, 40.0);
    approx(layout.rect(child).y, 0.0);
}

#[test]
fn align_stretch_fills_the_cross_axis() {
    let mut tree = Tree::new(
        Style::column()
            .width(px(100.0))
            .height(px(200.0))
            .align(Align::Stretch),
    );
    let root = tree.root();
    let child = tree.add_child(root, Style::default().height(px(20.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(100.0, 200.0), &mut text);

    approx(layout.rect(child).w, 100.0);
    approx(layout.rect(child).h, 20.0);
}

#[test]
fn justify_space_between_pushes_items_to_the_edges() {
    let mut tree = Tree::new(
        Style::row()
            .width(px(300.0))
            .height(px(50.0))
            .justify(Justify::SpaceBetween),
    );
    let root = tree.root();
    let a = tree.add_child(root, Style::default().width(px(50.0)));
    let b = tree.add_child(root, Style::default().width(px(50.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(300.0, 50.0), &mut text);

    approx(layout.rect(a).x, 0.0);
    approx(layout.rect(b).x, 250.0);
}

#[test]
fn row_reverse_mirrors_placement() {
    let mut tree = Tree::new(
        Style::row()
            .width(px(300.0))
            .height(px(50.0))
            .direction(Direction::RowReverse),
    );
    let root = tree.root();
    let a = tree.add_child(root, Style::default().width(px(50.0)));
    let b = tree.add_child(root, Style::default().width(px(50.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(300.0, 50.0), &mut text);

    approx(layout.rect(a).x, 250.0);
    approx(layout.rect(b).x, 200.0);
}

#[test]
fn margins_are_reserved_in_flow() {
    let mut tree = Tree::new(Style::row().width(px(200.0)).height(px(50.0)));
    let root = tree.root();
    let a = tree.add_child(
        root,
        Style::default()
            .width(px(50.0))
            .margin(Edges::new(0.0, 0.0, 0.0, 10.0)),
    );
    let b = tree.add_child(root, Style::default().width(px(50.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(200.0, 50.0), &mut text);

    approx(layout.rect(a).x, 10.0);
    approx(layout.rect(b).x, 60.0);
}

#[test]
fn absolute_children_position_against_the_padding_box() {
    let mut tree = Tree::new(Style::default().padding(Edges::all(5.0)));
    let root = tree.root();
    let child = tree.add_child(
        root,
        Style::default()
            .absolute()
            .inset(Some(10.0), None, None, Some(20.0))
            .width(px(30.0))
            .height(px(40.0)),
    );

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(200.0, 200.0), &mut text);

    assert_eq!(layout.rect(child), Rect::new(25.0, 15.0, 30.0, 40.0));
}

#[test]
fn absolute_children_with_both_edges_stretch() {
    let mut tree = Tree::new(Style::default());
    let root = tree.root();
    let child = tree.add_child(
        root,
        Style::default()
            .absolute()
            .inset(Some(10.0), Some(20.0), Some(10.0), Some(20.0)),
    );

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(200.0, 100.0), &mut text);

    assert_eq!(layout.rect(child), Rect::new(20.0, 10.0, 160.0, 80.0));
}

#[test]
fn stack_overlaps_and_aligns_children() {
    let mut tree = Tree::new(
        Style::stack()
            .width(px(200.0))
            .height(px(200.0))
            .align(Align::Center)
            .justify(Justify::Center),
    );
    let root = tree.root();
    let a = tree.add_child(root, Style::default().width(px(50.0)).height(px(50.0)));
    let b = tree.add_child(root, Style::default().width(px(20.0)).height(px(20.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(200.0, 200.0), &mut text);

    approx(layout.rect(a).x, 75.0);
    approx(layout.rect(a).y, 75.0);
    approx(layout.rect(b).x, 90.0);
    approx(layout.rect(b).y, 90.0);
}

#[test]
fn grid_resolves_fixed_and_fractional_tracks() {
    let mut tree = Tree::new(
        Style::grid(vec![Track::px(100.0), Track::fr(1.0), Track::fr(1.0)])
            .width(px(300.0))
            .height(px(60.0)),
    );
    let root = tree.root();
    let a = tree.add_child(root, Style::default());
    let b = tree.add_child(root, Style::default());
    let c = tree.add_child(root, Style::default());

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(300.0, 60.0), &mut text);

    approx(layout.rect(a).x, 0.0);
    approx(layout.rect(a).w, 100.0);
    approx(layout.rect(b).x, 100.0);
    approx(layout.rect(b).w, 100.0);
    approx(layout.rect(c).x, 200.0);
    approx(layout.rect(c).w, 100.0);
}

#[test]
fn text_is_measured_by_the_supplied_measurer() {
    let mut tree = Tree::new(Style::column().width(px(100.0)).align(Align::Start));
    let root = tree.root();
    let mut text = TestText::default();
    let run = text.add("abcd");
    let node = tree.add_text(root, Style::default(), run);

    let mut engine = Engine::new();
    let layout = engine.layout(&tree, viewport(100.0, 50.0), &mut text);

    approx(layout.rect(node).w, 32.0);
    approx(layout.rect(node).h, 16.0);
}

#[test]
fn min_and_max_constrain_content_size() {
    let mut tree = Tree::new(Style::column());
    let root = tree.root();
    let wide = tree.add_child(
        root,
        Style::default()
            .width(px(500.0))
            .max_width(px(100.0))
            .height(px(10.0)),
    );
    let short = tree.add_child(
        root,
        Style::default()
            .width(px(10.0))
            .min_width(px(40.0))
            .height(px(10.0)),
    );

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(600.0, 100.0), &mut text);

    approx(layout.rect(wide).w, 100.0);
    approx(layout.rect(short).w, 40.0);
}

#[test]
fn second_layout_pass_is_served_from_cache() {
    let mut tree = Tree::new(Style::column());
    let root = tree.root();
    for _ in 0..20 {
        tree.add_child(root, Style::default().height(px(10.0)));
    }

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let vp = viewport(300.0, 500.0);

    engine.layout(&tree, vp, &mut text);
    let first = engine.stats();
    engine.layout(&tree, vp, &mut text);
    let second = engine.stats();

    assert_eq!(
        second.measures, first.measures,
        "a repeat layout must not re-measure anything"
    );
    assert!(second.cache_hits > first.cache_hits);
    assert!(second.hit_rate() > 0.5);
}

#[test]
fn invalidating_a_subtree_forces_a_re_measure() {
    let mut tree = Tree::new(Style::column());
    let root = tree.root();
    let child = tree.add_child(root, Style::default().height(px(10.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let vp = viewport(300.0, 500.0);

    engine.layout(&tree, vp, &mut text);
    let before = engine.stats().measures;

    engine.invalidate(&tree, child);
    tree.set_style(child, Style::default().height(px(20.0)));
    engine.layout(&tree, vp, &mut text);

    assert!(
        engine.stats().measures > before,
        "the changed subtree must be measured again"
    );
}

#[test]
fn rotation_relayouts_percentages_against_the_new_viewport() {
    let mut tree = Tree::new(Style::column());
    let root = tree.root();
    let half = tree.add_child(root, Style::default().width(pct(50.0)).height(px(10.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();

    let mut vp = Viewport::new(Size::new(400.0, 800.0));
    let portrait = engine.layout(&tree, Rect::from_size(vp.size), &mut text);
    approx(portrait.rect(half).w, 200.0);

    vp.rotate();
    let landscape = engine.layout(&tree, Rect::from_size(vp.size), &mut text);
    approx(landscape.rect(half).w, 400.0);
    assert_eq!(landscape.size(), Size::new(800.0, 400.0));
}

#[test]
fn hit_test_returns_the_deepest_node_under_the_point() {
    let mut tree = Tree::new(Style::column());
    let root = tree.root();
    let card = tree.add_child(root, Style::default().height(px(100.0)));
    let label = tree.add_child(card, Style::default().height(px(20.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(200.0, 200.0), &mut text);

    assert_eq!(layout.hit_test(&tree, Point::new(10.0, 10.0)), Some(label));
    assert_eq!(layout.hit_test(&tree, Point::new(10.0, 50.0)), Some(card));
    assert_eq!(layout.hit_test(&tree, Point::new(500.0, 500.0)), None);
}

#[test]
fn display_none_takes_no_space() {
    let mut tree = Tree::new(Style::column());
    let root = tree.root();
    let hidden = tree.add_child(
        root,
        Style {
            display: mre_layout::Display::None,
            height: px(50.0),
            ..Style::default()
        },
    );
    let visible = tree.add_child(root, Style::default().height(px(20.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(200.0, 200.0), &mut text);

    assert_eq!(layout.rect(hidden), Rect::EMPTY);
    approx(layout.rect(visible).y, 0.0);
}

#[test]
fn aspect_ratio_derives_the_missing_axis() {
    let mut tree = Tree::new(Style::column());
    let root = tree.root();
    let square = tree.add_child(
        root,
        Style {
            width: px(80.0),
            aspect_ratio: Some(2.0),
            ..Style::default()
        },
    );

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(200.0, 200.0), &mut text);

    approx(layout.rect(square).w, 80.0);
    approx(layout.rect(square).h, 40.0);
}

#[test]
fn position_absolute_does_not_affect_siblings() {
    let mut tree = Tree::new(Style::row().width(px(200.0)).height(px(50.0)));
    let root = tree.root();
    let a = tree.add_child(root, Style::default().width(px(30.0)));
    let _floating = tree.add_child(
        root,
        Style {
            position: Position::Absolute,
            width: px(100.0),
            height: px(100.0),
            ..Style::default()
        },
    );
    let b = tree.add_child(root, Style::default().width(px(30.0)));

    let mut engine = Engine::new();
    let mut text = TestText::default();
    let layout = engine.layout(&tree, viewport(200.0, 50.0), &mut text);

    approx(layout.rect(a).x, 0.0);
    approx(layout.rect(b).x, 30.0);
}
