pub fn dpi(bytes: &[u8]) -> Option<[f64; 2]> {
    if let Some(mut chunks) = bytes.strip_prefix(b"\x89PNG\r\n\x1a\n") {
        while chunks.len() >= 12 {
            let len = u32::from_be_bytes(chunks[..4].try_into().ok()?) as usize;
            let end = len.checked_add(12)?;
            let chunk = chunks.get(..end)?;
            match &chunk[4..8] {
                b"IDAT" | b"IEND" => break,
                b"pHYs" if len == 9 && chunk[16] == 1 => {
                    let x = u32::from_be_bytes(chunk[8..12].try_into().ok()?);
                    let y = u32::from_be_bytes(chunk[12..16].try_into().ok()?);
                    return density(x, y, 0.0254);
                }
                _ => {}
            }
            chunks = &chunks[end..];
        }
    } else if let Some(mut segments) = bytes.strip_prefix(b"\xff\xd8") {
        while let Some(marker) = segments.strip_prefix(b"\xff") {
            let marker = &marker[marker.iter().take_while(|&&b| b == 0xff).count()..];
            let tag = *marker.first()?;
            if matches!(tag, 0xda | 0xd9) {
                break;
            }
            let len = u16::from_be_bytes(marker.get(1..3)?.try_into().ok()?) as usize;
            if len < 2 {
                break;
            }
            let data = marker.get(3..len + 1)?;
            if tag == 0xe0 && data.starts_with(b"JFIF\0") && data.len() >= 12 {
                let scale = match data[7] {
                    1 => 1.,
                    2 => 2.54,
                    _ => return None,
                };
                let x = u16::from_be_bytes(data[8..10].try_into().ok()?);
                let y = u16::from_be_bytes(data[10..12].try_into().ok()?);
                return density(x.into(), y.into(), scale);
            }
            segments = &marker[len + 1..];
        }
    }
    None
}

fn density(x: u32, y: u32, scale: f64) -> Option<[f64; 2]> {
    (x > 0 && y > 0).then_some([f64::from(x) * scale, f64::from(y) * scale])
}

pub struct ImageGroup {
    pub png: Vec<u8>,
    pub size: [f64; 2],
    pub tiles: Vec<[u32; 4]>,
    pub placements: Vec<mc630_plt_studio::sheet::Placement>,
}

pub fn append_image(
    layout: &mc630_plt_studio::sheet::Layout,
    source: &[u8],
    path: &std::path::Path,
    inset: f64,
) -> Result<(Vec<u8>, mc630_plt_studio::sheet::Layout), String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(30_000_001).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    append_image_bytes(layout, source, &bytes, inset)
}

pub fn append_image_bytes(
    layout: &mc630_plt_studio::sheet::Layout,
    source: &[u8],
    bytes: &[u8],
    inset: f64,
) -> Result<(Vec<u8>, mc630_plt_studio::sheet::Layout), String> {
    use image::{ImageFormat, RgbaImage, imageops};
    use mc630_plt_studio::sheet::Placement;
    use std::io::Cursor;

    let items = layout.items()?;
    if items.len() >= 24 {
        return Err("This sheet already has 24 images.".into());
    }
    if bytes.len() > 30_000_000 {
        return Err("Image exceeds 30 MB.".into());
    }
    let [dx, dy] = dpi(bytes).unwrap_or([300.; 2]);
    let image = decode_rgba(bytes)?;
    let width = (f64::from(image.width()) * 25.4 / dx).max(1.);
    let height = (f64::from(image.height()) * 25.4 / dy).max(1.);

    let [left, right, top, bottom] = layout.margins().map(|margin| margin.max(inset));
    let mut xs = vec![left];
    let mut ys = vec![top];
    for item in &items {
        xs.push((item.x + item.width + layout.column_gap).max(left));
        ys.push((item.y + item.height + layout.row_gap).max(top));
    }
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    let mut placement = None;
    'search: for y in ys {
        for &x in &xs {
            if x + width <= layout.page_width - right + 1e-8
                && y + height <= layout.page_height - bottom + 1e-8
                && items.iter().all(|item| {
                    x + width + layout.column_gap <= item.x + 1e-8
                        || item.x + item.width + layout.column_gap <= x + 1e-8
                        || y + height + layout.row_gap <= item.y + 1e-8
                        || item.y + item.height + layout.row_gap <= y + 1e-8
                })
            {
                placement = Some(Placement {
                    x,
                    y,
                    width,
                    height,
                });
                break 'search;
            }
        }
    }
    let placement = placement.ok_or("No room for the new image at its original size. Free space on the sheet or resize the incoming image.")?;

    let original = decode_rgba(source)?;
    let (w, h) = original.dimensions();
    let (iw, ih) = image.dimensions();
    let choices = [(w, 0, w + iw, h.max(ih)), (0, h, w.max(iw), h + ih)];
    let (x, y, total_w, total_h) = choices
        .into_iter()
        .filter(|&(_, _, w, h)| w <= 8000 && h <= 8000 && u64::from(w) * u64::from(h) <= 32_000_000)
        .min_by_key(|&(_, _, w, h)| u64::from(w) * u64::from(h))
        .ok_or("The combined artwork exceeds the image size limit. Use smaller source images.")?;
    let mut atlas = RgbaImage::new(total_w, total_h);
    imageops::replace(&mut atlas, &original, 0, 0);
    imageops::replace(&mut atlas, &image, x.into(), y.into());
    let mut encoded = Cursor::new(Vec::new());
    atlas
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let encoded = encoded.into_inner();
    if encoded.len() > 30_000_000 {
        return Err("The combined artwork exceeds 30 MB. Use smaller source images.".into());
    }

    let mut next = layout.clone();
    if next.image_tiles.is_empty() {
        next.image_tiles = vec![[0, 0, w, h]; items.len()];
    }
    next.image_tiles.push([x, y, iw, ih]);
    next.custom = items.into_iter().chain([placement]).enumerate().collect();
    next.count = next.image_tiles.len();
    Ok((encoded, next))
}

