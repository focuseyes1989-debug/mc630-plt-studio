use crate::{project::Project, recovery::SnapshotData};
use eframe::egui;
use mc630_plt_studio::sheet::Artwork;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
};

// Serialize heavyweight jobs so restoring several tabs cannot saturate the CPU.
static WORKER: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub struct Loaded {
    pub project: Project,
    pub source: Vec<u8>,
    pub art: Option<Artwork>,
    pub preview: Option<egui::ColorImage>,
    pub back_previews: crate::duplex::PreparedPreviews,
}
pub enum Completed {
    Saved,
    Opened(Loaded),
}
pub struct Job {
    pub path: PathBuf,
    pub saving: bool,
    pub progress: Arc<AtomicU8>,
    pub result: mpsc::Receiver<Result<Completed, String>>,
}
impl Job {
    fn start(
        path: PathBuf,
        saving: bool,
        work: impl FnOnce(&PathBuf, &AtomicU8) -> Result<Completed, String> + Send + 'static,
    ) -> Self {
        let progress = Arc::new(AtomicU8::new(0));
        let progress_worker = progress.clone();
        let (tx, result) = mpsc::channel();
        let target = path.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = WORKER.lock().unwrap_or_else(|e| e.into_inner());
                progress_worker.store(5, Ordering::Relaxed);
                work(&target, &progress_worker)
            }))
            .unwrap_or_else(|_| Err("Project operation stopped unexpectedly.".into()));
            let _ = tx.send(result);
        });
        Self {
            path,
            saving,
            progress,
            result,
        }
    }
    pub fn save(path: PathBuf, data: SnapshotData) -> Self {
        Self::start(path, true, move |path, progress| {
            let project = data.record().project;
            progress.store(25, Ordering::Relaxed);
            project.save_progress(path, |stage| progress.store(stage, Ordering::Relaxed))?;
            Ok(Completed::Saved)
        })
    }
    pub fn open(path: PathBuf) -> Self {
        Self::start(path, false, |path, progress| {
            let project = Project::read(path)?;
            progress.store(25, Ordering::Relaxed);
            let source = project.source_untraced()?;
            progress.store(50, Ordering::Relaxed);
            let art = if source.is_empty() {
                None
            } else {
                Some(project.settings.layout.trace_images(
                    &source,
                    project.settings.remove_white,
                    project.settings.threshold,
                    project.settings.cut_shape,
                    project.settings.background_tolerance,
                    project.settings.print_without_background,
                )?)
            };
            progress.store(80, Ordering::Relaxed);
            let preview = art.as_ref().map(|art| {
                let thumb = image::imageops::thumbnail(&art.pixels, 1500, 1500);
                egui::ColorImage::from_rgba_unmultiplied(
                    [thumb.width() as usize, thumb.height() as usize],
                    thumb.as_raw(),
                )
            });
            let back_previews = project.back.prepare_previews()?;
            progress.store(95, Ordering::Relaxed);
            Ok(Completed::Opened(Loaded {
                project,
                source,
                art,
                preview,
                back_previews,
            }))
        })
    }
    pub fn stage(&self) -> (f32, &'static str) {
        let p = self.progress.load(Ordering::Relaxed);
        let label = match (self.saving, p) {
            (_, 0) => "Waiting for another project…",
            (true, 1..=24) => "Preparing project…",
            (true, 25..=49) => "Checking project…",
            (true, 50..=79) => "Encoding project…",
            (true, _) => "Writing file…",
            (false, 1..=24) => "Reading project…",
            (false, 25..=49) => "Checking images…",
            (false, 50..=79) => "Preparing cut contours…",
            (false, _) => "Preparing preview…",
        };
        (f32::from(p) / 100., label)
    }
}
