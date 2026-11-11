use std::{
    io::{Cursor, Read},
    path::Path,
    process::Stdio,
};

pub const EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "webp", "gif", "bmp", "tif", "tiff", "ico", "tga", "ppm", "pgm",
    "pbm", "pam", "qoi", "svg",
];

const MAX_BYTES: usize = 30_000_000;

pub struct ImportedImage {
    pub bytes: Vec<u8>,
    pub name: String,
}

pub fn read_file(path: &Path) -> Result<ImportedImage, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    prepare(
        bytes,
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
    )
}

pub fn prepare(mut bytes: Vec<u8>, name: String) -> Result<ImportedImage, String> {
    if bytes.len() > MAX_BYTES {
        return Err("Image exceeds 30 MB.".into());
    }
    let format = image::guess_format(&bytes);
    if format.is_err() {
        let extension = Path::new(&name).extension().and_then(|ext| ext.to_str());
        if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("tga")) {
            let mut reader =
                image::ImageReader::with_format(Cursor::new(&bytes), image::ImageFormat::Tga);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(8000);
            limits.max_image_height = Some(8000);
            limits.max_alloc = Some(128_000_000);
            reader.limits(limits);
            let decoded = reader
                .decode()
                .map_err(|error| format!("Cannot read TGA: {error}"))?;
            let mut png = Cursor::new(Vec::new());
            decoded
                .write_to(&mut png, image::ImageFormat::Png)
                .map_err(|error| error.to_string())?;
            bytes = png.into_inner();
        } else if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
            || std::str::from_utf8(&bytes).ok().is_some_and(|s| {
                s.trim_start_matches('\u{feff}')
                    .trim_start()
                    .starts_with('<')
            })
        {
            bytes = render_svg(&bytes)?;
        } else {
            return Err("Unsupported image format. Use PNG, JPEG, WebP, GIF, BMP, TIFF, ICO, TGA, PNM, QOI or SVG.".into());
        }
        if bytes.len() > MAX_BYTES {
            return Err("Converted image exceeds 30 MB. Use a smaller image.".into());
        }
    } else if !format.is_ok_and(|format| format.reading_enabled()) {
        return Err(
            "This image format is unavailable. Save it as PNG, JPEG or WebP and try again.".into(),
        );
    }
    Ok(ImportedImage { bytes, name })
}

fn render_svg(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let source = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let document = roxmltree::Document::parse(source)
        .map_err(|_| "The response is not a supported image. Copy the image address rather than the page address.".to_string())?;
    if document.root_element().tag_name().name() != "svg" {
        return Err("This link returned a webpage. Copy the image address and try again.".into());
    }
    let mut options = usvg::Options::default();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let resolve_data = usvg::ImageHrefResolver::default_data_resolver();
    options.image_href_resolver.resolve_data = Box::new(move |mime, data, options| {
        let format = image::guess_format(&data).ok()?;
        if !matches!(
            format,
            image::ImageFormat::Png
                | image::ImageFormat::Jpeg
                | image::ImageFormat::Gif
                | image::ImageFormat::WebP
        ) {
            return None;
        }
        let (width, height) = image::ImageReader::with_format(Cursor::new(data.as_slice()), format)
            .into_dimensions()
            .ok()?;
        if width > 4000 || height > 4000 {
            return None;
        }
        resolve_data(mime, data, options)
    });
    options.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(source, &options)
        .map_err(|error| format!("Cannot read SVG: {error}"))?;
    let size = tree.size();
    let scale = (300. / 96_f32).min(4000. / size.width().max(size.height()));
    let width = (size.width() * scale).ceil().max(1.) as u32;
    let height = (size.height() * scale).ceil().max(1.) as u32;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width, height).ok_or("Cannot allocate SVG preview.")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|error| format!("Cannot convert SVG to PNG: {error}"))
}

pub fn web_url(input: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(input.trim()).map_err(|_| "Enter a direct image URL.")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Use an HTTP or HTTPS image URL without login credentials.".into());
    }
    Ok(url)
}

pub fn download(url: url::Url) -> Result<ImportedImage, String> {
    let mut child = crate::update_install::curl()?
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--max-redirs",
            "5",
            "--proto",
            "=http,https",
            "--proto-redir",
            "=http,https",
            "--connect-timeout",
            "8",
            "--max-time",
            "35",
            "--max-filesize",
            "30000000",
            "--user-agent",
            "MC630-PLT-Studio",
            "--globoff",
            "--",
        ])
        .arg(url.as_str())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Cannot start image download: {error}"))?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .unwrap()
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() > MAX_BYTES {
        let _ = child.kill();
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    read.map_err(|error| format!("Cannot read image download: {error}"))?;
    if bytes.len() > MAX_BYTES || output.status.code() == Some(63) {
        return Err("Image exceeds 30 MB.".into());
    }
    if !output.status.success() {
        return Err(format!(
            "Cannot download image: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let name = url
        .path_segments()
        .and_then(|mut parts| parts.next_back())
        .filter(|name| !name.is_empty())
        .unwrap_or("Web image")
        .to_owned();
    prepare(bytes, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webp_and_tga_can_be_prepared_for_the_same_image_pipeline() {
        let pixels = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            3,
            2,
            image::Rgba([17, 93, 201, 128]),
        ));
        for (format, name) in [
            (image::ImageFormat::WebP, "cdn-image"),
            (image::ImageFormat::Tga, "test.tga"),
        ] {
            let mut bytes = Cursor::new(Vec::new());
            pixels.write_to(&mut bytes, format).unwrap();
            let imported = prepare(bytes.into_inner(), name.into()).unwrap();
            let decoded = mc630_plt_studio::sheet::decode_image(&imported.bytes).unwrap();
            assert_eq!(decoded.into_rgba8(), pixels.to_rgba8());
        }
    }

    #[test]
    fn svg_import_rasterizes_with_transparency_and_rejects_webpages() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="8" height="8" fill="red" fill-opacity="0.5"/></svg>"#;
        let image = prepare(svg.to_vec(), "extensionless".into()).unwrap();
        let pixels = mc630_plt_studio::sheet::decode_image(&image.bytes)
            .unwrap()
            .into_rgba8();
        assert_eq!(pixels.dimensions(), (50, 50));
        assert_eq!(pixels.get_pixel(10, 10).0, [255, 0, 0, 128]);
        assert_eq!(pixels.get_pixel(40, 40)[3], 0);
        assert!(prepare(b"<html><body>Page</body></html>".to_vec(), "page".into()).is_err());
    }
}
