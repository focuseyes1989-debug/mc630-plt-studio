//! A4 artwork and cut paths share top-left millimetre coordinates.
use crate::Point;

/// Maximum user-selectable smoothing radius in physical millimetres.
pub const MAX_SMOOTHING_MM: f64 = 10.;
use base64::Engine;
use image::RgbaImage;
use std::{collections::HashMap, fmt::Write};
/// Reuses physical contour geometry while copies are moved or rearranged.
#[derive(Default)]
pub struct OutlineCache {
    artwork: Option<u64>,
    entries: HashMap<[u64; 19], Result<Vec<Vec<Point>>, String>>,
    svg_entries: HashMap<[u64;11],Result<Vec<Vec<Point>>,String>>,
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
        let mut items: Vec<_> = layout
            .items()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !layout.hidden_images.contains(i))
            .map(|(_, placement)| placement)
            .collect();
        for shape in &layout.design_shapes {
            if shape.path.is_empty() {continue;}
            let x0=shape.path.iter().map(|p|p[0]).fold(f64::INFINITY,f64::min);
            let y0=shape.path.iter().map(|p|p[1]).fold(f64::INFINITY,f64::min);
            let x1=shape.path.iter().map(|p|p[0]).fold(f64::NEG_INFINITY,f64::max);
            let y1=shape.path.iter().map(|p|p[1]).fold(f64::NEG_INFINITY,f64::max);
            let border=if shape.print {shape.stroke_width/2.}else{0.};
            items.push(Placement{x:x0-border,y:y0-border,width:(x1-x0+border*2.).max(0.01),height:(y1-y0+border*2.).max(0.01),orientation:Default::default()});
        }
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
    fn validated_cut_paths(
        &self,
        layout: &Layout,
        art: &Artwork,
    ) -> Result<Vec<Vec<Point>>, String> {
        if !layout.has_visible_images() && layout.design_shapes.is_empty() {
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
        let paths = self.cut_paths(layout, art)?;
        for p in paths.iter().flatten() {
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
        Ok(paths)
    }

    pub fn pair(&self, layout: &Layout, art: &Artwork) -> Result<(String, String), String> {
        let paths = self.validated_cut_paths(layout, art)?;
        let dots = self.marks(layout);
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
        for path in paths {
            write_plt_path(&mut plt, &path, layout.units, true);
        }
        plt.push_str("PU0,0;!PG;");
        Ok((html, plt))
    }
}
fn group_cut_outline(mut paths: Vec<Vec<Point>>) -> Result<Vec<Point>, String> {
    use geo::orient::{Direction, Orient};
    use geo::{LineString, Polygon, algorithm::bool_ops::unary_union};

    if paths.len() == 1 {
        return Ok(paths.remove(0));
    }
    let polygons: Vec<_> = paths
        .into_iter()
        .filter(|path| path.len() >= 4)
        .map(|path| {
            Polygon::new(
                LineString::from_iter(path.into_iter().map(|[x, y]| (x, y))),
                vec![],
            )
            .orient(Direction::Default)
        })
        .collect();
    if polygons.is_empty() {
        return Err("No outer contour found for the image group.".into());
    }
    let mut components = unary_union(&polygons).0;
    let polygon = if components.len() > 1 {
        bridge_cut_components(components)?
    } else {
        components
            .pop()
            .ok_or("No outer contour found for the image group.")?
    };
    Ok(polygon
        .exterior()
        .points()
        .map(|p| [p.x(), p.y()])
        .collect())
}

fn bridge_cut_components(
    mut components: Vec<geo::Polygon<f64>>,
) -> Result<geo::Polygon<f64>, String> {
    use geo::orient::{Direction, Orient};
    use geo::{
        BoundingRect, Buffer, LineString, Point as GeoPoint, algorithm::bool_ops::unary_union,
    };
    use rstar::{RTree, primitives::Line};

    // A 1 mm bridge joins outlines only where they meet the shortest gap.
    const RADIUS: f64 = 0.5;
    let count = components.len();
    let bounds: Vec<_> = components
        .iter()
        .map(|polygon| {
            polygon
                .bounding_rect()
                .expect("union components have an exterior")
        })
        .collect();
    let boundaries: Vec<_> = components
        .iter()
        .map(|polygon| {
            RTree::bulk_load(
                polygon
                    .exterior()
                    .lines()
                    .map(|edge| Line::new([edge.start.x, edge.start.y], [edge.end.x, edge.end.y]))
                    .collect(),
            )
        })
        .collect();
    let mut joined = vec![false; count];
    let mut nearest = vec![(f64::INFINITY, [[0.; 2]; 2]); count];
    let mut current = 0;

    // Grow a minimum spanning tree so a group with several gaps only gets the
    // bridges it needs. The closest points of disjoint segments include a vertex.
    for _ in 1..count {
        joined[current] = true;
        for candidate in 0..count {
            if joined[candidate] {
                continue;
            }
            let a = bounds[current];
            let b = bounds[candidate];
            let dx = (a.min().x - b.max().x).max(b.min().x - a.max().x).max(0.);
            let dy = (a.min().y - b.max().y).max(b.min().y - a.max().y).max(0.);
            if dx * dx + dy * dy >= nearest[candidate].0 {
                continue;
            }
            for (from, to) in [(current, candidate), (candidate, current)] {
                for vertex in &components[from].exterior().0 {
                    let vertex = [vertex.x, vertex.y];
                    if let Some(edge) = boundaries[to].nearest_neighbor(&vertex) {
                        let other = edge.nearest_point(&vertex);
                        let gap = (vertex[0] - other[0]).powi(2) + (vertex[1] - other[1]).powi(2);
                        if gap < nearest[candidate].0 {
                            nearest[candidate] = (gap, [vertex, other]);
                        }
                    }
                }
            }
        }
        current = (0..count)
            .filter(|&i| !joined[i])
            .min_by(|&a, &b| nearest[a].0.total_cmp(&nearest[b].0))
            .ok_or("Cannot connect the grouped cut outlines.")?;
        let (gap, [a, b]) = nearest[current];
        if !gap.is_finite() {
            return Err("Cannot connect the grouped cut outlines.".into());
        }
        let bridge = if gap < 1e-12 {
            GeoPoint::new(a[0], a[1]).buffer(RADIUS)
        } else {
            LineString::from(vec![(a[0], a[1]), (b[0], b[1])]).buffer(RADIUS)
        };
        components.extend(bridge.0.into_iter().map(|p| p.orient(Direction::Default)));
    }
    let mut merged = unary_union(&components);
    if merged.0.len() != 1 {
        return Err(
            "Cannot connect the grouped cut outlines. Move the images closer together.".into(),
        );
    }
    Ok(merged.0.remove(0))
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
    let decoded = decode_image(bytes)?;
    trace_artwork(
        decoded.into_rgba8(),
        remove_white,
        white,
        shape,
        tolerance,
        print_without_background,
    )
}

pub fn decode_image(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8000);
    limits.max_image_height = Some(8000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|e| e.to_string())
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
    Ok(Artwork {
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
    smooth_path(path, amount, true)
}

fn smooth_path(path: &[Point], amount: f64, closed: bool) -> Vec<Point> {
    if amount == 0. || path.len() < 3 {
        return path.to_vec();
    }
    let mut ring = path.to_vec();
    if closed && ring.first() == ring.last() {
        ring.pop();
    }
    ring.dedup();
    if ring.len() < 3 {
        return path.to_vec();
    }
    let mut lengths = vec![0.];
    let edges = if closed { ring.len() } else { ring.len() - 1 };
    for i in 0..edges {
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
    let mut samples = Vec::with_capacity(count + 1);
    let mut edge = 0;
    for i in 0..count {
        let distance = i as f64 * step;
        while edge + 1 < edges && lengths[edge + 1] <= distance {
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
    if !closed {
        samples.push(*ring.last().unwrap());
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
    let sample = |i: isize| {
        let n = count as isize;
        if closed {
            samples[i.rem_euclid(n) as usize]
        } else if i < 0 {
            let p = samples[(-i) as usize];
            [2. * samples[0][0] - p[0], 2. * samples[0][1] - p[1]]
        } else if i > n {
            let p = samples[(2 * n - i) as usize];
            [2. * samples[count][0] - p[0], 2. * samples[count][1] - p[1]]
        } else {
            samples[i as usize]
        }
    };
    for i in 0..samples.len() {
        let mut p = [samples[i][0] * weights[0], samples[i][1] * weights[0]];
        for j in 1..=window {
            let a = sample((i + j) as isize);
            let b = sample(i as isize - j as isize);
            p[0] += (a[0] + b[0]) * weights[j];
            p[1] += (a[1] + b[1]) * weights[j];
        }
        rounded.push([p[0] / total, p[1] / total]);
    }
    if closed {
        rounded.push(rounded[0]);
    } else {
        rounded[0] = path[0];
        *rounded.last_mut().unwrap() = *path.last().unwrap();
    }
    let mut compact = vec![rounded[0]];
    simplify(&rounded, 0.005, &mut compact);
    if !closed || compact.len() >= 4 {
        compact
    } else {
        rounded
    }
}

// Recognize genuinely polygonal contours within source-pixel uncertainty.
// Curves need many short segments at this tolerance and are left untouched.
fn straighten_polygon(path:&[Point],pixel_size:f64)->Vec<Point> {
    if path.len()<4 || path.first()!=path.last() {return path.to_vec();}
    let ring=&path[..path.len()-1];
    let min=[ring.iter().map(|p|p[0]).fold(f64::INFINITY,f64::min),ring.iter().map(|p|p[1]).fold(f64::INFINITY,f64::min)];
    let max=[ring.iter().map(|p|p[0]).fold(f64::NEG_INFINITY,f64::max),ring.iter().map(|p|p[1]).fold(f64::NEG_INFINITY,f64::max)];
    let tolerance=(pixel_size*1.5).min(0.2).min((max[0]-min[0]).min(max[1]-min[1])*0.003);
    if tolerance<=1e-8 {return path.to_vec();}
    let split=(1..ring.len()).max_by(|&a,&b| {
        let distance=|i:usize|(ring[i][0]-ring[0][0]).hypot(ring[i][1]-ring[0][1]);
        distance(a).total_cmp(&distance(b))
    }).unwrap();
    let mut simple=vec![ring[0]];
    simplify(&path[..=split],tolerance,&mut simple);
    simplify(&path[split..],tolerance,&mut simple);
    if !(4..=65).contains(&simple.len()) {return path.to_vec();}
    let n=simple.len()-1;
    for i in 0..n {
        let a=simple[(i+n-1)%n];let b=simple[i];let c=simple[(i+1)%n];
        let u=[b[0]-a[0],b[1]-a[1]];let v=[c[0]-b[0],c[1]-b[1]];
        if v[0].hypot(v[1])<tolerance*12. {return path.to_vec();}
        let turn=(u[0]*v[1]-u[1]*v[0]).atan2(u[0]*v[0]+u[1]*v[1]).abs();
        if turn<25_f64.to_radians() {return path.to_vec();}
    }
    refine_polygon_lines(path,&simple,tolerance)
}

// Fit the interior of each straight edge, then intersect adjacent fitted lines.
// This avoids choosing noisy raster endpoints as the actual logo corners.
fn refine_polygon_lines(original:&[Point],polygon:&[Point],tolerance:f64)->Vec<Point> {
    let n=polygon.len()-1;
    if original.len().saturating_mul(n)>200_000 {return polygon.to_vec();}
    let mut lines=Vec::with_capacity(n);
    for edge in polygon.windows(2) {
        let a=edge[0];let b=edge[1];let dx=b[0]-a[0];let dy=b[1]-a[1];let length=dx.hypot(dy);
        let mut samples=Vec::new();
        for segment in original.windows(2) {
            let distance=(segment[1][0]-segment[0][0]).hypot(segment[1][1]-segment[0][1]);
            let count=((distance/tolerance.max(0.005)).ceil() as usize).clamp(1,32);
            for i in 0..count {
                let t=(i as f64+0.5)/count as f64;
                let p=[segment[0][0]+(segment[1][0]-segment[0][0])*t,segment[0][1]+(segment[1][1]-segment[0][1])*t];
                let along=((p[0]-a[0])*dx+(p[1]-a[1])*dy)/(length*length);
                let across=((p[0]-a[0])*dy-(p[1]-a[1])*dx).abs()/length;
                if (0.1..=0.9).contains(&along) && across<=tolerance*1.5 {samples.push((p,distance/count as f64));}
            }
        }
        let weight=samples.iter().map(|(_,w)|w).sum::<f64>();
        if samples.len()<3 || weight<=1e-9 {lines.push((a,[dx/length,dy/length]));continue;}
        let center=[samples.iter().map(|(p,w)|p[0]*w).sum::<f64>()/weight,samples.iter().map(|(p,w)|p[1]*w).sum::<f64>()/weight];
        let (mut xx,mut xy,mut yy)=(0.,0.,0.);
        for (p,w) in samples {let x=p[0]-center[0];let y=p[1]-center[1];xx+=w*x*x;xy+=w*x*y;yy+=w*y*y;}
        let angle=0.5*(2.*xy).atan2(xx-yy);
        let direction=[angle.cos(),angle.sin()];
        // Require the fitted axis to agree with the detected edge.
        if (direction[0]*dx+direction[1]*dy).abs()/length<0.999 {lines.push((a,[dx/length,dy/length]));}
        else {lines.push((center,direction));}
    }
    let mut refined=Vec::with_capacity(n+1);
    for i in 0..n {
        let (a,u)=lines[(i+n-1)%n];let (b,v)=lines[i];
        let cross=u[0]*v[1]-u[1]*v[0];
        if cross.abs()<1e-8 {return polygon.to_vec();}
        let t=((b[0]-a[0])*v[1]-(b[1]-a[1])*v[0])/cross;
        let mut p=[a[0]+t*u[0],a[1]+t*u[1]];
        for axis in 0..2 {if (p[axis]-polygon[i][axis]).abs()<1e-9 {p[axis]=polygon[i][axis];}}
        if !p.iter().all(|v|v.is_finite()) || (p[0]-polygon[i][0]).hypot(p[1]-polygon[i][1])>tolerance*2. {return polygon.to_vec();}
        refined.push(p);
    }
    refined.push(refined[0]);
    let distance=|point:Point|refined.windows(2).map(|edge| {
        let a=edge[0];let dx=edge[1][0]-a[0];let dy=edge[1][1]-a[1];let denominator=dx*dx+dy*dy;
        let t=if denominator>0. {(((point[0]-a[0])*dx+(point[1]-a[1])*dy)/denominator).clamp(0.,1.)}else{0.};
        (point[0]-a[0]-t*dx).hypot(point[1]-a[1]-t*dy)
    }).fold(f64::INFINITY,f64::min);
    if original.iter().any(|p|distance(*p)>tolerance*2.) {polygon.to_vec()}else{refined}
}

// Detect turns that stay sharp at two distances, then smooth only between them.
fn smooth_mixed_contour(path: &[Point], amount: f64) -> Vec<Point> {
    if path.len() < 4 {
        return path.to_vec();
    }
    let ring = path.strip_suffix(&path[..1]).unwrap_or(path);
    if ring.len() < 3 {
        return path.to_vec();
    }
    let mut lengths = vec![0.];
    for (i, p) in ring.iter().enumerate() {
        let q = ring[(i + 1) % ring.len()];
        lengths.push(lengths[i] + (q[0] - p[0]).hypot(q[1] - p[1]));
    }
    let perimeter = lengths[ring.len()];
    if !perimeter.is_finite() || perimeter <= 1e-9 {
        return path.to_vec();
    }
    let probe = (amount * 0.65).clamp(0.35, 1.5).min(perimeter / 18.);
    let at = |distance: f64| {
        let d = distance.rem_euclid(perimeter);
        let i = lengths
            .partition_point(|&l| l <= d)
            .saturating_sub(1)
            .min(ring.len() - 1);
        let span = lengths[i + 1] - lengths[i];
        let t = if span > 0. {
            (d - lengths[i]) / span
        } else {
            0.
        };
        let p = ring[i];
        let q = ring[(i + 1) % ring.len()];
        [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]
    };
    let mut candidates = Vec::new();
    for (i, p) in ring.iter().enumerate() {
        let turn = |distance| {
            let a = at(lengths[i] - distance);
            let b = at(lengths[i] + distance);
            let u = [p[0] - a[0], p[1] - a[1]];
            let v = [b[0] - p[0], b[1] - p[1]];
            (u[0] * v[1] - u[1] * v[0])
                .atan2(u[0] * v[0] + u[1] * v[1])
                .abs()
        };
        let near = turn(probe);
        let far = turn(probe * 2.);
        if near >= 50_f64.to_radians() && far >= 50_f64.to_radians() && near >= far * 0.7 {
            candidates.push((i, near.min(far)));
        }
    }
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut corners = std::collections::BTreeSet::<usize>::new();
    for (i, _) in candidates {
        let previous = corners.range(..i).next_back().or_else(|| corners.last());
        let next = corners.range(i..).next().or_else(|| corners.first());
        if previous.into_iter().chain(next).all(|&j| {
            let distance: f64 = (lengths[i] - lengths[j]).abs();
            distance.min(perimeter - distance) >= probe
        }) {
            corners.insert(i);
        }
    }
    if corners.is_empty() {
        return smooth_contour(path, amount);
    }
    let corners: Vec<_> = corners.into_iter().collect();
    let mut out = vec![ring[corners[0]]];
    for (i, &start) in corners.iter().enumerate() {
        let mut end = corners[(i + 1) % corners.len()];
        if end <= start {
            end += ring.len();
        }
        let span: Vec<_> = (start..=end).map(|j| ring[j % ring.len()]).collect();
        let a=span[0];let b=*span.last().unwrap();
        let dx=b[0]-a[0];let dy=b[1]-a[1];let length=dx.hypot(dy);
        // Pixel stair steps along a long edge should become one straight cut,
        // rather than being converted into a shallow bowed curve.
        let tolerance=(amount*0.08).min(0.08).min(length*0.005);
        let straight=length>1e-8 && span.iter().all(|p| {
            let t=(((p[0]-a[0])*dx+(p[1]-a[1])*dy)/(length*length)).clamp(0.,1.);
            (p[0]-a[0]-t*dx).hypot(p[1]-a[1]-t*dy)<=tolerance
        });
        if straight {out.push(b);}else{out.extend(smooth_path(&span, amount, false).into_iter().skip(1));}
    }
    out
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
/// Clockwise rotation followed by reflections along the page axes.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Orientation {
    turns: u8,
    flip_x: bool,
    flip_y: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    free_rotation: Option<FreeRotation>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct FreeRotation {
    degrees: f64,
    source_aspect: f64,
}

impl Orientation {
    pub fn degrees(self) -> f64 {
        self.free_rotation
            .map_or(f64::from(self.turns % 4) * 90., |r| r.degrees)
    }

    fn rotation_components(degrees: f64) -> (f64, f64) {
        match degrees {
            0. => (0., 1.),
            90. => (1., 0.),
            180. => (0., -1.),
            270. => (-1., 0.),
            _ => degrees.to_radians().sin_cos(),
        }
    }

    fn bounding_size(degrees: f64, width: f64, height: f64) -> (f64, f64) {
        let (sin, cos) = Self::rotation_components(degrees);
        (
            cos.abs() * width + sin.abs() * height,
            sin.abs() * width + cos.abs() * height,
        )
    }

    pub fn flip(&mut self, horizontal: bool) {
        if horizontal {
            self.flip_x = !self.flip_x;
        } else {
            self.flip_y = !self.flip_y;
        }
    }

    pub fn source_size(self, width: f64, height: f64) -> (f64, f64) {
        if let Some(rotation) = self.free_rotation {
            let (w, h) = Self::bounding_size(rotation.degrees, rotation.source_aspect, 1.);
            let scale = ((width / w) * (height / h)).sqrt();
            return (rotation.source_aspect * scale, scale);
        }
        if self.turns % 2 == 1 {
            (height, width)
        } else {
            (width, height)
        }
    }

    fn valid(self) -> bool {
        self.free_rotation.is_none_or(|r| {
            (0. ..360.).contains(&r.degrees) && r.source_aspect.is_finite() && r.source_aspect > 0.
        })
    }

    /// Map source coordinates in millimetres into the image's page-aligned frame.
    fn point(self, [x, y]: Point, width: f64, height: f64) -> Point {
        let (w, h) = self.source_size(width, height);
        let (sin, cos) = Self::rotation_components(self.degrees());
        let (bw, bh) = Self::bounding_size(self.degrees(), w, h);
        let (x, y) = (x - w / 2., y - h / 2.);
        let x2 = (cos * x - sin * y) * (width / bw);
        let y2 = (sin * x + cos * y) * (height / bh);
        [
            width / 2. + if self.flip_x { -x2 } else { x2 },
            height / 2. + if self.flip_y { -y2 } else { y2 },
        ]
    }

    pub fn unit_corners(self) -> [Point; 4] {
        let (w, h) = self.source_size(1., 1.);
        [[0., 0.], [w, 0.], [w, h], [0., h]].map(|p| self.point(p, 1., 1.))
    }

    fn raster_key(self) -> (u64, u64, bool, bool) {
        (
            self.degrees().to_bits(),
            self.free_rotation.map_or(0, |r| r.source_aspect.to_bits()),
            self.flip_x,
            self.flip_y,
        )
    }

    fn raster(self, source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
        use image::imageops;
        let (w, h) = self.source_size(f64::from(width), f64::from(height));
        let mut raster = imageops::resize(
            source,
            w.round().max(1.) as u32,
            h.round().max(1.) as u32,
            imageops::FilterType::Lanczos3,
        );
        if self.free_rotation.is_some() {
            let [origin, right, _, bottom] = self.unit_corners();
            let [ax, ay] = [right[0] - origin[0], right[1] - origin[1]];
            let [bx, by] = [bottom[0] - origin[0], bottom[1] - origin[1]];
            let inv_det = 1. / (ax * by - ay * bx);
            return RgbaImage::from_fn(width, height, |x, y| {
                let dx = (f64::from(x) + 0.5) / f64::from(width) - origin[0];
                let dy = (f64::from(y) + 0.5) / f64::from(height) - origin[1];
                let u = ((by * dx - bx * dy) * inv_det) as f32;
                let v = ((ax * dy - ay * dx) * inv_det) as f32;
                imageops::sample_bilinear(&raster, u, v).unwrap_or(image::Rgba([0; 4]))
            });
        }
        raster = match self.turns % 4 {
            1 => imageops::rotate90(&raster),
            2 => imageops::rotate180(&raster),
            3 => imageops::rotate270(&raster),
            _ => raster,
        };
        if self.flip_x {
            imageops::flip_horizontal_in_place(&mut raster);
        }
        if self.flip_y {
            imageops::flip_vertical_in_place(&mut raster);
        }
        raster
    }
}

/// Physical page placement of one repeated image, measured from the top-left in mm.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default, skip_serializing_if = "orientation_is_default")]
    pub orientation: Orientation,
}
fn orientation_is_default(orientation: &Orientation) -> bool {
    *orientation == Orientation::default()
}
impl Placement {
    /// Crop in normalized source coordinates, retaining physical scale and orientation.
    pub fn cropped(self, bounds: [f64; 4]) -> Self {
        let [u, v, uw, vh] = bounds;
        let (sw, sh) = self.orientation.source_size(self.width, self.height);
        let corners = [[u, v], [u + uw, v], [u, v + vh], [u + uw, v + vh]]
            .map(|[x,y]| self.orientation.point([x * sw, y * sh], self.width, self.height));
        let min_x = corners.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        let min_y = corners.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        let max_x = corners.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
        let max_y = corners.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
        let mut orientation = self.orientation;
        if let Some(ref mut rotation) = orientation.free_rotation {
            rotation.source_aspect = sw * uw / (sh * vh);
        }
        Self { x: self.x + min_x, y: self.y + min_y, width: max_x - min_x,
            height: max_y - min_y, orientation }
    }
    pub fn with_rotation(mut self, degrees: f64) -> Self {
        let degrees = degrees.rem_euclid(360.);
        if !degrees.is_finite() || degrees == self.orientation.degrees() {
            return self;
        }
        let (w, h) = self.orientation.source_size(self.width, self.height);
        let (width, height) = Orientation::bounding_size(degrees, w, h);
        self.x += (self.width - width) / 2.;
        self.y += (self.height - height) / 2.;
        self.width = width;
        self.height = height;
        self.orientation.turns = (degrees / 90.) as u8;
        self.orientation.free_rotation = (degrees % 90. != 0.).then_some(FreeRotation {
            degrees,
            source_aspect: w / h,
        });
        self
    }

    pub fn turned_quarter(self, clockwise: bool) -> Self {
        // Turning the displayed image also turns its reflection axes.
        let mut item =
            self.with_rotation(self.orientation.degrees() + if clockwise { 90. } else { -90. });
        std::mem::swap(&mut item.orientation.flip_x, &mut item.orientation.flip_y);
        item
    }

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
            ..self
        }
    }
}
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DesignShape {
    pub path: Vec<Point>,
    pub fill: Option<[u8;4]>,
    pub stroke: [u8;4],
    pub stroke_width: f64,
    pub print: bool,
    pub cut: bool,
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
    #[serde(default = "preserve_corners_default")]
    pub preserve_corners: bool,
    pub cut_inner_holes: bool,
    pub custom: std::collections::BTreeMap<usize, Placement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_tiles: Vec<[u32; 4]>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub hidden_images: std::collections::BTreeSet<usize>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub image_opacity: std::collections::BTreeMap<usize,u8>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub image_groups: std::collections::BTreeMap<usize, usize>,
    /// Vertical (X) and horizontal (Y) guides in page millimetres.
    #[serde(default)]
    pub guides: [Vec<f64>; 2],
    /// Hand-drawn paths in page millimetres, with Y measured from the top.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manual_cut_paths: Vec<Vec<Point>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub design_shapes: Vec<DesignShape>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub svg_sources: std::collections::BTreeMap<usize,crate::svg_artwork::SvgArtwork>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub image_cut_paths: std::collections::BTreeMap<usize, Vec<Vec<Point>>>,
    #[serde(default)]
    pub use_manual_cut_paths: bool,
    pub margin: f64,
    /// Left, right, top and bottom margins in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_margins: Option<[f64; 4]>,
    pub count: usize,
    pub units: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub flip_x: bool,
    pub flip_y: bool,
}
fn preserve_corners_default() -> bool {
    true
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
            preserve_corners: preserve_corners_default(),
            cut_inner_holes: false,
            custom: Default::default(),
            image_tiles: vec![],
            hidden_images: Default::default(),image_opacity:Default::default(),
            image_groups: Default::default(),
            guides: Default::default(),
            manual_cut_paths: vec![],
            design_shapes: vec![],svg_sources:Default::default(),
            image_cut_paths: Default::default(),
            use_manual_cut_paths: false,
            margin: 15.,
            page_margins: None,
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
    pub fn margins(&self) -> [f64; 4] {
        self.page_margins.unwrap_or([self.margin; 4])
    }

    pub fn margin_lines(&self) -> [[f64; 2]; 2] {
        let [left, right, top, bottom] = self.margins();
        [
            [left, self.page_width - right],
            [top, self.page_height - bottom],
        ]
    }

    pub fn margins_fit(margins: [f64; 4], width: f64, height: f64) -> bool {
        let [left, right, top, bottom] = margins;
        margins
            .iter()
            .all(|value| value.is_finite() && *value >= 0.)
            && width - left - right >= 1.
            && height - top - bottom >= 1.
    }

    pub fn validate_manual_cut_paths(&self) -> Result<(), String> {
        if self.svg_sources.len()>1000 || self.svg_sources.values().map(|s|s.source.len()).sum::<usize>()>30_000_000 || self.svg_sources.iter().any(|(i,s)|*i>=self.count || s.raster_rect[2]==0 || s.raster_rect[3]==0 || s.viewport.iter().any(|v|!v.is_finite()) || s.viewport[2]<=0. || s.viewport[3]<=0.) {return Err("Invalid SVG artwork metadata.".into());}
        if self.design_shapes.len()>100 || self.design_shapes.iter().map(|s|s.path.len()).sum::<usize>()>100_000
            || self.design_shapes.iter().any(|s| !(2..=4000).contains(&s.path.len()) || !s.stroke_width.is_finite()
                || !(0. ..=20.).contains(&s.stroke_width) || (!s.print && !s.cut)
                || s.path.iter().flatten().any(|v|!v.is_finite() || v.abs()>100_000.)) {
            return Err("Invalid design shapes.".into());
        }
        if self.image_cut_paths.keys().any(|i| *i>=self.count)
            || self.image_cut_paths.values().flatten().count()>100
            || self.image_cut_paths.values().flatten().map(Vec::len).sum::<usize>()>10_000
            || self.image_cut_paths.values().flatten().any(|path| !(2..=4000).contains(&path.len())
                || path.iter().flatten().any(|v| !v.is_finite() || v.abs()>1000.)) {
            return Err("Invalid image-linked cutlines.".into());
        }
        if self.manual_cut_paths.len() > 100
            || self.manual_cut_paths.iter().map(Vec::len).sum::<usize>() > 10_000
            || self.manual_cut_paths.iter().any(|path| {
                !(2..=4000).contains(&path.len())
                    || path.iter().any(|&[x, y]| {
                        !(0. ..=self.page_width).contains(&x)
                            || !(0. ..=self.page_height).contains(&y)
                    })
            })
        {
            return Err("Manual cut paths must stay within the page limits.".into());
        }
        Ok(())
    }
    pub fn has_manual_cuts(&self) -> bool { self.design_shapes.iter().any(|s|s.cut) || !self.manual_cut_paths.is_empty() || self.image_cut_paths.values().any(|paths| !paths.is_empty()) }
    pub fn link_cut_paths(&mut self, index: usize, paths: Vec<Vec<Point>>) -> Result<(), String> {
        let items = self.editor_items()?;
        let item = items.get(index).ok_or("Select an image to link the cutline.")?;
        let [origin,right,_,bottom] = item.orientation.unit_corners();
        let ax=[right[0]-origin[0],right[1]-origin[1]];
        let ay=[bottom[0]-origin[0],bottom[1]-origin[1]];
        let det=ax[0]*ay[1]-ax[1]*ay[0];
        let normalized = paths.into_iter().map(|path| path.into_iter().map(|p| {
            let q=[(p[0]-item.x)/item.width-origin[0],(p[1]-item.y)/item.height-origin[1]];
            [(q[0]*ay[1]-q[1]*ay[0])/det,(ax[0]*q[1]-ax[1]*q[0])/det]
        }).collect()).collect::<Vec<_>>();
        self.image_cut_paths.entry(index).or_default().extend(normalized);
        Ok(())
    }
    /// Repeated placements of the same source image inherit its manual outline.
    /// Explicit per-image outlines take precedence; unrelated atlas tiles stay separate.
    pub fn image_cut_paths_for(&self, index: usize) -> Option<&Vec<Vec<Point>>> {
        self.image_cut_paths.get(&index).or_else(|| {
            self.image_cut_paths.iter().find_map(|(&owner, paths)| {
                let same = if self.image_tiles.is_empty() {true}
                    else {self.image_tiles.get(index).zip(self.image_tiles.get(owner)).is_some_and(|(a,b)| a==b)};
                (same && !paths.is_empty()).then_some(paths)
            })
        })
    }

    /// Detach one placement without removing outlines inherited by its other copies.
    /// Bake the offset, but leave smoothing to the usual page-cut pipeline.
    pub fn unlink_image_cut_paths(&mut self,index:usize)->Result<(),String> {
        if self.image_cut_paths_for(index).is_none_or(|paths|paths.is_empty()) {
            return Err("This image has no linked manual cutline.".into());
        }
        let mut isolated=self.clone();
        isolated.manual_cut_paths.clear();isolated.design_shapes.clear();isolated.smoothing=0.;
        isolated.hidden_images=(0..self.count).filter(|i|*i!=index).collect();
        let detached=isolated.manual_page_cut_paths()?;
        let mut next=self.clone();
        next.image_cut_paths=(0..self.count).filter_map(|i|self.image_cut_paths_for(i).cloned().map(|paths|(i,paths))).collect();
        // An explicit empty entry prevents the detached placement from inheriting
        // another copy's template again.
        next.image_cut_paths.insert(index,vec![]);
        next.manual_cut_paths.extend(detached);
        next.validate_manual_cut_paths()?;
        *self=next;Ok(())
    }
    pub fn manual_page_cut_paths(&self) -> Result<Vec<Vec<Point>>, String> {
        self.validate_manual_cut_paths()?;
        if !self.contour_offset.is_finite() || self.contour_offset.abs()>5. {return Err("Cut border must be between -5 and +5 mm.".into());}
        let mut paths=self.manual_cut_paths.clone();

        if !self.image_cut_paths.is_empty() {
            let items=self.editor_items()?;
            for (index,item) in items.iter().enumerate() {
                if self.hidden_images.contains(&index) {continue;}
                let Some(outlines)=self.image_cut_paths_for(index) else {continue};
                let [origin,right,_,bottom]=item.orientation.unit_corners();
                for outline in outlines {
                    let page:Vec<Point>=outline.iter().map(|p|[
                        item.x+(origin[0]+(right[0]-origin[0])*p[0]+(bottom[0]-origin[0])*p[1])*item.width,
                        item.y+(origin[1]+(right[1]-origin[1])*p[0]+(bottom[1]-origin[1])*p[1])*item.height]).collect();
                    if self.contour_offset==0. {paths.push(page);} else {
                        if page.first()!=page.last() {return Err("Close the linked cutline before adjusting Outline distance.".into());}
                        use geo::{Buffer, LineString, Polygon};
                        use geo::algorithm::buffer::{BufferStyle, LineJoin};
                        let polygon=Polygon::new(LineString::from(page.iter().map(|p|(p[0],p[1])).collect::<Vec<_>>()),vec![]);
                        let offset=polygon.buffer_with_style(BufferStyle::new(self.contour_offset).line_join(LineJoin::Miter(0.75)));
                        if offset.0.is_empty() {return Err("Outline distance removes the linked cutline. Use a smaller negative distance.".into());}
                        for polygon in offset.0 {paths.push(polygon.exterior().0.iter().map(|p|[p.x,p.y]).collect());}
                    }
                }
            }
        }
        if !self.smoothing.is_finite() || !(0. ..=MAX_SMOOTHING_MM).contains(&self.smoothing) {
            return Err("Outline smoothing must be between 0 and 10 mm.".into());
        }
        if self.smoothing > 0. {
            for path in &mut paths {
                let closed=path.len()>2 && path.first()==path.last();
                *path=if closed && self.preserve_corners {smooth_mixed_contour(path,self.smoothing)}
                    else {smooth_path(path,self.smoothing,closed)};
            }
        }
        paths.extend(self.design_shapes.iter().filter(|s|s.cut).map(|s|s.path.clone()));
        if paths.len()>100 || paths.iter().map(Vec::len).sum::<usize>()>100_000 {return Err("Too many manual cutline points.".into());}
        if paths.iter().flatten().any(|p|!p[0].is_finite() || !p[1].is_finite() || p[0]< -1e-7 || p[1]< -1e-7 || p[0]>self.page_width+1e-7 || p[1]>self.page_height+1e-7) {
            return Err("Linked cutline is outside the page. Adjust the image or Outline distance.".into());
        }
        Ok(paths)
    }
    pub fn has_print_designs(&self)->bool {self.design_shapes.iter().any(|s|s.print)}
    pub fn design_svg_paths(&self)->String {
        let mut svg=String::new();
        for shape in self.design_shapes.iter().filter(|s|s.print) {
            if shape.path.len()<2 {continue;}
            let closed=shape.path.first()==shape.path.last();
            let mut data=format!("M {} {}",shape.path[0][0],shape.path[0][1]);
            for p in shape.path.iter().skip(1) {data.push_str(&format!(" L {} {}",p[0],p[1]));}
            if closed {data.push_str(" Z");}
            let fill=shape.fill.filter(|_|closed).map_or("none".into(),|c|format!("rgb({},{},{})",c[0],c[1],c[2]));
            let opacity=shape.fill.map_or(1.,|c|c[3] as f64/255.);
            let c=shape.stroke;
            svg.push_str(&format!(r#"<path d="{}" fill="{}" fill-opacity="{}" stroke="rgb({},{},{})" stroke-opacity="{}" stroke-width="{}" stroke-linejoin="round" stroke-linecap="round"/>"#,
                data,fill,opacity,c[0],c[1],c[2],c[3] as f64/255.,shape.stroke_width));
        }
        svg
    }
    pub fn render_designs(&self,width:u32,height:u32)->Result<RgbaImage,String> {
        self.render_design_region(width,height,[0.,0.,self.page_width,self.page_height])
    }
    pub fn render_design_region(&self,width:u32,height:u32,region:[f64;4])->Result<RgbaImage,String> {
        self.validate_manual_cut_paths()?;
        if region.iter().any(|v|!v.is_finite()) || region[2]<=0. || region[3]<=0. {return Err("Invalid design preview region.".into());}
        if width==0 || height==0 || width as u64*height as u64>80_000_000 {return Err("Design preview is too large.".into());}
        let mut markup=self.svg_artwork_markup()?;markup.push_str(&self.design_svg_paths());
        let svg=format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="{} {} {} {}" preserveAspectRatio="none">{}</svg>"#,width,height,region[0],region[1],region[2],region[3],markup);
        let tree=usvg::Tree::from_str(&svg,&usvg::Options::default()).map_err(|e|e.to_string())?;
        let mut pixmap=resvg::tiny_skia::Pixmap::new(width,height).ok_or("Cannot allocate design preview")?;
        resvg::render(&tree,resvg::tiny_skia::Transform::identity(),&mut pixmap.as_mut());
        let mut result=RgbaImage::new(width,height);
        for (target,pixel) in result.pixels_mut().zip(pixmap.pixels()) {
            let c=pixel.demultiply();*target=image::Rgba([c.red(),c.green(),c.blue(),c.alpha()]);
        }
        Ok(result)
    }
    pub fn design_only_artwork(&self)->Result<(Self,Artwork),String> {
        let points:Vec<_>=self.design_shapes.iter().flat_map(|s|s.path.iter().copied()).collect();
        if points.is_empty() {return Err("Draw a design first.".into());}
        let x0=points.iter().map(|p|p[0]).fold(f64::INFINITY,f64::min);
        let y0=points.iter().map(|p|p[1]).fold(f64::INFINITY,f64::min);
        let x1=points.iter().map(|p|p[0]).fold(f64::NEG_INFINITY,f64::max);
        let y1=points.iter().map(|p|p[1]).fold(f64::NEG_INFINITY,f64::max);
        let mut layout=self.clone();layout.count=1;layout.custom.clear();layout.image_tiles.clear();layout.hidden_images.clear();layout.image_cut_paths.clear();
        layout.width=(x1-x0).max(1.);layout.height=(y1-y0).max(1.);layout.use_manual_cut_paths=true;
        layout.custom.insert(0,Placement{x:x0,y:y0,width:layout.width,height:layout.height,orientation:Default::default()});
        let art=Artwork{pixels:RgbaImage::new(1,1),outlines:vec![],holes:vec![],tiles:vec![],ellipse_bounds:None};
        Ok((layout,art))
    }
    pub fn print_design_raster(&self)->Result<RgbaImage,String> {self.print_design_raster_with_ccd(None)}
    pub fn print_design_raster_with_ccd(&self,ccd:Option<&CcdTrial>)->Result<RgbaImage,String> {
        let (layout,art)=self.design_only_artwork()?;
        layout.print_raster(&art,ccd)
    }
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
        if !self.smoothing.is_finite() || !(0.0..=MAX_SMOOTHING_MM).contains(&self.smoothing) {
            return Err("Smoothing must be between 0 and 10 mm.".into());
        }
        let preserve_corners = self.preserve_corners;
        if preserve_corners {
            let pixel=(self.width/art.pixels.width().max(1) as f64).max(self.height/art.pixels.height().max(1) as f64);
            for path in original.iter_mut().chain(holes.iter_mut()) {*path=straighten_polygon(path,pixel);}
        }
        if self.smoothing > 0. && (self.contour_offset == 0. || preserve_corners) {
            for path in original.iter_mut().chain(holes.iter_mut()) {
                *path = if preserve_corners {
                    smooth_mixed_contour(path, self.smoothing)
                } else {
                    smooth_contour(path, self.smoothing)
                };
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
        let result = if preserve_corners {
            use geo::algorithm::buffer::{BufferStyle, LineJoin};
            polygons.buffer_with_style(
                BufferStyle::new(self.contour_offset).line_join(LineJoin::Miter(0.75)),
            )
        } else {
            polygons.buffer(self.contour_offset)
        };
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
                *path = if preserve_corners {
                    // Buffering introduces fresh tiny joins even after input cleanup.
                    // A bounded finishing pass removes them without rounding logo tips.
                    smooth_mixed_contour(path,self.smoothing.min(0.25))
                }else{smooth_contour(path, self.smoothing)};
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
        if !self.has_visible_images() && !self.has_print_designs() {
            return Err("Show at least one image before printing.".into());
        }
        let items = self.items()?;
        if let Some(profile) = ccd {
            profile.validated_cut_paths(self, a)?;
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
            if let Some(asset)=self.svg_source_for(i) {
                let tile=self.image_tiles.get(i).copied().unwrap_or(asset.raster_rect);
                let mut raster=asset.raster(tile,mm(item.width).max(1),mm(item.height).max(1),item.orientation)?;self.apply_opacity(i,&mut raster);
                image::imageops::overlay(&mut page,&raster,mm(item.x) as i64,mm(item.y) as i64);continue;
            }
            let key = (
                mm(item.width).max(1),
                mm(item.height).max(1),
                if a.tiles.is_empty() { 0 } else { i },
                item.orientation.raster_key(),
            );
            let raster = cache
                .entry(key)
                .or_insert_with(|| item.orientation.raster(&a.at(i).pixels, key.0, key.1));
            if self.opacity_for(i)==255 {image::imageops::overlay(&mut page,raster,mm(item.x) as i64,mm(item.y) as i64);}else {
                let mut faded=raster.clone();self.apply_opacity(i,&mut faded);
                image::imageops::overlay(&mut page,&faded,mm(item.x) as i64,mm(item.y) as i64);
            }
        }
        if self.has_print_designs() {
            let mut designs=self.clone();designs.svg_sources.clear();
            let overlay=designs.render_designs(page.width(),page.height())?;
            image::imageops::overlay(&mut page,&overlay,0,0);
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
            || (self.page_margins.is_none() && self.margin < 5.)
            || self.units <= 0.
            || self.units > 1000.
        {
            return Err("Invalid layout values. Margin must be at least 5 mm.".into());
        }
        let [left, right, top, bottom] = self.margins();
        if self.page_margins.is_some()
            && !Self::margins_fit(self.margins(), self.page_width, self.page_height)
        {
            return Err("Margins must leave at least 1 mm of usable width and height.".into());
        }
        let cols = ((self.page_width - left - right + self.column_gap)
            / (self.width + self.column_gap))
            .floor()
            .max(0.) as usize;
        let rows = ((self.page_height - top - bottom + self.row_gap) / (self.height + self.row_gap))
            .floor()
            .max(0.) as usize;
        Ok((cols, rows))
    }
    fn resolved_items(&self, fill: bool) -> Result<Vec<Placement>, String> {
        let (cols, rows) = self.capacity()?;
        let [left, right, top, bottom] = self.margins();
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
                    x: left + (i % cols) as f64 * (self.width + self.column_gap),
                    y: top + (i / cols) as f64 * (self.height + self.row_gap),
                    width: self.width,
                    height: self.height,
                    orientation: Default::default(),
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
                orientation: Default::default(),
            }
        };
        let limit = if fill { 1000 } else { self.count };
        let mut slots = vec![None; limit];
        for i in 0..self.count.min(limit) {
            let item = if let Some(p) = self.custom.get(&i) {
                Some(*p)
            } else if i < nominal && !(prefix && last.is_some_and(|last| i > last)) {
                Some(Placement {
                    x: left + (i % cols) as f64 * (self.width + self.column_gap),
                    y: top + (i / cols) as f64 * (self.height + self.row_gap),
                    width: self.width,
                    height: self.height,
                    orientation: Default::default(),
                })
            } else {
                None
            };
            if let Some(p) = item {
                if [p.x, p.y, p.width, p.height].iter().any(|v| !v.is_finite())
                    || !p.orientation.valid()
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
            let mut xs = vec![left];
            let mut ys = vec![top];
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
                    if x < left.min(occupied.iter().map(|p| p.x).fold(left, f64::min)) - 1e-8
                        || y < top.min(occupied.iter().map(|p| p.y).fold(top, f64::min)) - 1e-8
                        || x + prototype.width > self.page_width - right + 1e-8
                        || y + prototype.height > self.page_height - bottom + 1e-8
                    {
                        continue;
                    }
                    let p = Placement {
                        x,
                        y,
                        width: prototype.width,
                        ..prototype
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
    pub fn can_repeat_tiled_image(&self) -> bool {
        self.image_tiles.first().is_some_and(|first| self.image_tiles.iter().all(|tile| tile==first))
    }
    pub fn with_copy_count(&self, count: usize) -> Result<Self,String> {
        if count==0 || count>if self.image_tiles.is_empty(){1000}else{24} {return Err("Too many copies for this layout.".into());}
        let mut next=self.clone();
        if !self.image_tiles.is_empty() && !self.can_repeat_tiled_image() {return Err("Use Duplicate for a sheet with different images.".into());}
        let old_count=self.count;
        next.count=count;
        next.custom.retain(|i,_|*i<count);
        next.hidden_images.retain(|i|*i<count);next.image_opacity.retain(|i,_|*i<count);
        next.image_groups.retain(|i,_|*i<count);
        next.image_cut_paths.retain(|i,_|*i<count);next.svg_sources.retain(|i,_|*i<count);
        if let Some(tile)=self.image_tiles.first() {
            let items=next.items()?;
            next.custom=items.into_iter().enumerate().collect();
            next.image_tiles=vec![*tile;count];
            for new in old_count..count {
                if let Some(paths)=self.image_cut_paths_for(0) {next.image_cut_paths.insert(new,paths.clone());}
            }
        } else {next.items()?;}
        Ok(next)
    }
    pub fn available_capacity(&self) -> Result<usize, String> {
        Ok(self.resolved_items(true)?.len())
    }
    pub fn items(&self) -> Result<Vec<Placement>, String> {
        self.resolved_items(false)
    }
    /// Keep all valid artwork visible while editing an overflowing layout.
    /// Production output still uses `items` and its strict page validation.
    pub fn editor_items(&self) -> Result<Vec<Placement>, String> {
        match self.items() {
            Ok(items) => Ok(items),
            Err(error) => {
                let (cols, _) = self.capacity()?;
                if !(1..=1000).contains(&self.count) { return Err(error); }
                let [left,right,top,_] = self.margins();
                let fitting = if self.custom.is_empty() {Vec::new()} else {self.resolved_items(true).unwrap_or_default()};
                let prototype=self.custom.iter().next_back().map(|(_,p)|*p).unwrap_or(Placement {
                    x:left,y:top,width:self.width,height:self.height,orientation:Default::default(),
                });
                let valid=|p:&Placement| [p.x,p.y,p.width,p.height].iter().all(|v|v.is_finite() && v.abs()<1_000_000.)
                    && p.width>=1. && p.height>=1. && p.orientation.valid();
                if !valid(&prototype) || self.custom.values().any(|p|!valid(p)) {return Err(error);}
                let overflow_top=fitting.iter().chain(self.custom.values()).map(|p|p.y+p.height).fold(top,f64::max)+self.row_gap;
                let overflow_cols=((self.page_width-left-right+self.column_gap)/(prototype.width+self.column_gap)).floor().max(1.) as usize;
                let columns=cols.max(1);
                Ok((0..self.count).map(|i| {
                    if let Some(p)=self.custom.get(&i) {return *p;}
                    if let Some(p)=fitting.get(i) {return *p;}
                    if self.custom.is_empty() {
                        Placement{x:left+(i%columns) as f64*(self.width+self.column_gap),
                            y:top+(i/columns) as f64*(self.height+self.row_gap),width:self.width,height:self.height,orientation:Default::default()}
                    } else {
                        let extra=i.saturating_sub(fitting.len());
                        Placement{x:left+(extra%overflow_cols) as f64*(prototype.width+self.column_gap),
                            y:overflow_top+(extra/overflow_cols) as f64*(prototype.height+self.row_gap),..prototype}
                    }
                }).collect())
            }
        }
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
        if self.use_manual_cut_paths {
            self.validate_manual_cut_paths()?;
            let paths=self.manual_page_cut_paths()?;
            if paths.is_empty() { return Err("Draw a manual cut line or switch to automatic outlines.".into()); }
            return Ok(paths);
        }
        cache.prepare(a);
        let mut paths = Vec::new();
        let mut groups = std::collections::BTreeMap::<usize, Vec<Vec<Point>>>::new();
        for (i, item) in self.items()?.into_iter().enumerate() {
            if self.hidden_images.contains(&i) {
                continue;
            }
            let group = self.image_groups.get(&i);
            let cut_holes = self.cut_inner_holes && group.is_none();
            // Position is deliberately excluded; validation-affecting settings are retained.
            let [left, right, top, bottom] = self.margins();
            let (width, height) = item.orientation.source_size(item.width, item.height);
            // SVG base geometry is independent of raster DPI; optional smoothing
            // cleans the final vector offset without retracing pixels.
            let raster=a.at(i);
            let frame=raster.holes.is_empty() && raster.outlines.len()==1 && raster.outlines[0].len()==5 && [[0.,0.],[1.,0.],[1.,1.],[0.,1.]].iter().all(|corner|raster.outlines[0].contains(corner));
            if let Some(asset)=self.svg_source_for(i).filter(|_|raster.ellipse_bounds.is_none() && !frame) {
                use std::hash::{Hash,Hasher};
                let tile=self.image_tiles.get(i).copied().unwrap_or(asset.raster_rect);
                let mut hash=std::collections::hash_map::DefaultHasher::new();
                asset.source.hash(&mut hash);asset.viewport.map(f64::to_bits).hash(&mut hash);asset.raster_rect.hash(&mut hash);
                if !self.smoothing.is_finite() || !(0. ..=MAX_SMOOTHING_MM).contains(&self.smoothing) {return Err("Smoothing must be between 0 and 10 mm.".into());}
                let key=[hash.finish(),width.to_bits(),height.to_bits(),self.contour_offset.to_bits(),cut_holes as u64,tile[0] as u64,tile[1] as u64,tile[2] as u64,tile[3] as u64,self.smoothing.to_bits(),self.preserve_corners as u64];
                if !cache.svg_entries.contains_key(&key) {
                    if cache.svg_entries.len()>=64 {cache.svg_entries.clear();}
                    let result=asset.vector_cut_paths(tile,width,height,self.contour_offset,cut_holes).and_then(|mut paths| {
                        if self.smoothing>0. {
                            for path in &mut paths {*path=if self.preserve_corners {smooth_mixed_contour(path,self.smoothing)}else{smooth_contour(path,self.smoothing)};}
                        }
                        let mut local=self.clone();local.width=width;local.height=height;
                        if !self.custom.is_empty() {local.count=1;local.custom.clear();}
                        local.validate_outline_spacing(&paths)?;Ok(paths)
                    });
                    cache.svg_entries.insert(key,result);
                }
                let [origin,right,_,bottom]=item.orientation.unit_corners();
                let target=group.map_or(&mut paths,|id|groups.entry(*id).or_default());
                for outline in cache.svg_entries[&key].as_ref().map_err(Clone::clone)? {
                    target.push(outline.iter().map(|p| {let u=p[0]/width;let v=p[1]/height;[
                        item.x+(origin[0]+(right[0]-origin[0])*u+(bottom[0]-origin[0])*v)*item.width,
                        item.y+(origin[1]+(right[1]-origin[1])*u+(bottom[1]-origin[1])*v)*item.height]}).collect());
                }
                continue;
            }
            let key = [
                width.to_bits(),
                height.to_bits(),
                self.contour_offset.to_bits(),
                self.smoothing.to_bits(),
                cut_holes as u64 | ((self.preserve_corners as u64) << 1),
                self.custom.is_empty() as u64,
                self.count as u64,
                self.page_width.to_bits(),
                self.page_height.to_bits(),
                left.to_bits(),
                right.to_bits(),
                top.to_bits(),
                bottom.to_bits(),
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
                local.width = width;
                local.height = height;
                local.cut_inner_holes = cut_holes;
                if !self.custom.is_empty() {
                    local.count = 1;
                    local.custom.clear();
                }
                cache.entries.insert(key, local.outline_mm(a.at(i)));
            }
            let [origin, right, _, bottom] = item.orientation.unit_corners();
            let [dx, dy] = [right[0] - origin[0], right[1] - origin[1]];
            let [ex, ey] = [bottom[0] - origin[0], bottom[1] - origin[1]];
            let target = group.map_or(&mut paths, |id| groups.entry(*id).or_default());
            for outline in cache.entries[&key].as_ref().map_err(Clone::clone)? {
                target.push(
                    outline
                        .iter()
                        .map(|p| {
                            let (u, v) = (p[0] / width, p[1] / height);
                            [
                                item.x + (origin[0] + dx * u + ex * v) * item.width,
                                item.y + (origin[1] + dy * u + ey * v) * item.height,
                            ]
                        })
                        .collect(),
                );
            }
        }
        for outlines in groups.into_values() {
            paths.push(group_cut_outline(outlines)?);
        }
        paths.extend(self.design_shapes.iter().filter(|s|s.cut).map(|s|s.path.clone()));
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
        self.to_cutter_coordinates(self.page_cut_paths_cached(a, cache)?)
    }
    fn to_cutter_coordinates(&self, outlines: Vec<Vec<Point>>) -> Result<Vec<Vec<Point>>, String> {
        let mut paths = Vec::new();
        for outline in outlines {
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
    pub fn manual_design_plt(&self) -> Result<String, String> {
        self.validate_manual_cut_paths()?;
        let resolved=self.manual_page_cut_paths()?;
        if resolved.is_empty() {
            return Err("Draw a cut shape before exporting.".into());
        }
        if !(0. ..=1000.).contains(&self.units) || self.units == 0. {
            return Err("Invalid plotter units.".into());
        }
        let mut page = self.clone();
        page.offset_x = 0.;
        page.offset_y = 0.;
        let paths = page.to_cutter_coordinates(resolved)?;
        let mut out = String::from("PA;PU0,0;");
        for path in paths {
            write_plt_path(&mut out, &path, self.units, false);
        }
        out.push_str("PU0,0;!PG;");
        Ok(out)
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
        if !self.has_visible_images() && !self.has_print_designs() {
            return Err("Show at least one image before printing or exporting.".into());
        }
        let mut encoded_images = HashMap::new();
        let mut s = String::from(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>A4 Sticker Print</title><style>@page{size:A4 portrait;margin:0}*{box-sizing:border-box}body{margin:0;background:#ddd;font:16px sans-serif}.toolbar{padding:16px;background:#fff}.page{width:210mm;height:297mm;background:white;margin:15px auto}svg{display:block;width:210mm;height:297mm}@media print{.toolbar{display:none}body{background:white}.page{margin:0;break-after:avoid}}</style></head><body><div class=\"toolbar\"><button onclick=\"window.print()\">Print A4</button> A4 portrait / Scale 100% (Actual size) / Margins None / Headers and footers Off. Disable Fit to page. Printed crosses are manual alignment references only.</div><div class=\"page\"><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"210mm\" height=\"297mm\" viewBox=\"0 0 210 297\">",
        );
        s.push_str("<rect width=\"210\" height=\"297\" fill=\"white\"/>");
        for (i, p) in self.items()?.into_iter().enumerate() {
            if self.hidden_images.contains(&i) {
                continue;
            }
            if let Some(asset)=self.svg_source_for(i) {
                write!(s,"<g opacity=\"{}\">",self.opacity_for(i) as f64/255.).unwrap();s.push_str(&asset.markup(p,self.image_tiles.get(i).copied().unwrap_or(asset.raster_rect),i)?);s.push_str("</g>");continue;
            }
            let image = a.at(i);
            let key = std::ptr::from_ref(image);
            let data = match encoded_images.entry(key) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let mut png = std::io::Cursor::new(Vec::new());
                    image
                        .pixels
                        .write_to(&mut png, image::ImageFormat::Png)
                        .map_err(|error| format!("Cannot encode print image: {error}"))?;
                    entry.insert(base64::engine::general_purpose::STANDARD.encode(png.into_inner()))
                }
            };
            write!(s,"<g opacity=\"{}\">",self.opacity_for(i) as f64/255.).unwrap();
            if p.orientation == Orientation::default() {
                write!(s,"<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{}\"/>",p.x,p.y,p.width,p.height,data).unwrap();
            } else {
                let (width, height) = p.orientation.source_size(p.width, p.height);
                let [x, y] = p.orientation.point([0., 0.], p.width, p.height);
                let [xx, xy] = p.orientation.point([1., 0.], p.width, p.height);
                let [yx, yy] = p.orientation.point([0., 1.], p.width, p.height);
                write!(s, r#"<image width="{width}" height="{height}" preserveAspectRatio="none" transform="matrix({} {} {} {} {} {})" href="data:image/png;base64,{data}"/>"#,
                    xx - x, xy - y, yx - x, yy - y, p.x + x, p.y + y).unwrap();
            }
            s.push_str("</g>");
        }
        s.push_str(&self.design_svg_paths());
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
    fn html_export_embeds_the_processed_pixels_for_visible_tiles() {
        let mut art = sample();
        let mut first = sample();
        first.pixels.put_pixel(0, 0, image::Rgba([1, 2, 3, 0]));
        let first = std::sync::Arc::new(first);
        let second = std::sync::Arc::new(sample());
        art.tiles = vec![first.clone(), second, first.clone()];
        let mut layout = Layout {
            count: 3,
            ..Layout::default()
        };
        layout.hidden_images.insert(1);
        let html = layout.print_html(&art).unwrap();
        let images: Vec<_> = html.split("data:image/png;base64,").skip(1).collect();
        assert_eq!(images.len(), 2);
        for image in images {
            let encoded = image.split_once('"').unwrap().0;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap();
            assert_eq!(
                image::load_from_memory(&bytes).unwrap().into_rgba8(),
                first.pixels
            );
        }
    }

    #[test]
    fn per_edge_margins_round_trip_and_set_grid_origin_and_capacity() {
        let mut layout = Layout {
            count: 2,
            ..Layout::default()
        };
        let legacy: Layout =
            serde_json::from_value(serde_json::to_value(&layout).unwrap()).unwrap();
        assert_eq!(legacy.margins(), [15.; 4]);
        layout.page_margins = Some([10., 100., 25., 70.]);
        let restored: Layout =
            serde_json::from_value(serde_json::to_value(&layout).unwrap()).unwrap();
        assert_eq!(restored.margins(), [10., 100., 25., 70.]);
        assert_eq!(restored.capacity().unwrap(), (2, 5));
        assert_eq!(
            (
                restored.items().unwrap()[0].x,
                restored.items().unwrap()[0].y
            ),
            (10., 25.)
        );
        layout.page_margins = Some([110., 100., 25., 70.]);
        assert!(layout.capacity().is_err());
    }

    #[test]
    fn manual_cut_line_replaces_auto_trace_in_preview_and_plt() {
        let art = sample();
        let path = vec![[20., 20.], [50., 22.], [40., 45.], [20., 20.]];
        let mut layout = Layout {
            count: 1,
            manual_cut_paths: vec![path.clone()],
            use_manual_cut_paths: true,
            ..Layout::default()
        };
        assert_eq!(layout.page_cut_paths(&art).unwrap(), vec![path]);
        assert!(
            layout
                .plt(&art)
                .unwrap()
                .contains("PU800,11080;PD2000,11000;PD1600,10080;PD800,11080;")
        );
        let (_, ccd) = CcdTrial::default().pair(&layout, &art).unwrap();
        assert!(ccd.contains("PU480,480;PD560,1680;PD1480,1280;PD480,480;"));
        assert_eq!(
            layout.manual_design_plt().unwrap(),
            "PA;PU0,0;PU800,11080;PD2000,11000;PD1600,10080;PD800,11080;PU0,0;!PG;"
        );
        layout.use_manual_cut_paths = false;
        assert_ne!(
            layout.page_cut_paths(&art).unwrap(),
            layout.manual_cut_paths
        );
    }
    #[test]
    fn image_opacity_roundtrips_and_changes_print_but_not_cut_geometry() {
        let art=sample();let mut layout=Layout{count:1,..Default::default()};
        let original=layout.page_cut_paths(&art).unwrap();
        assert_eq!(layout.opacity_for(0),255);
        layout.image_opacity.insert(0,128);
        let loaded:Layout=serde_json::from_str(&serde_json::to_string(&layout).unwrap()).unwrap();
        assert_eq!(loaded.opacity_for(0),128);
        assert_eq!(loaded.page_cut_paths(&art).unwrap(),original);
        assert!(loaded.print_html(&art).unwrap().contains("<g opacity=\"0.5019607843137255\">"));
        let mut raster=RgbaImage::from_pixel(1,1,image::Rgba([20,40,60,128]));
        loaded.apply_opacity(0,&mut raster);assert_eq!(raster.get_pixel(0,0).0,[20,40,60,64]);
        layout.image_opacity.insert(0,0);
        let page=layout.print_raster(&art,None).unwrap();
        let item=layout.items().unwrap()[0];
        let x=((item.x+item.width/2.)/25.4*300.).round() as u32;
        let y=((item.y+item.height/2.)/25.4*300.).round() as u32;
        assert_eq!(page.get_pixel(x,y).0,[255,255,255,255]);
    }
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
                    orientation: Default::default(),
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
                    height: 52.,
                    orientation: Default::default(),
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
    fn mixed_outline_smooths_arc_and_keeps_rectangle_corners() {
        let mut path = vec![[0., 20.]];
        for i in 0..=1000 {
            let t = i as f64 * std::f64::consts::PI / 1000.;
            let r = 20. + 0.07 * (100. * t).sin();
            path.push([40. - r * t.cos(), 20. - r * t.sin()]);
        }
        let corners = [
            [0., 20.],
            [20., 20.],
            [60., 20.],
            [80., 20.],
            [80., 55.],
            [0., 55.],
        ];
        path.extend_from_slice(&[[80., 20.], [80., 55.], [0., 55.], [0., 20.]]);
        let smoothed = smooth_mixed_contour(&path, 1.);
        assert_eq!(smoothed.first(), smoothed.last());
        for corner in corners {
            assert!(
                smoothed
                    .iter()
                    .any(|p| (p[0] - corner[0]).hypot(p[1] - corner[1]) < 0.001),
                "lost corner {corner:?}"
            );
        }
        let arc: Vec<_> = smoothed.iter().filter(|p| p[1] < 18.).collect();
        assert!(arc.len() > 10);
        assert!(
            arc.iter()
                .all(|p| ((p[0] - 40.).hypot(p[1] - 20.) - 20.).abs() < 0.04)
        );
        assert!(
            smoothed
                .windows(2)
                .any(|edge| edge == [[80., 55.], [0., 55.]])
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
            preserve_corners: false,
            count: 1,
            ..Default::default()
        };
        let before = base.outline_mm(&art).unwrap();
        let after = Layout {
            smoothing: 0.5,
            preserve_corners: false,
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
            preserve_corners: false,
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
    fn image_groups_emit_one_outer_path_for_overlaps_and_gaps() {
        let art = sample();
        let mut layout = Layout {
            count: 2,
            ..Default::default()
        };
        let first = layout.items().unwrap()[0];
        layout.custom.insert(0, first);
        layout.custom.insert(1, Placement { x: 85., ..first });
        let separate = layout.page_cut_paths(&art).unwrap();
        let html = layout.print_html(&art).unwrap();
        assert_eq!(separate.len(), 2);

        layout.image_groups.extend([(0, 0), (1, 0)]);
        let grouped = layout.page_cut_paths(&art).unwrap();
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].first(), grouped[0].last());
        let area = |path: &[Point]| {
            path.windows(2)
                .map(|edge| edge[0][0] * edge[1][1] - edge[1][0] * edge[0][1])
                .sum::<f64>()
                .abs()
                / 2.
        };
        let bridge_area = area(&grouped[0]) - separate.iter().map(|path| area(path)).sum::<f64>();
        assert!(
            (0. ..100.).contains(&bridge_area),
            "Only the narrow bridge adds area"
        );
        assert_eq!(layout.print_html(&art).unwrap(), html);
        assert!(layout.plt(&art).is_ok());
        assert_eq!(
            CcdTrial::default().cut_paths(&layout, &art).unwrap().len(),
            1
        );

        layout.custom.get_mut(&1).unwrap().x = first.x + 10.;
        layout.custom.get_mut(&1).unwrap().y = first.y + 10.;
        let joined = layout.page_cut_paths(&art).unwrap();
        assert_eq!(joined.len(), 1);
        assert!(
            joined[0].len() >= 9,
            "Preserve the concave outline of connected images"
        );

        layout.hidden_images.insert(1);
        assert_eq!(
            layout.page_cut_paths(&art).unwrap(),
            vec![separate[0].clone()]
        );
        layout.hidden_images.clear();
        layout.image_groups.clear();
        assert_eq!(layout.page_cut_paths(&art).unwrap().len(), 2);
    }

    #[test]
    fn fractional_rotation_preserves_center_and_aligns_raster_and_contour() {
        let mut layout = Layout {
            count: 1,
            width: 30.,
            height: 20.,
            ..Default::default()
        };
        let original = layout.items().unwrap()[0];
        let mut art = sample();
        art.pixels = RgbaImage::from_fn(20, 20, |x, y| {
            if x >= 10 && y < 10 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([255, 255, 255, 255])
            }
        });
        let path = layout.page_cut_paths(&art).unwrap().remove(0);
        let rotated = original.with_rotation(37.5);
        layout.custom.insert(0, rotated);
        let center = [original.x + 15., original.y + 10.];
        let (sin, cos) = 37.5_f64.to_radians().sin_cos();
        let rotate = |[x, y]: Point| {
            let (dx, dy) = (x - center[0], y - center[1]);
            [
                center[0] + cos * dx - sin * dy,
                center[1] + sin * dx + cos * dy,
            ]
        };
        for (p, actual) in path
            .into_iter()
            .zip(layout.page_cut_paths(&art).unwrap().remove(0))
        {
            let expected = rotate(p);
            assert!((actual[0] - expected[0]).abs() < 1e-8);
            assert!((actual[1] - expected[1]).abs() < 1e-8);
        }
        let raster = layout.print_raster(&art, None).unwrap();
        let [x, y] = rotate([original.x + 22.5, original.y + 5.]);
        let mm = |v: f64| (v / 25.4 * 300.).round() as u32;
        assert!(raster.get_pixel(mm(x), mm(y))[1] < 20);
        assert!(
            layout
                .print_html(&art)
                .unwrap()
                .contains("transform=\"matrix(")
        );
        assert!(layout.plt(&art).is_ok());

        let json = serde_json::to_string(&rotated).unwrap();
        let loaded: Placement = serde_json::from_str(&json).unwrap();
        assert!((loaded.orientation.degrees() - 37.5).abs() < 1e-8);
        let reset = loaded.with_rotation(0.);
        assert!((reset.width - original.width).abs() < 1e-8);
        assert!((reset.height - original.height).abs() < 1e-8);
    }

    #[test]
    fn rotated_and_flipped_image_keeps_print_and_cut_aligned() {
        let art = sample();
        let mut layout = Layout {
            count: 1,
            width: 30.,
            height: 20.,
            ..Default::default()
        };
        let original = layout.items().unwrap()[0];
        let first = layout.page_cut_paths(&art).unwrap().remove(0);
        let mut item = original.turned_quarter(true);
        item.orientation.flip(true);
        item.x = original.x;
        item.y = original.y;
        layout.custom.insert(0, item);

        let transformed = layout.page_cut_paths(&art).unwrap().remove(0);
        assert_eq!(first.len(), transformed.len());
        for ([x, y], actual) in first.into_iter().zip(transformed) {
            assert!((actual[0] - (item.x + y - original.y)).abs() < 1e-8);
            assert!((actual[1] - (item.y + x - original.x)).abs() < 1e-8);
        }
        assert!(layout.plt(&art).is_ok());
        assert!(layout.print_html(&art).unwrap().contains(
            r#"width="30" height="20" preserveAspectRatio="none" transform="matrix(0 1 1 0 15 15)""#
        ));

        let mut pixels = RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255]));
        for y in 0..2 {
            for x in 2..4 {
                pixels.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let raster_art = Artwork { pixels, ..art };
        let raster = layout.print_raster(&raster_art, None).unwrap();
        let mm = |v: f64| (v / 25.4 * 300.).round() as u32;
        assert!(raster.get_pixel(mm(19.), mm(39.))[1] < 20);
        assert!(raster.get_pixel(mm(31.), mm(21.))[1] > 240);

        let old: Placement =
            serde_json::from_str(r#"{"x":15,"y":15,"width":30,"height":20}"#).unwrap();
        assert_eq!(old, original);
        let saved = serde_json::to_string(&item).unwrap();
        assert_eq!(serde_json::from_str::<Placement>(&saved).unwrap(), item);
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
                orientation: Default::default(),
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
            orientation: Default::default(),
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
                orientation: Default::default(),
            },
        );
        l.custom.insert(
            1,
            Placement {
                x: 80.,
                y: 90.,
                width: 30.,
                height: 12.,
                orientation: Default::default(),
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
                orientation: Default::default(),
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
            orientation: Default::default(),
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

#[cfg(test)]
mod oversized_preview_tests {
    use super::*;
    #[test]
    fn oversized_artwork_keeps_dimensions_in_editor_and_output_stays_validated() {
        let layout = Layout { count: 1, width: 350., height: 420., ..Layout::default() };
        let items = layout.editor_items().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].width, items[0].height), (350., 420.));
        assert!(layout.items().is_err());
        let repeated = Layout { count: 2, ..layout };
        assert_eq!(repeated.editor_items().unwrap().len(),2);
    }
}

#[cfg(test)]
mod crop_tests {
    use super::*;
    #[test]
    fn crop_retains_physical_scale_and_rotation() {
        let item = Placement { x: 10., y: 20., width: 100., height: 80., orientation: Default::default() };
        let cropped = item.cropped([0.1, 0.25, 0.6, 0.5]);
        assert_eq!((cropped.x, cropped.y, cropped.width, cropped.height), (20., 40., 60., 40.));
        for angle in [90., 33., 180.] {
            let rotated = item.with_rotation(angle);
            let crop = rotated.cropped([0.1, 0.25, 0.6, 0.5]);
            let (w, h) = crop.orientation.source_size(crop.width, crop.height);
            assert!((w - 60.).abs() < 1e-8 && (h - 40.).abs() < 1e-8);
            assert_eq!(crop.orientation.degrees(), rotated.orientation.degrees());
        }
    }
}

#[cfg(test)]
mod linked_cut_geometry_tests {
    use super::*;
    #[test]
    fn linked_path_tracks_scale_rotation_reflection_and_physical_offset() {
        let mut layout=Layout{count:1,width:40.,height:40.,use_manual_cut_paths:true,..Default::default()};
        let item=Placement{x:50.,y:50.,width:40.,height:40.,orientation:Default::default()};
        layout.custom.insert(0,item);
        layout.link_cut_paths(0,vec![vec![[55.,55.],[85.,55.],[85.,85.],[55.,85.],[55.,55.]]]).unwrap();
        assert_eq!(layout.manual_page_cut_paths().unwrap()[0][0],[55.,55.]);
        layout.custom.insert(0,Placement{width:80.,height:80.,..item});
        assert_eq!(layout.manual_page_cut_paths().unwrap()[0][0],[60.,60.]);
        let rotated=layout.custom[&0].with_rotation(90.);
        layout.custom.insert(0,rotated);
        let points=layout.manual_page_cut_paths().unwrap();
        assert!((points[0][0][0]-120.).abs()<1e-8);
        assert!((points[0][0][1]-60.).abs()<1e-8);
        layout.contour_offset=2.;
        let points=layout.manual_page_cut_paths().unwrap();
        let min_x=points.iter().flatten().map(|p|p[0]).fold(f64::INFINITY,f64::min);
        assert!((min_x-58.).abs()<1e-6);
        layout.contour_offset=-2.;
        let points=layout.manual_page_cut_paths().unwrap();
        let min_x=points.iter().flatten().map(|p|p[0]).fold(f64::INFINITY,f64::min);
        assert!((min_x-62.).abs()<1e-6);
        layout.contour_offset=0.;
        let item=layout.custom[&0].with_rotation(33.);
        layout.custom.insert(0,item);
        let before=layout.manual_page_cut_paths().unwrap();
        let mut flipped=item;
        flipped.orientation.flip(true);
        layout.custom.insert(0,flipped);
        let after=layout.manual_page_cut_paths().unwrap();
        for (a,b) in before[0].iter().zip(&after[0]) {
            assert!((b[0]-(item.x+item.width-(a[0]-item.x))).abs()<1e-8);
            assert!((b[1]-a[1]).abs()<1e-8);
        }
        layout.hidden_images.insert(0);
        assert!(layout.manual_page_cut_paths().unwrap().is_empty());
    }
}

#[cfg(test)]
mod manual_smoothing_tests {
    use super::*;
    #[test]
    fn smoothing_changes_export_without_changing_original_points_and_keeps_endpoints() {
        let path=vec![[30.,30.],[70.,30.],[70.,70.],[30.,70.],[30.,30.]];
        let mut layout=Layout{count:1,use_manual_cut_paths:true,manual_cut_paths:vec![path.clone()],..Default::default()};
        let original_export=layout.manual_design_plt().unwrap();
        layout.smoothing=1.;layout.preserve_corners=false;
        let smooth=layout.manual_page_cut_paths().unwrap();
        assert_ne!(smooth[0],path);
        assert_eq!(smooth[0].first(),smooth[0].last());
        assert_eq!(layout.manual_cut_paths[0],path);
        assert_ne!(layout.manual_design_plt().unwrap(),original_export);
        layout.manual_cut_paths=vec![vec![[30.,30.],[40.,31.],[50.,29.],[60.,30.]]];
        let open=layout.manual_page_cut_paths().unwrap();
        assert_eq!(open[0].first(),Some(&[30.,30.]));
        assert_eq!(open[0].last(),Some(&[60.,30.]));
    }
    #[test]
    fn smoothing_applies_to_image_linked_paths_after_offset() {
        let mut layout=Layout{count:1,width:40.,height:40.,use_manual_cut_paths:true,..Default::default()};
        layout.link_cut_paths(0,vec![vec![[20.,20.],[50.,20.],[50.,50.],[20.,50.],[20.,20.]]]).unwrap();
        layout.contour_offset=1.;
        let offset=layout.manual_page_cut_paths().unwrap();
        let original=layout.image_cut_paths.clone();
        layout.smoothing=1.;layout.preserve_corners=false;
        assert_ne!(layout.manual_page_cut_paths().unwrap(),offset);
        assert_eq!(layout.image_cut_paths,original);
        layout.smoothing=0.;
        assert_eq!(layout.manual_page_cut_paths().unwrap(),offset);
    }
}

#[cfg(test)]
mod repeat_manual_outline_tests {
    use super::*;
    #[test]
    fn twelve_repeated_photos_all_receive_outline_in_preview_and_export() {
        let mut layout=Layout{count:12,width:40.,height:50.,use_manual_cut_paths:true,..Default::default()};
        layout.link_cut_paths(0,vec![vec![[20.,20.],[50.,20.],[50.,60.],[20.,60.],[20.,20.]]]).unwrap();
        let paths=layout.manual_page_cut_paths().unwrap();
        let items=layout.items().unwrap();
        assert_eq!(paths.len(),12);
        for (path,item) in paths.iter().zip(&items) {
            assert!((path[0][0]-item.x-5.).abs()<1e-8);
            assert!((path[0][1]-item.y-5.).abs()<1e-8);
        }
        assert_eq!(layout.manual_design_plt().unwrap().matches("PD").count(),48);
        layout.hidden_images.insert(0);
        assert_eq!(layout.manual_page_cut_paths().unwrap().len(),11);
        layout.hidden_images.clear();
        layout.custom=items.into_iter().enumerate().collect();
        layout.image_tiles=vec![[0,0,100,100];12];
        assert_eq!(layout.manual_page_cut_paths().unwrap().len(),12);
        layout.image_tiles[11]=[100,0,100,100];
        assert_eq!(layout.manual_page_cut_paths().unwrap().len(),11);
        assert!(layout.image_cut_paths_for(11).is_none());
    }
}

#[cfg(test)]
mod tiled_repeat_tests {
    use super::*;
    #[test]
    fn cropped_single_image_can_repeat_and_shrink_with_linked_outlines() {
        let mut layout=Layout{count:1,width:40.,height:50.,image_tiles:vec![[10,10,80,100]],use_manual_cut_paths:true,..Default::default()};
        layout.custom.insert(0,Placement{x:15.,y:15.,width:40.,height:50.,orientation:Default::default()});
        layout.link_cut_paths(0,vec![vec![[20.,20.],[50.,20.],[50.,60.],[20.,60.],[20.,20.]]]).unwrap();
        assert!(layout.can_repeat_tiled_image());
        let repeat=layout.with_copy_count(12).unwrap();
        assert_eq!(repeat.image_tiles,vec![[10,10,80,100];12]);
        assert_eq!(repeat.items().unwrap().len(),12);
        assert_eq!(repeat.manual_page_cut_paths().unwrap().len(),12);
        let shrink=repeat.with_copy_count(2).unwrap();
        assert_eq!(shrink.count,2);
        assert_eq!(shrink.image_tiles.len(),2);
        assert_eq!(shrink.image_cut_paths.len(),2);
        assert_eq!(shrink.manual_page_cut_paths().unwrap().len(),2);
        assert!(shrink.with_copy_count(1000).is_err());
    }
}

#[cfg(test)]
mod overflow_recovery_tests {
    use super::*;
    #[test]
    fn overflowing_six_copies_stay_visible_at_original_size() {
        let mut layout=Layout{count:6,width:80.,height:130.,..Default::default()};
        assert!(layout.items().is_err());
        let items=layout.editor_items().unwrap();
        assert_eq!(items.len(),6);
        assert!(items.iter().all(|p|p.width==80. && p.height==130.));
        assert!(items.iter().any(|p|p.y+p.height>layout.page_height));
        layout.custom=items.into_iter().enumerate().collect();
        layout.custom.get_mut(&0).unwrap().x=-20.;
        assert_eq!(layout.editor_items().unwrap()[0].x,-20.);
        assert!(layout.items().is_err());
        layout.custom.get_mut(&0).unwrap().width=f64::NAN;
        assert!(layout.editor_items().is_err());
    }
}

#[cfg(test)]
mod design_print_tests {
    use super::*;
    fn shape(print:bool,cut:bool)->DesignShape {DesignShape{path:vec![[20.,20.],[60.,20.],[60.,60.],[20.,60.],[20.,20.]],fill:Some([255,0,0,255]),stroke:[0,0,0,255],stroke_width:0.4,print,cut}}
    #[test]
    fn design_fill_stroke_and_cut_flags_are_separate_and_persisted() {
        let mut layout=Layout{count:1,..Default::default()};
        layout.design_shapes.push(shape(true,false));
        let pixels=layout.render_designs(210,297).unwrap();
        assert_eq!(pixels.get_pixel(30,30).0,[255,0,0,255]);
        assert_eq!(pixels.get_pixel(10,10).0,[0,0,0,0]);
        assert!(layout.manual_design_plt().is_err());
        layout.design_shapes.push(shape(false,true));
        assert_eq!(layout.manual_design_plt().unwrap().matches("PD").count(),4);
        let restored:Layout=serde_json::from_slice(&serde_json::to_vec(&layout).unwrap()).unwrap();
        assert!(restored.design_shapes==layout.design_shapes);
        let raster=layout.print_design_raster().unwrap();
        let p=(30_f64/25.4*300.).round() as u32;
        assert_eq!(raster.get_pixel(p,p).0,[255,0,0,255]);
        layout.design_shapes[0].print=false;
        assert!(!layout.design_svg_paths().contains("<path"));
    }
}

#[cfg(test)]
mod design_export_tests {
    use super::*;
    #[test]
    fn standalone_print_cut_svg_ccd_and_raster_share_shape_geometry() {
        let mut layout=Layout::default();
        layout.design_shapes.push(DesignShape{path:vec![[30.,30.],[70.,30.],[70.,70.],[30.,70.],[30.,30.]],fill:Some([0,150,0,255]),stroke:[0,0,0,255],stroke_width:0.4,print:true,cut:true});
        let (standalone,art)=layout.design_only_artwork().unwrap();
        let html=standalone.print_html(&art).unwrap();
        assert!(html.contains("rgb(0,150,0)"));
        assert_eq!(standalone.page_cut_paths(&art).unwrap()[0],layout.design_shapes[0].path);
        let ccd=CcdTrial::default();
        let (ccd_html,plt)=ccd.pair(&standalone,&art).unwrap();
        assert!(ccd_html.contains("rgb(0,150,0)"));
        assert!(plt.contains("CT1"));
        let raster=layout.print_design_raster_with_ccd(Some(&ccd)).unwrap();
        let p=(50_f64/25.4*300.).round() as u32;
        assert_eq!(raster.get_pixel(p,p).0,[0,150,0,255]);
        let restored:Layout=serde_json::from_slice(&serde_json::to_vec(&layout).unwrap()).unwrap();
        assert_eq!(restored.design_svg_paths(),layout.design_svg_paths());
    }
}

#[cfg(test)]
mod design_region_tests {
    use super::*;
    #[test]
    fn zoom_region_renders_original_geometry_at_requested_resolution() {
        let mut layout=Layout::default();
        layout.design_shapes.push(DesignShape {path:vec![[20.,20.],[40.,20.],[40.,40.],[20.,40.],[20.,20.]],fill:Some([0,103,192,255]),stroke:[0,0,0,255],stroke_width:0.3,print:true,cut:false});
        let pixels=layout.render_design_region(800,800,[10.,10.,40.,40.]).unwrap();
        assert_eq!(pixels.get_pixel(400,400).0,[0,103,192,255]);
        assert_eq!(pixels.get_pixel(50,50).0,[0,0,0,0]);
        assert_eq!(pixels.get_pixel(700,400).0,[0,0,0,0]);
        assert!(layout.render_design_region(50,50,[0.,0.,0.,10.]).is_err());
        assert!(layout.render_design_region(50,50,[f64::NAN,0.,10.,10.]).is_err());
    }
}

#[cfg(test)]
mod sharp_polygon_tests {
    use super::*;
    #[test]
    fn polygon_cleanup_removes_pixel_tip_kinks_but_keeps_curves() {
        let triangle=vec![[0.,40.],[59.96,0.04],[60.,0.05],[60.04,0.1],[60.,40.],[0.,40.]];
        let clean=straighten_polygon(&triangle,0.1);
        assert_eq!(clean.len(),4);
        assert_eq!(clean.first(),clean.last());
        let mut circle:Vec<_>=(0..180).map(|i| {let a=i as f64*std::f64::consts::TAU/180.;[30.+20.*a.cos(),30.+20.*a.sin()]}).collect();
        circle.push(circle[0]);
        assert_eq!(straighten_polygon(&circle,0.1),circle);
    }
    #[test]
    fn sharp_corner_offset_works_when_smoothing_is_off() {
        let art=Artwork {pixels:RgbaImage::new(400,400),outlines:vec![vec![[0.1,0.1],[0.9,0.1],[0.9,0.9],[0.1,0.9],[0.1,0.1]]],holes:vec![],tiles:vec![],ellipse_bounds:None};
        let layout=Layout {width:40.,height:40.,count:1,contour_offset:1.,smoothing:0.,preserve_corners:true,..Default::default()};
        let sharp=layout.outline_mm(&art).unwrap();
        assert_eq!(sharp[0].len(),5);
        assert!(sharp[0].iter().all(|p| (p[0]-3.).abs()<1e-6 || (p[0]-37.).abs()<1e-6));
        let round=Layout {preserve_corners:false,..layout}.outline_mm(&art).unwrap();
        assert!(round[0].len()>sharp[0].len());
    }
}

#[cfg(test)]
mod unlink_tests {
    use super::*;
    #[test]
    fn unlink_preserves_position_rotation_offset_and_other_copy_links() {
        let mut layout=Layout {count:2,width:40.,height:40.,smoothing:0.,contour_offset:0.5,use_manual_cut_paths:true,..Default::default()};
        layout.custom.insert(0,Placement {x:20.,y:20.,width:40.,height:40.,orientation:Default::default()}.with_rotation(90.));
        layout.custom.insert(1,Placement {x:90.,y:20.,width:40.,height:40.,orientation:Default::default()});
        layout.link_cut_paths(0,vec![vec![[25.,25.],[55.,25.],[55.,55.],[25.,55.],[25.,25.]]]).unwrap();
        let before=layout.manual_page_cut_paths().unwrap();
        layout.unlink_image_cut_paths(0).unwrap();
        let after=layout.manual_page_cut_paths().unwrap();
        assert_eq!(before,after);
        assert!(layout.image_cut_paths_for(0).unwrap().is_empty());
        assert!(!layout.image_cut_paths_for(1).unwrap().is_empty());
        // New repeated placements inherit a remaining linked template, not a tombstone.
        layout.count=3;
        assert!(!layout.image_cut_paths_for(2).unwrap().is_empty());
        let json=serde_json::to_string(&layout).unwrap();
        let restored:Layout=serde_json::from_str(&json).unwrap();
        assert!(restored.image_cut_paths_for(0).unwrap().is_empty());
    }
}

#[cfg(test)]
mod smoothing_range_tests {
    use super::*;
    #[test]
    fn stronger_smoothing_is_supported_for_auto_and_manual_paths_and_survives_save() {
        let art=Artwork {pixels:RgbaImage::new(500,500),outlines:vec![vec![[0.2,0.2],[0.8,0.2],[0.8,0.8],[0.2,0.8],[0.2,0.2]]],holes:vec![],tiles:vec![],ellipse_bounds:None};
        let layout=Layout {width:100.,height:100.,count:1,preserve_corners:false,smoothing:MAX_SMOOTHING_MM,..Default::default()};
        let high=layout.outline_mm(&art).unwrap();
        let normal=Layout {smoothing:4.,..layout.clone()}.outline_mm(&art).unwrap();
        assert_ne!(high,normal);
        assert_eq!(high[0].first(),high[0].last());
        assert!(high.iter().flatten().flatten().all(|v|v.is_finite()));
        let manual=Layout {manual_cut_paths:vec![vec![[20.,20.],[80.,20.],[80.,80.],[20.,80.],[20.,20.]]],use_manual_cut_paths:true,..layout.clone()};
        assert!(manual.manual_page_cut_paths().is_ok());
        let json=serde_json::to_string(&manual).unwrap();
        let restored:Layout=serde_json::from_str(&json).unwrap();
        assert_eq!(restored.smoothing,10.);
        assert!(restored.manual_page_cut_paths().is_ok());
        assert!(Layout {smoothing:10.1,..layout}.outline_mm(&art).is_err());
    }
}

#[cfg(test)]
mod live_smoothing_profile {
    use super::*;
    #[test]
    #[ignore = "manual smoothing diagnostic using MC630_PROFILE_RECOVERY"]
    fn inspect_open_document_smoothing() {
        let path=std::env::var("MC630_PROFILE_RECOVERY").unwrap();
        let record:serde_json::Value=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let settings=&record["project"]["settings"];
        let mut layout:Layout=serde_json::from_value(settings["layout"].clone()).unwrap();
        let source=base64::engine::general_purpose::STANDARD.decode(record["project"]["image_base64"].as_str().unwrap()).unwrap();
        let art=layout.trace_images(&source,settings["remove_white"].as_bool().unwrap(),settings["threshold"].as_u64().unwrap() as u8,serde_json::from_value(settings["cut_shape"].clone()).unwrap(),settings["background_tolerance"].as_u64().unwrap() as u8,settings["print_without_background"].as_bool().unwrap()).unwrap();
        let item=layout.editor_items().unwrap()[0];layout.width=item.width;layout.height=item.height;
        let before=layout.outline_mm(&art).unwrap();
        let after:Vec<_>=before.iter().map(|p|smooth_mixed_contour(p,layout.smoothing)).collect();
        let mut svg=format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900" viewBox="0 0 {} {}"><rect width="100%" height="100%" fill="white"/>"#,layout.width*2.+10.,layout.height);
        for (paths,x) in [(&before,0.),(&after,layout.width+10.)] {
            svg.push_str(&format!(r#"<g transform="translate({} 0)" fill="none" stroke="red" stroke-width="0.08">"#,x));
            for path in paths {svg.push_str(&format!(r#"<polyline points="{}"/>"#,path.iter().map(|p|format!("{},{}",p[0],p[1])).collect::<Vec<_>>().join(" ")));}
            svg.push_str("</g>");
        }
        svg.push_str("</svg>");
        let tree=usvg::Tree::from_str(&svg,&usvg::Options::default()).unwrap();
        let mut pixels=resvg::tiny_skia::Pixmap::new(1600,900).unwrap();resvg::render(&tree,resvg::tiny_skia::Transform::identity(),&mut pixels.as_mut());pixels.save_png("target/smoothing-comparison.png").unwrap();
        println!("paths={} before_points={} after_points={}",before.len(),before.iter().map(Vec::len).sum::<usize>(),after.iter().map(Vec::len).sum::<usize>());
    }
}

#[cfg(test)]
mod mixed_edge_finish_tests {
    use super::*;
    #[test]
    fn noisy_straight_edges_are_not_smoothed_into_bowed_curves() {
        let mut path:Vec<_>=(0..=400).map(|i|[20.+i as f64*0.1,20.+0.02*(i as f64*std::f64::consts::TAU/40.).sin()]).collect();
        path.extend([[60.,60.],[20.,60.],[20.,20.]]);
        let smoothed=smooth_mixed_contour(&path,1.);
        assert_eq!(smoothed.len(),5);
        assert_eq!(smoothed.first(),smoothed.last());
        for point in smoothed {assert!((point[0]-20.).abs()<1e-6 || (point[0]-60.).abs()<1e-6);}
    }
}

#[cfg(test)]
mod line_fit_precision_tests {
    use super::*;
    #[test]
    fn fitted_rotated_edges_recover_corners_without_axis_snapping() {
        let angle=0.31_f64;let rotate=|p:Point|[100.+p[0]*angle.cos()-p[1]*angle.sin(),100.+p[0]*angle.sin()+p[1]*angle.cos()];
        let exact=[[0.,0.],[60.,0.],[60.,40.],[0.,40.]].map(rotate);
        let mut original=Vec::new();
        for edge in 0..4 {
            let a=exact[edge];let b=exact[(edge+1)%4];let dx=b[0]-a[0];let dy=b[1]-a[1];let length=dx.hypot(dy);
            for i in 0..100 {
                let t=i as f64/100.;let noise=0.02*(i as f64*std::f64::consts::TAU/10.).sin();
                original.push([a[0]+t*dx-noise*dy/length,a[1]+t*dy+noise*dx/length]);
            }
        }
        original.push(original[0]);
        let mut coarse:Vec<_>=exact.iter().map(|p|[p[0]+0.08,p[1]-0.06]).collect();coarse.push(coarse[0]);
        let refined=refine_polygon_lines(&original,&coarse,0.15);
        assert_eq!(refined.len(),5);assert_eq!(refined.first(),refined.last());
        for (got,expected) in refined.iter().zip(exact.iter()) {assert!((got[0]-expected[0]).hypot(got[1]-expected[1])<0.01);}
        let edge=[refined[1][0]-refined[0][0],refined[1][1]-refined[0][1]];
        assert!((edge[1].atan2(edge[0])-angle).abs()<0.001);
    }
}

impl Layout {
    pub fn svg_source_for(&self,index:usize)->Option<&crate::svg_artwork::SvgArtwork> {
        self.svg_sources.get(&index).or_else(||self.svg_sources.iter().find_map(|(owner,asset)| {
            let same=if self.image_tiles.is_empty() {true}else{self.image_tiles.get(index).zip(self.image_tiles.get(*owner)).is_some_and(|(a,b)|a==b)};
            same.then_some(asset)
        }))
    }
    pub fn opacity_for(&self,index:usize)->u8 { self.image_opacity.get(&index).copied().unwrap_or(255) }
    pub fn apply_opacity(&self,index:usize,raster:&mut RgbaImage) {
        let opacity=self.opacity_for(index) as u16;
        if opacity<255 {for pixel in raster.pixels_mut() {pixel[3]=((pixel[3] as u16*opacity+127)/255) as u8;}}
    }
    pub fn svg_artwork_markup(&self)->Result<String,String> {
        if self.svg_sources.is_empty() {return Ok(String::new());}
        let mut markup=String::new();
        for (index,item) in self.editor_items()?.into_iter().enumerate() {
            if self.hidden_images.contains(&index) {continue;}
            if let Some(asset)=self.svg_source_for(index) {markup.push_str(&format!("<g opacity=\"{}\">",self.opacity_for(index) as f64/255.));markup.push_str(&asset.markup(item,self.image_tiles.get(index).copied().unwrap_or(asset.raster_rect),index)?);markup.push_str("</g>");}
        }
        Ok(markup)
    }
}
