//! A4 artwork and cut paths share top-left millimetre coordinates.
use crate::Point;
use base64::Engine;
use image::RgbaImage;
use std::{collections::HashMap, fmt::Write};
/// Reuses physical contour geometry while copies are moved or rearranged.
#[derive(Default)]
pub struct OutlineCache {
    artwork: Option<u64>,
    entries: HashMap<[u64; 16], Result<Vec<Vec<Point>>, String>>,
}
impl OutlineCache {
    fn prepare(&mut self, art: &Artwork) {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        art.ellipse_bounds
            .map(|b| b.map(f64::to_bits))
            .hash(&mut hash);
        for paths in [&art.outlines, &art.holes] {
            paths.len().hash(&mut hash);
            for path in paths {
                path.len().hash(&mut hash);
                for p in path {
                    p[0].to_bits().hash(&mut hash);
                    p[1].to_bits().hash(&mut hash);
                }
            }
        }
        let key = hash.finish();
        if self.artwork != Some(key) {
            self.entries.clear();
            self.artwork = Some(key);
        }
    }
}
type Pixel = (i32, i32);
pub const LOWER_LEFT_MARK: Point = [7., 7.];

/// Display cutter coordinates relative to the physical origin; export is unchanged.
pub fn cutter_to_page(point: Point, origin: Point) -> Point {
    cutter_to_page_on(point, origin, 297.)
}
pub fn cutter_to_page_on(point: Point, origin: Point, page_height: f64) -> Point {
    [point[0] + origin[0], page_height - (point[1] + origin[1])]
}
pub const PAPER_PRESETS: &[(&str, f64, f64)] = &[
    ("A4", 210., 297.),
    ("A3", 297., 420.),
    ("A5", 148., 210.),
    ("A6", 105., 148.),
    ("A2", 420., 594.),
    ("A1", 594., 841.),
    ("Letter", 215.9, 279.4),
    ("Legal", 215.9, 355.6),
    ("Tabloid", 279.4, 431.8),
    ("B4 (ISO)", 250., 353.),
    ("B5 (ISO)", 176., 250.),
];

/// Research profile: four solid circles observed in the manufacturer's MC630 video.
/// Dimensions and TB25 mapping are hypotheses, not a certified Ver5.022 protocol.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CcdTrial {
    pub diameter: f64,
    pub inset: f64,
}
impl Default for CcdTrial {
    fn default() -> Self {
        Self {
            diameter: 5.5,
            inset: 7.,
        }
    }
}
impl CcdTrial {
    /// Dot centers surround the occupied image frames, not the A4 sheet.
    pub fn marks(&self, layout: &Layout) -> [Point; 4] {
        let items: Vec<_> = layout
            .items()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !layout.hidden_images.contains(i))
            .map(|(_, placement)| placement)
            .collect();
        if items.is_empty() {
            return [[0.; 2]; 4];
        }
        let left = items.iter().map(|p| p.x).fold(layout.page_width, f64::min) - self.inset;
        let top = items.iter().map(|p| p.y).fold(layout.page_height, f64::min) - self.inset;
        let right = items.iter().map(|p| p.x + p.width).fold(0., f64::max) + self.inset;
        let bottom = items.iter().map(|p| p.y + p.height).fold(0., f64::max) + self.inset;
        [[left, top], [right, top], [left, bottom], [right, bottom]]
    }
    pub fn origin(&self, layout: &Layout) -> Point {
        let p = self.marks(layout)[0];
        [p[0], layout.page_height - p[1]]
    }
    /// Top-left dot is (0,0); positive Y runs down the printed sheet.
    pub fn cut_paths(&self, layout: &Layout, art: &Artwork) -> Result<Vec<Vec<Point>>, String> {
        self.cut_paths_cached(layout, art, &mut OutlineCache::default())
    }
    pub fn cut_paths_cached(
        &self,
        layout: &Layout,
        art: &Artwork,
        cache: &mut OutlineCache,
    ) -> Result<Vec<Vec<Point>>, String> {
        let origin = self.marks(layout)[0];
        let paths = layout
            .page_cut_paths_cached(art, cache)?
            .into_iter()
            .map(|path| {
                path.into_iter()
                    .map(|p| [p[0] - origin[0], p[1] - origin[1]])
                    .collect()
            })
            .collect();
        Ok(paths)
    }
    pub fn pair(&self, layout: &Layout, art: &Artwork) -> Result<(String, String), String> {
        if !layout.has_visible_images() {
            return Err("Show at least one image before exporting.".into());
        }
        if !self.diameter.is_finite()
            || !self.inset.is_finite()
            || !(1. ..=10.).contains(&self.diameter)
            || !(5. ..=30.).contains(&self.inset)
        {
            return Err("Invalid CCD trial mark geometry.".into());
        }
        layout.positions()?;
        let dots = self.marks(layout);
        let radius = self.diameter / 2.;
        if self.inset < radius + 2. {
            return Err("Leave at least 2 mm clear space between dots and image frames.".into());
        }
        if dots.iter().any(|p| {
            p[0] - radius < 3.
                || p[1] - radius < 3.
                || p[0] + radius > layout.page_width - 3.
                || p[1] + radius > layout.page_height - 3.
        }) {
            return Err("CCD dots need 3 mm page clearance. Move images away from page edges, increase margin or reduce image size / dot gap.".into());
        }
        if layout.flip_x || layout.flip_y {
            return Err(
                "CCD trial currently requires Reverse X/Y off; firmware axes are unverified."
                    .into(),
            );
        }
        let quiet = self.diameter / 2. + 2.;
        for p in self.cut_paths(layout, art)?.iter().flatten() {
            let x = p[0] + dots[0][0];
            let y = p[1] + dots[0][1];
            if x < dots[0][0] + quiet
                || x > dots[1][0] - quiet
                || y < dots[0][1] + quiet
                || y > dots[2][1] - quiet
            {
                return Err("Cut border enters CCD mark clearance. Increase Dot center gap or reduce Cut border.".into());
            }
        }
        let mut html = layout.print_html(art)?;
        for (x, y) in layout.manual_marks() {
            let old = format!(
                "<path d=\"M{},{}h4 M{},{}v4\" fill=\"none\" stroke=\"black\" stroke-width=\"0.2\"/>",
                x - 2.,
                y,
                x,
                y - 2.
            );
            html = html.replace(&old, "");
        }
        let mut marks = String::new();
        for p in self.marks(layout) {
            write!(
                marks,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"black\"/>",
                p[0],
                p[1],
                self.diameter / 2.
            )
            .unwrap();
        }
        html = html.replace("</svg>", &format!("{marks}</svg>"));
        html=html.replace("Printed crosses are manual alignment references only.","EXPERIMENTAL MC CCD trial. Four-circle shape observed in manufacturer video; dimensions and Ver5.022 compatibility are unverified. Remove blade before testing.");
        let mark_header = format!(
            "TB25,{},{};",
            ((dots[2][1] - dots[0][1]) * layout.units).round() as i64,
            ((dots[1][0] - dots[0][0]) * layout.units).round() as i64
        );
        let mut plt = format!("{mark_header}CT1;PA;PU0,0;");
        // MC630 CCD machine order: feed (page Y), then carriage (page X).
        // Keep cut_paths in page coordinates for the preview; transpose only on export.
        for path in self.cut_paths(layout, art)? {
            write_plt_path(&mut plt, &path, layout.units, true);
        }
        plt.push_str("PU0,0;!PG;");
        Ok((html, plt))
    }
}
fn write_plt_path(out: &mut String, path: &[Point], units: f64, transpose: bool) {
    let mut previous = None;
    for &[x, y] in path {
        let (x, y) = if transpose { (y, x) } else { (x, y) };
        let point = ((x * units).round() as i64, (y * units).round() as i64);
        if previous != Some(point) {
            let command = if previous.is_none() { "PU" } else { "PD" };
            write!(out, "{command}{},{};", point.0, point.1).unwrap();
            previous = Some(point);
        }
    }
}
pub struct Artwork {
    pub png: Vec<u8>,
    pub pixels: RgbaImage,
    pub outlines: Vec<Vec<Point>>,
    pub holes: Vec<Vec<Point>>,
    pub tiles: Vec<std::sync::Arc<Artwork>>,
    pub ellipse_bounds: Option<[f64; 4]>,
}
impl Artwork {
    fn at(&self, index: usize) -> &Self {
        self.tiles.get(index).map(AsRef::as_ref).unwrap_or(self)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CutShape {
    #[default]
    AlphaOutline,
    ImageFrame,
    PlainBackgroundObject,
    Ellipse,
}
impl CutShape {
    pub fn label(self) -> &'static str {
        match self {
            Self::AlphaOutline => "Image / transparent outline",
            Self::ImageFrame => "Original image frame",
            Self::PlainBackgroundObject => "Object / plain background",
            Self::Ellipse => "Circle / ellipse",
        }
    }
}
pub fn artwork(bytes: &[u8], remove_white: bool, white: u8) -> Result<Artwork, String> {
    artwork_with_shape(bytes, remove_white, white, CutShape::AlphaOutline, 40, true)
}
pub fn artwork_with_shape(
    bytes: &[u8],
    remove_white: bool,
    white: u8,
    shape: CutShape,
    tolerance: u8,
    print_without_background: bool,
) -> Result<Artwork, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8000);
    limits.max_image_height = Some(8000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|e| e.to_string())?;
    trace_artwork(
        decoded.into_rgba8(),
        remove_white,
        white,
        shape,
        tolerance,
        print_without_background,
    )
}

