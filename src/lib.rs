use std::fmt::Write;
pub mod sheet;
pub type Point = [f64; 2];
#[derive(Clone, Debug)]
pub struct Drawing {
    pub paths: Vec<Vec<Point>>,
    pub notes: Vec<String>,
}
impl Drawing {
    pub fn bounds(&self) -> [f64; 4] {
        let mut b = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for p in self.paths.iter().flatten() {
            b[0] = b[0].min(p[0]);
            b[1] = b[1].min(p[1]);
            b[2] = b[2].max(p[0]);
            b[3] = b[3].max(p[1]);
        }
        b
    }
    pub fn square() -> Self {
        Self {
            paths: vec![vec![[0., 0.], [10., 0.], [10., 10.], [0., 10.], [0., 0.]]],
            notes: vec![],
        }
    }
}
fn mid(a: Point, b: Point) -> Point {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}
fn cubic(a: Point, b: Point, c: Point, d: Point, tol: f64, depth: u32, out: &mut Vec<Point>) {
    let dx = d[0] - a[0];
    let dy = d[1] - a[1];
    let len = dx.hypot(dy);
    let dist = |p: Point| {
        if len < 1e-12 {
            (p[0] - a[0]).hypot(p[1] - a[1])
        } else {
            ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / len
        }
    };
    let polygon = (b[0] - a[0]).hypot(b[1] - a[1])
        + (c[0] - b[0]).hypot(c[1] - b[1])
        + (d[0] - c[0]).hypot(d[1] - c[1]);
    if depth >= 18 || (dist(b).max(dist(c)) <= tol && polygon - len <= tol) {
        out.push(d);
        return;
    }
    let ab = mid(a, b);
    let bc = mid(b, c);
    let cd = mid(c, d);
    let abc = mid(ab, bc);
    let bcd = mid(bc, cd);
    let m = mid(abc, bcd);
    cubic(a, ab, abc, m, tol, depth + 1, out);
    cubic(m, bcd, cd, d, tol, depth + 1, out);
}
pub fn svg(data: &[u8]) -> Result<Drawing, String> {
    let xml = std::str::from_utf8(data).map_err(|_| "Use an uncompressed UTF-8 SVG file.")?;
    // CorelDRAW X7 emits this external declaration even for plain vector paths.
    // Remove only that declaration, keeping custom DTDs disabled.
    const COREL_SVG_DTD: &str = r#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">"#;
    let cleaned;
    let xml = if xml.contains(COREL_SVG_DTD) {
        cleaned = xml.replacen(COREL_SVG_DTD, "", 1);
        cleaned.as_str()
    } else {
        xml
    };
    let document = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    for node in document.descendants().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "text" => return Err("Convert all text to paths before importing.".into()),
            "image" => return Err("Trace bitmap images to vector paths before importing.".into()),
            _ => {}
        }
    }
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(xml.as_bytes(), &opt).map_err(|e| e.to_string())?;
    let mut result = Drawing {
        paths: vec![],
        notes: vec![],
    };
    fn walk(g: &usvg::Group, r: &mut Drawing) -> Result<(), String> {
        if g.clip_path().is_some() || g.mask().is_some() {
            return Err(
                "Clipping/masks are unsupported. Flatten them to plain paths first.".into(),
            );
        }
        for n in g.children() {
            match n {
                usvg::Node::Group(g) => walk(g, r)?,
                usvg::Node::Path(p) => {
                    if !p.is_visible() {
                        continue;
                    }
                    let t = p.abs_transform();
                    let point = |p: usvg::tiny_skia_path::Point| {
                        let mut p = p;
                        t.map_point(&mut p);
                        [p.x as f64, p.y as f64]
                    };
                    let mut path = vec![];
                    let mut current = [0., 0.];
                    let mut first = [0., 0.];
                    for seg in p.data().segments() {
                        use usvg::tiny_skia_path::PathSegment::*;
                        match seg {
                            MoveTo(p) => {
                                if path.len() > 1 {
                                    r.paths.push(std::mem::take(&mut path));
                                } else {
                                    path.clear();
                                }
                                current = point(p);
                                first = current;
                                path.push(current);
                            }
                            LineTo(p) => {
                                current = point(p);
                                path.push(current);
                            }
                            QuadTo(b, d) => {
                                let b = point(b);
                                let d = point(d);
                                let c1 = [
                                    current[0] + (b[0] - current[0]) * 2. / 3.,
                                    current[1] + (b[1] - current[1]) * 2. / 3.,
                                ];
                                let c2 = [
                                    d[0] + (b[0] - d[0]) * 2. / 3.,
                                    d[1] + (b[1] - d[1]) * 2. / 3.,
                                ];
                                cubic(current, c1, c2, d, 0.02, 0, &mut path);
                                current = d;
                            }
                            CubicTo(b, c, d) => {
                                let d = point(d);
                                cubic(current, point(b), point(c), d, 0.02, 0, &mut path);
                                current = d;
                            }
                            Close => {
                                path.push(first);
                                current = first;
                            }
                        }
                    }
                    if path.len() > 1 {
                        r.paths.push(path);
                    }
                }
                usvg::Node::Text(_) => {
                    return Err(
                        "Convert text to paths in your design software before importing.".into(),
                    );
                }
                usvg::Node::Image(_) => {
                    return Err(
                        "Bitmap images cannot be cut. Trace the image to vector paths first."
                            .into(),
                    );
                }
            }
        }
        Ok(())
    }
    walk(tree.root(), &mut result)?;
    if result.paths.is_empty() {
        return Err("No vector paths found. Convert text to paths first.".into());
    }
    // SVG uses pixels at 96 dpi; store all drawings in millimetres, Y upwards.
    for p in result.paths.iter_mut().flatten() {
        p[0] *= 25.4 / 96.;
        p[1] *= -25.4 / 96.;
    }
    result.notes.push("Cuts vector path outlines/centerlines. Stroke thickness, fills and effects are not expanded.".into());
    Ok(result)
}
pub fn plt(text: &str, units: f64) -> Result<Drawing, String> {
    if !units.is_finite() || units <= 0. {
        return Err("Invalid units/mm".into());
    }
    let mut r = Drawing {
        paths: vec![],
        notes: vec![],
    };
    let mut cur = [0., 0.];
    let mut path = vec![];
    for raw in text.split(';') {
        let cmd = raw.trim().trim_start_matches('!');
        if cmd.is_empty() {
            continue;
        }
        if cmd.len() < 2 || !cmd.is_ascii() {
            return Err("Invalid PLT command".into());
        }
        let (op, args) = cmd.split_at(2);
        match op {
            "TB" | "CT" | "IN" | "PA" | "SP" | "PG" => {
                if op == "PA" && !args.trim().is_empty() {
                    return Err("PA coordinates are unsupported in this importer".into());
                }
            }
            "PU" | "PD" => {
                if op == "PU" && path.len() > 1 {
                    r.paths.push(std::mem::take(&mut path));
                } else if op == "PU" {
                    path.clear();
                }
                let nums: Vec<f64> = args
                    .split([',', ' ', '\r', '\n'])
                    .filter(|s| !s.is_empty())
                    .map(|s| {
                        s.parse::<f64>()
                            .map_err(|_| "Invalid PLT coordinate".to_string())
                    })
                    .collect::<Result<_, _>>()?;
                if nums.len() % 2 != 0 || nums.iter().any(|n| !n.is_finite()) {
                    return Err("Invalid PLT coordinate pair".into());
                }
                if op == "PD" && path.is_empty() {
                    path.push(cur);
                }
                for xy in nums.chunks_exact(2) {
                    cur = [xy[0] / units, xy[1] / units];
                    if op == "PD" {
                        path.push(cur);
                    }
                }
            }
            _ => return Err(format!("Unsupported PLT command: {op}")),
        }
    }
    if path.len() > 1 {
        r.paths.push(path);
    }
    if r.paths.is_empty() {
        return Err("No cutting paths found".into());
    }
    Ok(r)
}
#[derive(Clone)]
pub struct Settings {
    pub width: f64,
    pub height: f64,
    pub units: f64,
    pub mirror: bool,
    pub rotate: bool,
}
pub fn transformed(d: &Drawing, s: &Settings) -> Result<Vec<Vec<Point>>, String> {
    if [s.width, s.height, s.units]
        .iter()
        .any(|n| !n.is_finite() || *n <= 0.)
    {
        return Err("Dimensions and units/mm must be positive.".into());
    }
    if s.width > 630. || s.height > 10000. || s.units > 1000. {
        return Err("Limits: width 630 mm, length 10000 mm, units/mm 1000.".into());
    }
    let b = d.bounds();
    let w = b[2] - b[0];
    let h = b[3] - b[1];
    if !w.is_finite() || !h.is_finite() || w <= 0. || h <= 0. {
        return Err("Drawing needs non-zero width and height.".into());
    }
    Ok(d.paths
        .iter()
        .map(|path| {
            path.iter()
                .map(|p| {
                    let mut x = (p[0] - b[0]) / w;
                    let mut y = (p[1] - b[1]) / h;
                    if s.rotate {
                        (x, y) = (y, 1. - x);
                    }
                    if s.mirror {
                        x = 1. - x;
                    }
                    [x * s.width, y * s.height]
                })
                .collect()
        })
        .collect())
}
pub fn export(d: &Drawing, s: &Settings) -> Result<String, String> {
    let header = format!(
        "TB25,{},{};CT1;PA;PU0,0;",
        (s.width * s.units).round() as i64,
        (s.height * s.units).round() as i64
    );
    export_paths(d, s, header)
}

