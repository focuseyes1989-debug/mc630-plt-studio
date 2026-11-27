//! Windows driver DEVMODE preferences are kept intact for CreateDC/StartDoc.

use crate::ui_theme::{Icon, icon_button};

use image::RgbaImage;

use std::{
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Gdi::*, Printing::*},
    Storage::Xps::*,
    UI::Input::KeyboardAndMouse::GetActiveWindow,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe fn read_wide(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }

    let mut n = 0;

    unsafe {
        while *p.add(n) != 0 {
            n += 1;
        }

        String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
    }
}

fn error(action: &str) -> String {
    format!("{action}: {}", std::io::Error::last_os_error())
}

struct Printer(HANDLE);

impl Drop for Printer {
    fn drop(&mut self) {
        unsafe {
            ClosePrinter(self.0);
        }
    }
}

struct Dc(HDC);

impl Drop for Dc {
    fn drop(&mut self) {
        unsafe {
            DeleteDC(self.0);
        }
    }
}

fn open(name: &str) -> Result<Printer, String> {
    let mut h = null_mut();

    if unsafe { OpenPrinterW(wide(name).as_ptr(), &mut h, null()) } == 0 {
        return Err(error("Open printer"));
    }

    Ok(Printer(h))
}

/// u64 storage keeps both DEVMODE and the driver-private tail aligned.

#[derive(Clone)]

pub struct Preferences {
    pub name: String,

    mode: Vec<u64>,

    bytes: usize,
}

impl Preferences {
    fn ptr(&self) -> *const DEVMODEW {
        self.mode.as_ptr().cast()
    }

    fn mut_ptr(&mut self) -> *mut DEVMODEW {
        self.mode.as_mut_ptr().cast()
    }

    pub fn load(name: &str) -> Result<Self, String> {
        let printer = open(name)?;

        let n = wide(name);

        let bytes = unsafe {
            DocumentPropertiesW(null_mut(), printer.0, n.as_ptr(), null_mut(), null(), 0)
        };

        if bytes < (size_of::<DEVMODEW>() as i32) || bytes > 4_000_000 {
            return Err("Printer returned an invalid preference size.".into());
        }

        let mut result = Self {
            name: name.into(),

            mode: vec![0; (bytes as usize).div_ceil(8)],

            bytes: bytes as usize,
        };

        if unsafe {
            DocumentPropertiesW(
                null_mut(),
                printer.0,
                n.as_ptr(),
                result.mut_ptr(),
                null(),
                DM_OUT_BUFFER,
            )
        } != 1
        {
            return Err(error("Read printing preferences"));
        }

        // Initial app defaults only. Later driver choices are never overwritten.

        unsafe {
            let m = &mut *result.mut_ptr();

            m.dmFields = (m.dmFields & !(DM_PAPERLENGTH | DM_PAPERWIDTH))
                | DM_PAPERSIZE
                | DM_ORIENTATION
                | DM_SCALE;

            m.Anonymous1.Anonymous1.dmPaperSize = DMPAPER_A4 as i16;

            m.Anonymous1.Anonymous1.dmOrientation = DMORIENT_PORTRAIT as i16;

            m.Anonymous1.Anonymous1.dmScale = 100;
        }

        let input = result.clone();

        if unsafe {
            DocumentPropertiesW(
                null_mut(),
                printer.0,
                n.as_ptr(),
                result.mut_ptr(),
                input.ptr(),
                DM_IN_BUFFER | DM_OUT_BUFFER,
            )
        } != 1
        {
            return Err(error("Validate printer defaults"));
        }

        Ok(result)
    }

    pub fn edit(&mut self) -> Result<bool, String> {
        let printer = open(&self.name)?;

        let name = wide(&self.name);

        let input = self.clone();

        let mut output = self.clone();

        let result = unsafe {
            DocumentPropertiesW(
                GetActiveWindow(),
                printer.0,
                name.as_ptr(),
                output.mut_ptr(),
                input.ptr(),
                DM_IN_BUFFER | DM_OUT_BUFFER | DM_IN_PROMPT,
            )
        };

        if result == 1 {
            *self = output;

            self.persist()?;

            Ok(true)
        } else if result == 2 {
            Ok(false)
        } else {
            Err(error("Printing Preferences"))
        }
    }

