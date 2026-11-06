impl A4Studio {
    fn autosave_data(&self) -> crate::recovery::SnapshotData {
        let mut settings = self.preferences();
        settings.layout = self.layout.clone();
        crate::recovery::SnapshotData {
            name: self.name.clone(),
            source: self.source.clone(),
            settings,
            pan: [self.canvas_pan.x, self.canvas_pan.y],
            ratio: self.ratio,
            original_path: self.project_path.clone(),
        }
    }
    pub fn tick_autosave(&mut self, ctx: &egui::Context) {
        use std::time::Duration;
        self.autosave.poll();
        ctx.request_repaint_after(Duration::from_millis(500));
        if !self.autosave.offered.is_empty() || self.close_approved || self.tab_closed {
            return;
        }
        let signature = self.document_signature();
        if self.autosave.observed != Some(signature) {
            self.autosave.observed = Some(signature);
            self.autosave.changed_at = std::time::Instant::now();
        }
        if !self.is_dirty() {
            if self.autosave.last_written.is_some() || self.autosave.busy() {
                self.autosave.clear();
            }
            return;
        }
        if !self.autosave_enabled
            || self.autosave.busy()
            || self.autosave.store.is_none()
            || self.autosave.last_written == Some(signature)
            || self.drag.is_some()
        {
            return;
        }
        let quiet = self.autosave.changed_at.elapsed() >= Duration::from_secs(2);
        let due =
            self.autosave.attempted_at.elapsed() >= Duration::from_secs(self.autosave_seconds);
        // A failed write retries at the interval rather than continuously.
        if (quiet || due) && (self.autosave.problem.is_none() || due) {
            let data = self.autosave_data();
            self.autosave.begin(signature, data);
        }
    }
    fn restore_recovery(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let record = self
            .autosave
            .offered
            .first()
            .and_then(|c| c.record.as_ref())
            .ok_or("No recovery available")?;
        let source = record.project.recovery_source()?;
        let record = self.autosave.offered[0].record.take().unwrap();
        if self.workspace_enabled {
            let candidate = self.autosave.offered.remove(0);
            self.autosave.retained.push(candidate);
        } else {
            self.autosave.retained.append(&mut self.autosave.offered);
        }
        self.install_project(record.project, source, ctx);
        self.project_path = record.original_path;
        self.saved_signature = self.document_signature().wrapping_add(1);
        self.autosave.observed = Some(self.document_signature());
        self.autosave.changed_at = std::time::Instant::now() - std::time::Duration::from_secs(3);
        self.status =
            "Recovered unsaved image, layout and settings. Use Ctrl+S to save your project.".into();
        if self.workspace_enabled && !self.autosave.offered.is_empty() {
            let mut next = Self::fresh_tab();
            next.autosave.offered = std::mem::take(&mut self.autosave.offered);
            self.workspace_action = Some(WorkspaceAction::Recover(Box::new(next)));
        }
        Ok(())
    }
    fn discard_recoveries(&mut self) {
        for candidate in self.autosave.offered.drain(..) {
            candidate.discard();
        }
    }
    fn recovery_dialog(&mut self, ctx: &egui::Context) {
        let Some(record) = self
            .autosave
            .offered
            .first()
            .and_then(|c| c.record.as_ref())
        else {
            return;
        };
        let label = record
            .original_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled project".into());
        let age = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(record.saved_at);
        let mut choice = None;
        egui::Modal::new(egui::Id::new("crash-recovery")).show(ctx,|ui|{
            ui.set_width(480.);ui.heading("Recover your last work?");ui.add_space(8.);
            ui.label(format!("Recovered Auto Save: {label}"));
            ui.label(format!("{} - {} images. Saved about {} minutes ago.",record.project.artwork_name,record.project.settings.layout.count,age/60));
            ui.label("Restore the image, layout and settings from the latest recovery snapshot?");
            if self.autosave.offered.len()>1{ui.small(format!("{} previous sessions found. Restore uses the most recent; Discard removes these previous-session recoveries.",self.autosave.offered.len()));}
            ui.add_space(14.);ui.horizontal(|ui|{
                if ui.button("Restore").clicked(){choice=Some(true);}
                if ui.button("Discard recovery").clicked(){choice=Some(false);}
            });
        });
        match choice {
            Some(true) => {
                if let Err(e) = self.restore_recovery(ctx) {
                    self.status = format!("Cannot restore recovery: {e}");
                }
            }
            Some(false) => self.discard_recoveries(),
            None => {}
        }
    }
}