fn trace_artwork(
    mut pixels: RgbaImage,
    remove_white: bool,
    white: u8,
    shape: CutShape,
    tolerance: u8,
    print_without_background: bool,
) -> Result<Artwork, String> {
    if remove_white && shape != CutShape::ImageFrame {
        for p in pixels.pixels_mut() {
            if p[0] >= white && p[1] >= white && p[2] >= white {
                p[3] = 0;
            }
        }
    }
    let mut thumb = if shape == CutShape::PlainBackgroundObject {
        image::imageops::thumbnail(&pixels, pixels.width().min(1000), pixels.height().min(1000))
    } else {
        image::imageops::thumbnail(&pixels, 1000, 1000)
    };
    if shape == CutShape::PlainBackgroundObject {
        remove_edge_background(&mut thumb, tolerance)?;
        if print_without_background {
            let alpha = image::imageops::resize(
                &thumb,
                pixels.width(),
                pixels.height(),
                image::imageops::FilterType::Triangle,
            );
            for (p, mask) in pixels.pixels_mut().zip(alpha.pixels()) {
                p[3] = p[3].min(mask[3]);
            }
        }
    }
    let ellipse_bounds = if shape == CutShape::Ellipse {
        let mut bounds = [pixels.width(), pixels.height(), 0, 0];
        for (x, y, pixel) in pixels.enumerate_pixels() {
            if pixel[3] >= 128 {
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x + 1);
                bounds[3] = bounds[3].max(y + 1);
            }
        }
        if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
            return Err("No visible image bounds for an ellipse.".into());
        }
        let [x0, y0, x1, y1] = bounds.map(f64::from);
        let (w, h) = (f64::from(pixels.width()), f64::from(pixels.height()));
        Some([x0 / w, y0 / h, x1 / w, y1 / h])
    } else {
        None
    };
    let (outlines, holes) = if shape == CutShape::ImageFrame || ellipse_bounds.is_some() {
        (
            vec![vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.], [0., 0.]]],
            vec![],
        )
    } else {
        trace(&thumb)?
    };
    let mut png = std::io::Cursor::new(Vec::new());
    pixels
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(Artwork {
        png: png.into_inner(),
        pixels,
        outlines,
        holes,
        tiles: vec![],
        ellipse_bounds,
    })
}
/// Boundary-connected colour removal for simple backgrounds; not semantic AI segmentation.
/// Corner colours interpolate across the image to accommodate gentle gradients.
fn remove_edge_background(img: &mut RgbaImage, tolerance: u8) -> Result<(), String> {
    use std::collections::VecDeque;
    let (w, h) = img.dimensions();
    if w < 4 || h < 4 {
        return Err("Object mode needs an image at least 4 x 4 pixels.".into());
    }
    let patch = (w.min(h) / 30).clamp(1, 12);
    let mut corners = [[0.; 3]; 4];
    for (i, (ox, oy)) in [
        (0, 0),
        (w - patch, 0),
        (0, h - patch),
        (w - patch, h - patch),
    ]
    .into_iter()
    .enumerate()
    {
        for channel in 0..3 {
            let mut values = Vec::new();
            for y in oy..oy + patch {
                for x in ox..ox + patch {
                    let p = img.get_pixel(x, y);
                    if p[3] >= 128 {
                        values.push(p[channel]);
                    }
                }
            }
            values.sort_unstable();
            corners[i][channel] = values.get(values.len() / 2).copied().unwrap_or(255) as f64;
        }
    }
    let matches = |x: u32, y: u32| {
        let p = img.get_pixel(x, y);
        if p[3] < 128 {
            return true;
        }
        let tx = x as f64 / (w - 1) as f64;
        let ty = y as f64 / (h - 1) as f64;
        let distance: f64 = (0..3)
            .map(|c| {
                let top = corners[0][c] * (1. - tx) + corners[1][c] * tx;
                let bottom = corners[2][c] * (1. - tx) + corners[3][c] * tx;
                let d = p[c] as f64 - (top * (1. - ty) + bottom * ty);
                d * d
            })
            .sum();
        distance <= 3. * (tolerance as f64).powi(2)
    };
    let mut removed = vec![false; (w * h) as usize];
    let mut queue = VecDeque::new();
    for x in 0..w {
        queue.push_back((x, 0));
        queue.push_back((x, h - 1));
    }
    for y in 0..h {
        queue.push_back((0, y));
        queue.push_back((w - 1, y));
    }
    while let Some((x, y)) = queue.pop_front() {
        let index = (y * w + x) as usize;
        if removed[index] || !matches(x, y) {
            continue;
        }
        removed[index] = true;
        if x > 0 {
            queue.push_back((x - 1, y));
        }
        if x + 1 < w {
            queue.push_back((x + 1, y));
        }
        if y > 0 {
            queue.push_back((x, y - 1));
        }
        if y + 1 < h {
            queue.push_back((x, y + 1));
        }
    }
    let mut remaining = 0usize;
    for (i, p) in img.pixels_mut().enumerate() {
        if removed[i] {
            p[3] = 0;
        }
        if p[3] >= 128 {
            remaining += 1;
        }
    }
    if remaining < 4 {
        return Err(
            "No object remains. Lower Background tolerance or use a transparent PNG.".into(),
        );
    }
    Ok(())
}
fn trace(img: &RgbaImage) -> Result<(Vec<Vec<Point>>, Vec<Vec<Point>>), String> {
    let w = img.width() as i32;
    let h = img.height() as i32;
    let filled = |x: i32, y: i32| {
        x >= 0 && y >= 0 && x < w && y < h && img.get_pixel(x as u32, y as u32)[3] >= 128
    };
    let mut edges: HashMap<Pixel, Vec<Pixel>> = HashMap::new();
    let mut total = 0;
    for y in 0..h {
        for x in 0..w {
            if !filled(x, y) {
                continue;
            }
            for (neighbor, a, b) in [
                ((x, y - 1), (x, y), (x + 1, y)),
                ((x + 1, y), (x + 1, y), (x + 1, y + 1)),
                ((x, y + 1), (x + 1, y + 1), (x, y + 1)),
                ((x - 1, y), (x, y + 1), (x, y)),
            ] {
                if !filled(neighbor.0, neighbor.1) {
                    edges.entry(a).or_default().push(b);
                    total += 1;
                }
            }
        }
    }
    if total > 200_000 {
        return Err("Outline is too complex. Use a cleaner transparent image.".into());
    }
    let mut loops = vec![];
    let mut holes = vec![];
    while let Some(start) = edges.keys().min().copied() {
        let mut cur = start;
        let mut prev = (start.0 - 1, start.1);
        let mut path = vec![start];
        loop {
            let candidates = edges.get_mut(&cur).ok_or("Broken outline")?;
            let dx = cur.0 - prev.0;
            let dy = cur.1 - prev.1;
            let score = |p: &Pixel| {
                let nx = p.0 - cur.0;
                let ny = p.1 - cur.1;
                let cross = dx * ny - dy * nx;
                let dot = dx * nx + dy * ny;
                if cross > 0 {
                    0
                } else if dot > 0 {
                    1
                } else if cross < 0 {
                    2
                } else {
                    3
                }
            };
            let idx = (0..candidates.len())
                .min_by_key(|i| score(&candidates[*i]))
                .ok_or("Broken outline")?;
            let next = candidates.swap_remove(idx);
            if candidates.is_empty() {
                edges.remove(&cur);
            }
            prev = cur;
            cur = next;
            path.push(cur);
            if cur == start {
                break;
            }
            if path.len() > total + 1 {
                return Err("Outline loop failed".into());
            }
        }
        let area: i64 = path
            .windows(2)
            .map(|p| p[0].0 as i64 * p[1].1 as i64 - p[1].0 as i64 * p[0].1 as i64)
            .sum();
        // Positive loops enclose foreground; negative loops enclose transparent holes.
        if area.abs() < 8 {
            continue;
        }
        let mut clean = vec![path[0]];
        for i in 1..path.len() - 1 {
            let a = path[i - 1];
            let b = path[i];
            let c = path[i + 1];
            if (b.0 - a.0) * (c.1 - b.1) != (b.1 - a.1) * (c.0 - b.0) {
                clean.push(b);
            }
        }
        clean.push(start);
        let raw = clean
            .iter()
            .map(|p| [p.0 as f64, p.1 as f64])
            .collect::<Vec<_>>();
        let mut simple = vec![raw[0]];
        simplify(&raw, 0.65, &mut simple);
        for p in &mut simple {
            p[0] /= w as f64;
            p[1] /= h as f64;
        }
        if area > 0 {
            loops.push(simple);
        } else {
            holes.push(simple);
        }
    }
    if loops.is_empty() {
        return Err("No outline found. Use a transparent PNG or adjust white removal.".into());
    }
    Ok((loops, holes))
}
// Smooth by physical arc length, independent of source pixel/vertex density.
// A periodic Gaussian removes small zigzags, including across the closing seam.
fn smooth_contour(path: &[Point], amount: f64) -> Vec<Point> {
    if amount == 0. || path.len() < 4 {
        return path.to_vec();
    }
    let mut ring = path.to_vec();
    if ring.first() == ring.last() {
        ring.pop();
    }
    ring.dedup();
    if ring.len() < 3 {
        return path.to_vec();
    }
    let mut lengths = vec![0.];
    for i in 0..ring.len() {
        let p = ring[i];
        let q = ring[(i + 1) % ring.len()];
        lengths.push(lengths.last().unwrap() + (q[0] - p[0]).hypot(q[1] - p[1]));
    }
    let perimeter = *lengths.last().unwrap();
    if !perimeter.is_finite() || perimeter <= 1e-9 {
        return path.to_vec();
    }
    // Limit smoothing on tiny islands/holes rather than collapsing them.
    let radius = amount.min(perimeter / 12.);
    let count = ((perimeter / (radius / 6.).clamp(0.005, 0.15)).ceil() as usize).clamp(16, 100_000);
    let step = perimeter / count as f64;
    let mut samples = Vec::with_capacity(count);
    let mut edge = 0;
    for i in 0..count {
        let distance = i as f64 * step;
        while edge + 1 < ring.len() && lengths[edge + 1] <= distance {
            edge += 1;
        }
        let span = lengths[edge + 1] - lengths[edge];
        let t = if span > 0. {
            (distance - lengths[edge]) / span
        } else {
            0.
        };
        let p = ring[edge];
        let q = ring[(edge + 1) % ring.len()];
        samples.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
    }
    let window = ((radius * 1.5 / step).ceil() as usize)
        .min(count / 2)
        .min(128);
    let sigma = radius / 2.;
    let weights: Vec<_> = (0..=window)
        .map(|i| (-0.5 * (i as f64 * step / sigma).powi(2)).exp())
        .collect();
    let total = weights[0] + 2. * weights[1..].iter().sum::<f64>();
    let mut rounded = Vec::with_capacity(count + 1);
    for i in 0..count {
        let mut p = [samples[i][0] * weights[0], samples[i][1] * weights[0]];
        for j in 1..=window {
            let a = samples[(i + j) % count];
            let b = samples[(i + count - j) % count];
            p[0] += (a[0] + b[0]) * weights[j];
            p[1] += (a[1] + b[1]) * weights[j];
        }
        rounded.push([p[0] / total, p[1] / total]);
    }
    rounded.push(rounded[0]);
    let mut compact = vec![rounded[0]];
    simplify(&rounded, 0.005, &mut compact);
    if compact.len() >= 4 { compact } else { rounded }
}

