use base64::Engine as _;
use eframe::egui;
use image::{RgbaImage, imageops};
pub type PreparedPreviews = Vec<(u64, std::sync::Arc<egui::ColorImage>)>;

// Immutable pixels are shared by duplicates, drag snapshots and recovery jobs.
#[derive(Clone)]
pub struct ImageSource {
    encoded: std::sync::Arc<str>,
    digest: u64,
}
impl From<String> for ImageSource {
    fn from(value: String) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut hash);
        Self {
            encoded: value.into(),
            digest: hash.finish(),
        }
    }
}
impl From<&str> for ImageSource {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}
impl std::ops::Deref for ImageSource {
    type Target = str;
    fn deref(&self) -> &str {
        &self.encoded
    }
}
impl AsRef<[u8]> for ImageSource {
    fn as_ref(&self) -> &[u8] {
        self.encoded.as_bytes()
    }
}
impl serde::Serialize for ImageSource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.encoded)
    }
}
impl<'de> serde::Deserialize<'de> for ImageSource {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(<String as serde::Deserialize>::deserialize(deserializer)?.into())
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Flip {
    #[default]
    LeftRight,
    TopBottom,
}
impl Flip {
    pub fn point(self, p: [f64; 2], page: [f64; 2]) -> [f64; 2] {
        match self {
            Self::LeftRight => [page[0] - p[0], p[1]],
            Self::TopBottom => [p[0], page[1] - p[1]],
        }
    }
}
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BackDocument {
    pub flip: Flip,
    pub images: Vec<BackImage>,
    pub offset: [f64; 2],
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct BackImage {
    pub name: String,
    pub image_base64: ImageSource,
    pub rect: [f64; 4],
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub flip_x: bool,
    #[serde(default)]
    pub flip_y: bool,
}
impl BackImage {
    fn point(&self, uv: [f64; 2]) -> [f64; 2] {
        let [x, y, w, h] = self.rect;
        let (sin, cos) = self.angle.rem_euclid(360.).to_radians().sin_cos();
        let dx = (uv[0] - 0.5) * w;
        let dy = (uv[1] - 0.5) * h;
        [
            x + w / 2. + cos * dx - sin * dy,
            y + h / 2. + sin * dx + cos * dy,
        ]
    }
    fn uv(&self, p: [f64; 2]) -> [f64; 2] {
        let [x, y, w, h] = self.rect;
        let (sin, cos) = self.angle.rem_euclid(360.).to_radians().sin_cos();
        let dx = p[0] - x - w / 2.;
        let dy = p[1] - y - h / 2.;
        [
            0.5 + (cos * dx + sin * dy) / w,
            0.5 + (-sin * dx + cos * dy) / h,
        ]
    }
    fn corners(&self) -> [[f64; 2]; 4] {
        [[0., 0.], [1., 0.], [1., 1.], [0., 1.]].map(|uv| self.point(uv))
    }
    fn bounds(&self) -> [f64; 4] {
        let corners = self.corners();
        let left = corners.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        let top = corners.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        let right = corners
            .iter()
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max);
        let bottom = corners
            .iter()
            .map(|p| p[1])
            .fold(f64::NEG_INFINITY, f64::max);
        [left, top, right, bottom]
    }
    fn resized(&self, corner: usize, delta: [f64; 2], linked: bool) -> Self {
        let (sin, cos) = self.angle.rem_euclid(360.).to_radians().sin_cos();
        let local = [
            cos * delta[0] + sin * delta[1],
            -sin * delta[0] + cos * delta[1],
        ];
        let uv = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]][corner];
        let sign = [uv[0] * 2. - 1., uv[1] * 2. - 1.];
        let mut size = [
            (self.rect[2] + sign[0] * local[0]).max(0.1),
            (self.rect[3] + sign[1] * local[1]).max(0.1),
        ];
        if linked {
            let axis =
                usize::from((local[1] / self.rect[3]).abs() > (local[0] / self.rect[2]).abs());
            let factor = (size[axis] / self.rect[axis + 2])
                .max(0.1 / self.rect[2])
                .max(0.1 / self.rect[3]);
            size = [self.rect[2] * factor, self.rect[3] * factor];
        }
        let opposite = self.point([1. - uv[0], 1. - uv[1]]);
        let dx = sign[0] * size[0] / 2.;
        let dy = sign[1] * size[1] / 2.;
        let mut result = self.clone();
        result.rect = [
            opposite[0] + cos * dx - sin * dy - size[0] / 2.,
            opposite[1] + sin * dx + cos * dy - size[1] / 2.,
            size[0],
            size[1],
        ];
        result
    }
    fn constrain_position(&mut self, page: [f64; 2]) {
        let [l, t, r, b] = self.bounds();
        if r - l <= page[0] {
            self.rect[0] += (-l).max(0.) - (r - page[0]).max(0.);
        }
        if b - t <= page[1] {
            self.rect[1] += (-t).max(0.) - (b - page[1]).max(0.);
        }
    }

    fn decode(&self) -> Result<RgbaImage, String> {
        if self.image_base64.len() > 40_000_004 {
            return Err("Back image exceeds 30 MB.".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.image_base64)
            .map_err(|e| e.to_string())?;
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(128_000_000);
        reader.limits(limits);
        reader
            .decode()
            .map(|i| i.to_rgba8())
            .map_err(|e| e.to_string())
    }
}
impl BackDocument {
    pub fn hash_signature(&self, hash: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        (self.flip == Flip::TopBottom).hash(hash);
        for value in self.offset {
            value.to_bits().hash(hash);
        }
        self.images.len().hash(hash);
        for image in &self.images {
            image.name.hash(hash);
            image.image_base64.digest.hash(hash);
            for value in image.rect {
                value.to_bits().hash(hash);
            }
            image.angle.to_bits().hash(hash);
            image.flip_x.hash(hash);
            image.flip_y.hash(hash);
        }
    }

