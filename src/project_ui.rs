impl A4Studio {
    fn remember_recent(&mut self, path: &std::path::Path) {
        #[cfg(not(test))]
        { self.recent_projects = crate::recent::RecentProjects::load(crate::recent::storage_path()); }
        if let Err(e) = self.recent_projects.remember(path) {
            self.status
                .push_str(&format!(" (Recent history not saved: {e})"));
        }
    }
    pub fn recent_projects_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> bool {
        let mut chosen = None;
        let mut clear = false;
        ui.menu_button("Recent Projects", |ui| {
            if self.recent_projects.entries.is_empty() {
                ui.label("No recent projects yet");
            }
            for (index, path) in self.recent_projects.entries.iter().enumerate() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                if ui
                    .button(format!("{}. {}", index + 1, name))
                    .on_hover_text(path.display().to_string())
                    .clicked()
                {
                    chosen = Some(path.clone());
                    ui.close_menu();
                }
            }
            ui.separator();
            if ui
                .add_enabled(
                    !self.recent_projects.entries.is_empty(),
                    egui::Button::new("Clear recent history"),
                )
                .on_hover_text("Removes this list only. Project files are kept.")
                .clicked()
            {
                clear = true;
                ui.close_menu();
            }
        });
        if clear {
            if let Err(e) = self.recent_projects.clear() {
                self.status = format!("Cannot clear recent history: {e}");
            }
        }
        let requested = chosen.is_some();
        if let Some(path) = chosen {
            self.request_action(DocumentAction::OpenProject(path), ctx);
        }
        requested
    }
    pub fn queue_startup_project(&mut self, path: PathBuf) {
        self.startup_project = Some(path);
        self.startup_pending = true;
    }
    pub fn reopen_on_startup(&mut self, ctx: &egui::Context) {
        if !self.startup_pending {
            return;
        }
        if !self.autosave.offered.is_empty() {
            // Keep an explicit request queued until the recovery decision is complete.
            if self.startup_project.is_none() {
                self.startup_pending = false;
            }
            return;
        }
        self.startup_pending = false;
        if let Some(path) = self.startup_project.take() {
            self.request_action(DocumentAction::OpenProject(path), ctx);
            return;
        }
        if !self.reopen_last_project
            || self.project_path.is_some()
            || !self.source.is_empty()
            || self.is_dirty()
        {
            return;
        }
        if let Some(path) = self.recent_projects.entries.first().cloned() {
            self.open_tab_file(path,ctx);
        }
    }
    #[cfg(test)]
    fn project_snapshot(&self) -> crate::project::Project {
        let mut settings = self.preferences();
        settings.layout = self.layout.clone();
        let mut project = crate::project::Project::new(
            self.name.clone(),
            &self.source,
            settings,
            [self.canvas_pan.x, self.canvas_pan.y],
        );
        project.aspect_ratio = self.ratio;
        project.back = self.back.data.clone();
        project
    }
    #[cfg(test)]
    fn save_project_to(&mut self, path: PathBuf) -> Result<(), String> {
        self.project_snapshot().save(&path)?;
        self.project_path = Some(path.clone());
        self.mark_saved();
        self.status = format!("Project saved: {}", path.display());
        self.remember_recent(&path);
        Ok(())
    }
    fn save_project(&mut self, save_as: bool) -> bool {
        let path = if !save_as && self.project_path.is_some() {
            self.project_path.clone()
        } else {
            let mut dialog = rfd::FileDialog::new().add_filter("MC630 project", &["mc630"]);
            if let Some(path) = &self.project_path {
                if let Some(parent) = path.parent() {
                    dialog = dialog.set_directory(parent);
                }
                if let Some(name) = path.file_name() {
                    dialog = dialog.set_file_name(name.to_string_lossy());
                }
            } else {
                dialog = dialog.set_file_name("Untitled.mc630");
            }
            dialog.save_file().map(|mut path| {
                if !path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("mc630"))
                {
                    path.set_extension("mc630");
                }
                path
            })
        };
        if let Some(path) = path {
            if self.io_job.is_some(){return false;}
            self.io_job=Some(crate::project_io::Job::save(path,self.autosave_data()));
            true
        } else {
            self.status = "Save canceled. Current document is still open.".into();
            false
        }
    }
    fn choose_project(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("MC630 project", &["mc630"])
            .pick_file()
        {
            self.request_action(DocumentAction::OpenProject(path), ctx);
        }
    }
    #[cfg(test)]
    fn load_project_from(&mut self, path: PathBuf, ctx: &egui::Context) -> Result<(), String> {
        let project = crate::project::Project::read(&path)?;
        // Fully validate and trace first. Failed opens leave the current document intact.
        let source = project.source()?;
        self.install_project(project, source, ctx);
        self.project_path = Some(path.clone());
        self.mark_saved();
        self.status = format!("Project opened: {}", path.display());
        self.remember_recent(&path);
        Ok(())
    }
    fn install_project(
        &mut self,
        project: crate::project::Project,
        source: Vec<u8>,
        ctx: &egui::Context,
    ) {
        self.install_project_prepared(project,source,ctx,None,None,false);
    }
    fn install_project_prepared(&mut self,project:crate::project::Project,source:Vec<u8>,ctx:&egui::Context,art:Option<Artwork>,preview:Option<egui::ColorImage>,prepared:bool) {
        self.back_paths_cache=None;
        self.back.install(project.back);
        self.show_back = false;
        self.print_back = false;
        let layout = project.settings.layout.clone();
        let reopen_last_project = self.reopen_last_project;
        self.apply_preferences(project.settings);
        self.reopen_last_project = reopen_last_project;
        self.layout = layout;
        self.ratio = project.aspect_ratio;
        self.source = source;
        self.name = project.artwork_name;
        self.preview_cache = None;
        self.art = None;
        self.texture = None;
        self.refresh_source_digest();
        if prepared {
            self.art=art;
            self.texture=preview.map(|image|ctx.load_texture("sticker",image,egui::TextureOptions::LINEAR));
        } else if !self.source.is_empty() {
            self.retrace(ctx);
        }
        self.canvas_pan = egui::vec2(project.canvas_pan[0], project.canvas_pan[1]);
        self.clear_selection();
        self.drag = None;
        self.duplicate_step = None;
        self.guide_drag = None;
        self.selected_guide = None;
        self.cut_draft = None;
        self.pick_cut_shapes = false;
        self.selected_cut_shape = None;
        self.cut_shape_drag = None;
        self.camera = Default::default();
        self.undo.clear();
        self.redo.clear();
        self.last_print = None;
        self.show_new = false;
        self.show_settings = false;
          self.page_size_editor = None;
    }
    fn load_project(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.open_tab_file(path,ctx);
    }
}

