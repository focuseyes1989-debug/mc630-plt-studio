use crate::preferences::Preferences;
use base64::Engine as _;
use std::{io::Write, path::Path};
const FORMAT: &str = "MC630 Print and Cut Project";
const VERSION: u32 = 1;
const MAX_FILE: u64 = 64_000_000;
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Project {
    format: String,
    version: u32,
    pub artwork_name: String,
    pub image_base64: String,
    pub settings: Preferences,
    pub canvas_pan: [f32; 2],
    pub aspect_ratio: f64,
}
impl Project {
    pub fn new(
        artwork_name: String,
        source: &[u8],
        settings: Preferences,
        canvas_pan: [f32; 2],
    ) -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            artwork_name,
            image_base64: base64::engine::general_purpose::STANDARD.encode(source),
            settings,
            canvas_pan,
            aspect_ratio: 1.,
        }
    }
    pub fn source(&self) -> Result<Vec<u8>, String> {
        self.decode_source(false)
    }
    pub fn recovery_source(&self) -> Result<Vec<u8>, String> {
        self.decode_source(true)
    }
    fn decode_source(&self, recovery: bool) -> Result<Vec<u8>, String> {
        if self.format != FORMAT || self.version != VERSION {
            return Err("Unsupported project format or version.".into());
        }
        if recovery {
            self.settings.validate_values()?;
        } else {
            self.settings.validate()?;
        }
        if !self.aspect_ratio.is_finite() || self.aspect_ratio <= 0. {
            return Err("Invalid project aspect ratio.".into());
        }
        if self
            .settings
            .layout
            .custom
            .keys()
            .any(|i| *i >= self.settings.layout.count)
            || self.canvas_pan.iter().any(|v| !v.is_finite())
            || self.image_base64.len() > 40_000_004
        {
            return Err("Invalid or oversized project data.".into());
        }
        if self.settings.layout.custom.values().any(|p| {
            [p.x, p.y, p.width, p.height].iter().any(|v| !v.is_finite())
                || p.width < 1.
                || p.height < 1.
        }) {
            return Err("Invalid recovery image placement.".into());
        }
        let source = base64::engine::general_purpose::STANDARD
            .decode(&self.image_base64)
            .map_err(|_| "Project image data is damaged.".to_string())?;
        if source.len() > 30_000_000 {
            return Err("Project image exceeds 30 MB.".into());
        }
        if !source.is_empty() && recovery {
            image::load_from_memory(&source).map_err(|e| format!("Damaged recovery image: {e}"))?;
        }
        if (!source.is_empty() && !recovery) || !self.settings.layout.image_tiles.is_empty() {
            self.settings.layout.trace_images(
                &source,
                self.settings.remove_white,
                self.settings.threshold,
                self.settings.cut_shape,
                self.settings.background_tolerance,
                self.settings.print_without_background,
            )?;
        }
        Ok(source)
    }
    pub fn read(path: &Path) -> Result<Self, String> {
        let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        use std::io::Read as _;
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_FILE {
            return Err("Project exceeds the 64 MB limit.".into());
        }
        serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read project: {e}"))
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.source()?;
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_FILE {
            return Err("Project exceeds the 64 MB limit.".into());
        }
        write_atomic(path, &bytes)
    }
}
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temp = parent.join(format!(".mc630-save-{}-{stamp}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}
