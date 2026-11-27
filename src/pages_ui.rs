// Inactive pages retain source data, not GPU textures. Preparing a page uses the
// same background worker as Open, so large documents do not block the UI.
#[derive(Default)]
struct PageHistory {
    undo: Vec<ObjectEdit>,
    redo: Vec<ObjectEdit>,
}
impl A4Studio {
    fn selected_print_pages(&self) -> Result<Vec<usize>, String> {
        match self.print_range_mode {
            0 => Ok(vec![self.active_page]),
            1 => Ok((0..=self.other_pages.len()).collect()),
            _ => crate::print_range::parse(&self.print_range, self.other_pages.len() + 1),
        }
    }
    #[cfg(windows)]
    fn prepare_print_pages(
        &self,
        pages: &[usize],
    ) -> Result<Vec<crate::native_print::PrintPage>, String> {
        use crate::native_print::{PrintArtwork, PrintPage};
        pages
            .iter()
            .map(|&page| {
                if page > self.other_pages.len() {
                    return Err("Requested page no longer exists.".into());
                }
                if page == self.active_page {
                    let ccd = self.ccd_trial.then(|| self.ccd_profile.clone());
                    let artwork = if self.print_back {
                        PrintArtwork::Back(self.back.data.clone())
                    } else if let Some(art) = &self.art {
                        PrintArtwork::Front {
                            art: art.clone(),
                            layout: self.layout.clone(),
                            ccd,
                        }
                    } else if !self.source.is_empty() {
                        return Err(format!("Page {} artwork is not ready to print.",page+1));
                    } else {
                        PrintArtwork::Design(self.layout.clone(), ccd)
                    };
                    Ok(PrintPage {
                        size: [self.layout.page_width, self.layout.page_height],
                        artwork,
                    })
                } else {
                    let index = if page < self.active_page {
                        page
                    } else {
                        page - 1
                    };
                    let project = self.other_pages[index].clone();
                    Ok(PrintPage {
                        size: [
                            project.settings.layout.page_width,
                            project.settings.layout.page_height,
                        ],
                        artwork: PrintArtwork::Project(Box::new(project), self.print_back),
                    })
                }
            })
            .collect()
    }
    fn document_status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("a4-status").show(ctx, |ui| {
            ui.add_space(1.);
            ui.horizontal(|ui| {
                ui.small("Status");
                ui.separator();
                ui.label(&self.status);
            });
            ui.add_space(1.);
        });
    }
    fn stored_page_signature(page: &crate::project::Project) -> u64 {
        use base64::Engine as _;
        use std::hash::{Hash, Hasher};
        let source = base64::engine::general_purpose::STANDARD
            .decode(&page.image_base64)
            .unwrap_or_default();
        let mut source_hash = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut source_hash);
        let digest = if source.is_empty() {
            0
        } else {
            source_hash.finish()
        };
        let mut prefs = page.settings.clone();
        prefs.zoom = 1.;
        prefs.reopen_last_project = false;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        page.back.hash_signature(&mut hash);
        digest.hash(&mut hash);
        page.artwork_name.hash(&mut hash);
        page.aspect_ratio.to_bits().hash(&mut hash);
        serde_json::to_vec(&prefs)
            .unwrap_or_default()
            .hash(&mut hash);
        hash.finish()
    }
    fn request_page(&mut self, index: usize) {
        if index == self.active_page || index > self.other_pages.len() || self.tab_busy() {
            return;
        }
        let stored = if index < self.active_page {
            index
        } else {
            index - 1
        };
        self.pending_page = Some(index);
        self.io_job = Some(crate::project_io::Job::page(
            self.other_pages[stored].clone(),
        ));
    }
    fn finish_page_switch(&mut self, loaded: crate::project_io::Loaded, ctx: &egui::Context) {
        let Some(target) = self.pending_page.take() else {
            return;
        };
        let current = self.page_snapshot();
        let signature = self.current_page_signature();
        let mut pages = std::mem::take(&mut self.other_pages);
        let mut signatures = std::mem::take(&mut self.page_signatures);
        let mut histories = std::mem::take(&mut self.page_histories);
        histories.insert(
            self.active_page,
            PageHistory {
                undo: std::mem::take(&mut self.undo),
                redo: std::mem::take(&mut self.redo),
            },
        );
        let history = histories.remove(target);
        pages.insert(self.active_page, current);
        signatures.insert(self.active_page, signature);
        let old_active = self.active_page;
        let delete = std::mem::take(&mut self.delete_page_after_switch);
        let back = self.show_back;
        pages.remove(target);
        signatures.remove(target);
        let mut active = target;
        if delete {
            let removed = if old_active > target {
                old_active - 1
            } else {
                old_active
            };
            pages.remove(removed);
            signatures.remove(removed);
            histories.remove(removed);
            if old_active < target {
                active -= 1;
            }
        }
        self.install_project_prepared(
            loaded.project,
            loaded.source,
            ctx,
            loaded.art,
            loaded.preview,
            true,
        );
        self.back.install_previews(ctx, loaded.back_previews);
        self.other_pages = pages;
        self.page_signatures = signatures;
        self.active_page = active;
        self.page_histories = histories;
        self.undo = history.undo;
        self.redo = history.redo;
        self.show_back = back;
        self.print_back = back;
        self.status = format!("Page {} of {}", active + 1, self.other_pages.len() + 1);
        ctx.request_repaint();
    }
    fn add_page(&mut self, duplicate: bool) {
        if self.tab_busy() || self.other_pages.len() + 1 >= crate::project::MAX_PAGES {
            return;
        }
        let page = if duplicate {
            self.page_snapshot()
        } else {
            let mut prefs = self.preferences();
            prefs.layout = Layout {
                page_width: self.layout.page_width,
                page_height: self.layout.page_height,
                page_margins: Some(self.layout.margins()),
                units: self.layout.units,
                offset_x: -7.,
                offset_y: -7.,
                ..Default::default()
            };
            crate::project::Project::new("No image selected".into(), &[], prefs, [0., 0.])
        };
        // New pages follow the current one. Before switching, the active page is
        // represented separately, so its next page has the same stored index.
        let target = self.active_page + 1;
        self.page_signatures
            .insert(self.active_page, Self::stored_page_signature(&page));
        self.page_histories
            .insert(self.active_page, PageHistory::default());
        self.other_pages.insert(self.active_page, page);
        self.request_page(target);
    }
    fn delete_current_page(&mut self, _ctx: &egui::Context) {
        if self.other_pages.is_empty() || self.tab_busy() {
            return;
        }
        // Deletion is committed only after the replacement page loads correctly.
        let target = if self.active_page > 0 {
            self.active_page - 1
        } else {
            1
        };
        self.request_page(target);
        self.delete_page_after_switch = self.pending_page.is_some();
    }
    fn pages_bar(&mut self, ctx: &egui::Context) {
        let mut selected = self.active_page;
        let mut add = false;
        let mut duplicate = false;
        let mut delete = false;
        let count = self.other_pages.len() + 1;
        let enabled = !self.tab_busy()
            && self.cut_draft.is_none()
            && self.drag.is_none()
            && self.shape_group_drag.is_none()
            && self.design_drag.is_none()
            && self.cut_shape_drag.is_none()
            && !self.back.interacting()
            && !self.show_print_dialog;
        egui::TopBottomPanel::bottom("document-pages").show(ctx, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Pages");
                    if ui
                        .add_enabled(selected > 0, egui::Button::new("‹"))
                        .on_hover_text("Previous page")
                        .clicked()
                    {
                        selected -= 1;
                    }
                    egui::ComboBox::from_id_salt("document-page-selector")
                        .selected_text(format!("Page {} / {}", self.active_page + 1, count))
                        .width(110.)
                        .show_ui(ui, |ui| {
                            for page in 0..count {
                                ui.selectable_value(
                                    &mut selected,
                                    page,
                                    format!("Page {}", page + 1),
                                );
                            }
                        });
                    if ui
                        .add_enabled(selected + 1 < count, egui::Button::new("›"))
                        .on_hover_text("Next page")
                        .clicked()
                    {
                        selected += 1;
                    }
                    ui.separator();
                    ui.add_enabled_ui(count < crate::project::MAX_PAGES, |ui| {
                        add = icon_button(
                            ui,
                            Icon::New,
                            "Add blank page (same paper size and margins)",
                        );
                        duplicate = icon_button(
                            ui,
                            Icon::Duplicate,
                            "Duplicate page, including Main and Back artwork",
                        );
                    });
                    delete = ui
                        .add_enabled_ui(count > 1, |ui| {
                            icon_button(ui, Icon::Delete, "Delete current page")
                        })
                        .inner;
                })
            });
        });
        if selected != self.active_page {
            self.request_page(selected);
        } else if add || duplicate {
            self.add_page(duplicate);
        }
        if delete {
            self.delete_page_open = true;
        }
        if self.delete_page_open {
            let mut remove = false;
            let mut cancel = false;
            let modal =
                crate::ui_theme::dialog_modal(ctx, "delete-document-page").show(ctx, |ui| {
                    ui.set_width(340.);
                    cancel = crate::ui_theme::dialog_header(ui, "Delete page?", true);
                    ui.label(format!(
                        "Delete Page {} and its Main / Back artwork?",
                        self.active_page + 1
                    ));
                    ui.add_space(8.);
                    ui.separator();
                    crate::ui_theme::dialog_actions(ui, |ui| {
                        remove = crate::ui_theme::primary_button(ui, "Delete").clicked();
                        cancel |= ui
                            .add_sized([80., 28.], egui::Button::new("Cancel"))
                            .clicked();
                    });
                });
            if cancel || remove || modal.should_close() {
                self.delete_page_open = false;
            }
            if remove {
                self.delete_current_page(ctx);
            }
        }
    }
}

