use mc630_plt_studio::sheet::{CcdTrial, CutShape, Layout, artwork_with_shape};
fn main() {
    let path = std::env::args().nth(1).expect("source image path");
    let tolerance = std::env::args()
        .nth(2)
        .and_then(|x| x.parse().ok())
        .unwrap_or(40);
    let bytes = std::fs::read(path).unwrap();
    let art = artwork_with_shape(
        &bytes,
        false,
        245,
        CutShape::PlainBackgroundObject,
        tolerance,
        true,
    )
    .unwrap();
    let layout = Layout {
        width: 42.5,
        height: 42.5 * art.pixels.height() as f64 / art.pixels.width() as f64,
        count: 8,
        ..Default::default()
    };
    let (html, plt) = CcdTrial::default().pair(&layout, &art).unwrap();
    let out = std::path::Path::new("examples/OBJECT-DEMO");
    std::fs::create_dir_all(out).unwrap();
    std::fs::write(out.join("object.png"), &art.png).unwrap();
    std::fs::write(out.join("PRINT.html"), html).unwrap();
    std::fs::write(out.join("CUT.plt"), plt).unwrap();
    println!("{} contours, {} holes", art.outlines.len(), art.holes.len());
}