    pub fn validate_geometry(&self, page: [f64; 2]) -> Result<(), String> {
        if self.images.len() > 100 || self.offset.iter().any(|x| !x.is_finite() || x.abs() > 100.) {
            return Err("Invalid back-side settings.".into());
        }
        for image in &self.images {
            let [_, _, w, h] = image.rect;
            let [x, y, right, bottom] = image.bounds();
            if !image.angle.is_finite()
                || image.rect.iter().any(|v| !v.is_finite())
                || w < 0.1
                || h < 0.1
                || x < -0.001
                || y < -0.001
                || right > page[0] + 0.001
                || bottom > page[1] + 0.001
            {
                return Err("Back image is outside the page. Adjust its position or size.".into());
            }
        }
        Ok(())
    }
    pub fn validate(&self, page: [f64; 2]) -> Result<(), String> {
        self.validate_geometry(page)?;
        for image in &self.images {
            image.decode()?;
        }
        Ok(())
    }
    pub fn prepare_previews(&self) -> Result<PreparedPreviews, String> {
        let mut cache = std::collections::HashMap::new();
        let mut result = Vec::new();
        for item in &self.images {
            let digest = item.image_base64.digest;
            if !cache.contains_key(&digest) {
                let pixels = image::DynamicImage::ImageRgba8(item.decode()?)
                    .thumbnail(2048, 2048)
                    .to_rgba8();
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [pixels.width() as usize, pixels.height() as usize],
                    pixels.as_raw(),
                );
                cache.insert(digest, std::sync::Arc::new(image));
            }
            result.push((digest, cache[&digest].clone()));
        }
        Ok(result)
    }
    pub fn raster(&self, page: [f64; 2]) -> Result<RgbaImage, String> {
        self.validate(page)?;
        if self.images.is_empty() {
            return Err("Import a back-side image first.".into());
        }
        let mm = |v: f64| (v / 25.4 * 300.).round() as u32;
        let [w, h] = [mm(page[0]), mm(page[1])];
        if u64::from(w) * u64::from(h) > 80_000_000 {
            return Err("Page is too large for 300 dpi printing.".into());
        }
        let mut result = RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
        for item in &self.images {
            let pixels = item.decode()?;
            // Keep the existing high-quality resize for unrotated artwork.
            if item.angle.rem_euclid(360.).abs() < 1e-8 {
                let [x, y, w, h] = item.rect;
                let mut pixels = imageops::resize(
                    &pixels,
                    mm(w).max(1),
                    mm(h).max(1),
                    imageops::FilterType::Lanczos3,
                );
                if item.flip_x {
                    imageops::flip_horizontal_in_place(&mut pixels);
                }
                if item.flip_y {
                    imageops::flip_vertical_in_place(&mut pixels);
                }
                imageops::overlay(
                    &mut result,
                    &pixels,
                    ((x + self.offset[0]) / 25.4 * 300.).round() as i64,
                    ((y + self.offset[1]) / 25.4 * 300.).round() as i64,
                );
            } else {
                let [l, t, r, b] = item.bounds();
                let scale = 300. / 25.4;
                let x0 = ((l + self.offset[0]) * scale).floor().max(0.) as u32;
                let y0 = ((t + self.offset[1]) * scale).floor().max(0.) as u32;
                let x1 = ((r + self.offset[0]) * scale).ceil().min(f64::from(w)) as u32;
                let y1 = ((b + self.offset[1]) * scale).ceil().min(f64::from(h)) as u32;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let mut uv = item.uv([
                            (f64::from(x) + 0.5) / scale - self.offset[0],
                            (f64::from(y) + 0.5) / scale - self.offset[1],
                        ]);
                        if uv.iter().any(|v| !(0. ..=1.).contains(v)) {
                            continue;
                        }
                        if item.flip_x {
                            uv[0] = 1. - uv[0];
                        }
                        if item.flip_y {
                            uv[1] = 1. - uv[1];
                        }
                        let sx = (uv[0] * f64::from(pixels.width()) - 0.5)
                            .clamp(0., f64::from(pixels.width() - 1));
                        let sy = (uv[1] * f64::from(pixels.height()) - 0.5)
                            .clamp(0., f64::from(pixels.height() - 1));
                        let (ix, iy) = (sx.floor() as u32, sy.floor() as u32);
                        let mut rgba = [0.; 4];
                        for (px, py, weight) in [
                            (ix, iy, (1. - sx.fract()) * (1. - sy.fract())),
                            (
                                (ix + 1).min(pixels.width() - 1),
                                iy,
                                sx.fract() * (1. - sy.fract()),
                            ),
                            (
                                ix,
                                (iy + 1).min(pixels.height() - 1),
                                (1. - sx.fract()) * sy.fract(),
                            ),
                            (
                                (ix + 1).min(pixels.width() - 1),
                                (iy + 1).min(pixels.height() - 1),
                                sx.fract() * sy.fract(),
                            ),
                        ] {
                            let p = pixels.get_pixel(px, py).0;
                            let alpha = f64::from(p[3]) / 255.;
                            for c in 0..3 {
                                rgba[c] += f64::from(p[c]) * alpha * weight;
                            }
                            rgba[3] += alpha * weight;
                        }
                        let dst = result.get_pixel_mut(x, y);
                        for c in 0..3 {
                            dst.0[c] = (rgba[c] + f64::from(dst.0[c]) * (1. - rgba[3]))
                                .round()
                                .clamp(0., 255.) as u8;
                        }
                    }
                }
            }
        }
        Ok(result)
    }
}
#[derive(Default)]
pub struct BackEditor {
    pub data: BackDocument,
    selected: Option<usize>,
    textures: Vec<egui::TextureHandle>,
    drag_start: Option<(BackImage, Option<usize>)>,
    copied: Option<BackImage>,
    contour: usize,
    keep_ratio: bool,
    pub print_requested: bool,
    pub paste_requested: bool,
    pub message: String,
}
impl BackEditor {
    pub fn copy_selected(&mut self) {
        let Some(item) = self.selected.and_then(|i| self.data.images.get(i)).cloned() else {
            return;
        };
        let result = (|| {
            let pixels = item.decode()?;
            let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
            clipboard
                .set_image(arboard::ImageData {
                    width: pixels.width() as usize,
                    height: pixels.height() as usize,
                    bytes: std::borrow::Cow::Owned(pixels.into_raw()),
                })
                .map_err(|e| e.to_string())
        })();
        match result {
            Ok(()) => {
                self.copied = Some(item);
                self.message = "Image copied.".into();
            }
            Err(e) => self.message = e,
        }
    }
    pub fn paste_copied(&mut self, page: [f64; 2]) -> bool {
        let Some(item) = self.copied.clone() else {
            return false;
        };
        let matches = (|| {
            let pixels = item.decode().ok()?;
            let mut clipboard = arboard::Clipboard::new().ok()?;
            let image = clipboard.get_image().ok()?;
            Some(
                image.width == pixels.width() as usize
                    && image.height == pixels.height() as usize
                    && image.bytes.as_ref() == pixels.as_raw(),
            )
        })()
        .unwrap_or(false);
        if matches {
            self.append_copy(item, page);
        }
        matches
    }
    fn append_copy(&mut self, mut item: BackImage, page: [f64; 2]) {
        if self.data.images.len() >= 100 {
            self.message = "Back image limit exceeded.".into();
            return;
        }
        item.rect[0] += 3.;
        item.rect[1] += 3.;
        item.constrain_position(page);
        self.data.images.push(item);
        self.selected = Some(self.data.images.len() - 1);
    }
    fn duplicate_selected(&mut self, page: [f64; 2]) {
        if let Some(item) = self.selected.and_then(|i| self.data.images.get(i)).cloned() {
            self.append_copy(item, page);
        }
    }
    pub fn interacting(&self) -> bool {
        self.drag_start.is_some()
    }
    pub fn install_previews(&mut self, ctx: &egui::Context, previews: PreparedPreviews) {
        let mut cache = std::collections::HashMap::new();
        self.textures = previews
            .into_iter()
            .map(|(digest, image)| {
                cache
                    .entry(digest)
                    .or_insert_with(|| {
                        ctx.load_texture(
                            format!("back-{digest}"),
                            image.as_ref().clone(),
                            Default::default(),
                        )
                    })
                    .clone()
            })
            .collect();
    }
    pub fn install(&mut self, data: BackDocument) {
        *self = Self {
            data,
            keep_ratio: true,
            ..Default::default()
        };
    }
    fn bounds(&self, paths: &[Vec<[f64; 2]>], page: [f64; 2]) -> [f64; 4] {
        let Some(path) = paths.get(self.contour) else {
            return [
                15.,
                15.,
                (page[0] - 30.).min(100.),
                (page[1] - 30.).min(100.),
            ];
        };
        let mut b = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for p in path {
            let p = self.data.flip.point(*p, page);
            b[0] = b[0].min(p[0]);
            b[1] = b[1].min(p[1]);
            b[2] = b[2].max(p[0]);
            b[3] = b[3].max(p[1]);
        }
        if !b.iter().all(|v| v.is_finite()) {
            return [15., 15., 50., 50.];
        }
        [b[0], b[1], (b[2] - b[0]).max(0.1), (b[3] - b[1]).max(0.1)]
    }
    pub fn add_image_bytes(
        &mut self,
        bytes: Vec<u8>,
        name: String,
        page: [f64; 2],
        paths: &[Vec<[f64; 2]>],
    ) -> Result<(), String> {
        if bytes.len() > 30_000_000 || self.data.images.len() >= 100 {
            return Err("Back image limit exceeded.".into());
        }
        let mut item = BackImage {
            name,
            image_base64: base64::engine::general_purpose::STANDARD
                .encode(bytes)
                .into(),
            rect: self.bounds(paths, page),
            angle: 0.,
            flip_x: false,
            flip_y: false,
        };
        let pixels = item.decode()?;
        let ratio = f64::from(pixels.width()) / f64::from(pixels.height());
        let h = item.rect[2] / ratio;
        if h <= item.rect[3] {
            item.rect[3] = h;
        } else {
            item.rect[2] = item.rect[3] * ratio;
        }
        let candidate = BackDocument {
            images: vec![item.clone()],
            ..Default::default()
        };
        candidate.validate(page)?;
        self.data.images.push(item);
        self.selected = Some(self.data.images.len() - 1);
        self.keep_ratio = true;
        Ok(())
    }
    pub fn import(&mut self, page: [f64; 2], paths: &[Vec<[f64; 2]>]) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Back image", &["png", "jpg", "jpeg"])
            .pick_file()
        {
            let result = std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    self.add_image_bytes(
                        bytes,
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        page,
                        paths,
                    )
                });
            self.message = result.err().unwrap_or_default();
        }
    }
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        page: [f64; 2],
        paths: &[Vec<[f64; 2]>],
        unit: crate::preferences::Unit,
    ) {
        egui::SidePanel::right("back-properties")
            .exact_width(275.)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Back artwork");
                    let old = self.data.flip;
                    ui.horizontal(|ui| {
                        ui.label("Turn paper");
                        egui::ComboBox::from_id_salt("back-flip")
                            .selected_text(if self.data.flip == Flip::LeftRight {
                                "Left / right"
                            } else {
                                "Top / bottom"
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.data.flip,
                                    Flip::LeftRight,
                                    "Left / right",
                                );
                                ui.selectable_value(
                                    &mut self.data.flip,
                                    Flip::TopBottom,
                                    "Top / bottom",
                                );
                            });
                    });
                    if old != self.data.flip {
                        for item in &mut self.data.images {
                            item.rect[0] = page[0] - item.rect[0] - item.rect[2];
                            item.rect[1] = page[1] - item.rect[1] - item.rect[3];
                        }
                    }
                    ui.separator();
                    if ui.button("Import back image…").clicked() {
                        self.import(page, paths);
                    }
                    if ui.button("Paste image · Ctrl+V").clicked() {
                        self.paste_requested = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Copy").clicked() {
                            self.copy_selected();
                        }
                        if ui.button("Duplicate").clicked() {
                            self.duplicate_selected(page);
                        }
                    });
                    let mut remove = false;
                    for (i, image) in self.data.images.iter().enumerate() {
                        if ui
                            .selectable_label(self.selected == Some(i), &image.name)
                            .clicked()
                        {
                            self.selected = Some(i);
                        }
                    }
                    ui.separator();
                    self.contour = self.contour.min(paths.len().saturating_sub(1));
                    if !paths.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label("Cut contour");
                            ui.add(
                                egui::DragValue::new(&mut self.contour).range(0..=paths.len() - 1),
                            );
                        });
                    }
                    let fit = self.bounds(paths, page);
                    if let Some(i) = self.selected.filter(|i| *i < self.data.images.len()) {
                        let item = &mut self.data.images[i];
                        let old = item.rect;
                        egui::Grid::new("back-image-size")
                            .num_columns(2)
                            .show(ui, |ui| {
                                for (axis, label) in
                                    ["X", "Y", "Width", "Height"].iter().enumerate()
                                {
                                    ui.label(*label);
                                    ui.add(unit.distance(&mut item.rect[axis]).speed(0.1).range(
                                        if axis < 2 {
                                            0. ..=page[axis]
                                        } else {
                                            0.1..=page[axis - 2]
                                        },
                                    ));
                                    ui.end_row();
                                }
                            });
                        ui.checkbox(&mut self.keep_ratio, "Link width / height");
                        if self.keep_ratio && old[2] > 0. && old[3] > 0. {
                            if item.rect[2] != old[2] {
                                item.rect[3] = item.rect[2] * old[3] / old[2];
                            } else if item.rect[3] != old[3] {
                                item.rect[2] = item.rect[3] * old[2] / old[3];
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.label("Angle");
                            ui.add(
                                egui::DragValue::new(&mut item.angle)
                                    .suffix("°")
                                    .speed(1.)
                                    .range(-360. ..=360.),
                            );
                        });
                        ui.horizontal(|ui| {
                            if ui.button("↶ 90°").clicked() {
                                item.angle = (item.angle - 90.).rem_euclid(360.);
                            }
                            if ui.button("↷ 90°").clicked() {
                                item.angle = (item.angle + 90.).rem_euclid(360.);
                            }
                            if ui.button("Reset").clicked() {
                                item.angle = 0.;
                                item.flip_x = false;
                                item.flip_y = false;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut item.flip_x, "Flip H");
                            ui.checkbox(&mut item.flip_y, "Flip V");
                        });
                        item.constrain_position(page);
                        if ui.button("Fit to cut contour").clicked() {
                            item.angle = 0.;
                            item.rect = fit;
                        }
                        if ui.button("Delete image").clicked() {
                            remove = true;
                        }
                    }
                    if remove {
                        self.data.images.remove(self.selected.unwrap());
                        self.textures.clear();
                        self.selected = None;
                    }
                    ui.separator();
                    ui.label("Back print correction");
                    ui.horizontal(|ui| {
                        ui.label("X");
                        ui.add(
                            unit.distance(&mut self.data.offset[0])
                                .range(-100. ..=100.)
                                .speed(0.1),
                        );
                        ui.label("Y");
                        ui.add(
                            unit.distance(&mut self.data.offset[1])
                                .range(-100. ..=100.)
                                .speed(0.1),
                        );
                    });
                    if ui.button("Print back…").clicked() {
                        self.print_requested = true;
                    }
                    if ui.button("Export back PNG…").clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("PNG", &["png"])
                            .set_file_name("BACK.png")
                            .save_file()
                        {
                            self.message = match self.data.raster(page).and_then(|r| {
                                r.save_with_format(p, image::ImageFormat::Png)
                                    .map_err(|e| e.to_string())
                            }) {
                                Ok(()) => "Back PNG saved.".into(),
                                Err(e) => e,
                            };
                        }
                    }
                    if !self.message.is_empty() {
                        ui.label(&self.message);
                    }
                });
            });
        while self.textures.len() < self.data.images.len() {
            let i = self.textures.len();
            if let Some(shared) = self.data.images[..i].iter().position(|item| {
                std::sync::Arc::ptr_eq(
                    &item.image_base64.encoded,
                    &self.data.images[i].image_base64.encoded,
                )
            }) {
                self.textures.push(self.textures[shared].clone());
                continue;
            }
            match self.data.images[i].decode() {
                Ok(pixels) => {
                    let pixels = image::DynamicImage::ImageRgba8(pixels)
                        .thumbnail(2048, 2048)
                        .to_rgba8();
                    let size = [pixels.width() as usize, pixels.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_raw());
                    self.textures.push(ctx.load_texture(
                        format!("back-{i}"),
                        color,
                        Default::default(),
                    ));
                }
                Err(e) => {
                    self.message = e;
                    break;
                }
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            let available = ui.available_size() - egui::vec2(40., 40.);
            let scale = (available.x / page[0] as f32)
                .min(available.y / page[1] as f32)
                .max(0.01);
            let (canvas, _) =
                ui.allocate_exact_size(available + egui::vec2(40., 40.), egui::Sense::hover());
            let size = egui::vec2(page[0] as f32 * scale, page[1] as f32 * scale);
            let paper = egui::Rect::from_center_size(canvas.center(), size);
            ui.painter().rect_filled(paper, 0., egui::Color32::WHITE);
            let point =
                |p: [f64; 2]| paper.min + egui::vec2(p[0] as f32 * scale, p[1] as f32 * scale);
            let pointer = ctx.pointer_interact_pos();
            let mut begin = None;
            for (i, item) in self.data.images.iter().enumerate() {
                let corners = item
                    .corners()
                    .map(|p| point([p[0] + self.data.offset[0], p[1] + self.data.offset[1]]));
                if let Some(texture) = self.textures.get(i) {
                    let mut mesh = egui::Mesh::with_texture(texture.id());
                    for (p, mut uv) in
                        corners
                            .into_iter()
                            .zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]])
                    {
                        if item.flip_x {
                            uv[0] = 1. - uv[0];
                        }
                        if item.flip_y {
                            uv[1] = 1. - uv[1];
                        }
                        mesh.vertices.push(egui::epaint::Vertex {
                            pos: p,
                            uv: egui::pos2(uv[0], uv[1]),
                            color: egui::Color32::WHITE,
                        });
                    }
                    mesh.indices = vec![0, 1, 2, 0, 2, 3];
                    ui.painter()
                        .with_clip_rect(paper)
                        .add(egui::Shape::mesh(mesh));
                }
                let rect = egui::Rect::from_points(&corners);
                let over = pointer.is_some_and(|p| {
                    let uv = item.uv([
                        f64::from((p.x - paper.min.x) / scale) - self.data.offset[0],
                        f64::from((p.y - paper.min.y) / scale) - self.data.offset[1],
                    ]);
                    uv.iter().all(|v| (0. ..=1.).contains(v))
                });
                if over || self.selected == Some(i) {
                    let response = ui.interact(
                        rect,
                        ui.id().with(("back-image", i)),
                        egui::Sense::click_and_drag(),
                    );
                    if response.clicked() && over {
                        self.selected = Some(i);
                    }
                    if response.drag_started() && over {
                        self.selected = Some(i);
                        begin = Some((i, None));
                    }
                }
                if self.selected == Some(i) {
                    ui.painter().add(egui::Shape::closed_line(
                        corners.to_vec(),
                        egui::Stroke::new(1_f32, egui::Color32::BLUE),
                    ));
                    for (corner, p) in corners.into_iter().enumerate() {
                        let handle = egui::Rect::from_center_size(p, egui::vec2(8., 8.));
                        ui.painter().rect_filled(handle, 0., egui::Color32::WHITE);
                        ui.painter().rect_stroke(
                            handle,
                            0.,
                            egui::Stroke::new(1_f32, egui::Color32::BLUE),
                            egui::StrokeKind::Inside,
                        );
                        let response = ui
                            .interact(
                                handle.expand(3.),
                                ui.id().with(("back-resize", i, corner)),
                                egui::Sense::drag(),
                            )
                            .on_hover_cursor(egui::CursorIcon::ResizeNwSe);
                        if response.drag_started() {
                            begin = Some((i, Some(corner)));
                        }
                    }
                }
            }
            if let Some((mut index, corner)) = begin {
                if corner.is_none() && ctx.input(|i| i.modifiers.ctrl) {
                    let item = self.data.images[index].clone();
                    if self.data.images.len() < 100 {
                        self.data.images.push(item);
                        index = self.data.images.len() - 1;
                        self.selected = Some(index);
                    }
                }
                self.drag_start = Some((self.data.images[index].clone(), corner));
            }
            // Ghost contours are editor-only. Print raster contains back artwork alone.
            for path in paths {
                let points: Vec<_> = path
                    .iter()
                    .map(|p| point(self.data.flip.point(*p, page)))
                    .collect();
                if points.len() > 1 {
                    ui.painter().add(egui::Shape::line(
                        points,
                        egui::Stroke::new(
                            1_f32,
                            egui::Color32::from_rgba_unmultiplied(210, 50, 50, 120),
                        ),
                    ));
                }
            }
            if ctx.input(|i| i.pointer.primary_down()) {
                if let (Some(i), Some((start, corner))) = (self.selected, self.drag_start.as_ref())
                {
                    if let Some(item) = self.data.images.get_mut(i) {
                        if let Some(d) = ctx.input(|i| {
                            i.pointer
                                .press_origin()
                                .zip(i.pointer.interact_pos())
                                .map(|(a, b)| b - a)
                        }) {
                            let mut delta = [f64::from(d.x / scale), f64::from(d.y / scale)];
                            if let Some(corner) = corner {
                                let resized = start.resized(*corner, delta, self.keep_ratio);
                                let bounds = resized.bounds();
                                if bounds[2] - bounds[0] <= page[0]
                                    && bounds[3] - bounds[1] <= page[1]
                                {
                                    item.rect = resized.rect;
                                }
                            } else {
                                if ctx.input(|i| i.modifiers.shift) {
                                    if delta[0].abs() > delta[1].abs() {
                                        delta[1] = 0.;
                                    } else {
                                        delta[0] = 0.;
                                    }
                                }
                                item.rect[0] = start.rect[0] + delta[0];
                                item.rect[1] = start.rect[1] + delta[1];
                            }
                            item.constrain_position(page);
                        }
                    }
                }
            } else {
                self.drag_start = None;
            }
            if !ctx.wants_keyboard_input() {
                if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)))
                    || ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::C))
                {
                    self.copy_selected();
                }
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::D))
                    || ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::R))
                {
                    self.duplicate_selected(page);
                }
                if let Some(item) = self.selected.and_then(|i| self.data.images.get_mut(i)) {
                    let step = if ctx.input(|i| i.modifiers.shift) {
                        1.
                    } else {
                        0.1
                    };
                    for (key, axis, sign) in [
                        (egui::Key::ArrowLeft, 0, -1.),
                        (egui::Key::ArrowRight, 0, 1.),
                        (egui::Key::ArrowUp, 1, -1.),
                        (egui::Key::ArrowDown, 1, 1.),
                    ] {
                        if ctx.input(|i| i.key_pressed(key)) {
                            item.rect[axis] += step * sign;
                            item.constrain_position(page);
                        }
                    }
                }
            }
            if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                if let Some(i) = self.selected.take() {
                    self.data.images.remove(i);
                    self.textures.clear();
                }
            }
        });
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn image_item() -> BackImage {
        let mut pixels = RgbaImage::from_pixel(20, 10, image::Rgba([255, 0, 0, 255]));
        for y in 0..10 {
            for x in 10..20 {
                pixels.put_pixel(x, y, image::Rgba([0, 0, 255, 255]));
            }
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        pixels
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        BackImage {
            name: "asymmetric artwork".into(),
            image_base64: base64::engine::general_purpose::STANDARD
                .encode(bytes.into_inner())
                .into(),
            rect: [30., 40., 20., 10.],
            angle: 0.,
            flip_x: false,
            flip_y: false,
        }
    }
    #[test]
    fn duplicate_and_resize_share_pixel_storage_and_dirty_checks_track_edits() {
        use std::hash::Hasher;
        let mut item = image_item();
        let clone = item.clone();
        let resized = item.resized(2, [3., 2.], false);
        assert!(std::sync::Arc::ptr_eq(
            &item.image_base64.encoded,
            &clone.image_base64.encoded
        ));
        assert!(std::sync::Arc::ptr_eq(
            &item.image_base64.encoded,
            &resized.image_base64.encoded
        ));
        let signature = |item: BackImage| {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            BackDocument {
                images: vec![item],
                ..Default::default()
            }
            .hash_signature(&mut hash);
            hash.finish()
        };
        let initial = signature(item.clone());
        let loaded: BackImage =
            serde_json::from_value(serde_json::to_value(&item).unwrap()).unwrap();
        assert_eq!(initial, signature(loaded));
        item.rect[0] += 1.;
        assert_ne!(initial, signature(item.clone()));
        item.rect[0] -= 1.;
        assert_eq!(initial, signature(item.clone()));
        item.image_base64 = "different pixels".into();
        assert_ne!(initial, signature(item));
    }
    #[test]
    #[ignore = "manual timing comparison"]
    fn large_image_signature_benchmark() {
        use std::hash::{Hash, Hasher};
        let mut item = image_item();
        item.image_base64 = "A".repeat(8_000_000).into();
        let data = BackDocument {
            images: vec![item],
            ..Default::default()
        };
        let start = std::time::Instant::now();
        for _ in 0..20 {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            std::hint::black_box(serde_json::to_vec(&data).unwrap()).hash(&mut hash);
            std::hint::black_box(hash.finish());
        }
        let old = start.elapsed();
        let start = std::time::Instant::now();
        for _ in 0..20 {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            std::hint::black_box(&data).hash_signature(&mut hash);
            std::hint::black_box(hash.finish());
        }
        eprintln!(
            "8 MB image, 20 change checks: previous {:?}, cached {:?}",
            old,
            start.elapsed()
        );
    }
    #[test]
    fn corner_resize_preserves_rotated_opposite_anchor_and_supports_shrinking() {
        let mut image = image_item();
        image.angle = 30.;
        for corner in 0..4 {
            let uv = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]][corner];
            let opposite = [1. - uv[0], 1. - uv[1]];
            let resized = image.resized(corner, [4., 3.], false);
            for (a, b) in image
                .point(opposite)
                .into_iter()
                .zip(resized.point(opposite))
            {
                assert!((a - b).abs() < 1e-9);
            }
        }
        image.angle = 0.;
        let resized = image.resized(2, [-5., 0.], true);
        assert_eq!(resized.rect, [30., 40., 15., 7.5]);
    }
    #[test]
    fn transform_geometry_and_project_defaults_are_consistent() {
        let mut item = image_item();
        item.angle = 37.;
        item.flip_x = true;
        item.flip_y = true;
        for uv in [[0., 0.], [1., 0.], [1., 1.], [0.3, 0.7]] {
            let actual = item.uv(item.point(uv));
            assert!((actual[0] - uv[0]).abs() < 1e-10);
            assert!((actual[1] - uv[1]).abs() < 1e-10);
        }
        let json = serde_json::to_value(&item).unwrap();
        let restored: BackImage = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(restored.angle, 37.);
        assert!(restored.flip_x && restored.flip_y);
        let mut legacy = json;
        for key in ["angle", "flip_x", "flip_y"] {
            legacy.as_object_mut().unwrap().remove(key);
        }
        let restored: BackImage = serde_json::from_value(legacy).unwrap();
        assert_eq!(restored.angle, 0.);
        assert!(!restored.flip_x && !restored.flip_y);
        let mut editor = BackEditor::default();
        editor.data.images.push(item);
        editor.selected = Some(0);
        editor.duplicate_selected([210., 297.]);
        assert_eq!(editor.data.images.len(), 2);
        assert_eq!(editor.data.images[1].angle, 37.);
        assert!(editor.data.images[1].flip_x && editor.data.images[1].flip_y);
        assert_eq!(editor.data.images[0].rect, [30., 40., 20., 10.]);
    }
    #[test]
    fn print_rotation_and_flips_match_the_preview_transform() {
        let mut item = image_item();
        item.angle = 90.;
        let b = item.bounds();
        for (v, expected) in b.into_iter().zip([35., 35., 45., 55.]) {
            assert!((v - expected).abs() < 1e-8);
        }
        let data = BackDocument {
            images: vec![item.clone()],
            ..Default::default()
        };
        let raster = data.raster([210., 297.]).unwrap();
        let px = |v: f64| (v / 25.4 * 300.).round() as u32;
        assert_eq!(raster.get_pixel(px(40.), px(37.)).0, [255, 0, 0, 255]);
        assert_eq!(raster.get_pixel(px(40.), px(53.)).0, [0, 0, 255, 255]);
        item.flip_x = true;
        let flipped = BackDocument {
            images: vec![item.clone()],
            ..Default::default()
        }
        .raster([210., 297.])
        .unwrap();
        assert_eq!(flipped.get_pixel(px(40.), px(37.)).0, [0, 0, 255, 255]);
        item.angle = 0.;
        let flipped = BackDocument {
            images: vec![item],
            ..Default::default()
        }
        .raster([210., 297.])
        .unwrap();
        assert_eq!(flipped.get_pixel(px(32.), px(45.)).0, [0, 0, 255, 255]);
        assert_eq!(flipped.get_pixel(px(48.), px(45.)).0, [255, 0, 0, 255]);
    }
    #[test]
    fn rotated_bounds_are_checked_before_printing() {
        let mut item = image_item();
        item.rect = [0., 0., 20., 10.];
        item.angle = 45.;
        let mut data = BackDocument {
            images: vec![item],
            ..Default::default()
        };
        assert!(data.validate([210., 297.]).is_err());
        data.images[0].constrain_position([210., 297.]);
        assert!(data.validate([210., 297.]).is_ok());
    }
    #[test]
    fn pasted_pixels_append_preserving_existing_back_artwork_and_transparency() {
        let mut editor = BackEditor::default();
        editor.data.images.push(image_item());
        let original = editor.data.images[0].rect;
        let mut pixels = RgbaImage::from_pixel(20, 10, image::Rgba([0, 0, 0, 0]));
        pixels.put_pixel(10, 5, image::Rgba([10, 20, 30, 255]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        pixels
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let paths = vec![vec![
            [30., 40.],
            [50., 40.],
            [50., 60.],
            [30., 60.],
            [30., 40.],
        ]];
        editor
            .add_image_bytes(
                encoded.into_inner(),
                "Clipboard image".into(),
                [210., 297.],
                &paths,
            )
            .unwrap();
        assert_eq!(editor.data.images.len(), 2);
        assert_eq!(editor.data.images[0].rect, original);
        assert_eq!(editor.selected, Some(1));
        assert_eq!(editor.data.images[1].rect, [160., 40., 20., 10.]);
        assert_eq!(editor.data.images[1].decode().unwrap(), pixels);
        assert!(
            editor
                .add_image_bytes(vec![1, 2, 3], "bad".into(), [210., 297.], &paths)
                .is_err()
        );
        assert_eq!(editor.data.images.len(), 2);
    }
    #[test]
    fn opposite_side_contours_reflect_about_the_sheet_not_artwork_bounds() {
        let page = [210., 297.];
        let p = [30., 40.];
        assert_eq!(Flip::LeftRight.point(p, page), [180., 40.]);
        assert_eq!(Flip::TopBottom.point(p, page), [30., 257.]);
        for flip in [Flip::LeftRight, Flip::TopBottom] {
            assert_eq!(flip.point(flip.point(p, page), page), p);
        }
    }
    #[test]
    fn print_contains_readable_back_artwork_without_ghosts_or_registration_marks() {
        let data = BackDocument {
            images: vec![image_item()],
            ..Default::default()
        };
        let raster = data.raster([210., 297.]).unwrap();
        let px = |mm: f64| (mm / 25.4 * 300.).round() as u32;
        assert_eq!(raster.dimensions(), (2480, 3508));
        assert_eq!(raster.get_pixel(px(32.), px(45.)).0, [255, 0, 0, 255]);
        assert_eq!(raster.get_pixel(px(48.), px(45.)).0, [0, 0, 255, 255]);
        assert_eq!(raster.get_pixel(px(10.), px(10.)).0, [255, 255, 255, 255]);
        // Changing paper turn changes guides, not the readability of back artwork.
        let other = BackDocument {
            flip: Flip::TopBottom,
            ..data.clone()
        }
        .raster([210., 297.])
        .unwrap();
        assert_eq!(raster, other);
        let corrected = BackDocument {
            offset: [2., 3.],
            ..data
        }
        .raster([210., 297.])
        .unwrap();
        assert_eq!(
            corrected.get_pixel(px(31.), px(45.)).0,
            [255, 255, 255, 255]
        );
        assert_eq!(corrected.get_pixel(px(34.), px(48.)).0, [255, 0, 0, 255]);
    }
    #[test]
    fn invalid_back_images_are_rejected_before_save_or_print() {
        let mut data = BackDocument {
            images: vec![image_item()],
            ..Default::default()
        };
        data.images[0].rect[0] = 205.;
        assert!(data.validate([210., 297.]).is_err());
        data.images[0].rect[0] = 30.;
        data.images[0].image_base64 = "damaged".into();
        assert!(data.validate([210., 297.]).is_err());
    }
    #[test]
    fn editor_can_render_empty_and_populated_pages() {
        let ctx = egui::Context::default();
        let mut editor = BackEditor::default();
        let _ = ctx.run(Default::default(), |ctx| {
            editor.ui(ctx, [210., 297.], &[], crate::preferences::Unit::Mm)
        });
        editor.data.images.push(image_item());
        let _ = ctx.run(Default::default(), |ctx| {
            editor.ui(
                ctx,
                [210., 297.],
                &[vec![[30., 40.], [50., 40.], [50., 50.], [30., 40.]]],
                crate::preferences::Unit::Mm,
            )
        });
        assert_eq!(editor.textures.len(), 1);
    }
}