pub fn export_plain(d: &Drawing, s: &Settings) -> Result<String, String> {
    export_paths(d, s, "IN;PA;PU0,0;".into())
}

fn export_paths(d: &Drawing, s: &Settings, mut out: String) -> Result<String, String> {
    let paths = transformed(d, s)?;
    for path in paths {
        let mut points = path
            .iter()
            .map(|p| {
                [
                    (p[0] * s.units).round() as i64,
                    (p[1] * s.units).round() as i64,
                ]
            })
            .collect::<Vec<_>>();
        points.dedup();
        if points.len() < 2 {
            continue;
        }
        write!(out, "PU{},{};", points[0][0], points[0][1]).unwrap();
        for p in &points[1..] {
            write!(out, "PD{},{};", p[0], p[1]).unwrap();
        }
    }
    out.push_str("PU0,0;!PG;");
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> Settings {
        Settings {
            width: 10.,
            height: 10.,
            units: 40.,
            mirror: false,
            rotate: false,
        }
    }
    #[test]
    fn square_round_trip() {
        let s = settings();
        let text = export(&Drawing::square(), &s).unwrap();
        assert!(text.starts_with("TB25,400,400;CT1;PA;PU0,0;"));
        assert!(text.ends_with("PU0,0;!PG;"));
        let d = plt(&text, 40.).unwrap();
        assert_eq!(d.bounds(), [0., 0., 10., 10.]);
        assert_eq!(d.paths[0].first(), d.paths[0].last());
    }
    #[test]
    fn svg_transform_and_curve() {
        let d=svg(br#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><g transform="translate(10 20)"><path d="M0 0 C0 10 10 10 10 0 Z"/></g></svg>"#).unwrap();
        assert!(d.paths[0].len() > 10);
        let b = d.bounds();
        assert!((b[0] - 10.).abs() < 0.001);
        assert!((b[2] - 20.).abs() < 0.001);
    }
    #[test]
    fn reject_relative_plt() {
        assert!(plt("PR;PD1,2;", 40.).is_err());
    }
    #[test]
    fn reject_oversize() {
        let mut s = settings();
        s.width = 631.;
        assert!(export(&Drawing::square(), &s).is_err());
    }
}
