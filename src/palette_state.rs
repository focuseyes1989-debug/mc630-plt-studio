#[cfg(test)]
use eframe::egui;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Geometry {
    pub position: [f32; 2],
    /// Size of the content, excluding the window title and frame.
    pub size: [f32; 2],
}
impl Geometry {
    fn valid(self) -> bool {
        self.position
            .into_iter()
            .all(|v| v.is_finite() && v.abs() < 100_000.)
            && self
                .size
                .into_iter()
                .all(|v| v.is_finite() && (100. ..=20_000.).contains(&v))
    }
    #[cfg(test)]
    pub fn fit(self, bounds: egui::Rect) -> Self {
        let width = self.size[0].clamp(286., (bounds.width() - 16.).max(286.));
        let height = self.size[1].clamp(180., (bounds.height() - 55.).max(180.));
        Self {
            position: [
                self.position[0].clamp(
                    bounds.left(),
                    (bounds.right() - width - 14.).max(bounds.left()),
                ),
                self.position[1].clamp(
                    bounds.top(),
                    (bounds.bottom() - height - 45.).max(bounds.top()),
                ),
            ],
            size: [width, height],
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Section {
    pub geometry: Option<Geometry>,
    pub visible: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct State {
    #[serde(default)]
    pub sections: [Section; 3],
    pub geometry: Option<Geometry>,
    pub visible: bool,
    pub production: bool,
}
pub struct Store {
    path: Option<PathBuf>,
    pub state: State,
}
impl Default for Store {
    fn default() -> Self {
        #[cfg(test)]
        {
            Self {
                path: None,
                state: State::default(),
            }
        }
        #[cfg(not(test))]
        {
            Self::load(
                crate::preferences::path()
                    .parent()
                    .unwrap()
                    .join("palette-state.json"),
            )
        }
    }
}
impl Store {
    pub fn load(path: PathBuf) -> Self {
        use std::io::Read;
        let state = std::fs::File::open(&path)
            .ok()
            .and_then(|file| {
                let mut bytes = Vec::new();
                file.take(16_385).read_to_end(&mut bytes).ok()?;
                if bytes.len() > 16_384 {
                    return None;
                }
                serde_json::from_slice::<State>(&bytes).ok()
            })
            .filter(|s| {
                s.geometry.is_none_or(Geometry::valid)
                    && s.sections
                        .iter()
                        .all(|section| section.geometry.is_none_or(Geometry::valid))
            })
            .unwrap_or_default();
        Self {
            path: Some(path),
            state,
        }
    }
    pub fn persist(&mut self, state: State) -> Result<(), String> {
        if self.state == state {
            return Ok(());
        }
        if !state.geometry.is_none_or(Geometry::valid)
            || !state
                .sections
                .iter()
                .all(|section| section.geometry.is_none_or(Geometry::valid))
        {
            return Err("Invalid palette geometry".into());
        }
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            crate::project::write_atomic(
                path,
                &serde_json::to_vec(&state).map_err(|e| e.to_string())?,
            )?;
        }
        self.state = state;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn separate_dialog_geometry_roundtrips_and_old_palette_files_still_load() {
        let path =
            std::env::temp_dir().join(format!("mc630-section-dialogs-{}.json", std::process::id()));
        std::fs::write(
            &path,
            br#"{"geometry":null,"visible":true,"production":false}"#,
        )
        .unwrap();
        let mut store = Store::load(path.clone());
        assert!(store.state.visible);
        assert!(store.state.sections.iter().all(|s| !s.visible));
        let mut state = store.state;
        for (index, section) in state.sections.iter_mut().enumerate() {
            *section = Section {
                visible: true,
                geometry: Some(Geometry {
                    position: [100. + index as f32 * 300., 140.],
                    size: [310., 300. + index as f32 * 80.],
                }),
            };
        }
        store.persist(state).unwrap();
        assert_eq!(Store::load(path.clone()).state, state);
        std::fs::remove_file(path).unwrap();
    }

    use super::*;
    #[test]
    fn palette_roundtrip_preserves_position_inner_size_and_visibility() {
        let path = std::env::temp_dir().join(format!("mc630-palette-{}.json", std::process::id()));
        let mut store = Store::load(path.clone());
        let state = State {
            geometry: Some(Geometry {
                position: [173., 152.],
                size: [352., 478.],
            }),
            visible: true,
            production: false,
            sections: Default::default(),
        };
        store.persist(state).unwrap();
        assert_eq!(Store::load(path.clone()).state, state);
        let hidden = State {
            visible: false,
            ..state
        };
        store.persist(hidden).unwrap();
        assert_eq!(Store::load(path.clone()).state, hidden);
        std::fs::write(&path, b"broken").unwrap();
        assert_eq!(Store::load(path.clone()).state, State::default());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn geometry_fits_a_smaller_workspace_without_losing_title_bar() {
        let bounds = egui::Rect::from_min_max(egui::pos2(82., 110.), egui::pos2(1080., 690.));
        let geometry = Geometry {
            position: [1700., 900.],
            size: [800., 900.],
        }
        .fit(bounds);
        assert!(bounds.contains(egui::pos2(geometry.position[0], geometry.position[1])));
        assert!(geometry.position[0] + geometry.size[0] + 14. <= bounds.right());
        assert!(geometry.position[1] + geometry.size[1] + 45. <= bounds.bottom());
        assert_eq!(geometry.fit(bounds), geometry);
    }
}
