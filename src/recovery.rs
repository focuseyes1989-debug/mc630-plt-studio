use crate::{preferences::Preferences, project::Project};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    thread::JoinHandle,
    time::{Duration, Instant},
};
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Record {
    pub project: Project,
    pub original_path: Option<PathBuf>,
    pub saved_at: u64,
}
impl Record {
    pub fn write(&self, path: &Path) -> Result<(), String> {
        self.project.recovery_source()?;
        let data = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        if data.len() > 64_000_000 {
            return Err("Recovery exceeds 64 MB.".into());
        }
        crate::project::write_atomic(path, &data)
    }
    pub fn read(path: &Path) -> Result<Self, String> {
        use std::io::Read;
        let mut data = Vec::new();
        File::open(path)
            .map_err(|e| e.to_string())?
            .take(64_000_001)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if data.len() > 64_000_000 {
            return Err("Recovery exceeds 64 MB.".into());
        }
        let record: Self = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
        record.project.recovery_source()?;
        Ok(record)
    }
}
fn lock(path: &Path, create: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if create {
        options.create_new(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(path)
}
pub struct Candidate {
    pub dir: PathBuf,
    pub record: Option<Record>,
    guard: Option<File>,
}
impl Candidate {
    pub fn discard(mut self) {
        let _ = std::fs::remove_file(self.dir.join("recovery.json"));
        self.guard.take();
        let _ = std::fs::remove_file(self.dir.join("owner.lock"));
        let _ = std::fs::remove_dir(&self.dir);
    }
}
pub struct Store {
    pub dir: PathBuf,
    guard: Option<File>,
}
impl Store {
    pub fn open(root: &Path) -> Result<(Self, Vec<Candidate>), String> {
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let dir = root.join(format!("session-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
        let guard = lock(&dir.join("owner.lock"), true).map_err(|e| e.to_string())?;
        let store = Self {
            dir,
            guard: Some(guard),
        };
        let mut candidates = Vec::new();
        for entry in std::fs::read_dir(root)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            let dir = entry.path();
            if dir == store.dir || !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let path = dir.join("recovery.json");
            if !path.is_file() {
                continue;
            }
            // Active app instances own an exclusive lock; never claim their snapshots.
            let Ok(guard) = lock(&dir.join("owner.lock"), false) else {
                continue;
            };
            if let Ok(record) = Record::read(&path) {
                candidates.push(Candidate {
                    dir,
                    record: Some(record),
                    guard: Some(guard),
                });
            }
        }
        candidates.sort_by_key(|c| {
            std::cmp::Reverse((
                c.record.as_ref().unwrap().saved_at,
                std::fs::metadata(c.dir.join("recovery.json"))
                    .and_then(|m| m.modified())
                    .ok(),
            ))
        });
        Ok((store, candidates))
    }
    pub fn path(&self) -> PathBuf {
        self.dir.join("recovery.json")
    }
    pub fn clear(&self) -> Result<(), String> {
        match std::fs::remove_file(self.path()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        self.guard.take();
        if !self.path().exists() {
            let _ = std::fs::remove_file(self.dir.join("owner.lock"));
            let _ = std::fs::remove_dir(&self.dir);
        }
    }
}
pub struct SnapshotData {
    pub name: String,
    pub source: Vec<u8>,
    pub settings: Preferences,
    pub pan: [f32; 2],
    pub ratio: f64,
    pub original_path: Option<PathBuf>,
}
impl SnapshotData {
    pub fn record(self) -> Record {
        let mut project = Project::new(self.name, &self.source, self.settings, self.pan);
        project.aspect_ratio = self.ratio;
        Record {
            project,
            original_path: self.original_path,
            saved_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}
pub struct AutoSave {
    pub store: Option<Store>,
    pub offered: Vec<Candidate>,
    pub retained: Vec<Candidate>,
    job: Option<(u64, JoinHandle<Result<(), String>>)>,
    pub last_written: Option<u64>,
    pub observed: Option<u64>,
    pub changed_at: Instant,
    pub attempted_at: Instant,
    pub succeeded_at: Option<Instant>,
    pub problem: Option<String>,
}
impl Default for AutoSave {
    fn default() -> Self {
        Self {
            store: None,
            offered: vec![],
            retained: vec![],
            job: None,
            last_written: None,
            observed: None,
            changed_at: Instant::now(),
            attempted_at: Instant::now() - Duration::from_secs(3600),
            succeeded_at: None,
            problem: None,
        }
    }
}
impl AutoSave {
    pub fn start(&mut self, root: &Path) {
        match Store::open(root) {
            Ok((store, candidates)) => {
                self.store = Some(store);
                self.offered = candidates;
            }
            Err(e) => self.problem = Some(format!("Auto Save unavailable: {e}")),
        }
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    fn finish(&mut self) {
        if let Some((signature, job)) = self.job.take() {
            match job.join() {
                Ok(Ok(())) => {
                    self.last_written = Some(signature);
                    self.succeeded_at = Some(Instant::now());
                    self.problem = None;
                    for candidate in self.retained.drain(..) {
                        candidate.discard();
                    }
                }
                Ok(Err(e)) => self.problem = Some(format!("Auto Save failed: {e}")),
                Err(_) => self.problem = Some("Auto Save worker stopped unexpectedly.".into()),
            }
        }
    }
    pub fn poll(&mut self) {
        if self.job.as_ref().is_some_and(|(_, job)| job.is_finished()) {
            self.finish();
        }
    }
    pub fn begin(&mut self, signature: u64, data: SnapshotData) {
        let Some(store) = &self.store else {
            return;
        };
        if self.busy() {
            return;
        }
        let path = store.path();
        self.attempted_at = Instant::now();
        self.job = Some((
            signature,
            std::thread::spawn(move || data.record().write(&path)),
        ));
    }
    pub fn clear(&mut self) {
        self.finish();
        if let Some(store) = &self.store {
            if let Err(e) = store.clear() {
                self.problem = Some(format!("Cannot remove old recovery: {e}"));
            }
        }
        for candidate in self.retained.drain(..) {
            candidate.discard();
        }
        self.last_written = None;
        self.observed = None;
        self.succeeded_at = None;
    }
}
impl Drop for AutoSave {
    fn drop(&mut self) {
        self.finish();
    }
}
pub fn root() -> PathBuf {
    crate::preferences::path()
        .parent()
        .unwrap()
        .join("Recovery")
}