    pub fn summary(&self) -> String {
        unsafe {
            let m = &*self.ptr();

            let p = m.Anonymous1.Anonymous1;

            format!(
                "Paper ID {} · {} · scale {}% · copies {}",
                p.dmPaperSize,
                if p.dmOrientation == 1 {
                    "Portrait"
                } else {
                    "Landscape"
                },
                p.dmScale,
                p.dmCopies
            )
        }
    }

    fn config_dir() -> Option<std::path::PathBuf> {
        std::env::var_os("LOCALAPPDATA").map(|p| std::path::PathBuf::from(p).join("MC630PLTStudio"))
    }

    pub fn saved_name() -> Option<String> {
        std::fs::read_to_string(Self::config_dir()?.join("printer.txt")).ok()
    }

    fn persist(&self) -> Result<(), String> {
        let Some(dir) = Self::config_dir() else {
            return Ok(());
        };

        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let data =
            unsafe { std::slice::from_raw_parts(self.mode.as_ptr().cast::<u8>(), self.bytes) };

        std::fs::write(dir.join("devmode.bin"), data).map_err(|e| e.to_string())?;

        std::fs::write(dir.join("printer.txt"), &self.name).map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn restore(name: &str) -> Result<Self, String> {
        let mut current = Self::load(name)?;

        if Self::saved_name().as_deref() != Some(name) {
            return Ok(current);
        }

        let Some(dir) = Self::config_dir() else {
            return Ok(current);
        };

        let Ok(data) = std::fs::read(dir.join("devmode.bin")) else {
            return Ok(current);
        };

        if data.len() != current.bytes {
            return Ok(current);
        }

        let mut saved = current.clone();

        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                saved.mode.as_mut_ptr().cast::<u8>(),
                data.len(),
            );

            let s = &*saved.ptr();

            let c = &*current.ptr();

            if s.dmSize as usize + s.dmDriverExtra as usize > data.len()
                || s.dmSize < c.dmSize
                || s.dmDriverVersion != c.dmDriverVersion
                || s.dmDeviceName != c.dmDeviceName
            {
                return Ok(current);
            }
        }

        let printer = open(name)?;

        if unsafe {
            DocumentPropertiesW(
                null_mut(),
                printer.0,
                wide(name).as_ptr(),
                current.mut_ptr(),
                saved.ptr(),
                DM_IN_BUFFER | DM_OUT_BUFFER,
            )
        } != 1
        {
            return Self::load(name);
        }