fn simplify(p: &[Point], tol: f64, out: &mut Vec<Point>) {
    if p.len() < 2 {
        return;
    }
    let a = p[0];
    let b = p[p.len() - 1];
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let den = dx * dx + dy * dy;
    let dist = |q: Point| {
        let t = if den > 0. {
            (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / den).clamp(0., 1.)
        } else {
            0.
        };
        (q[0] - a[0] - t * dx).hypot(q[1] - a[1] - t * dy)
    };
    let furthest = (1..p.len() - 1)
        .map(|i| (i, dist(p[i])))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((i, d)) = furthest {
        if d > tol {
            simplify(&p[..=i], tol, out);
            simplify(&p[i..], tol, out);
            return;
        }
    }
    out.push(b);
}
/// Physical page placement of one repeated image, measured from the top-left in mm.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Placement {
    pub fn moved(self, dx: f64, dy: f64, inset: f64) -> Self {
        self.moved_on(dx, dy, inset, [210., 297.])
    }
    pub fn moved_on(self, dx: f64, dy: f64, inset: f64, page: [f64; 2]) -> Self {
        Self {
            x: (self.x + dx).clamp(inset, (page[0] - inset - self.width).max(inset)),
            y: (self.y + dy).clamp(inset, (page[1] - inset - self.height).max(inset)),
            ..self
        }
    }
    pub fn resized(self, corner: usize, dx: f64, dy: f64, lock: bool, inset: f64) -> Self {
        self.resized_on(corner, dx, dy, lock, inset, [210., 297.])
    }
    pub fn resized_on(
        self,
        corner: usize,
        dx: f64,
        dy: f64,
        lock: bool,
        inset: f64,
        page: [f64; 2],
    ) -> Self {
        let left = corner == 0 || corner == 3;
        let top = corner < 2;
        let ax = if left { self.x + self.width } else { self.x };
        let ay = if top { self.y + self.height } else { self.y };
        let maxw = if left {
            ax - inset
        } else {
            page[0] - inset - ax
        };
        let maxh = if top {
            ay - inset
        } else {
            page[1] - inset - ay
        };
        let raww = self.width + if left { -dx } else { dx };
        let rawh = self.height + if top { -dy } else { dy };
        let (width, height) = if lock {
            let min = (1. / self.width).max(1. / self.height);
            let max = (maxw / self.width).min(maxh / self.height).max(min);
            let factor = ((raww * self.width + rawh * self.height)
                / (self.width.powi(2) + self.height.powi(2)))
            .clamp(min, max);
            (self.width * factor, self.height * factor)
        } else {
            (raww.clamp(1., maxw.max(1.)), rawh.clamp(1., maxh.max(1.)))
        };
        Self {
            x: if left { ax - width } else { ax },
            y: if top { ay - height } else { ay },
            width,
            height,
        }
    }
}
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Layout {
    pub page_width: f64,
    pub page_height: f64,
    pub width: f64,
    pub height: f64,
    pub column_gap: f64,
    pub row_gap: f64,
    /// Signed contour distance in physical mm; does not resize the printed image.
    pub contour_offset: f64,
    pub smoothing: f64,
    pub cut_inner_holes: bool,
    pub custom: std::collections::BTreeMap<usize, Placement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_tiles: Vec<[u32; 4]>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub hidden_images: std::collections::BTreeSet<usize>,
    /// Vertical (X) and horizontal (Y) guides in page millimetres.
    #[serde(default)]
    pub guides: [Vec<f64>; 2],
    pub margin: f64,
    pub count: usize,
    pub units: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub flip_x: bool,
    pub flip_y: bool,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            page_width: 210.,
            page_height: 297.,
            width: 35.,
            height: 35.,
            column_gap: 3.,
            row_gap: 3.,
            contour_offset: 0.,
            smoothing: 0.,
            cut_inner_holes: false,
            custom: Default::default(),
            image_tiles: vec![],
            hidden_images: Default::default(),
            guides: Default::default(),
            margin: 15.,
            count: 20,
            units: 40.,
            offset_x: 0.,
            offset_y: 0.,
            flip_x: false,
            flip_y: false,
        }
    }
}
impl Layout {
    pub fn has_visible_images(&self) -> bool {
        (0..self.count).any(|index| !self.hidden_images.contains(&index))
    }
    pub fn trace_images(
        &self,
        bytes: &[u8],
        remove_white: bool,
        white: u8,
        shape: CutShape,
        tolerance: u8,
        print_without_background: bool,
    ) -> Result<Artwork, String> {
        if self.image_tiles.is_empty() {
            return artwork_with_shape(
                bytes,
                remove_white,
                white,
                shape,
                tolerance,
                print_without_background,
            );
        }
        if self.image_tiles.len() != self.count
            || self.count > 24
            || self.custom.len() != self.count
        {
            return Err("Invalid multi-image layout.".into());
        }
        let mut atlas =
            artwork_with_shape(bytes, false, white, CutShape::ImageFrame, tolerance, false)?;
        let mut traced = HashMap::new();
        let mut total_pixels = 0_u64;
        for (i, &rect) in self.image_tiles.iter().enumerate() {
            let [x, y, w, h] = rect;
            if w == 0
                || h == 0
                || x.saturating_add(w) > atlas.pixels.width()
                || y.saturating_add(h) > atlas.pixels.height()
                || !self.custom.contains_key(&i)
            {
                return Err("Invalid image bounds in multi-image project.".into());
            }
            if let Some(tile) = traced.get(&rect) {
                atlas.tiles.push(std::sync::Arc::clone(tile));
                continue;
            }
            total_pixels += u64::from(w) * u64::from(h);
            if total_pixels > 32_000_000 {
                return Err("Multi-image artwork exceeds the pixel limit.".into());
            }
            let pixels = image::imageops::crop_imm(&atlas.pixels, x, y, w, h).to_image();
            let tile = trace_artwork(
                pixels,
                remove_white,
                white,
                shape,
                tolerance,
                print_without_background,
            )
            .map_err(|e| format!("Image {}: {e}", i + 1))?;
            let tile = std::sync::Arc::new(tile);
            traced.insert(rect, std::sync::Arc::clone(&tile));
            atlas.tiles.push(tile);
        }
        for ([x, y, _, _], tile) in traced {
            image::imageops::replace(&mut atlas.pixels, &tile.pixels, x.into(), y.into());
        }
        Ok(atlas)
    }
    pub fn paper_label(&self) -> String {
        for &(name, w, h) in PAPER_PRESETS {
            if (self.page_width - w).abs() < 0.1 && (self.page_height - h).abs() < 0.1 {
                return format!("{name} Portrait");
            }
            if (self.page_width - h).abs() < 0.1 && (self.page_height - w).abs() < 0.1 {
                return format!("{name} Landscape");
            }
        }
        format!("Custom {:.1} x {:.1} mm", self.page_width, self.page_height)
    }
    pub fn manual_marks(&self) -> [(f64, f64); 4] {
        [
            (7., 7.),
            (self.page_width - 7., 7.),
            (7., self.page_height - 7.),
            (self.page_width - 7., self.page_height - 7.),
        ]
    }

