use mc630_plt_studio::{Settings, export, export_plain, plt, svg};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let mut a = std::env::args().skip(1).collect::<Vec<_>>();
    let plain = a.last().is_some_and(|arg| arg == "--plain");
    if plain {
        a.pop();
    }
    if a.len() < 3 || a.len() > 4 {
        return Err(
            "Usage: plt-convert INPUT.svg|INPUT.plt OUTPUT.plt WIDTH_MM [UNITS_PER_MM] [--plain]"
                .into(),
        );
    }
    let input = std::path::Path::new(&a[0]);
    let output = std::path::Path::new(&a[1]);
    if output.exists() {
        return Err("Output already exists; choose a new filename.".into());
    }
    let units = a
        .get(3)
        .map(|s| s.parse::<f64>())
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or(40.);
    let bytes = std::fs::read(input).map_err(|e| e.to_string())?;
    let drawing = if input
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("plt"))
    {
        plt(&String::from_utf8(bytes).map_err(|e| e.to_string())?, units)?
    } else {
        svg(&bytes)?
    };
    let b = drawing.bounds();
    let width = a[2].parse::<f64>().map_err(|e| e.to_string())?;
    let s = Settings {
        width,
        height: width * (b[3] - b[1]) / (b[2] - b[0]),
        units,
        mirror: false,
        rotate: false,
    };
    let text = if plain {
        export_plain(&drawing, &s)?
    } else {
        export(&drawing, &s)?
    };
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|e| e.to_string())?;
    println!(
        "Saved {}: {:.3} x {:.3} mm, {} paths",
        output.display(),
        s.width,
        s.height,
        drawing.paths.len()
    );
    Ok(())
}