        Ok(current)
    }

    fn dc(&self) -> Result<Dc, String> {
        let h = unsafe {
            CreateDCW(
                wide("WINSPOOL").as_ptr(),
                wide(&self.name).as_ptr(),
                null(),
                self.ptr(),
            )
        };

        if h.is_null() {
            Err(error("Create printer context"))
        } else {
            Ok(Dc(h))
        }
    }

    pub fn inspect(&self) -> Result<String, String> {
        let dc = self.dc()?;

        let (w, h, _, _) = page_geometry(dc.0)?;

        Ok(format!(
            "{} · physical page {:.1} x {:.1} mm",
            self.summary(),
            w,
            h
        ))
    }

    #[cfg(test)]
    fn print_artwork(
        &self,
        artwork: PrintArtwork,
        page: [f64; 2],
        output: Option<&std::path::Path>,
    ) -> Result<i32, String> {
        self.print_pages(
            vec![PrintPage {
                size: page,
                artwork,
            }],
            output,
        )
    }
    #[cfg(test)]
    fn print_pages(
        &self,
        pages: Vec<PrintPage>,
        output: Option<&std::path::Path>,
    ) -> Result<i32, String> {
        self.print_pages_progress(pages,output, |_|{})
    }
    fn print_pages_progress(&self,pages:Vec<PrintPage>,output:Option<&std::path::Path>,progress:impl Fn(usize))->Result<i32,String> {
        if pages.is_empty() {
            return Err("Choose at least one page.".into());
        }
        progress(1);
        let dc = self.dc()?;
        let (actual_w, actual_h, dpi_x, dpi_y) = page_geometry(dc.0)?;
        // Validate every requested paper size before submitting any sheets.
        for page in &pages {
            validate_page_size([actual_w, actual_h], page.size)?;
        }

        let title = wide("MC630 Sticker Print");

        let output_name = output.map(|p| wide(&p.to_string_lossy()));

        let doc = DOCINFOW {
            cbSize: size_of::<DOCINFOW>() as i32,

            lpszDocName: title.as_ptr(),

            lpszOutput: output_name.as_ref().map_or(null(), |s| s.as_ptr()),

            lpszDatatype: null(),

            fwType: 0,
        };

        let job = unsafe { StartDocW(dc.0, &doc) };

        if job <= 0 {
            return Err(error("Start print job"));
        }

        let count=pages.len();
        let operation = (|| {
            for (index,PrintPage {
                size: page,
                artwork,
            }) in pages.into_iter().enumerate()
            {
                progress(2+index*3);
                // Only one raster is retained at a time, even for large ranges.
                let image = artwork.raster(page)?;
                let bits = bitmap_bgra(&image);
                progress(3+index*3);
                let mut info: BITMAPINFO = unsafe { zeroed() };
                info.bmiHeader = BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: image.width() as i32,
                    biHeight: -(image.height() as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..unsafe { zeroed() }
                };
                if unsafe { StartPage(dc.0) } <= 0 {
                    return Err(error("Start print page"));
                }

                unsafe {
                    SetStretchBltMode(dc.0, HALFTONE as i32);

                    SetBrushOrgEx(dc.0, 0, 0, null_mut());
                }

                let result = unsafe {
                    StretchDIBits(
                        dc.0,
                        -GetDeviceCaps(dc.0, PHYSICALOFFSETX as i32),
                        -GetDeviceCaps(dc.0, PHYSICALOFFSETY as i32),
                        (page[0] / 25.4 * dpi_x as f64).round() as i32,
                        (page[1] / 25.4 * dpi_y as f64).round() as i32,
                        0,
                        0,
                        image.width() as i32,
                        image.height() as i32,
                        bits.as_ptr().cast(),
                        &info,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    )
                };

                if result <= 0 {
                    return Err(error("Render print page"));
                }

                if unsafe { EndPage(dc.0) } <= 0 {
                    return Err(error("Finish print page"));
                }
                progress(4+index*3);
            }

            progress(2+count*3);
            if unsafe { EndDoc(dc.0) } <= 0 {
                return Err(error("Submit print job"));
            }
            progress(3+count*3);

            Ok(())
        })();

        if let Err(e) = operation {
            unsafe {
                AbortDoc(dc.0);
            }

            return Err(e);
        }

        Ok(job)
    }
}

fn validate_page_size(actual: [f64; 2], requested: [f64; 2]) -> Result<(), String> {
    if (actual[0] - requested[0]).abs() > 2. || (actual[1] - requested[1]).abs() > 2. {
        return Err(format!(
            "Printer page is {:.1} x {:.1} mm; document is {:.1} x {:.1} mm. Choose the matching paper size and orientation in Printing Preferences (100%).",
            actual[0], actual[1], requested[0], requested[1]
        ));
    }
    Ok(())
}
fn page_geometry(h: HDC) -> Result<(f64, f64, i32, i32), String> {
    unsafe {
        let dx = GetDeviceCaps(h, LOGPIXELSX as i32);

        let dy = GetDeviceCaps(h, LOGPIXELSY as i32);

        if dx <= 0 || dy <= 0 {
            return Err("Invalid printer resolution.".into());
        }

        let w = GetDeviceCaps(h, PHYSICALWIDTH as i32) as f64 / dx as f64 * 25.4;

        let height = GetDeviceCaps(h, PHYSICALHEIGHT as i32) as f64 / dy as f64 * 25.4;

        Ok((w, height, dx, dy))
    }
}