#[cfg(test)]
mod multipage_tests {
    use super::*;
    #[test]
    fn print_range_uses_requested_pages_and_the_selected_side_without_navigation() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::fresh_tab();
        app.name = "One".into();
        app.add_page(false);
        app.wait_project_io(&ctx);
        app.name = "Two".into();
        app.add_page(false);
        app.wait_project_io(&ctx);
        app.name = "Three".into();
        app.print_range_mode = 0;
        assert_eq!(app.selected_print_pages().unwrap(), vec![2]);
        app.print_range_mode = 1;
        assert_eq!(app.selected_print_pages().unwrap(), vec![0, 1, 2]);
        app.print_range_mode = 2;
        app.print_range = "3, 1-2, 1".into();
        let selected = app.selected_print_pages().unwrap();
        assert_eq!(selected, vec![2, 0, 1]);
        #[cfg(windows)]
        {
            use crate::native_print::PrintArtwork;
            app.source=include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
            app.retrace(&ctx);
            app.print_back = true;
            let pages = app.prepare_print_pages(&selected).unwrap();
            assert!(matches!(&pages[0].artwork, PrintArtwork::Back(_)));
            assert!(
                matches!(&pages[1].artwork,PrintArtwork::Project(project,true) if project.artwork_name=="One")
            );
            assert!(
                matches!(&pages[2].artwork,PrintArtwork::Project(project,true) if project.artwork_name=="Two")
            );
            app.print_back = false;
            assert!(matches!(
                &app.prepare_print_pages(&[1]).unwrap()[0].artwork,
                PrintArtwork::Project(_, false)
            ));
        }
        assert_eq!(app.active_page, 2);
        assert_eq!(app.name, "Three");
        app.print_range = "1, 4".into();
        assert!(app.selected_print_pages().is_err());
    }
    fn switch(app: &mut A4Studio, page: usize, ctx: &egui::Context) {
        app.request_page(page);
        app.wait_project_io(ctx);
        assert_eq!(app.active_page, page);
    }
    #[test]
    fn multipage_navigation_preserves_main_back_geometry_history_and_clean_state() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::fresh_tab();
        app.name = "First artwork".into();
        app.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.retrace(&ctx);
        app.back
            .data
            .images
            .push(crate::duplex::tests::image_item());
        app.drawing_mode = 2;
        app.commit_manual_cut(vec![[20., 20.], [40., 20.], [40., 40.], [20., 20.]]);
        let original = app.current_page_signature();
        let undo = app.undo.len();
        assert_eq!(
            original,
            A4Studio::stored_page_signature(&app.page_snapshot())
        );
        app.add_page(false);
        app.wait_project_io(&ctx);
        assert_eq!(app.active_page, 1);
        assert_eq!(app.other_pages.len(), 1);
        assert!(app.source.is_empty() && app.back.data.images.is_empty());
        assert!(app.layout.design_shapes.is_empty());
        app.name = "Second artwork".into();
        app.layout.page_width = 297.;
        app.layout.page_height = 420.;
        app.mark_saved();
        assert!(!app.is_dirty());
        switch(&mut app, 0, &ctx);
        assert_eq!(app.name, "First artwork");
        assert_eq!(app.current_page_signature(), original);
        assert_eq!(app.back.data.images.len(), 1);
        assert_eq!(app.layout.design_shapes.len(), 1);
        assert_eq!(app.undo.len(), undo);
        assert!(
            !app.is_dirty(),
            "Page navigation must not mark the project edited"
        );
        switch(&mut app, 1, &ctx);
        assert_eq!(app.layout.page_height, 420.);
        assert!(!app.is_dirty());
    }
    #[test]
    fn multipage_duplicate_and_delete_keep_order_and_never_remove_last_page() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::fresh_tab();
        app.name = "A".into();
        app.back
            .data
            .images
            .push(crate::duplex::tests::image_item());
        app.add_page(true);
        app.wait_project_io(&ctx);
        assert_eq!(app.back.data.images.len(), 1);
        app.name = "B".into();
        app.add_page(false);
        app.wait_project_io(&ctx);
        app.name = "C".into();
        switch(&mut app, 1, &ctx);
        app.delete_current_page(&ctx);
        app.wait_project_io(&ctx);
        assert_eq!(app.active_page, 0);
        assert_eq!(app.name, "A");
        assert_eq!(app.other_pages.len(), 1);
        switch(&mut app, 1, &ctx);
        assert_eq!(app.name, "C");
        switch(&mut app, 0, &ctx);
        app.delete_current_page(&ctx);
        app.wait_project_io(&ctx);
        assert_eq!(app.active_page, 0);
        assert_eq!(app.name, "C");
        assert!(app.other_pages.is_empty());
        app.delete_current_page(&ctx);
        assert!(!app.project_io_busy());
    }
    #[test]
    fn multipage_save_open_and_recovery_include_every_page_and_active_index() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::fresh_tab();
        app.name = "Front one".into();
        app.back
            .data
            .images
            .push(crate::duplex::tests::image_item());
        app.add_page(false);
        app.wait_project_io(&ctx);
        app.name = "Page two".into();
        let path = std::env::temp_dir().join(format!(
            "mc630-pages-{}-{}.mc630",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        app.io_job = Some(crate::project_io::Job::save(
            path.clone(),
            app.autosave_data(),
        ));
        app.wait_project_io(&ctx);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(json["version"], 2);
        let mut restored = A4Studio::fresh_tab();
        assert!(restored.open_tab_file(path.clone(), &ctx));
        restored.wait_project_io(&ctx);
        assert_eq!(restored.active_page, 1);
        assert_eq!(restored.name, "Page two");
        assert!(!restored.is_dirty());
        switch(&mut restored, 0, &ctx);
        assert_eq!(restored.name, "Front one");
        assert_eq!(restored.back.data.images.len(), 1);
        assert!(!restored.is_dirty());
        let recovery = app.autosave_data().record();
        recovery.project.recovery_source().unwrap();
        assert_eq!(recovery.project.active_page, 1);
        assert_eq!(recovery.project.other_pages[0].back.images.len(), 1);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn multipage_invalid_page_or_failed_switch_preserves_current_page() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::fresh_tab();
        app.name = "Keep".into();
        app.add_page(false);
        app.wait_project_io(&ctx);
        switch(&mut app, 0, &ctx);
        app.other_pages[0].image_base64 = "broken".into();
        app.request_page(1);
        app.wait_project_io(&ctx);
        assert_eq!(app.active_page, 0);
        assert_eq!(app.name, "Keep");
        assert!(app.pending_page.is_none());
        assert!(app.project_snapshot().source_untraced().is_err());
        let mut invalid = app.page_snapshot();
        invalid.active_page = 1;
        assert!(invalid.source_untraced().is_err());
        let mut nested = app.page_snapshot();
        nested.set_pages(vec![app.project_snapshot()], 0);
        assert!(nested.source_untraced().is_err());
    }
}
