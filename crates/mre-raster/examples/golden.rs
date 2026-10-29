//! Renders the golden test scene.
//!
//! ```text
//! cargo run -p mre-raster --example golden -- <out.rgba> [out.png]
//! ```

use mre_raster::demo_card;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args
        .next()
        .unwrap_or_else(|| "assets/golden/text-card.rgba".to_string());
    let png = args.next();

    let font = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/Roboto-Regular.ttf"
    ));

    let canvas = demo_card(font);
    std::fs::write(&out, canvas.pixels()).expect("write golden");
    println!("wrote {out} ({} bytes)", canvas.pixels().len());

    if let Some(png) = png {
        canvas.save_png(&png).expect("write png");
        println!("wrote {png}");
    }
}