fn decode_rgba(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8000);
    limits.max_image_height = Some(8000);
    limits.max_alloc = Some(128_000_000);
    reader.limits(limits);
    Ok(reader.decode().map_err(|e| e.to_string())?.into_rgba8())
}

pub fn image_group(
    paths: &[std::path::PathBuf],
    available: [f64; 2],
) -> Result<ImageGroup, String> {
    use image::{ImageFormat, RgbaImage, imageops};
    use std::io::{Cursor, Read};

    const GAP: f64 = 5.;
    const PIXELS_PER_MM: f64 = 300. / 25.4;
    if !(2..=24).contains(&paths.len()) || available.iter().any(|&v| v < 10. || !v.is_finite()) {
        return Err("Select 2 to 24 images and leave room inside the page margins.".into());
    }
    let mut sources = Vec::new();
    let mut decoded_bytes = 0;
    for path in paths {
        let result = (|| -> Result<_, String> {
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .and_then(|file| file.take(30_000_001).read_to_end(&mut bytes))
                .map_err(|e| e.to_string())?;
            if bytes.len() > 30_000_000 {
                return Err("Image exceeds 30 MB.".into());
            }
            let [dx, dy] = dpi(&bytes).unwrap_or([300.; 2]);
            let pixels = decode_rgba(&bytes)?;
            decoded_bytes += pixels.as_raw().len();
            if decoded_bytes > 128_000_000 {
                return Err(
                    "Selected images exceed the group memory limit. Use smaller files.".into(),
                );
            }
            let size = [
                f64::from(pixels.width()) * 25.4 / dx,
                f64::from(pixels.height()) * 25.4 / dy,
            ];
            Ok((pixels, size))
        })();
        sources.push(result.map_err(|e| format!("{}: {e}", path.display()))?);
    }

    let mut best = (0., 0, [0.; 2]);
    for cols in 1..=sources.len() {
        let rows = sources.chunks(cols);
        let gaps_y = GAP * (rows.len() - 1) as f64;
        let mut scale = 1_f64;
        let mut sum_heights = 0.;
        for row in rows {
            let width = row.iter().map(|(_, s)| s[0]).sum::<f64>();
            scale = scale.min((available[0] - GAP * (row.len() - 1) as f64) / width);
            sum_heights += row.iter().map(|(_, s)| s[1]).fold(0., f64::max);
        }
        scale = scale.min((available[1] - gaps_y) / sum_heights);
        if scale > best.0 {
            let width = sources
                .chunks(cols)
                .map(|row| {
                    row.iter().map(|(_, s)| s[0] * scale).sum::<f64>()
                        + GAP * (row.len() - 1) as f64
                })
                .fold(0., f64::max);
            best = (scale, cols, [width, sum_heights * scale + gaps_y]);
        }
    }
    let (scale, cols, size) = best;
    if cols == 0
        || sources
            .iter()
            .any(|(_, s)| s.iter().any(|v| v * scale < 1.))
    {
        return Err("Images cannot fit on one page at a usable size.".into());
    }
    let ppm = PIXELS_PER_MM.min(4000. / size[0].max(size[1]));
    let px = |mm: f64| (mm * ppm).round() as u32;
    let mut canvas = RgbaImage::new(px(size[0]).max(1), px(size[1]).max(1));
    let mut tiles = Vec::new();
    let mut placements = Vec::new();
    let mut y = 0.;
    for row in sources.chunks(cols) {
        let mut x = 0.;
        let mut row_height = 0_f64;
        for (image, [w, h]) in row {
            let (w, h) = (w * scale, h * scale);
            let rect = [
                px(x),
                px(y),
                (px(x + w) - px(x)).max(1),
                (px(y + h) - px(y)).max(1),
            ];
            let image = imageops::resize(image, rect[2], rect[3], imageops::FilterType::Lanczos3);
            imageops::replace(&mut canvas, &image, px(x).into(), px(y).into());
            tiles.push(rect);
            placements.push(mc630_plt_studio::sheet::Placement {
                x,
                y,
                width: w,
                height: h,
            });
            x += w + GAP;
            row_height = row_height.max(h);
        }
        y += row_height + GAP;
    }
    let mut encoded = Cursor::new(Vec::new());
    canvas
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let encoded = encoded.into_inner();
    if encoded.len() > 30_000_000 {
        return Err("The combined image exceeds 30 MB. Use fewer or smaller images.".into());
    }
    Ok(ImageGroup {
        png: encoded,
        size,
        tiles,
        placements,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn imported_group_produces_separate_contours_in_one_ccd_job() {
        use mc630_plt_studio::sheet::{CcdTrial, CutShape, Layout};
        let temp = std::env::temp_dir();
        let stem = format!("mc630-group-test-{}", std::process::id());
        let paths = [
            temp.join(format!("{stem}-a.png")),
            temp.join(format!("{stem}-b.png")),
        ];
        for (i, path) in paths.iter().enumerate() {
            image::RgbaImage::from_pixel(
                300,
                300 + 60 * i as u32,
                image::Rgba([255 * i as u8, 0, 0, 255]),
            )
            .save(path)
            .unwrap();
        }
        let result = super::image_group(&paths, [180., 267.]);
        let original = std::fs::read(&paths[0]).unwrap();
        let mut old = Layout {
            count: 2,
            ..Layout::default()
        };
        let mut old_items = old.items().unwrap();
        old_items[0].x += 4.;
        old_items[0].height += 3.;
        old.custom = old_items.iter().copied().enumerate().collect();
        let (appended, updated) = super::append_image(&old, &original, &paths[1], 15.).unwrap();
        for path in paths {
            std::fs::remove_file(path).unwrap();
        }
        assert_eq!(&updated.items().unwrap()[..2], &old_items);
        assert_eq!(updated.count, 3);
        assert_eq!(updated.image_tiles[0], updated.image_tiles[1]);
        let appended = updated
            .trace_images(&appended, false, 245, CutShape::AlphaOutline, 20, true)
            .unwrap();
        assert_eq!(updated.page_cut_paths(&appended).unwrap().len(), 3);
        let super::ImageGroup {
            png,
            size: [width, height],
            tiles,
            placements,
        } = result.unwrap();
        let mut layout = Layout {
            count: 2,
            width,
            height,
            image_tiles: tiles,
            custom: placements
                .into_iter()
                .enumerate()
                .map(|(i, mut p)| {
                    p.x += 20.;
                    p.y += 20.;
                    (i, p)
                })
                .collect(),
            ..Layout::default()
        };
        let art = layout
            .trace_images(&png, false, 245, CutShape::AlphaOutline, 20, true)
            .unwrap();
        let before = layout.page_cut_paths(&art).unwrap();
        let second = layout.custom.get_mut(&1).unwrap();
        second.y += 25.;
        second.width *= 1.2;
        let after = layout.page_cut_paths(&art).unwrap();
        assert_eq!(before.len(), 2);
        assert_eq!(before[0], after[0]);
        assert_ne!(before[1], after[1]);
        let profile = CcdTrial::default();
        let (html, plt) = profile.pair(&layout, &art).unwrap();
        assert_eq!(html.matches("<circle ").count(), 4);
        assert_eq!(plt.matches("PU").count(), 4);
        let raster = layout.print_raster(&art, Some(&profile)).unwrap();
        let mm = |value: f64| (value * 300. / 25.4).round() as u32;
        let p = layout.custom[&1];
        assert_eq!(
            raster.get_pixel(mm(p.x + 5.), mm(p.y + 5.)).0,
            [255, 0, 0, 255]
        );
    }
}
