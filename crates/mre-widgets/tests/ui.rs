//! End-to-end tests: view -> layout -> pixels.

use mre_core::{Point, Rect, Rgba8};
use mre_layout::NodeId;
use mre_raster::Canvas;
use mre_text::{FontDesc, TextEngine, TextStore, TextStyle};
use mre_widgets::{Theme, Ui, View};

fn store() -> TextStore {
    let mut engine = TextEngine::new();
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/Roboto-Regular.ttf"
    );
    engine
        .add_font_file(path, FontDesc::new("Roboto"))
        .expect("bundled font");
    TextStore::new(engine)
}

fn sample(theme: &Theme) -> View {
    View::column()
        .padding(16.0)
        .gap(12.0)
        .child(View::text("Dashboard", TextStyle::new(20.0)))
        .child(
            View::row()
                .background(theme.surface)
                .border(theme.border, 1.0)
                .radius(theme.radius)
                .padding(8.0)
                .gap(8.0)
                .child(View::text("Open", TextStyle::new(14.0)))
                .child(View::button("Refresh")),
        )
}

fn build() -> Ui {
    let theme = Theme::light();
    let mut ui = Ui::new(store());
    ui.set_theme(theme.clone());
    ui.set_view(&sample(&theme));
    ui.layout(Rect::new(0.0, 0.0, 320.0, 200.0));
    ui
}

#[test]
fn the_tree_has_one_layout_node_per_view_node() {
    let theme = Theme::light();
    let view = sample(&theme);
    let mut ui = Ui::new(store());
    ui.set_view(&view);

    // +1 for the invisible root that fills the viewport.
    assert_eq!(ui.node_count(), view.node_count() + 1);
}

#[test]
fn padding_and_gap_position_children() {
    let ui = build();
    let title = NodeId::from_raw(2);
    let row = NodeId::from_raw(3);

    let layout = ui.layout_result();
    assert_eq!(layout.rect(title).x, 16.0);
    assert_eq!(layout.rect(title).y, 16.0);
    assert!(layout.rect(row).y > layout.rect(title).y);
    assert_eq!(layout.rect(row).x, 16.0);
    assert_eq!(layout.rect(row).w, 288.0);
}

#[test]
fn painting_uses_the_theme_background() {
    let mut ui = build();
    let mut canvas = Canvas::new(320, 200);
    ui.paint(&mut canvas);

    assert_eq!(canvas.pixel(1, 1), Some(Theme::light().background));
}

#[test]
fn card_surface_is_painted() {
    let theme = Theme::light();
    let mut ui = build();
    let mut canvas = Canvas::new(320, 200);
    ui.paint(&mut canvas);

    // A point inside the card, away from the text.
    let card = ui.layout_result().rect(NodeId::from_raw(3));
    let pixel = canvas
        .pixel(
            (card.x + card.w - 12.0) as u32,
            (card.y + card.h - 8.0) as u32,
        )
        .expect("inside the canvas");

    assert_eq!(pixel, theme.surface);
}

#[test]
fn resizing_relayouts_against_the_new_width() {
    let mut ui = build();
    let row = NodeId::from_raw(3);

    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));
    assert_eq!(ui.layout_result().rect(row).w, 168.0);

    ui.layout(Rect::new(0.0, 0.0, 600.0, 200.0));
    assert_eq!(ui.layout_result().rect(row).w, 568.0);
}

#[test]
fn a_button_has_the_accent_fill_and_a_centred_label() {
    let theme = Theme::light();
    let ui = build();
    let button = NodeId::from_raw(5);
    let label = NodeId::from_raw(6);

    let button_rect = ui.layout_result().rect(button);
    let label_rect = ui.layout_result().rect(label);

    assert!(button_rect.w > 0.0);
    assert!(button_rect.h >= theme.control_height);
    assert!(label_rect.x > button_rect.x);
    assert!(
        (label_rect.x - button_rect.x - (button_rect.w - label_rect.w) / 2.0).abs() < 1.0,
        "label should be horizontally centred"
    );
}