pub fn printers() -> Result<Vec<String>, String> {
    unsafe {
        let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;

        let mut needed = 0;

        let mut count = 0;

        EnumPrintersW(flags, null(), 4, null_mut(), 0, &mut needed, &mut count);

        if needed == 0 {
            return Ok(vec![]);
        }

        let mut data = vec![0u64; (needed as usize).div_ceil(8)];

        if EnumPrintersW(
            flags,
            null(),
            4,
            data.as_mut_ptr().cast(),
            needed,
            &mut needed,
            &mut count,
        ) == 0
        {
            return Err(error("List printers"));
        }

        let records =
            std::slice::from_raw_parts(data.as_ptr().cast::<PRINTER_INFO_4W>(), count as usize);

        let mut names = records
            .iter()
            .map(|r| read_wide(r.pPrinterName))
            .collect::<Vec<_>>();

        names.sort();

        names.dedup();

        Ok(names)
    }
}

pub fn default_printer() -> Option<String> {
    unsafe {
        let mut n = 0;

        GetDefaultPrinterW(null_mut(), &mut n);

        if n == 0 {
            return None;
        }

        let mut name = vec![0u16; n as usize];

        if GetDefaultPrinterW(name.as_mut_ptr(), &mut n) == 0 {
            return None;
        }

        Some(read_wide(name.as_ptr()))
    }
}

fn bitmap_bgra(image: &RgbaImage) -> Vec<u8> {
    image.pixels().flat_map(|p| [p[2], p[1], p[0], 0]).collect()
}

fn discover_printers() -> std::sync::mpsc::Receiver<Result<(Vec<String>, Option<String>), String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = printers().map(|names| (names, default_printer()));
        let _ = tx.send(result);
    });
    rx
}
pub enum PrintArtwork {
    Project(Box<crate::project::Project>, bool),
    Design(
        mc630_plt_studio::sheet::Layout,
        Option<mc630_plt_studio::sheet::CcdTrial>,
    ),
    Front {
        art: std::sync::Arc<mc630_plt_studio::sheet::Artwork>,
        layout: mc630_plt_studio::sheet::Layout,
        ccd: Option<mc630_plt_studio::sheet::CcdTrial>,
    },
    Back(crate::duplex::BackDocument),
}

impl PrintArtwork {
    fn raster(self, page: [f64; 2]) -> Result<RgbaImage, String> {
        match self {
            Self::Project(project, back) => {
                if back {
                    return Self::Back(project.back).raster(page);
                }
                let source = project.source_untraced()?;
                let ccd = project.settings.ccd.then_some(&project.settings.profile);
                let layout = &project.settings.layout;
                if source.is_empty() {
                    return Self::Design(layout.clone(), ccd.cloned()).raster(page);
                }
                let art = layout.trace_images(
                    &source,
                    project.settings.remove_white,
                    project.settings.threshold,
                    project.settings.cut_shape,
                    project.settings.background_tolerance,
                    project.settings.print_without_background,
                )?;
                layout.print_raster(&art, ccd)
            }
            Self::Design(layout, ccd) => {
                if layout.design_shapes.is_empty() {
                    blank_page(page)
                } else {
                    layout.print_design_raster_with_ccd(ccd.as_ref())
                }
            }
            Self::Front { art, layout, ccd } => layout.print_raster(&art, ccd.as_ref()),
            Self::Back(data) => if data.images.is_empty(){blank_page(page)}else{data.raster(page)},
        }
    }
}

fn blank_page(page:[f64;2])->Result<RgbaImage,String> {
    if page.iter().any(|v|!v.is_finite() || *v<=0.) {return Err("Invalid print page size.".into());}
    let [w,h]=page.map(|v|(v/25.4*300.).round() as u32);
    if w==0 || h==0 || u64::from(w)*u64::from(h)>80_000_000 {
        return Err("Page is too large for 300 dpi printing.".into());
    }
    Ok(RgbaImage::from_pixel(w,h,image::Rgba([255,255,255,255])))
}

pub struct PrintPage {
    pub size: [f64; 2],
    pub artwork: PrintArtwork,
}

