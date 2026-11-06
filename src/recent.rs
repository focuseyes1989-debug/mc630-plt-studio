use std::path::{Path, PathBuf};
const LIMIT: usize = 10;
#[derive(Default)]
pub struct RecentProjects {
    pub entries: Vec<PathBuf>,
    storage: Option<PathBuf>,
}
fn key(path: &Path) -> String {
    let value = path.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        return value.replace('/', "\\").to_lowercase();
    }
    #[cfg(not(windows))]
    value
}
fn absolute(path: &Path) -> PathBuf {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        }
    });
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
            return PathBuf::from(format!("\\\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix("\\\\?\\") {
            return PathBuf::from(rest);
        }
    }
    path
}
impl RecentProjects {
    pub fn load(path: PathBuf) -> Self {
        let mut recent = Self {
            entries: vec![],
            storage: Some(path.clone()),
        };
        if let Ok(file) = std::fs::File::open(&path) {
            use std::io::Read;
            let mut bytes = Vec::new();
            let _ = file.take(128_001).read_to_end(&mut bytes);
            if bytes.len() <= 128_000 {
                if let Ok(entries) = serde_json::from_slice::<Vec<PathBuf>>(&bytes) {
                    for entry in entries {
                        if entry.is_absolute()
                            && !recent.entries.iter().any(|p| key(p) == key(&entry))
                        {
                            recent.entries.push(entry);
                        }
                        if recent.entries.len() == LIMIT {
                            break;
                        }
                    }
                }
            }
        }
        recent
    }
    fn persist(&self) -> Result<(), String> {
        if let Some(path) = &self.storage {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            crate::project::write_atomic(
                path,
                &serde_json::to_vec(&self.entries).map_err(|e| e.to_string())?,
            )?;
        }
        Ok(())
    }
    pub fn remember(&mut self, path: &Path) -> Result<(), String> {
        let path = absolute(path);
        self.entries.retain(|p| key(p) != key(&path));
        self.entries.insert(0, path);
        self.entries.truncate(LIMIT);
        self.persist()
    }
    pub fn clear(&mut self) -> Result<(), String> {
        self.entries.clear();
        self.persist()
    }
}
#[cfg(not(test))]
pub fn storage_path() -> PathBuf {
    crate::preferences::path()
        .parent()
        .unwrap()
        .join("recent-projects.json")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recency_deduplication_limit_and_persistence() {
        let path = std::env::temp_dir().join(format!(
            "mc630-recents-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut recent = RecentProjects::load(path.clone());
        for i in 0..12 {
            recent
                .remember(&std::env::temp_dir().join(format!("project {i}.mc630")))
                .unwrap();
        }
        assert_eq!(recent.entries.len(), 10);
        let old = recent.entries[5].clone();
        recent.remember(&old).unwrap();
        assert_eq!(recent.entries[0], old);
        assert_eq!(recent.entries.len(), 10);
        #[cfg(windows)]
        {
            recent
                .remember(&PathBuf::from(old.to_string_lossy().to_uppercase()))
                .unwrap();
            assert_eq!(recent.entries.len(), 10);
        }
        let loaded = RecentProjects::load(path.clone());
        assert_eq!(loaded.entries, recent.entries);
        recent.clear().unwrap();
        assert!(RecentProjects::load(path.clone()).entries.is_empty());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn damaged_history_is_ignored() {
        let path =
            std::env::temp_dir().join(format!("mc630-bad-recents-{}.json", std::process::id()));
        std::fs::write(&path, b"invalid json").unwrap();
        assert!(RecentProjects::load(path.clone()).entries.is_empty());
        std::fs::remove_file(path).unwrap();
    }
}
