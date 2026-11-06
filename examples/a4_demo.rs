use mc630_plt_studio::sheet::{Layout, artwork};
fn main() {
    let mut img = image::RgbaImage::new(400, 400);
    for y in 0..400 {
        for x in 0..400 {
            let dx = x as f64 - 200.;
            let dy = y as f64 - 200.;
            let theta = dy.atan2(dx);
            let edge = 150. + 25. * (5. * theta).cos();
            if dx.hypot(dy) <= edge {
                img.put_pixel(
                    x,
                    y,
                    image::Rgba([30, (110. + x as f64 / 4.) as u8, 180, 255]),
                );
            }
        }
    }
    let root = std::path::Path::new("examples/A4-DEMO");
    std::fs::create_dir_all(root).unwrap();
    img.save(root.join("sample-sticker.png")).unwrap();
    let a = artwork(
        &std::fs::read(root.join("sample-sticker.png")).unwrap(),
        false,
        245,
    )
    .unwrap();
    let l = Layout::default();
    std::fs::write(root.join("PRINT.html"), l.print_html(&a).unwrap()).unwrap();
    std::fs::write(root.join("CUT.plt"), l.plt(&a).unwrap()).unwrap();
    println!(
        "Generated 20-copy A4 sample, {} outer outlines per image.",
        a.outlines.len()
    );
}