struct PrintProgress {
    pages:usize,
    step:std::sync::atomic::AtomicUsize,
}
impl PrintProgress {
    fn new(pages:usize)->Self {Self {pages,step:std::sync::atomic::AtomicUsize::new(0)}}
    fn snapshot(&self)->(f32,String) {
        let step=self.step.load(std::sync::atomic::Ordering::Relaxed);
        let total=3*self.pages+3;
        let label=if step>=total {"Submitted to printer queue".into()}
            else if step==0 {"Preparing printer…".into()}
            else if step==1 {"Checking paper size…".into()}
            else if step>=2+3*self.pages {"Submitting print job…".into()}
            else {let page=(step-2)/3+1;let stage=match (step-2)%3 {0=>"Preparing",1=>"Sending",_=>"Finished"};format!("{stage} page {page} of {}…",self.pages)};
        ((step as f32/total as f32).clamp(0.,1.),label)
    }
}
pub struct PrintPanel {
    discovery: Option<std::sync::mpsc::Receiver<Result<(Vec<String>, Option<String>), String>>>,
    names: Vec<String>,

    selected: String,

    preferences: Option<Preferences>,

    pending: Option<std::sync::mpsc::Receiver<Result<i32, String>>>,
    progress:std::sync::Arc<PrintProgress>,

    message: String,
}

impl Default for PrintPanel {
    fn default() -> Self {
        let names = Vec::new();
        let selected = Preferences::saved_name().unwrap_or_default();
        Self {
            discovery: Some(discover_printers()),
            names,

            selected,

            preferences: None,

            pending: None,
            progress:std::sync::Arc::new(PrintProgress::new(0)),

            message: String::new(),
        }
    }
}

impl PrintPanel {
    fn prepare(&mut self) -> Result<(), String> {
        if self.selected.is_empty() {
            return Err("Choose a printer first.".into());
        }

        if self
            .preferences
            .as_ref()
            .is_none_or(|p| p.name != self.selected)
        {
            self.preferences = Some(Preferences::restore(&self.selected)?);
        }

        Ok(())
    }

    pub fn poll_job(&mut self, ctx: &eframe::egui::Context, status: &mut String) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.message = match result {
                        Ok(id) => format!(
                            "Submitted to {} (job {id}). Check printer queue for completion.",
                            self.selected
                        ),

                        Err(e) => format!("Print failed: {e}"),
                    };

                    *status = self.message.clone();

