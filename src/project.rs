use crate::preferences::Preferences;
use base64::Engine as _;
use std::{io::Write, path::Path};
const FORMAT: &str = "MC630 Print and Cut Project";
const VERSION: u32 = 1;
pub const MAX_FILE: u64 = 256_000_000;
pub const MAX_PAGES: usize = 100;
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Project {
    format: String,
    version: u32,
    pub artwork_name: String,
    pub image_base64: String,
    pub settings: Preferences,
    pub canvas_pan: [f32; 2],
    pub aspect_ratio: f64,
    #[serde(default)]
    pub back: crate::duplex::BackDocument,
    /// The root contains the active page; these are the remaining pages in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_pages: Vec<Project>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub active_page: usize,
}
fn is_zero(value: &usize) -> bool { *value == 0 }
impl Project {
    pub fn set_pages(&mut self, pages: Vec<Project>, active: usize) {
        self.version = if pages.is_empty() { VERSION } else { 2 };
        self.other_pages = pages;
        self.active_page = active;
    }
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
            back: Default::default(),
            other_pages: Vec::new(),
            active_page: 0,
        }
    }
    #[cfg(test)]
    pub fn source(&self) -> Result<Vec<u8>, String> {
        self.decode_source(false, true)
    }
    pub fn recovery_source(&self) -> Result<Vec<u8>, String> {
        self.decode_source(true, true)
    }
    pub fn source_untraced(&self)->Result<Vec<u8>,String>{self.decode_source(false,false)}
    fn decode_source(&self, recovery: bool, trace:bool) -> Result<Vec<u8>, String> {
        if self.format != FORMAT || ![VERSION, 2].contains(&self.version) {
            return Err("Unsupported project format or version.".into());
        }
        if self.other_pages.len() >= MAX_PAGES || self.active_page > self.other_pages.len() {
            return Err("Invalid project page count or active page.".into());
        }
        for page in &self.other_pages {
            if !page.other_pages.is_empty() || page.active_page != 0 {
                return Err("Nested project pages are not supported.".into());
            }
            page.decode_source(recovery, trace)?;
        }
        if trace {self.back.validate([self.settings.layout.page_width, self.settings.layout.page_height])?;}else{self.back.validate_geometry([self.settings.layout.page_width,self.settings.layout.page_height])?;}
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
            mc630_plt_studio::sheet::decode_image(&source).map_err(|e| format!("Damaged recovery image: {e}"))?;
        }
        if trace && ((!source.is_empty() && !recovery) || !self.settings.layout.image_tiles.is_empty()) {
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
            return Err("Project exceeds the 256 MB limit.".into());
        }
        serde_json::from_slice(&bytes).map_err(|e| format!("Cannot read project: {e}"))
    }
    #[cfg(test)]
    pub fn save(&self, path: &Path) -> Result<(), String> { self.source()?; self.save_progress(path, |_|{}) }
    pub fn save_progress(&self, path:&Path, progress:impl Fn(u8))->Result<(),String>{
        self.source_untraced()?;
        progress(50);
        // Older applications must reject multi-page files rather than silently
        // opening and then overwriting only their active page.
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_FILE {
            return Err("Project exceeds the 256 MB limit.".into());
        }
        progress(85);
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
