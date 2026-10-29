//! Renders a data-dense dashboard to a PNG.
//!
//! ```text
//! cargo run -p mre-widgets --example dashboard -- /tmp/dashboard.png
//! ```
//!
//! This is the visual counterpart to the benchmark: the same shape of workload
//! (a table with sticky header, aligned numeric columns and conditional colour)
//! rendered to pixels a human can inspect.

use mre_core::Rgba8;
use mre_layout::Dim;
use mre_raster::Canvas;
use mre_text::{FontDesc, TextAlign, TextEngine, TextStore, TextStyle};
use mre_widgets::{Theme, Ui, View};

fn store() -> TextStore {
    let mut engine = TextEngine::new();
    engine
        .add_font_file(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/Roboto-Regular.ttf"
            ),
            FontDesc::new("Roboto"),
        )
        .expect("bundled font");
    TextStore::new(engine)
}

fn cell(text: String, style: TextStyle, color: Rgba8, width: f32, align: TextAlign) -> View {
    View::text(text, style.align(align))
        .color(color)
        .width(Dim::Px(width))
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/opencode/dashboard.png".to_string());

    let theme = Theme::light();
    let mono = TextStyle::new(13.0).family("Roboto");
    let body = TextStyle::new(13.0);
    let head = TextStyle::new(12.0).weight(600);

    let mut rows: Vec<View> = vec![
        View::row()
            .height(Dim::Px(36.0))
            .padding_edges(mre_layout::Edges::symmetric(0.0, 12.0))
            .gap(12.0)
            .background(theme.surface_alt)
            .child(cell(
                "SYMBOL".into(),
                head.clone(),
                theme.text_muted,
                110.0,
                TextAlign::Start,
            ))
            .child(cell(
                "PRICE".into(),
                head.clone(),
                theme.text_muted,
                100.0,
                TextAlign::End,
            ))
            .child(cell(
                "CHANGE".into(),
                head.clone(),
                theme.text_muted,
                90.0,
                TextAlign::End,
            ))
            .child(cell(
                "VOLUME".into(),
                head.clone(),
                theme.text_muted,
                110.0,
                TextAlign::End,
            ))
            .child(cell(
                "MARKET CAP".into(),
                head.clone(),
                theme.text_muted,
                120.0,
                TextAlign::End,
            )),
    ];

    let data = [
        ("NVDA", 121.44, 2.31, "412.8M", "2.99T"),
        ("AAPL", 231.87, -0.42, "38.1M", "3.52T"),
        ("MSFT", 428.15, 0.87, "21.6M", "2.48T"),
        ("GOOGL", 172.63, 1.12, "24.9M", "2.11T"),
        ("AMZN", 201.09, -1.03, "33.4M", "3.18T"),
        ("META", 587.31, 3.44, "12.2M", "1.49T"),
        ("TSLA", 248.50, -2.18, "88.7M", "793B"),
        ("AVGO", 182.76, 1.90, "9.4M", "851B"),
        ("BRK.B", 468.02, 0.21, "2.8M", "1.01T"),
        ("JPM", 221.45, 0.66, "7.1M", "635B"),
        ("LLY", 812.20, -0.35, "1.9M", "772B"),
        ("V", 289.77, 0.44, "5.3M", "572B"),
        ("UNH", 568.10, 1.07, "3.2M", "523B"),
        ("XOM", 118.32, -0.88, "14.6M", "521B"),
        ("WMT", 81.44, 0.92, "18.3M", "655B"),
        ("MA", 492.61, 0.38, "2.4M", "455B"),
        ("ORCL", 173.90, 2.05, "8.8M", "478B"),
        ("COST", 902.33, -0.17, "1.7M", "400B"),
        ("HD", 402.18, 0.73, "3.1M", "399B"),
        ("PG", 171.05, 0.29, "5.9M", "403B"),
    ];

    for (index, (symbol, price, change, volume, cap)) in data.iter().enumerate() {
        let tint = if index % 2 == 0 {
            theme.background
        } else {
            theme.surface
        };
        let change_color = if *change >= 0.0 {
            Rgba8::from_rgba(0x15, 0x80, 0x3D, 255)
        } else {
            Rgba8::from_rgba(0xB9, 0x1C, 0x1C, 255)
        };
        let change_text = if *change >= 0.0 {
            format!("+{change:.2}%")
        } else {
            format!("{change:.2}%")
        };

        rows.push(
            View::row()
                .height(Dim::Px(30.0))
                .padding_edges(mre_layout::Edges::symmetric(0.0, 12.0))
                .gap(12.0)
                .background(tint)
                .child(cell(
                    symbol.to_string(),
                    body.clone().weight(500),
                    theme.text,
                    110.0,
                    TextAlign::Start,
                ))
                .child(cell(
                    format!("${price:.2}"),
                    mono.clone(),
                    theme.text,
                    100.0,
                    TextAlign::End,
                ))
                .child(cell(
                    change_text,
                    mono.clone(),
                    change_color,
                    90.0,
                    TextAlign::End,
                ))
                .child(cell(
                    volume.to_string(),
                    mono.clone(),
                    theme.text_muted,
                    110.0,
                    TextAlign::End,
                ))
                .child(cell(
                    cap.to_string(),
                    mono.clone(),
                    theme.text_muted,
                    120.0,
                    TextAlign::End,
                )),
        );
    }

    let view = View::column()
        .background(theme.background)
        .child(
            View::row()
                .height(Dim::Px(64.0))
                .padding_edges(mre_layout::Edges::symmetric(0.0, 16.0))
                .align(mre_layout::Align::Center)
                .child(
                    View::column()
                        .grow(1.0)
                        .child(View::text("Markets", TextStyle::new(22.0).weight(600)))
                        .child(
                            View::text("Live · 20 instruments · USD", TextStyle::new(12.0))
                                .color(theme.text_muted),
                        ),
                )
                .child(View::button("Refresh")),
        )
        .child(
            View::column()
                .padding_edges(mre_layout::Edges::new(0.0, 16.0, 0.0, 16.0))
                .children(rows),
        );

    let mut ui = Ui::new(store());
    ui.set_theme(theme);
    ui.set_view(&view);
    ui.layout(mre_core::Rect::new(0.0, 0.0, 900.0, 760.0));

    let mut canvas = Canvas::new(900, 760);
    ui.paint(&mut canvas);
    canvas.save_png(&out).expect("write png");
    println!("wrote {out}");
}