                    self.pending = None;
                }

                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.message = "Print worker stopped unexpectedly.".into();

                    *status = self.message.clone();

                    self.pending = None;
                }

                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100))
                }
            }
        }
    }

    pub fn ui(&mut self, ui: &mut eframe::egui::Ui, has_artwork: bool) -> bool {
        use eframe::egui;
        let mut requested = false;

        if let Some(rx) = &self.discovery {
            match rx.try_recv() {
                Ok(Ok((names, default))) => {
                    self.names = names;
                    if !self.names.contains(&self.selected) {
                        self.selected = default
                            .filter(|n| self.names.contains(n))
                            .or_else(|| self.names.first().cloned())
                            .unwrap_or_default();
                        self.preferences = None;
                    }
                    self.discovery = None;
                }
                Ok(Err(e)) => {
                    self.message = e;
                    self.discovery = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.message = "Printer discovery stopped.".into();
                    self.discovery = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(200));
                }
            }
        }
        if self.discovery.is_some() {
            ui.small("Discovering printers in background...");
        }
        ui.label("Printer");

        ui.add_enabled_ui(self.pending.is_none(), |ui| {
            let old = self.selected.clone();
            ui.horizontal(|ui| {
                crate::ui_theme::compact_spacing(ui);
                egui::ComboBox::from_id_salt("native_printer")
                    .selected_text(if self.selected.is_empty() {
                        "Choose printer"
                    } else {
                        &self.selected
                    })
                    .width((ui.available_width() - 72.).max(160.))
                    .show_ui(ui, |ui| {
                        for name in &self.names {
                            ui.selectable_value(&mut self.selected, name.clone(), name);
                        }
                    });
                if old != self.selected {
                    self.preferences = None;
                    self.message.clear();
                }

                if icon_button(ui, Icon::Refresh, "Refresh printer list") {
                    if self.discovery.is_none() {
                        self.discovery = Some(discover_printers());
                    }
                }

                if icon_button(ui, Icon::Settings, "Printing preferences") {
                    let result = (|| {
                        self.prepare()?;
                        let p = self.preferences.as_mut().unwrap();
                        if p.edit()? {
                            p.inspect()
                        } else {
                            Ok("Preferences cancelled; previous settings kept.".into())
                        }
                    })();
                    self.message = match result {
                        Ok(s) => s,
                        Err(e) => e,
                    };
                }
            });

            if let Some(p) = &self.preferences {
                ui.collapsing("Printer details", |ui| {
                    ui.small(p.summary());
                });
            }
            ui.add_space(4.);
            ui.separator();
            let can_print = has_artwork && !self.selected.is_empty();
            let response = ui
                .horizontal(|ui| {
                    ui.add_space(
                        (ui.available_width() - 120. - ui.spacing().item_spacing.x).max(0.),
                    );
                    ui.add_enabled(
                        can_print,
                        egui::Button::new(
                            egui::RichText::new("Print Sheet").color(egui::Color32::WHITE),
                        )
                        .fill(crate::ui_theme::ACCENT)
                        .min_size(egui::vec2(120., 30.)),
                    )
                })
                .inner
                .on_disabled_hover_text(if !has_artwork {
                    "Enter a valid page range before printing."
                } else {
                    "Choose a printer first."
                });
            requested = response.clicked();
        });

        if self.pending.is_some() {
            let (progress,stage)=self.progress.snapshot();
            ui.add(crate::ui_theme::progress_bar(progress).animate(false));
            ui.small(format!("{stage} · {:.0}%",progress*100.));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }

        if !self.message.is_empty() {
            ui.small(&self.message);
        }
        requested
    }
    pub fn submit(
        &mut self,
        pages: Result<Vec<PrintPage>, String>,
        status: &mut String,
        ctx: &eframe::egui::Context,
    ) {
        if self.pending.is_some() {
            return;
        }
        {
            let pages = match pages {
                Ok(pages) => pages,
                Err(error) => {
                    self.message = error;
                    *status = self.message.clone();
                    return;
                }
            };
            let page_count = pages.len();
            self.progress=std::sync::Arc::new(PrintProgress::new(page_count));
            let progress=self.progress.clone();
            let name = self.selected.clone();
            let preferences = self.preferences.clone();
            let worker_ctx = ctx.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(|| {
                    let preferences = match preferences {
                        Some(preferences) => preferences,
                        None => Preferences::restore(&name)?,
                    };
                    preferences.print_pages_progress(pages,None,|step|{
                        progress.step.store(step,std::sync::atomic::Ordering::Relaxed);
                    })
                })
                .unwrap_or_else(|_| Err("Print worker stopped unexpectedly.".into()));
                let _ = tx.send(result);
                worker_ctx.request_repaint();
            });
            self.pending = Some(rx);
            self.message = format!("Preparing {page_count} page(s) for {}…", self.selected);
            *status = self.message.clone();
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]