    /// Build one sticker's outer cut contours in physical millimetres.
    pub fn outline_mm(&self, art: &Artwork) -> Result<Vec<Vec<Point>>, String> {
        if !self.contour_offset.is_finite() || self.contour_offset.abs() > 5. {
            return Err("Cut border must be between -5 and +5 mm.".into());
        }
        if let Some([x0, y0, x1, y1]) = art.ellipse_bounds {
            let cx = (x0 + x1) * self.width / 2.;
            let cy = (y0 + y1) * self.height / 2.;
            let rx = (x1 - x0) * self.width / 2.;
            let ry = (y1 - y0) * self.height / 2.;
            let offset = self.contour_offset;
            if -offset >= rx.min(ry).powi(2) / rx.max(ry) {
                return Err("Negative cut border is too large for this ellipse.".into());
            }
            // Sample the mathematical curve, then offset along its outward normal.
            let radius = rx.max(ry) + offset.abs();
            let count = ((std::f64::consts::TAU * radius / 0.4).ceil() as usize)
                .clamp(96, 4096)
                .next_multiple_of(4);
            let mut path = Vec::with_capacity(count + 1);
            for i in 0..count {
                let (sin, cos) = (std::f64::consts::TAU * i as f64 / count as f64).sin_cos();
                let (nx, ny) = (ry * cos, rx * sin);
                let norm = nx.hypot(ny);
                path.push([
                    cx + rx * cos + offset * nx / norm,
                    cy + ry * sin + offset * ny / norm,
                ]);
            }
            path.push(path[0]);
            let paths = vec![path];
            if offset != 0. {
                self.validate_outline_spacing(&paths)?;
            }
            return Ok(paths);
        }
        let mut original: Vec<Vec<Point>> = art
            .outlines
            .iter()
            .map(|path| {
                path.iter()
                    .map(|p| [p[0] * self.width, p[1] * self.height])
                    .collect()
            })
            .collect();
        let mut holes: Vec<Vec<Point>> = if self.cut_inner_holes {
            art.holes
                .iter()
                .map(|path| {
                    path.iter()
                        .map(|p| [p[0] * self.width, p[1] * self.height])
                        .collect()
                })
                .collect()
        } else {
            vec![]
        };
        if !self.smoothing.is_finite() || !(0.0..=4.0).contains(&self.smoothing) {
            return Err("Smoothing must be between 0 and 4 mm.".into());
        }
        if self.smoothing > 0. && self.contour_offset == 0. {
            for path in original.iter_mut().chain(holes.iter_mut()) {
                *path = smooth_contour(path, self.smoothing);
            }
        }
        if self.contour_offset == 0. {
            // Cut internal details before releasing the outside boundary.
            return Ok(holes.into_iter().chain(original).collect());
        }
        use geo::{Area, Buffer, Contains, LineString, MultiPolygon, Polygon};
        let line = |path: &Vec<Point>| {
            LineString::from(path.iter().map(|p| (p[0], p[1])).collect::<Vec<_>>())
        };
        let outer: Vec<_> = original
            .iter()
            .map(|path| Polygon::new(line(path), vec![]))
            .collect();
        let mut inner: Vec<Vec<LineString<f64>>> = vec![vec![]; outer.len()];
        for hole in &holes {
            let probe = geo::Point::new(hole[0][0], hole[0][1]);
            // Associate holes with the smallest containing outer component (nested islands supported).
            if let Some((owner, _)) = outer
                .iter()
                .enumerate()
                .filter(|(_, p)| p.contains(&probe))
                .min_by(|(_, a), (_, b)| a.unsigned_area().total_cmp(&b.unsigned_area()))
            {
                inner[owner].push(line(hole));
            }
        }
        let polygons = MultiPolygon(
            outer
                .into_iter()
                .zip(inner)
                .map(|(p, h)| Polygon::new(p.exterior().clone(), h))
                .collect(),
        );
        let result = polygons.buffer(self.contour_offset);
        let mut outlines = Vec::new();
        for polygon in &result.0 {
            if self.cut_inner_holes {
                for hole in polygon.interiors() {
                    outlines.push(hole.0.iter().map(|c| [c.x, c.y]).collect::<Vec<_>>());
                }
            }
            outlines.push(
                polygon
                    .exterior()
                    .0
                    .iter()
                    .map(|c| [c.x, c.y])
                    .collect::<Vec<_>>(),
            );
        }
        if outlines.is_empty() {
            return Err(
                "Cut border removes the entire outline. Use a smaller negative value.".into(),
            );
        }
        // Smooth the final offset geometry: buffering can introduce new corners.
        if self.smoothing > 0. {
            for path in &mut outlines {
                *path = smooth_contour(path, self.smoothing);
            }
        }
        self.validate_outline_spacing(&outlines)?;
        Ok(outlines)
    }
    fn validate_outline_spacing(&self, outlines: &[Vec<Point>]) -> Result<(), String> {
        // Keep repeated sticker contours inside their own spacing cells.
        let points: Vec<_> = outlines.iter().flatten().collect();
        let minx = points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        let maxx = points
            .iter()
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max);
        let miny = points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        let maxy = points
            .iter()
            .map(|p| p[1])
            .fold(f64::NEG_INFINITY, f64::max);
        let (cols, _) = self.capacity()?;
        if (self.count > 1 && cols > 1 && maxx - minx >= self.width + self.column_gap)
            || (self.count > 1 && self.count > cols && maxy - miny >= self.height + self.row_gap)
        {
            return Err(
                "Cut borders may overlap adjacent stickers. Increase Spacing or reduce Cut border."
                    .into(),
            );
        }
        Ok(())
    }
    /// A4 artwork raster for direct Windows printing. No cut lines or origin labels.
    /// The same millimetre placements and registration geometry as print_html/pair.
    pub fn print_raster(&self, a: &Artwork, ccd: Option<&CcdTrial>) -> Result<RgbaImage, String> {
        if !self.has_visible_images() {
            return Err("Show at least one image before printing.".into());
        }
        let items = self.items()?;
        if let Some(profile) = ccd {
            profile.pair(self, a)?;
        }
        let dpi = 300.;
        let mm = |x: f64| (x / 25.4 * dpi).round() as u32;
        if u64::from(mm(self.page_width)) * u64::from(mm(self.page_height)) > 80_000_000 {
            return Err("Page is too large for 300 dpi direct printing. Use a smaller page or export the print sheet.".into());
        }
        let mut page = RgbaImage::from_pixel(
            mm(self.page_width),
            mm(self.page_height),
            image::Rgba([255, 255, 255, 255]),
        );
        let mut cache = HashMap::new();
        for (i, item) in items.into_iter().enumerate() {
            if self.hidden_images.contains(&i) {
                continue;
            }
            let key = (
                mm(item.width).max(1),
                mm(item.height).max(1),
                if a.tiles.is_empty() { 0 } else { i },
            );
            let raster = cache.entry(key).or_insert_with(|| {
                image::imageops::resize(
                    &a.at(i).pixels,
                    key.0,
                    key.1,
                    image::imageops::FilterType::Lanczos3,
                )
            });
            image::imageops::overlay(&mut page, raster, mm(item.x) as i64, mm(item.y) as i64);
        }
        let mut rect = |x: f64, y: f64, w: f64, h: f64| {
            for py in mm(y)..mm(y + h).min(page.height()) {
                for px in mm(x)..mm(x + w).min(page.width()) {
                    page.put_pixel(px, py, image::Rgba([0, 0, 0, 255]));
                }
            }
        };
        if ccd.is_none() {
            for (x, y) in self.manual_marks() {
                rect(x - 2., y - 0.1, 4., 0.2);
                rect(x - 0.1, y - 2., 0.2, 4.);
            }
        }
        if let Some(profile) = ccd {
            for center in profile.marks(self) {
                let cx = center[0] / 25.4 * dpi;
                let cy = center[1] / 25.4 * dpi;
                let r = profile.diameter / 2. / 25.4 * dpi;
                for y in (cy - r - 1.).floor().max(0.) as u32
                    ..(cy + r + 1.).ceil().min(page.height() as f64) as u32
                {
                    for x in (cx - r - 1.).floor().max(0.) as u32
                        ..(cx + r + 1.).ceil().min(page.width() as f64) as u32
                    {
                        let coverage = (r + 0.5 - (x as f64 + 0.5 - cx).hypot(y as f64 + 0.5 - cy))
                            .clamp(0., 1.);
                        if coverage > 0. {
                            let c = ((1. - coverage) * 255.).round() as u8;
                            page.put_pixel(x, y, image::Rgba([c, c, c, 255]));
                        }
                    }
                }
            }
        }
        Ok(page)
    }
    pub fn capacity(&self) -> Result<(usize, usize), String> {
        if [
            self.page_width,
            self.page_height,
            self.width,
            self.height,
            self.column_gap,
            self.row_gap,
            self.margin,
            self.units,
            self.offset_x,
            self.offset_y,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || !(50. ..=630.).contains(&self.page_width)
            || !(50. ..=2000.).contains(&self.page_height)
            || self.width < 1.
            || self.height < 1.
            || self.column_gap < 0.
            || self.row_gap < 0.
            || self.margin < 5.
            || self.units <= 0.
            || self.units > 1000.
        {
            return Err("Invalid layout values. Margin must be at least 5 mm.".into());
        }
        let cols = ((self.page_width - 2. * self.margin + self.column_gap)
            / (self.width + self.column_gap))
            .floor()
            .max(0.) as usize;
        let rows = ((self.page_height - 2. * self.margin + self.row_gap)
            / (self.height + self.row_gap))
            .floor()
            .max(0.) as usize;
        Ok((cols, rows))
    }
    fn resolved_items(&self, fill: bool) -> Result<Vec<Placement>, String> {
        let (cols, rows) = self.capacity()?;
        let nominal = cols * rows;
        if self.count == 0 || self.count > 1000 {
            return Err("Copies must be between 1 and 1000.".into());
        }
        if self.custom.is_empty() {
            if self.count > nominal && !fill {
                return Err(format!(
                    "Requested {} copies, but only {} fit on this page.",
                    self.count, nominal
                ));
            }
            let count = if fill { nominal.min(1000) } else { self.count };
            return Ok((0..count)
                .map(|i| Placement {
                    x: self.margin + (i % cols) as f64 * (self.width + self.column_gap),
                    y: self.margin + (i / cols) as f64 * (self.height + self.row_gap),
                    width: self.width,
                    height: self.height,
                })
                .collect());
        }
        let last = self
            .custom
            .keys()
            .copied()
            .filter(|i| *i < self.count)
            .max();
        let prefix = last.is_some_and(|last| (0..=last).all(|i| self.custom.contains_key(&i)));
        let prototype = if prefix {
            self.custom[&last.unwrap()]
        } else {
            Placement {
                x: 0.,
                y: 0.,
                width: self.width,
                height: self.height,
            }
        };
        let limit = if fill { 1000 } else { self.count };
        let mut slots = vec![None; limit];
        for i in 0..self.count.min(limit) {
            let item = if let Some(p) = self.custom.get(&i) {
                Some(*p)
            } else if i < nominal && !(prefix && last.is_some_and(|last| i > last)) {
                Some(Placement {
                    x: self.margin + (i % cols) as f64 * (self.width + self.column_gap),
                    y: self.margin + (i / cols) as f64 * (self.height + self.row_gap),
                    width: self.width,
                    height: self.height,
                })
            } else {
                None
            };
            if let Some(p) = item {
                if [p.x, p.y, p.width, p.height].iter().any(|v| !v.is_finite())
                    || p.width < 1.
                    || p.height < 1.
                    || p.x < 0.
                    || p.y < 0.
                    || p.x + p.width > self.page_width + 1e-8
                    || p.y + p.height > self.page_height + 1e-8
                {
                    return Err(format!(
                        "Image {} is outside the page or has an invalid size.",
                        i + 1
                    ));
                }
                slots[i] = Some(p);
            }
        }
        for i in 0..limit {
            if slots[i].is_some() {
                continue;
            }
            let occupied: Vec<_> = slots.iter().filter_map(|p| *p).collect();
            let mut xs = vec![self.margin];
            let mut ys = vec![self.margin];
            for p in &occupied {
                xs.extend([p.x, p.x + p.width + self.column_gap]);
                ys.extend([p.y, p.y + p.height + self.row_gap]);
            }
            xs.sort_by(f64::total_cmp);
            ys.sort_by(f64::total_cmp);
            xs.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
            ys.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
            let mut found = None;
            'search: for y in ys {
                for &x in &xs {
                    if x < self
                        .margin
                        .min(occupied.iter().map(|p| p.x).fold(self.margin, f64::min))
                        - 1e-8
                        || y < self
                            .margin
                            .min(occupied.iter().map(|p| p.y).fold(self.margin, f64::min))
                            - 1e-8
                        || x + prototype.width > self.page_width - self.margin + 1e-8
                        || y + prototype.height > self.page_height - self.margin + 1e-8
                    {
                        continue;
                    }
                    let p = Placement {
                        x,
                        y,
                        width: prototype.width,
                        height: prototype.height,
                    };
                    let clear = occupied.iter().all(|q| {
                        p.x + p.width + self.column_gap <= q.x + 1e-8
                            || q.x + q.width + self.column_gap <= p.x + 1e-8
                            || p.y + p.height + self.row_gap <= q.y + 1e-8
                            || q.y + q.height + self.row_gap <= p.y + 1e-8
                    });
                    if clear {
                        found = Some(p);
                        break 'search;
                    }
                }
            }
            if let Some(p) = found {
                slots[i] = Some(p);
            } else {
                if fill {
                    return Ok(slots.into_iter().flatten().collect());
                }
                return Err(format!(
                    "Requested {} copies, but only {} fit with the current sizes and spacing. Reduce Copies or resize the images.",
                    self.count,
                    slots.iter().flatten().count()
                ));
            }
        }
        Ok(slots.into_iter().flatten().collect())
    }
    pub fn available_capacity(&self) -> Result<usize, String> {
        Ok(self.resolved_items(true)?.len())
    }
    pub fn items(&self) -> Result<Vec<Placement>, String> {
        self.resolved_items(false)
    }
    pub fn positions(&self) -> Result<Vec<Point>, String> {
        Ok(self.items()?.iter().map(|p| [p.x, p.y]).collect())
    }
    pub fn page_cut_paths(&self, a: &Artwork) -> Result<Vec<Vec<Point>>, String> {
        self.page_cut_paths_cached(a, &mut OutlineCache::default())
    }
    pub fn page_cut_paths_cached(
        &self,
        a: &Artwork,
        cache: &mut OutlineCache,
    ) -> Result<Vec<Vec<Point>>, String> {
        cache.prepare(a);
        let mut paths = Vec::new();
        for (i, item) in self.items()?.into_iter().enumerate() {
            if self.hidden_images.contains(&i) {
                continue;
            }
            // Position is deliberately excluded; validation-affecting settings are retained.
            let key = [
                item.width.to_bits(),
                item.height.to_bits(),
                self.contour_offset.to_bits(),
                self.smoothing.to_bits(),
                self.cut_inner_holes as u64,
                self.custom.is_empty() as u64,
                self.count as u64,
                self.page_width.to_bits(),
                self.page_height.to_bits(),
                self.margin.to_bits(),
                self.column_gap.to_bits(),
                self.row_gap.to_bits(),
                self.units.to_bits(),
                self.offset_x.to_bits(),
                self.offset_y.to_bits(),
                if a.tiles.is_empty() { 0 } else { i as u64 },
            ];
            if !cache.entries.contains_key(&key) {
                if cache.entries.len() >= 64 {
                    cache.entries.clear();
                }
                let mut local = self.clone();
                local.width = item.width;
                local.height = item.height;
                if !self.custom.is_empty() {
                    local.count = 1;
                    local.custom.clear();
                }
                cache.entries.insert(key, local.outline_mm(a.at(i)));
            }
            for outline in cache.entries[&key].as_ref().map_err(Clone::clone)? {
                paths.push(
                    outline
                        .iter()
                        .map(|p| [item.x + p[0], item.y + p[1]])
                        .collect(),
                );
            }
        }
        Ok(paths)
    }
    pub fn cut_paths(&self, a: &Artwork) -> Result<Vec<Vec<Point>>, String> {
        self.cut_paths_cached(a, &mut OutlineCache::default())
    }
    pub fn cut_paths_cached(
        &self,
        a: &Artwork,
        cache: &mut OutlineCache,
    ) -> Result<Vec<Vec<Point>>, String> {
        let mut paths = Vec::new();
        for outline in self.page_cut_paths_cached(a, cache)? {
            let mut path = Vec::new();
            for p in outline {
                let mut x = p[0];
                let mut y = self.page_height - p[1];
                if self.flip_x {
                    x = self.page_width - x;
                }
                if self.flip_y {
                    y = self.page_height - y;
                }
                x += self.offset_x;
                y += self.offset_y;
                if !(0. ..=self.page_width).contains(&x) || !(0. ..=self.page_height).contains(&y) {
                    return Err("Cut offset moves a path outside the page.".into());
                }
                path.push([x, y]);
            }
            paths.push(path);
        }
        Ok(paths)
    }
    pub fn plt(&self, a: &Artwork) -> Result<String, String> {
        if !self.has_visible_images() {
            return Err("Show at least one image before exporting.".into());
        }
        let paths = self.cut_paths(a)?;
        let mut s = format!(
            "TB25,{},{};CT1;PA;PU0,0;",
            (self.page_width * self.units).round() as i64,
            (self.page_height * self.units).round() as i64
        );
        for path in paths {
            write_plt_path(&mut s, &path, self.units, false);
        }
        s.push_str("PU0,0;!PG;");
        Ok(s)
    }
    pub fn print_html(&self, a: &Artwork) -> Result<String, String> {
        if !self.has_visible_images() {
            return Err("Show at least one image before printing or exporting.".into());
        }
        let data = base64::engine::general_purpose::STANDARD.encode(&a.png);
        let mut s = String::from(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>A4 Sticker Print</title><style>@page{size:A4 portrait;margin:0}*{box-sizing:border-box}body{margin:0;background:#ddd;font:16px sans-serif}.toolbar{padding:16px;background:#fff}.page{width:210mm;height:297mm;background:white;margin:15px auto}svg{display:block;width:210mm;height:297mm}@media print{.toolbar{display:none}body{background:white}.page{margin:0;break-after:avoid}}</style></head><body><div class=\"toolbar\"><button onclick=\"window.print()\">Print A4</button> A4 portrait / Scale 100% (Actual size) / Margins None / Headers and footers Off. Disable Fit to page. Printed crosses are manual alignment references only.</div><div class=\"page\"><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"210mm\" height=\"297mm\" viewBox=\"0 0 210 297\">",
        );
        s.push_str("<rect width=\"210\" height=\"297\" fill=\"white\"/>");
        for (i, p) in self.items()?.into_iter().enumerate() {
            if self.hidden_images.contains(&i) {
                continue;
            }
            let tile_data;
            let data = if a.tiles.is_empty() {
                &data
            } else {
                tile_data = base64::engine::general_purpose::STANDARD.encode(&a.at(i).png);
                &tile_data
            };
            write!(s,"<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{}\"/>",p.x,p.y,p.width,p.height,data).unwrap();
        }
        for (x, y) in self.manual_marks() {
            write!(s,"<path d=\"M{},{}h4 M{},{}v4\" fill=\"none\" stroke=\"black\" stroke-width=\"0.2\"/>",x-2.,y,x,y-2.).unwrap();
        }
        s.push_str("</svg></div></body></html>");
        let s = s
            .replace("A4 Sticker Print", "Sticker Print")
            .replace(
                "size:A4 portrait",
                &format!("size:{}mm {}mm", self.page_width, self.page_height),
            )
            .replace(
                "width:210mm;height:297mm",
                &format!("width:{}mm;height:{}mm", self.page_width, self.page_height),
            )
            .replace(
                "width=\"210mm\" height=\"297mm\" viewBox=\"0 0 210 297\"",
                &format!(
                    "width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\"",
                    self.page_width, self.page_height, self.page_width, self.page_height
                ),
            )
            .replace(
                "<rect width=\"210\" height=\"297\"",
                &format!(
                    "<rect width=\"{}\" height=\"{}\"",
                    self.page_width, self.page_height
                ),
            )
            .replace("Print A4", "Print sheet")
            .replace("A4 portrait /", &format!("{} /", self.paper_label()));
        Ok(s)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hidden_layers_keep_placement_but_are_excluded_from_print_cut_and_ccd() {
        let art = sample();
        let mut layout = Layout {
            count: 2,
            ..Layout::default()
        };
        let original = layout.items().unwrap();
        let profile = CcdTrial::default();
        let marks = profile.marks(&layout);
        layout.hidden_images.insert(1);
        assert_eq!(layout.items().unwrap(), original);
        assert_eq!(layout.page_cut_paths(&art).unwrap().len(), 1);
        let (html, plt) = profile.pair(&layout, &art).unwrap();
        assert_eq!(html.matches("<image ").count(), 1);
        assert_eq!(plt.matches("PU").count(), 3);
        assert!(profile.marks(&layout)[1][0] < marks[1][0]);
        layout.hidden_images.insert(0);
        assert!(layout.print_raster(&art, None).is_err());
        assert!(layout.plt(&art).is_err());
    }
    #[test]
    fn circle_cut_uses_visible_bounds_and_preserves_physical_border() {
        let mut image = RgbaImage::new(100, 100);
        for y in 10..90 {
            for x in 10..90 {
                image.put_pixel(x, y, image::Rgba([0, 0, 255, 255]));
            }
        }
        let art = trace_artwork(image, false, 245, CutShape::Ellipse, 20, true).unwrap();
        let layout = Layout {
            width: 50.,
            height: 50.,
            count: 1,
            contour_offset: 0.5,
            smoothing: 4.,
            ..Layout::default()
        };
        let paths = layout.outline_mm(&art).unwrap();
        let path = &paths[0];
        assert_eq!(path.first(), path.last());
        assert!(
            path.iter()
                .all(|p| ((p[0] - 25.).hypot(p[1] - 25.) - 20.5).abs() < 1e-8)
        );
        let mut out = String::new();
        write_plt_path(
            &mut out,
            &[[0., 0.], [0.001, 0.], [1., 0.], [0., 0.]],
            40.,
            false,
        );
        assert_eq!(out, "PU0,0;PD40,0;PD0,0;");
    }
    fn sample() -> Artwork {
        let mut im = RgbaImage::new(20, 20);
        for y in 3..17 {
            for x in 3..17 {
                im.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let (outlines, holes) = trace(&im).unwrap();
        Artwork {
            png: vec![],
            pixels: im,
            outlines,
            holes,
            tiles: vec![],
            ellipse_bounds: None,
        }
    }
    #[test]
    fn resized_sixteen_copies_allow_four_more_in_free_side_column() {
        let mut l = Layout {
            width: 40.,
            height: 60.,
            count: 16,
            ..Default::default()
        };
        assert_eq!(l.capacity().unwrap(), (4, 4));
        for i in 0..16 {
            l.custom.insert(
                i,
                Placement {
                    x: 15. + (i % 4) as f64 * 35.,
                    y: 15. + (i / 4) as f64 * 55.,
                    width: 32.,
                    height: 52.,
                },
            );
        }
        let original = l.items().unwrap();
        assert_eq!(l.available_capacity().unwrap(), 20);
        for count in 17..=20 {
            l.count = count;
            let placed = l.items().unwrap();
            assert_eq!(&placed[..16], &original);
            assert_eq!(
                placed[count - 1],
                Placement {
                    x: 155.,
                    y: 15. + (count - 17) as f64 * 55.,
                    width: 32.,
                    height: 52.
                }
            );
        }
        let art = sample();
        assert_eq!(l.page_cut_paths(&art).unwrap().len(), 20);
        assert!(l.print_html(&art).is_ok());
        assert!(CcdTrial::default().pair(&l, &art).is_ok());
        // Explicit manual layouts also remain valid beyond the original grid limit.
        for (i, p) in l.items().unwrap().into_iter().enumerate() {
            l.custom.insert(i, p);
        }
        assert_eq!(l.items().unwrap().len(), 20);
        l.count = 21;
        assert!(l.items().is_err());
        assert_eq!(l.available_capacity().unwrap(), 20);
    }
    #[test]
    fn independent_column_and_row_spacing_drive_layout_and_exports() {
        let l = Layout {
            column_gap: 5.,
            row_gap: 10.,
            ..Default::default()
        };
        assert_eq!(l.capacity().unwrap(), (4, 6));
        let items = l.items().unwrap();
        assert_eq!(items[1].x - items[0].x - l.width, 5.);
        assert_eq!(items[4].y - items[0].y - l.height, 10.);
        let art = sample();
        let paths = l.page_cut_paths(&art).unwrap();
        assert!((paths[1][0][0] - paths[0][0][0] - 40.).abs() < 1e-9);
        assert!((paths[4][0][1] - paths[0][0][1] - 45.).abs() < 1e-9);
        let print = l.print_html(&art).unwrap();
        let changed = Layout {
            row_gap: 12.,
            ..l.clone()
        };
        assert_eq!(changed.items().unwrap()[1].x, items[1].x);
        assert_eq!(changed.items().unwrap()[4].y, items[4].y + 2.);
        assert_ne!(print, changed.print_html(&art).unwrap());
        assert_ne!(
            CcdTrial::default().pair(&l, &art).unwrap().1,
            CcdTrial::default().pair(&changed, &art).unwrap().1
        );
        assert!(
            Layout {
                row_gap: f64::NAN,
                ..l.clone()
            }
            .capacity()
            .is_err()
        );
        assert!(
            Layout {
                column_gap: -1.,
                ..l
            }
            .capacity()
            .is_err()
        );
    }
    #[test]
    fn physical_smoothing_removes_pixel_scale_zigzags() {
        let mut path = Vec::new();
        for i in 0..4000 {
            let t = i as f64 * std::f64::consts::TAU / 4000.;
            let r = 10. + 0.08 * (100. * t).sin();
            path.push([r * t.cos(), r * t.sin()]);
        }
        path.push(path[0]);
        let smoothed = smooth_contour(&path, 0.5);
        let radii: Vec<_> = smoothed.iter().map(|p| p[0].hypot(p[1])).collect();
        let span = radii.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - radii.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(span < 0.025, "Residual zigzag span {span} mm");
        assert!(radii.iter().all(|r| (*r - 10.).abs() < 0.03));
        assert!(smoothed.len() < path.len() / 4);
    }
    #[test]
    fn smoothing_runs_on_final_offset_and_is_vertex_density_independent() {
        let square = vec![[0., 0.], [10., 0.], [10., 10.], [0., 10.], [0., 0.]];
        let mut dense = vec![];
        for edge in square.windows(2) {
            for i in 0..100 {
                let t = i as f64 / 100.;
                dense.push([
                    edge[0][0] + (edge[1][0] - edge[0][0]) * t,
                    edge[0][1] + (edge[1][1] - edge[0][1]) * t,
                ]);
            }
        }
        dense.push(dense[0]);
        let coarse = smooth_contour(&square, 0.5);
        let fine = smooth_contour(&dense, 0.5);
        let distance_to = |p: Point, curve: &[Point]| {
            curve
                .windows(2)
                .map(|e| {
                    let dx = e[1][0] - e[0][0];
                    let dy = e[1][1] - e[0][1];
                    let t = (((p[0] - e[0][0]) * dx + (p[1] - e[0][1]) * dy) / (dx * dx + dy * dy))
                        .clamp(0., 1.);
                    (p[0] - e[0][0] - t * dx).hypot(p[1] - e[0][1] - t * dy)
                })
                .fold(f64::INFINITY, f64::min)
        };
        for p in &coarse {
            assert!(distance_to(*p, &fine) < 0.01);
        }
        for p in &fine {
            assert!(distance_to(*p, &coarse) < 0.01);
        }
        let art = sample();
        let base = Layout {
            contour_offset: 1.,
            count: 1,
            ..Default::default()
        };
        let before = base.outline_mm(&art).unwrap();
        let after = Layout {
            smoothing: 0.5,
            ..base
        }
        .outline_mm(&art)
        .unwrap();
        assert_eq!(
            after,
            before
                .iter()
                .map(|p| smooth_contour(p, 0.5))
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn smoothing_compacts_dense_contours_without_losing_closure() {
        let mut path = Vec::new();
        for i in 0..2000 {
            let t = i as f64 * std::f64::consts::TAU / 2000.;
            path.push([10. * t.cos(), 10. * t.sin()]);
        }
        path.push(path[0]);
        let rounded = smooth_contour(&path, 0.5);
        assert!(rounded.len() < path.len());
        assert_eq!(rounded.first(), rounded.last());
        assert!(
            rounded
                .iter()
                .all(|p| (p[0].hypot(p[1]) - 10.).abs() < 0.02)
        );
    }
    #[test]
    fn smoothing_is_optional_closed_and_shared_by_export() {
        let mut a = sample();
        a.holes = vec![vec![
            [0.4, 0.4],
            [0.6, 0.4],
            [0.6, 0.6],
            [0.4, 0.6],
            [0.4, 0.4],
        ]];
        let base = Layout {
            cut_inner_holes: true,
            ..Default::default()
        };
        let before = base.outline_mm(&a).unwrap();
        let smooth = Layout {
            smoothing: 0.5,
            ..base.clone()
        };
        let after = smooth.outline_mm(&a).unwrap();
        assert_eq!(after.len(), before.len());
        for (old, new) in before.iter().zip(&after) {
            assert_eq!(new.first(), new.last());
            assert_ne!(old, new);
            assert!(new.iter().all(|p| p.iter().all(|v| v.is_finite())));
            assert_eq!(smooth_contour(old, 0.), *old);
        }
        assert_eq!(base.print_html(&a).unwrap(), smooth.print_html(&a).unwrap());
        let ccd = CcdTrial::default();
        assert_eq!(ccd.marks(&base), ccd.marks(&smooth));
        assert_ne!(
            ccd.pair(&base, &a).unwrap().1,
            ccd.pair(&smooth, &a).unwrap().1
        );
        assert!(
            Layout {
                smoothing: f64::NAN,
                ..base
            }
            .outline_mm(&a)
            .is_err()
        );
    }
    #[test]
    fn closed_outline() {
        let a = sample();
        assert_eq!(a.outlines.len(), 1);
        assert_eq!(a.outlines[0].first(), a.outlines[0].last());
    }
    #[test]
    fn ccd_trial_uses_four_circles_and_consistent_mark_spans() {
        let a = sample();
        let l = Layout::default();
        let p = CcdTrial::default();
        let (html, plt) = p.pair(&l, &a).unwrap();
        assert_eq!(html.matches("<circle ").count(), 4);
        assert!(!html.contains("h4 M"));
        assert!(html.contains("UNVERIFIED") || html.contains("unverified"));
        assert!(plt.starts_with("TB25,8040,6520;CT1;PA;"));
        let origin = p.marks(&l)[0];
        assert_eq!(origin, [8., 8.]);
        for (path, position) in p
            .cut_paths(&l, &a)
            .unwrap()
            .iter()
            .zip(l.positions().unwrap())
        {
            let q = path[0];
            assert!(
                (q[0] + origin[0] - (position[0] + a.outlines[0][0][0] * l.width)).abs() < 1e-8
            );
            assert!(
                (q[1] + origin[1] - (position[1] + a.outlines[0][0][1] * l.height)).abs() < 1e-8
            );
        }
        assert_eq!(p.marks(&l)[2], [8., 209.]);
    }
    #[test]
    fn ccd_machine_axes_round_trip_all_twenty_contours() {
        let a = sample();
        let l = Layout::default();
        let profile = CcdTrial::default();
        let (_, plt) = profile.pair(&l, &a).unwrap();
        let page_paths = profile.cut_paths(&l, &a).unwrap();
        let commands: Vec<_> = plt
            .split(';')
            .filter(|s| s.starts_with("PU") || s.starts_with("PD"))
            .collect();
        let mut index = 1; // Initial PU0,0.
        assert_eq!(page_paths.len(), 20);
        for path in &page_paths {
            for p in path {
                let machine: Vec<f64> = commands[index][2..]
                    .split(',')
                    .map(|v| v.parse().unwrap())
                    .collect();
                // Decode the observed feed-first firmware convention back to page mm.
                assert!((machine[1] / l.units - p[0]).abs() <= 0.5 / l.units + 1e-9);
                assert!((machine[0] / l.units - p[1]).abs() <= 0.5 / l.units + 1e-9);
                index += 1;
            }
        }
        assert_eq!(index + 1, commands.len()); // Final PU0,0.
        // Adjacent printed columns change carriage; printed rows change feed.
        assert!((page_paths[1][0][0] - page_paths[0][0][0] - 38.).abs() < 1e-8);
        assert!((page_paths[4][0][1] - page_paths[0][0][1] - 38.).abs() < 1e-8);
        assert!(!plt.contains("FS"));
    }
    #[test]
    fn persistent_outlines_match_fresh_geometry_and_invalidate_for_new_art_and_spacing() {
        let mut art = rectangular_art();
        let mut layout = Layout {
            count: 4,
            width: 20.,
            height: 20.,
            contour_offset: 0.5,
            smoothing: 0.3,
            ..Default::default()
        };
        let mut cache = OutlineCache::default();
        assert_eq!(
            layout.cut_paths_cached(&art, &mut cache),
            layout.cut_paths(&art)
        );
        let entries = cache.entries.len();
        layout.custom = layout.items().unwrap().into_iter().enumerate().collect();
        assert_eq!(
            layout.cut_paths_cached(&art, &mut cache),
            layout.cut_paths(&art)
        );
        let custom_entries = cache.entries.len();
        layout.custom.get_mut(&0).unwrap().x += 1.;
        assert_eq!(
            layout.cut_paths_cached(&art, &mut cache),
            layout.cut_paths(&art)
        );
        assert_eq!(cache.entries.len(), custom_entries);
        assert!(entries > 0);
        art.outlines[0][1][0] = 0.7;
        assert_eq!(
            layout.cut_paths_cached(&art, &mut cache),
            layout.cut_paths(&art)
        );
        assert_eq!(cache.entries.len(), 1);
        layout.custom.clear();
        layout.column_gap = 0.;
        layout.row_gap = 0.;
        assert_eq!(
            layout.cut_paths_cached(&art, &mut cache),
            layout.cut_paths(&art)
        );
        assert!(layout.cut_paths_cached(&art, &mut cache).is_err());
        layout.column_gap = 4.;
        layout.row_gap = 4.;
        layout.smoothing = 0.6;
        assert_eq!(
            layout.cut_paths_cached(&art, &mut cache),
            layout.cut_paths(&art)
        );
        assert_eq!(
            CcdTrial::default().cut_paths_cached(&layout, &art, &mut cache),
            CcdTrial::default().cut_paths(&layout, &art)
        );
    }
    fn rectangular_art() -> Artwork {
        Artwork {
            png: vec![],
            pixels: RgbaImage::new(10, 10),
            outlines: vec![vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.], [0., 0.]]],
            holes: vec![],
            tiles: vec![],
            ellipse_bounds: None,
        }
    }
    #[test]
    fn contour_offset_is_physical_mm_and_preserves_print_and_registration() {
        let a = rectangular_art();
        let base = Layout {
            width: 20.,
            height: 10.,
            count: 1,
            ..Default::default()
        };
        let bounds = |paths: Vec<Vec<Point>>| {
            let mut b = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for p in paths.iter().flatten() {
                b[0] = b[0].min(p[0]);
                b[1] = b[1].min(p[1]);
                b[2] = b[2].max(p[0]);
                b[3] = b[3].max(p[1]);
            }
            b
        };
        for (distance, expected) in [(1., [-1., -1., 21., 11.]), (-1., [1., 1., 19., 9.])] {
            let l = Layout {
                contour_offset: distance,
                ..base.clone()
            };
            let b = bounds(l.outline_mm(&a).unwrap());
            for (v, w) in b.iter().zip(expected) {
                assert!((v - w).abs() < 0.01);
            }
            assert_eq!(
                CcdTrial::default().marks(&l),
                CcdTrial::default().marks(&base)
            );
            assert_eq!(base.print_html(&a).unwrap(), l.print_html(&a).unwrap());
            let paths = CcdTrial::default().cut_paths(&l, &a).unwrap();
            let first = paths[0][0];
            let (_, plt) = CcdTrial::default().pair(&l, &a).unwrap();
            assert!(plt.contains(&format!(
                "PU{},{};",
                (first[1] * l.units).round() as i64,
                (first[0] * l.units).round() as i64
            )));
        }
        assert_eq!(
            base.outline_mm(&a).unwrap()[0],
            vec![[0., 0.], [20., 0.], [20., 10.], [0., 10.], [0., 0.]]
        );
    }
    #[test]
    fn contour_offset_rejects_disappearing_overlap_and_mark_clearance() {
        let a = rectangular_art();
        assert!(
            Layout {
                width: 5.,
                height: 5.,
                count: 1,
                contour_offset: -3.,
                ..Default::default()
            }
            .outline_mm(&a)
            .unwrap_err()
            .contains("entire outline")
        );
        assert!(
            Layout {
                contour_offset: 2.,
                ..Default::default()
            }
            .outline_mm(&a)
            .unwrap_err()
            .contains("overlap")
        );
        let l = Layout {
            count: 1,
            contour_offset: 3.,
            ..Default::default()
        };
        assert!(
            CcdTrial::default()
                .pair(&l, &a)
                .unwrap_err()
                .contains("clearance")
        );
        assert!(
            Layout {
                contour_offset: f64::NAN,
                ..Default::default()
            }
            .outline_mm(&a)
            .is_err()
        );
    }
    #[test]
    fn ccd_trial_rejects_mark_artwork_overlap() {
        let l = Layout {
            margin: 10.,
            ..Default::default()
        };
        assert!(CcdTrial::default().pair(&l, &sample()).is_err());
    }
    #[test]
    fn native_raster_places_images_and_marks_at_print_coordinates() {
        let a = sample();
        let l = Layout {
            count: 1,
            ..Default::default()
        };
        let p = l.print_raster(&a, None).unwrap();
        let mm = |v: f64| (v / 25.4 * 300.).round() as u32;
        assert_eq!((p.width(), p.height()), (2480, 3508));
        assert!(p.get_pixel(mm(7.), mm(7.))[0] < 20);
        assert!(p.get_pixel(mm(30.), mm(30.))[0] > 240);
        assert!(p.get_pixel(mm(30.), mm(30.))[1] < 20);
        assert_eq!(
            *p.get_pixel(mm(50.), mm(100.)),
            image::Rgba([255, 255, 255, 255])
        );
        let dots = l.print_raster(&a, Some(&CcdTrial::default())).unwrap();
        assert!(dots.get_pixel(mm(8.), mm(8.))[0] < 20);
        assert_eq!(
            *dots.get_pixel(mm(11.), mm(11.)),
            image::Rgba([255, 255, 255, 255])
        );
    }
    #[test]
    fn print_cut_positions_match() {
        let a = sample();
        let l = Layout {
            count: 1,
            ..Default::default()
        };
        let p = l.cut_paths(&a).unwrap();
        assert!((p[0][0][0] - (15. + 3. / 20. * 35.)).abs() < 0.001);
        assert!((p[0][0][1] - (297. - 15. - 3. / 20. * 35.)).abs() < 0.001);
        assert!(l.plt(&a).unwrap().starts_with("TB25,8400,11880;"));
    }
    #[test]
    fn overflow_rejected() {
        let l = Layout {
            count: 1000,
            ..Default::default()
        };
        assert!(l.positions().is_err());
    }
    #[test]
    fn mark_origin_preview_overlays_print_with_minus_seven_offsets() {
        let a = sample();
        let corner = Layout {
            count: 20,
            ..Default::default()
        };
        let mark = Layout {
            offset_x: -7.,
            offset_y: -7.,
            ..corner.clone()
        };
        let expected = corner.cut_paths(&a).unwrap();
        let actual = mark.cut_paths(&a).unwrap();
        for (p, q) in expected.iter().flatten().zip(actual.iter().flatten()) {
            let preview = cutter_to_page(*q, LOWER_LEFT_MARK);
            assert!((preview[0] - p[0]).abs() < 1e-9);
            assert!((preview[1] - (297. - p[1])).abs() < 1e-9);
            assert!((q[0] - (p[0] - 7.)).abs() < 1e-9);
            assert!((q[1] - (p[1] - 7.)).abs() < 1e-9);
        }
        assert_eq!(cutter_to_page([0., 0.], LOWER_LEFT_MARK), [7., 290.]);
    }
    #[test]
    fn corner_preview_still_exposes_offsets() {
        assert_eq!(cutter_to_page([20., 30.], [0., 0.]), [20., 267.]);
        assert_eq!(cutter_to_page([20., 30.], LOWER_LEFT_MARK), [27., 260.]);
    }
    fn encoded_plain_background() -> Vec<u8> {
        let mut image = RgbaImage::from_pixel(80, 100, image::Rgba([235, 180, 170, 255]));
        for y in 15..90 {
            for x in 20..60 {
                image.put_pixel(x, y, image::Rgba([20, 80, 180, 255]));
            }
        }
        // Background-coloured detail enclosed inside the subject must be preserved.
        for y in 40..50 {
            for x in 35..45 {
                image.put_pixel(x, y, image::Rgba([235, 180, 170, 255]));
            }
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }
    #[test]
    fn frame_and_object_modes_preserve_size_and_optional_print_background() {
        let bytes = encoded_plain_background();
        let frame = artwork_with_shape(&bytes, true, 180, CutShape::ImageFrame, 30, true).unwrap();
        assert_eq!(
            frame.outlines[0],
            vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.], [0., 0.]]
        );
        assert_eq!(frame.pixels.get_pixel(0, 0)[3], 255);
        let object = artwork_with_shape(
            &bytes,
            false,
            245,
            CutShape::PlainBackgroundObject,
            30,
            true,
        )
        .unwrap();
        assert_eq!(object.pixels.dimensions(), (80, 100));
        assert_eq!(object.pixels.get_pixel(0, 0)[3], 0);
        assert_eq!(object.pixels.get_pixel(40, 45)[3], 255);
        assert_eq!(object.pixels.get_pixel(25, 20)[3], 255);
        assert_eq!(object.outlines.len(), 1);
        assert!(
            object.outlines[0]
                .iter()
                .all(|p| p[0] >= 0.24 && p[0] <= 0.76 && p[1] >= 0.14 && p[1] <= 0.91)
        );
        let keep = artwork_with_shape(
            &bytes,
            false,
            245,
            CutShape::PlainBackgroundObject,
            30,
            false,
        )
        .unwrap();
        assert_eq!(keep.outlines, object.outlines);
        assert_eq!(keep.pixels, frame.pixels);
    }
    #[test]
    fn object_mode_handles_gradient_background_and_reports_empty_mask() {
        let mut image = RgbaImage::new(60, 60);
        for y in 0..60 {
            for x in 0..60 {
                image.put_pixel(x, y, image::Rgba([180 + x as u8, 150 + y as u8, 170, 255]));
            }
        }
        for y in 12..48 {
            for x in 20..40 {
                image.put_pixel(x, y, image::Rgba([10, 30, 80, 255]));
            }
        }
        remove_edge_background(&mut image, 8).unwrap();
        assert_eq!(image.get_pixel(0, 30)[3], 0);
        assert_eq!(image.get_pixel(59, 59)[3], 0);
        assert_eq!(image.get_pixel(30, 30)[3], 255);
        let mut blank = RgbaImage::from_pixel(10, 10, image::Rgba([200, 150, 150, 255]));
        assert!(
            remove_edge_background(&mut blank, 10)
                .unwrap_err()
                .contains("No object")
        );
    }
    #[test]
    fn paper_sizes_drive_html_manual_cut_and_ccd_coordinates() {
        let a = rectangular_art();
        for (w, h) in [
            (297., 420.),
            (420., 297.),
            (148., 210.),
            (215.9, 279.4),
            (330., 480.),
        ] {
            let mut l = Layout {
                page_width: w,
                page_height: h,
                count: 1,
                ..Default::default()
            };
            let item = Placement {
                x: w - 60.,
                y: h - 70.,
                width: 35.,
                height: 35.,
            };
            l.custom.insert(0, item);
            let html = l.print_html(&a).unwrap();
            assert!(html.contains(&format!("size:{w}mm {h}mm")));
            assert!(html.contains(&format!("viewBox=\"0 0 {w} {h}\"")));
            assert!(!html.contains("size:A4"));
            assert!(l.plt(&a).unwrap().starts_with(&format!(
                "TB25,{},{};",
                (w * 40.).round() as i64,
                (h * 40.).round() as i64
            )));
            let point = l.cut_paths(&a).unwrap()[0][0];
            assert!((point[1] - 70.).abs() < 1e-8);
            assert_eq!(cutter_to_page_on(point, [0., 0.], h), [item.x, item.y]);
            let c = CcdTrial::default();
            assert_eq!(c.marks(&l)[3], [item.x + 42., item.y + 42.]);
            assert!((c.origin(&l)[1] - 77.).abs() < 1e-8);
            assert!(c.pair(&l, &a).is_ok());
            assert_eq!(l.manual_marks()[3], (w - 7., h - 7.));
        }
        assert_eq!(
            Layout {
                page_width: 420.,
                page_height: 297.,
                ..Default::default()
            }
            .paper_label(),
            "A3 Landscape"
        );
        assert!(
            Layout {
                page_width: 841.,
                page_height: 594.,
                ..Default::default()
            }
            .capacity()
            .is_err()
        );
    }
    #[test]
    fn landscape_raster_and_drag_bounds_follow_selected_paper() {
        let mut a = rectangular_art();
        a.pixels = RgbaImage::from_pixel(10, 10, image::Rgba([20, 80, 180, 255]));
        let l = Layout {
            page_width: 210.,
            page_height: 148.,
            count: 1,
            ..Default::default()
        };
        let raster = l.print_raster(&a, None).unwrap();
        assert_eq!(raster.dimensions(), (2480, 1748));
        let point = Placement {
            x: 300.,
            y: 120.,
            width: 40.,
            height: 30.,
        }
        .moved_on(1000., 1000., 10., [420., 297.]);
        assert_eq!((point.x, point.y), (370., 257.));
        let resized = point.resized_on(0, -20., -15., true, 10., [420., 297.]);
        assert!((resized.width / resized.height - 4. / 3.).abs() < 1e-8);
        assert!((resized.x + resized.width - 410.).abs() < 1e-8);
    }
    #[test]
    fn individual_placements_share_print_cut_and_ccd_geometry() {
        let mut a = rectangular_art();
        a.pixels = RgbaImage::from_pixel(10, 10, image::Rgba([20, 80, 180, 255]));
        let mut l = Layout {
            width: 20.,
            height: 10.,
            count: 2,
            ..Default::default()
        };
        l.custom.insert(
            0,
            Placement {
                x: 30.,
                y: 40.,
                width: 15.,
                height: 25.,
            },
        );
        l.custom.insert(
            1,
            Placement {
                x: 80.,
                y: 90.,
                width: 30.,
                height: 12.,
            },
        );
        let paths = l.page_cut_paths(&a).unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(
            paths[0],
            vec![[30., 40.], [45., 40.], [45., 65.], [30., 65.], [30., 40.]]
        );
        assert_eq!(paths[1][2], [110., 102.]);
        let c = CcdTrial::default();
        assert_eq!(
            c.marks(&l),
            [[23., 33.], [117., 33.], [23., 109.], [117., 109.]]
        );
        let (html, plt) = c.pair(&l, &a).unwrap();
        assert!(html.contains("x=\"30\" y=\"40\" width=\"15\" height=\"25\""));
        assert!(plt.starts_with("TB25,3040,3760;CT1;PA;PU0,0;PU280,280;PD280,880;PD1280,880;"));
        let manual = l.cut_paths(&a).unwrap();
        assert_eq!(manual[0][0], [30., 257.]);
        let native = l.print_raster(&a, Some(&c)).unwrap();
        let mm = |v: f64| (v / 25.4 * 300.).round() as u32;
        assert_eq!(
            *native.get_pixel(mm(35.), mm(45.)),
            image::Rgba([20, 80, 180, 255])
        );
        assert_eq!(
            *native.get_pixel(mm(85.), mm(95.)),
            image::Rgba([20, 80, 180, 255])
        );
        assert_eq!(
            *native.get_pixel(mm(20.), mm(20.)),
            image::Rgba([255, 255, 255, 255])
        );
        l.custom.insert(
            0,
            Placement {
                x: -1.,
                y: 20.,
                width: 10.,
                height: 10.,
            },
        );
        assert!(l.items().is_err());
    }
    #[test]
    fn corner_resize_anchors_opposite_corner_and_preserves_ratio() {
        let p = Placement {
            x: 40.,
            y: 50.,
            width: 20.,
            height: 30.,
        };
        for corner in 0..4 {
            let left = corner == 0 || corner == 3;
            let top = corner < 2;
            let q = p.resized(
                corner,
                if left { -10. } else { 10. },
                if top { -15. } else { 15. },
                true,
                0.,
            );
            assert!((q.width - 30.).abs() < 1e-8);
            assert!((q.height - 45.).abs() < 1e-8);
            assert!(
                ((if left { q.x + q.width } else { q.x }) - (if left { 60. } else { 40. })).abs()
                    < 1e-8
            );
            assert!(
                ((if top { q.y + q.height } else { q.y }) - (if top { 80. } else { 50. })).abs()
                    < 1e-8
            );
            let bounded = p.resized(corner, -1000., 1000., true, 12.75);
            assert!(
                bounded.x >= 12.75 - 1e-8
                    && bounded.y >= 12.75 - 1e-8
                    && bounded.x + bounded.width <= 197.25 + 1e-8
                    && bounded.y + bounded.height <= 284.25 + 1e-8
            );
            assert!((bounded.width / bounded.height - 2. / 3.).abs() < 1e-8);
        }
        let free = p.resized(2, 10., 0., false, 0.);
        assert_eq!((free.width, free.height), (30., 30.));
        let moved = p.moved(1000., -1000., 12.75);
        assert_eq!((moved.x, moved.y), (177.25, 12.75));
    }
    #[test]
    fn inner_holes_follow_material_offset_and_nested_components() {
        let mut a = rectangular_art();
        a.holes = vec![vec![
            [0.3, 0.3],
            [0.3, 0.7],
            [0.7, 0.7],
            [0.7, 0.3],
            [0.3, 0.3],
        ]];
        let l = Layout {
            width: 20.,
            height: 20.,
            count: 1,
            cut_inner_holes: true,
            ..Default::default()
        };
        let extent = |path: &Vec<Point>| {
            let mut b = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for p in path {
                b[0] = b[0].min(p[0]);
                b[1] = b[1].min(p[1]);
                b[2] = b[2].max(p[0]);
                b[3] = b[3].max(p[1]);
            }
            b
        };
        for (offset, expected) in [(1., [7., 7., 13., 13.]), (-1., [5., 5., 15., 15.])] {
            let paths = Layout {
                contour_offset: offset,
                ..l.clone()
            }
            .outline_mm(&a)
            .unwrap();
            assert_eq!(paths.len(), 2);
            for (actual, expected) in extent(&paths[0]).iter().zip(expected) {
                assert!((actual - expected).abs() < 0.02);
            }
            assert_eq!(paths[0].first(), paths[0].last());
        }
        assert_eq!(
            Layout {
                contour_offset: 5.,
                ..l.clone()
            }
            .outline_mm(&a)
            .unwrap()
            .len(),
            1
        );
        // Foreground island inside the hole must remain a separate component.
        a.outlines.push(vec![
            [0.45, 0.45],
            [0.55, 0.45],
            [0.55, 0.55],
            [0.45, 0.55],
            [0.45, 0.45],
        ]);
        assert_eq!(
            Layout {
                contour_offset: 0.2,
                ..l
            }
            .outline_mm(&a)
            .unwrap()
            .len(),
            3
        );
    }
    #[test]
    fn inner_holes_toggle_controls_preview_and_export() {
        let mut im = sample().pixels;
        for y in 7..13 {
            for x in 7..13 {
                im.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
            }
        }
        let (outlines, holes) = trace(&im).unwrap();
        assert_eq!(outlines.len(), 1);
        assert_eq!(holes.len(), 1);
        let a = Artwork {
            pixels: im,
            png: vec![],
            outlines,
            holes,
            tiles: vec![],
            ellipse_bounds: None,
        };
        assert_eq!(Layout::default().outline_mm(&a).unwrap().len(), 1);
        let l = Layout {
            cut_inner_holes: true,
            ..Default::default()
        };
        assert_eq!(l.outline_mm(&a).unwrap().len(), 2);
        let ccd = CcdTrial::default();
        let (_, off) = ccd.pair(&Layout::default(), &a).unwrap();
        let (_, on) = ccd.pair(&l, &a).unwrap();
        assert_eq!(
            on.matches("PU").count() - off.matches("PU").count(),
            l.count
        );
        assert_eq!(
            l.print_html(&a).unwrap(),
            Layout::default().print_html(&a).unwrap()
        );
    }
}