#[test]
fn hit_test_finds_the_deepest_text_node() {
    let ui = build();
    let title = NodeId::from_raw(2);

    assert_eq!(ui.hit_test(Point::new(20.0, 20.0)), Some(title));
    assert_eq!(
        ui.hit_test(Point::new(310.0, 30.0)),
        Some(NodeId::from_raw(1))
    );
    assert_eq!(ui.hit_test(Point::new(400.0, 400.0)), None);
}

#[test]
fn changing_the_theme_changes_the_pixels() {
    let mut ui = build();
    let mut canvas = Canvas::new(320, 200);

    ui.paint(&mut canvas);
    assert_eq!(canvas.pixel(1, 1), Some(Theme::light().background));

    ui.set_theme(Theme::dark());
    ui.paint(&mut canvas);
    assert_eq!(canvas.pixel(1, 1), Some(Theme::dark().background));
}

#[test]
fn rebuilding_the_same_view_reuses_shaped_text() {
    let theme = Theme::light();
    let view = sample(&theme);
    let mut ui = Ui::new(store());
    ui.set_theme(theme.clone());
    ui.set_view(&view);
    ui.layout(Rect::new(0.0, 0.0, 320.0, 200.0));

    let mut canvas = Canvas::new(320, 200);
    ui.paint(&mut canvas);
    let shapes = ui.texts().engine().stats().shapes;

    ui.set_view(&view);
    ui.layout(Rect::new(0.0, 0.0, 320.0, 200.0));
    ui.paint(&mut canvas);

    assert_eq!(
        ui.texts().engine().stats().shapes,
        shapes,
        "re-shaping unchanged text is wasted work"
    );
}

#[test]
fn opacity_blends_towards_the_background() {
    let theme = Theme::light();
    let mut ui = Ui::new(store());
    ui.set_theme(theme.clone());
    ui.set_view(
        &View::column().child(
            View::column()
                .width(mre_layout::Dim::Px(100.0))
                .height(mre_layout::Dim::Px(50.0))
                .background(Rgba8::from_rgba(0, 0, 0, 255))
                .opacity(0.5)
                .child(View::spacer(1.0)),
        ),
    );
    ui.layout(Rect::new(0.0, 0.0, 200.0, 100.0));

    let mut canvas = Canvas::new(200, 100);
    ui.paint(&mut canvas);
    let pixel = canvas.pixel(50, 25).expect("inside");

    assert!(
        pixel.r() > 100 && pixel.r() < 160,
        "half-opacity black over white should be mid-grey, got {pixel:?}"
    );
}

#[test]
fn a_grid_dashboard_lays_out_columns() {
    let theme = Theme::light();
    let mut ui = Ui::new(store());
    ui.set_theme(theme);
    ui.set_view(
        &View::grid(vec![
            mre_layout::Track::fr(1.0),
            mre_layout::Track::fr(1.0),
            mre_layout::Track::fr(2.0),
        ])
        .padding(8.0)
        .gap(8.0)
        .height(mre_layout::Dim::Px(100.0))
        .child(View::column().background(Rgba8::WHITE))
        .child(View::column().background(Rgba8::WHITE))
        .child(View::column().background(Rgba8::WHITE)),
    );
    ui.layout(Rect::new(0.0, 0.0, 416.0, 200.0));

    let first = ui.layout_result().rect(NodeId::from_raw(2));
    let second = ui.layout_result().rect(NodeId::from_raw(3));
    let third = ui.layout_result().rect(NodeId::from_raw(4));

    assert!((first.w - 96.0).abs() < 0.1);
    assert!((second.w - 96.0).abs() < 0.1);
    assert!((third.w - 192.0).abs() < 0.1);
    assert!(third.x > second.x);
}
