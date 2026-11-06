use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub projects: Vec<PathBuf>,
    pub active: Option<PathBuf>,
}
impl Session {
    pub fn normalized(mut self) -> Self {
        let mut paths: Vec<PathBuf> = Vec::new();
        for path in self.projects.into_iter().take(1000) {
            if path.is_absolute() && !paths.iter().any(|p| same_path(p, &path)) {
                paths.push(path);
            }
        }
        self.projects = paths;
        self.active = self
            .active
            .filter(|a| self.projects.iter().any(|p| same_path(p, a)));
        self
    }
}
pub fn same_path(a: &Path, b: &Path) -> bool {
    #[cfg(windows)]
    {
        a.to_string_lossy()
            .replace('/', "\\")
            .eq_ignore_ascii_case(&b.to_string_lossy().replace('/', "\\"))
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}
pub struct Store {
    path: Option<PathBuf>,
    pub saved: Option<Session>,
}
impl Default for Store {
    fn default() -> Self {
        #[cfg(test)]
        {
            Self {
                path: None,
                saved: None,
            }
        }
        #[cfg(not(test))]
        {
            Self::load(
                crate::preferences::path()
                    .parent()
                    .unwrap()
                    .join("workspace-session.json"),
            )
        }
    }
}
impl Store {
    pub fn load(path: PathBuf) -> Self {
        use std::io::Read;
        let saved = std::fs::File::open(&path).ok().and_then(|file| {
            let mut bytes = Vec::new();
            file.take(1_000_001).read_to_end(&mut bytes).ok()?;
            if bytes.len() > 1_000_000 {
                return None;
            }
            serde_json::from_slice::<Session>(&bytes)
                .ok()
                .map(Session::normalized)
        });
        Self {
            path: Some(path),
            saved,
        }
    }
    pub fn persist(&mut self, session: Session) -> Result<(), String> {
        let session = session.normalized();
        if self.saved.as_ref() == Some(&session) {
            return Ok(());
        }
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            crate::project::write_atomic(
                path,
                &serde_json::to_vec(&session).map_err(|e| e.to_string())?,
            )?;
        }
        self.saved = Some(session);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordered_session_and_selected_tab_roundtrip_including_empty_session() {
        let path =
            std::env::temp_dir().join(format!("mc630-workspace-{}.json", std::process::id()));
        let a = std::env::temp_dir().join("first project.mc630");
        let b = std::env::temp_dir().join("second project.mc630");
        let mut store = Store::load(path.clone());
        let session = Session {
            projects: vec![a.clone(), b.clone(), a],
            active: Some(b),
        }
        .normalized();
        store.persist(session.clone()).unwrap();
        assert_eq!(Store::load(path.clone()).saved, Some(session));
        store.persist(Session::default()).unwrap();
        assert_eq!(Store::load(path.clone()).saved, Some(Session::default()));
        std::fs::write(&path, b"broken json").unwrap();
        assert!(Store::load(path.clone()).saved.is_none());
        std::fs::remove_file(path).unwrap();
    }
}
