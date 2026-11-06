impl A4Studio {
    fn refresh_source_digest(&mut self) {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.source.hash(&mut hash);
        self.source_digest = if self.source.is_empty() {
            0
        } else {
            hash.finish()
        };
    }
    fn document_signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut prefs = self.preferences();
        prefs.layout = self.layout.clone();
        // Navigation does not edit the document, but is still included when saving it.
        prefs.zoom = 1.;
        prefs.reopen_last_project=false;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.source_digest.hash(&mut hash);
        self.name.hash(&mut hash);
        self.ratio.to_bits().hash(&mut hash);
        serde_json::to_vec(&prefs)
            .unwrap_or_default()
            .hash(&mut hash);
        hash.finish()
    }
    fn mark_saved(&mut self) {
        self.autosave.clear();
        self.saved_signature = self.document_signature();
    }
    fn is_dirty(&self) -> bool {
        self.saved_signature != self.document_signature()
    }
    fn execute_action(&mut self, action: DocumentAction, ctx: &egui::Context) {
        match action {
            DocumentAction::New => self.show_new = true,
            DocumentAction::OpenImage(path) => self.load(path, ctx),
            DocumentAction::OpenImageGroup(paths) => self.load_image_group(&paths, ctx),
            DocumentAction::OpenProject(path) => self.load_project(path, ctx),
            DocumentAction::CloseImage => self.close_image(),
            DocumentAction::CloseProject => {
                self.autosave.clear();
                self.tab_closed = true;
            }
            #[cfg(test)]
            DocumentAction::Quit => {
                self.autosave.clear();
                self.close_approved = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
    fn request_action(&mut self, action: DocumentAction, ctx: &egui::Context) {
        if self.pending_action.is_some() || !self.autosave.offered.is_empty() {
            return;
        }
        if self.workspace_enabled {
            match &action {
                DocumentAction::New => { self.workspace_action = Some(WorkspaceAction::New); return; }
                DocumentAction::OpenProject(path) => { self.workspace_action = Some(WorkspaceAction::Open(path.clone())); return; }
                _ => {}
            }
        }
        let first_image = self.source.is_empty()
            && matches!(&action, DocumentAction::OpenImage(_) | DocumentAction::OpenImageGroup(_));
        if self.is_dirty() && !first_image {
            self.pending_action = Some(action);
            ctx.request_repaint();
        } else {
            self.execute_action(action, ctx);
        }
    }
    #[cfg(test)]
    pub fn intercept_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.is_dirty() {
            self.autosave.clear();
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.close_approved && self.is_dirty()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.pending_action.is_none() {
                self.pending_action = Some(DocumentAction::Quit);
            }
        }
    }
    fn finish_unsaved(&mut self, choice: UnsavedChoice, ctx: &egui::Context) {
        let Some(action) = self.pending_action.take() else {
            return;
        };
        match choice {
            UnsavedChoice::Cancel => { self.close_canceled = true; }
            UnsavedChoice::Discard => self.execute_action(action, ctx),
            UnsavedChoice::Save => {
                if self.save_project(false) {
                    self.execute_action(action, ctx);
                } else { self.close_canceled = true; }
            }
        }
    }
    pub fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        if !self.autosave.offered.is_empty() {
            self.recovery_dialog(ctx);
            return;
        }
        if self.pending_action.is_none() {
            return;
        }
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("unsaved-project")).show(ctx, |ui| {
            ui.set_width(440.);
            ui.heading("Save changes?");
            ui.add_space(8.);
            let name = self
                .project_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled project".into());
            ui.label(format!("{name} has unsaved changes."));
            ui.label("Save your image, layout and settings before continuing?");
            ui.add_space(16.);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(UnsavedChoice::Save);
                }
                if ui.button("Don't Save").clicked() {
                    choice = Some(UnsavedChoice::Discard);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(UnsavedChoice::Cancel);
                }
            });
        });
        if modal.should_close() {
            choice = Some(UnsavedChoice::Cancel);
        }
        if let Some(choice) = choice {
            self.finish_unsaved(choice, ctx);
        }
    }
}

impl A4Studio {
    pub fn tab_label(&self) -> String {
        let name = self.project_path.as_ref().and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
        format!("{}{}", name, if self.is_dirty() { " *" } else { "" })
    }
    pub fn tab_busy(&self) -> bool {
        self.pending_action.is_some() || !self.autosave.offered.is_empty()
    }
    pub fn request_tab_close(&mut self, ctx: &egui::Context) {
        self.close_canceled = false;
        self.request_action(DocumentAction::CloseProject, ctx);
    }
    pub fn fresh_tab() -> Self {
        let mut doc = Self::default();
        doc.workspace_enabled = true;
        doc.startup_pending = false;
        doc
    }
    pub fn open_tab_file(&mut self, path: PathBuf, ctx: &egui::Context) -> bool {
        match self.load_project_from(path, ctx) {
            Ok(()) => true,
            Err(e) => { self.status = format!("Cannot open project: {e}"); false }
        }
    }
    pub fn show_new_tab_dialog(&mut self) { self.show_new = true; }
    pub fn same_project(&self, path: &std::path::Path) -> bool {
        self.project_path.as_ref().is_some_and(|p| p.to_string_lossy().eq_ignore_ascii_case(&path.to_string_lossy()))
    }
    pub fn workspace_status(&mut self, status: String) { self.status = status; }
}

impl A4Studio {
    pub fn project_file(&self) -> Option<PathBuf> { self.project_path.clone() }
    pub fn reopen_tabs_enabled(&self) -> bool { self.reopen_last_project }
    pub fn set_reopen_tabs(&mut self, enabled: bool) { self.reopen_last_project = enabled; }
    pub fn suppress_single_project_startup(&mut self) {
        if self.startup_project.is_none() { self.startup_pending = false; }
    }
    pub fn startup_finished(&self) -> bool { !self.startup_pending }
    pub fn blank_tab(&self) -> bool { self.source.is_empty() && self.project_path.is_none() && !self.is_dirty() }
}

#[cfg(test)]
impl A4Studio {
    pub fn save_workspace_fixture(&mut self, path: PathBuf) { self.save_project_to(path).unwrap(); }
}

#[cfg(test)]
impl A4Studio {
    pub fn edit_workspace_fixture(&mut self, count: usize) { self.layout.count = count; }
}

#[cfg(test)]
impl A4Studio {
    pub fn choose_close_fixture(&mut self, choice: &str, ctx: &egui::Context) {
        self.finish_unsaved(match choice { "save" => UnsavedChoice::Save, "discard" => UnsavedChoice::Discard, _ => UnsavedChoice::Cancel }, ctx);
    }
}

impl A4Studio {
    pub fn tab_navigation_blocked(&self) -> bool {
        self.tab_busy() || self.show_new || self.show_settings
    }
}