impl A4Studio {
    pub fn save_for_workspace(&mut self) -> Result<(), String> {
        if let Some(result)=self.io_workspace_result.take(){return result;}
        if self.save_project(false) { self.io_workspace_waiting=true; Ok(()) } else { Err(self.status.clone()) }
    }
}

impl A4Studio {
    pub fn save_current_tab(&mut self){self.save_project(false);}
    pub fn project_io_busy(&self)->bool{self.io_job.is_some()}
    pub fn poll_project_io(&mut self,ctx:&egui::Context) {
        let Some(job)=self.io_job.take() else{return;};
        let result=match job.result.try_recv(){
            Ok(result)=>result,
            Err(std::sync::mpsc::TryRecvError::Empty)=>{self.io_job=Some(job);return;},
            Err(std::sync::mpsc::TryRecvError::Disconnected)=>Err("Project worker stopped unexpectedly.".into()),
        };
        match result {
            Ok(crate::project_io::Completed::Saved)=>{
                self.project_path=Some(job.path.clone());self.mark_saved();
                self.status=format!("Project saved: {}",job.path.display());self.remember_recent(&job.path);
                if self.io_workspace_waiting{self.io_workspace_result=Some(Ok(()));}
                if let Some(action)=self.io_after_save.take(){self.execute_action(action,ctx);}
            }
            Ok(crate::project_io::Completed::Opened(loaded))=>{
                self.install_project_prepared(loaded.project,loaded.source,ctx,loaded.art,loaded.preview,true);
                self.back.install_previews(ctx,loaded.back_previews);
                self.project_path=Some(job.path.clone());self.mark_saved();
                self.status=format!("Project opened: {}",job.path.display());self.remember_recent(&job.path);
            }
            Err(error)=>{
                self.status=format!("Cannot {} project: {error}",if job.saving{"save"}else{"open"});
                if self.io_workspace_waiting{self.io_workspace_result=Some(Err(self.status.clone()));}
                if self.io_after_save.take().is_some(){self.close_canceled=true;}
            }
        }
        self.io_workspace_waiting=false;
        if self.io_close_requested && self.io_job.is_none(){self.io_close_requested=false;self.request_tab_close(ctx);}
        ctx.request_repaint();
    }
    pub fn project_io_progress(&self,ctx:&egui::Context) {
        let Some(job)=&self.io_job else{return;};
        egui::CentralPanel::default().show(ctx,|_|{});
        egui::Modal::new(egui::Id::new("project-io-progress")).show(ctx,|ui|{
            ui.set_width(340.);
            ui.heading(if job.saving{"Saving project"}else{"Opening project"});
            ui.add(egui::Label::new(job.path.file_name().unwrap_or_default().to_string_lossy()).truncate());
            ui.add_space(10.);let (progress,stage)=job.stage();
            ui.add(egui::ProgressBar::new(progress).animate(true));
            ui.horizontal(|ui|{ui.spinner();ui.label(stage);});
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

#[cfg(test)]
impl A4Studio {
    pub fn wait_project_io(&mut self,ctx:&egui::Context){
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(15);
        while self.project_io_busy(){assert!(std::time::Instant::now()<deadline,"Project worker timed out");self.poll_project_io(ctx);std::thread::sleep(std::time::Duration::from_millis(1));}
    }
}
