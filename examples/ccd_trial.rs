use mc630_plt_studio::sheet::{CcdTrial, Layout, artwork};
fn main() {
    let art = artwork(include_bytes!("A4-DEMO/sample-sticker.png"), false, 245).unwrap();
    let (html, plt) = CcdTrial::default().pair(&Layout::default(), &art).unwrap();
    let root = std::path::Path::new("examples/CCD-MACHINE-SETTINGS");
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(root.join("PRINT.html"), html).unwrap();
    std::fs::write(root.join("CCD_TEST.plt"), &plt).unwrap();
    std::fs::write(root.join("READ-FIRST.txt"),"Origin: TOP-LEFT dot, page X right, page Y down; machine PLT exports feed Y first, carriage X second.\nEXPERIMENTAL - NOT VERIFIED ON MC630 Ver5.022\n20 copies; 35 x 35 mm image boxes; 3 mm gap; 15 mm margins. Four black 5.5 mm circles, centers 7 mm outside the occupied image frame corners. TB25 trial spans 163 x 201 mm at 40 units/mm. These dimensions and axis mapping are NOT certified firmware specifications.\n\nPrint PRINT.html fresh at A4 portrait, Actual size 100%, no margins, no headers. Do not use the previous cross-mark sheet. REMOVE THE BLADE before selecting CCD_TEST.plt: it includes all cut paths and may start drawing after scanning. First-dot corner, feed orientation, scan protocol and units/mm remain unverified. Record the scan behavior/error; do not infer camera failure from a rejected trial.\n\nSee research/CCD-FINDINGS.md for sources and remaining uncertainties.\n").unwrap();
    std::fs::write(root.join("CUT.plt"), &plt).unwrap();
    println!("Created experimental 20-copy CCD trial pair; physical scan unverified.");
}