mod tests {
    #[test]
    fn print_progress_reports_page_and_submission_stages_without_early_completion() {
        let progress=PrintProgress::new(2);
        assert_eq!(progress.snapshot().0,0.);
        progress.step.store(2,std::sync::atomic::Ordering::Relaxed);
        assert!(progress.snapshot().1.contains("Preparing page 1 of 2"));
        progress.step.store(6,std::sync::atomic::Ordering::Relaxed);
        assert!(progress.snapshot().1.contains("Sending page 2 of 2"));
        progress.step.store(8,std::sync::atomic::Ordering::Relaxed);
        let (value,label)=progress.snapshot();assert!(value<1.);assert_eq!(label,"Submitting print job…");
        progress.step.store(9,std::sync::atomic::Ordering::Relaxed);
        assert_eq!(progress.snapshot().0,1.);
        assert_eq!(progress.snapshot().1,"Submitted to printer queue");
    }
    #[test]
    fn empty_pages_in_a_range_render_as_white_sheets() {
        for artwork in [PrintArtwork::Design(Default::default(),None),PrintArtwork::Back(Default::default())] {
            let image=artwork.raster([25.4,25.4]).unwrap();
            assert_eq!(image.dimensions(),(300,300));
            assert!(image.pixels().all(|p|p.0==[255;4]));
        }
        assert!(blank_page([f64::NAN,297.]).is_err());
        assert!(blank_page([10000.,10000.]).is_err());
    }
    #[test]
    #[ignore = "Explicit local multi-page XPS test; never targets a physical printer"]
    fn virtual_xps_multiple_pages() {
        let path = std::path::PathBuf::from(
            std::env::var_os("MC630_TEST_XPS").expect("Set MC630_TEST_XPS"),
        );
        assert!(!path.exists());
        let prefs = Preferences::load("Microsoft XPS Document Writer").unwrap();
        let art: std::sync::Arc<_> = mc630_plt_studio::sheet::artwork(
            include_bytes!("../examples/A4-DEMO/sample-sticker.png"),
            false,
            245,
        )
        .unwrap()
        .into();
        let pages = (0..3)
            .map(|i| PrintPage {
                size: [210., 297.],
                artwork: PrintArtwork::Front {
                    art: art.clone(),
                    layout: mc630_plt_studio::sheet::Layout {
                        offset_x: 10. + i as f64 * 5.,
                        ..Default::default()
                    },
                    ccd: None,
                },
            })
            .collect();
        let steps=std::cell::RefCell::new(Vec::new());
        let job = prefs.print_pages_progress(pages, Some(&path),|step|steps.borrow_mut().push(step)).unwrap();
        assert_eq!(*steps.borrow(),(1..=12).collect::<Vec<_>>());
        assert!(job > 0);
        for _ in 0..100 {
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("Virtual XPS file was not produced");
    }
    #[test]
    fn print_page_validation_rejects_size_or_orientation_mismatch() {
        assert!(validate_page_size([297., 420.], [297., 420.]).is_ok());
        assert!(validate_page_size([420., 297.], [297., 420.]).is_err());
        assert!(validate_page_size([210., 297.], [215.9, 279.4]).is_err());
    }

    use super::*;

    #[test]

    fn dib_is_top_down_bgra() {
        let im = RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 255]));

        assert_eq!(bitmap_bgra(&im), [30, 20, 10, 0]);
    }

    #[test]
    #[ignore = "Explicit local virtual-printer smoke test; never targets a physical printer"]

    fn virtual_xps_print_job() {
        let path = std::env::var_os("MC630_TEST_XPS")
            .expect("Set MC630_TEST_XPS to a new local .oxps output path");

        let path = std::path::Path::new(&path);

        assert!(!path.exists());

        let mut prefs = Preferences::load("Microsoft XPS Document Writer").unwrap();
        let a3 = std::env::var_os("MC630_TEST_A3").is_some();
        if a3 {
            unsafe {
                let m = &mut *prefs.mut_ptr();
                m.Anonymous1.Anonymous1.dmPaperSize = DMPAPER_A3 as i16;
                m.Anonymous1.Anonymous1.dmOrientation = DMORIENT_LANDSCAPE as i16;
            }
            let input = prefs.clone();
            let printer = open(&prefs.name).unwrap();
            let name = wide(&prefs.name);
            assert_eq!(
                unsafe {
                    DocumentPropertiesW(
                        null_mut(),
                        printer.0,
                        name.as_ptr(),
                        prefs.mut_ptr(),
                        input.ptr(),
                        DM_IN_BUFFER | DM_OUT_BUFFER,
                    )
                },
                1
            );
        }

        println!("{}", prefs.inspect().unwrap());

        let a = mc630_plt_studio::sheet::artwork(
            include_bytes!("../examples/A4-DEMO/sample-sticker.png"),
            false,
            245,
        )
        .unwrap();

        let l = mc630_plt_studio::sheet::Layout {
            page_width: if a3 { 420. } else { 210. },
            page_height: 297.,
            ..Default::default()
        };

        let job = prefs
            .print_artwork(
                PrintArtwork::Front {
                    art: a.into(),
                    layout: l,
                    ccd: Some(Default::default()),
                },
                [if a3 { 420. } else { 210. }, 297.],
                Some(path),
            )
            .unwrap();

        println!("Virtual printer job {job}");

        for _ in 0..100 {
            if path.exists() && std::fs::metadata(path).unwrap().len() > 0 {
                return;
            }

            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        panic!("Virtual XPS file was not produced");
    }
}
