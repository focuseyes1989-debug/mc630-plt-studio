#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crate::ui_theme::{Icon, icon_button};

use eframe::egui;

use mc630_plt_studio::{Drawing, Settings};

use std::path::PathBuf;

mod a4_ui;
mod canvas_camera;
mod duplex;
mod cut_tools;
mod file_association;
mod image_import;
mod palette_state;
mod photo_size;
mod preferences;
mod project;
mod project_io;
mod recent;
mod recovery;
mod ruler;
mod ui_theme;
mod update_install;
mod updater;
mod workspace_session;

#[cfg(windows)]
mod native_print;

#[derive(Clone, Copy)]
enum TabCommand {
    Save,
    Close,
    CloseOthers,
    CloseAll,
}

struct SaveAll {
    next: usize,
    saved: usize,
    original: usize,
    prepared: bool,
}

struct Studio {
    a4: a4_ui::A4Studio,

    a4_mode: bool,
    tabs: Vec<Option<a4_ui::A4Studio>>,
    active_tab: usize,
    empty_workspace: bool,
    pending_new: Option<a4_ui::A4Studio>,
    closing_all: bool,
    close_tabs_only: bool,
    keep_tab: Option<usize>,
    exit_approved: bool,
    session: workspace_session::Store,
    restore_pending: bool,
    launch_requested: bool,
    update_resume: bool,
    update_cleanup: Option<PathBuf>,
    exit_active: Option<usize>,
    save_all: Option<SaveAll>,
    tab_drag: Option<usize>,
    palette: palette_state::Store,
    updates: updater::UpdateDialog,

    drawing: Drawing,

    settings: Settings,

    name: String,

    status: String,

    lock: bool,

    ratio: f64,
}

impl Default for Studio {
    fn default() -> Self {
        Self {
            a4: a4_ui::A4Studio::default(),

            a4_mode: true,
            tabs: vec![None],
            active_tab: 0,
            empty_workspace: true,
            pending_new: None,
            closing_all: false,
            close_tabs_only: false,
            keep_tab: None,
            exit_approved: false,
            session: Default::default(),
            restore_pending: true,
            launch_requested: false,
            update_resume: false,
            update_cleanup: None,
            exit_active: None,
            save_all: None,
            tab_drag: None,
            palette: Default::default(),
            updates: Default::default(),

            drawing: Drawing::square(),

            settings: Settings {
                width: 10.,

                height: 10.,

                units: 40.,

                mirror: false,

                rotate: false,
            },

            name: "10 mm calibration square".into(),

            status: "Open an SVG or PLT, adjust size, then export to your USB stick.".into(),

            lock: true,

            ratio: 1.,
        }
    }
}

impl Studio {
    fn load(&mut self, path: PathBuf) {
        let result = (|| {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;

            if bytes.len() > 20_000_000 {
                return Err("File too large (20 MB limit).".into());
            }

            if path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("plt"))
            {
                mc630_plt_studio::plt(
                    &String::from_utf8(bytes).map_err(|e| e.to_string())?,
                    self.settings.units,
                )
            } else {
                mc630_plt_studio::svg(&bytes)
            }
        })();

        match result {
            Ok(d) => {
                let b = d.bounds();

                let w = b[2] - b[0];

                let h = b[3] - b[1];

                if w <= 0. || h <= 0. {
                    self.status = "Drawing must have width and height.".into();

                    return;
                }

                self.ratio = w / h;

                self.settings.width = w;

                self.settings.height = h;

                self.settings.rotate = false;

                self.settings.mirror = false;

                self.name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into();

                self.drawing = d;

                self.status = "Loaded. Check dimensions and path preview before exporting.".into();
            }

            Err(e) => self.status = format!("Import failed: {e}"),
        }
    }
}

impl Studio {
    fn begin_update_if_ready(&mut self, ctx: &egui::Context) {
        if self.updates.wants_install()
            && !self.closing_all
            && self.save_all.is_none()
            && !self.a4.tab_busy()
        {
            self.updates.begin_install();
            self.exit_active = Some(self.active_tab);
            self.close_tabs_only = false;
            self.keep_tab = None;
            self.closing_all = true;
            self.a4.request_tab_close(ctx);
        }
    }
    fn cancel_close_all_if_needed(&mut self) {
        if self.a4.close_canceled {
            self.updates.cancel_install();
            self.close_tabs_only = false;
            self.keep_tab = None;
            self.closing_all = false;
            self.a4.close_canceled = false;
            self.exit_active = None;
            self.a4.tab_closed = false;
            for tab in self.tabs.iter_mut().flatten() {
                tab.tab_closed = false;
            }
        }
    }
    fn tick_save_all_with(
        &mut self,
        ctx: &egui::Context,
        mut save: impl FnMut(&mut a4_ui::A4Studio) -> Result<(), String>,
    ) {
        let Some(mut run) = self.save_all.take() else {
            return;
        };
        if self.a4.tab_busy() || self.closing_all {
            self.save_all = Some(run);
            return;
        }
        if !run.prepared {
            self.switch_tab(run.next);
            run.prepared = true;
            self.save_all = Some(run);
            ctx.request_repaint();
            return;
        }
        let result=save(&mut self.a4);
        if self.a4.project_io_busy() {self.save_all=Some(run);return;}
        match result {
            Ok(()) => {
                run.saved += 1;
                run.next += 1;
                run.prepared = false;
            }
            Err(error) => {
                self.a4.workspace_status(format!(
                    "Save All stopped: {} of {} projects saved. {error}",
                    run.saved,
                    self.tabs.len()
                ));
                self.persist_workspace();
                return;
            }
        }
        if run.next == self.tabs.len() {
            self.switch_tab(run.original);
            self.a4
                .workspace_status(format!("Save All complete: {} projects saved.", run.saved));
            self.persist_workspace();
        } else {
            self.save_all = Some(run);
            ctx.request_repaint();
        }
    }
    fn workspace_snapshot(&self) -> workspace_session::Session {
        let mut projects = Vec::new();
        let mut active = None;
        for index in 0..self.tabs.len() {
            let doc = if index == self.active_tab {
                &self.a4
            } else {
                self.tabs[index].as_ref().unwrap()
            };
            if let Some(path) = doc.project_file() {
                if index == self.exit_active.unwrap_or(self.active_tab) {
                    active = Some(path.clone());
                }
                projects.push(path);
            }
        }
        workspace_session::Session { projects, active }
    }
    fn persist_workspace(&mut self) {
        if let Err(error) = self.session.persist(self.workspace_snapshot()) {
            self.a4
                .workspace_status(format!("Could not save open project tabs: {error}"));
        }
    }
    fn restore_workspace(&mut self, ctx: &egui::Context) {
        if !self.restore_pending
            || self.a4.tab_busy()
            || !self.a4.startup_finished()
            || self.a4.workspace_action.is_some()
            || self.closing_all
        {
            return;
        }
        self.restore_pending = false;
        if !self.a4.reopen_tabs_enabled() && !self.update_resume {
            return;
        }
        self.update_resume = false;
        let Some(session) = self.session.saved.clone() else {
            return;
        };
        let priority =
            if self.launch_requested || !self.a4.blank_tab() && self.a4.project_file().is_none() {
                Some(self.active_tab)
            } else {
                None
            };
        let mut selected = None;
        let mut failed = Vec::new();
        for path in session.projects {
            let existing = if self.a4.same_project(&path) {
                Some(self.active_tab)
            } else {
                self.tabs
                    .iter()
                    .position(|t| t.as_ref().is_some_and(|d| d.same_project(&path)))
            };
            let index = if let Some(index) = existing {
                index
            } else {
                let mut next = a4_ui::A4Studio::fresh_tab();
                if !next.open_tab_file(path.clone(), ctx) {
                    failed.push(path.display().to_string());
                    continue;
                }
                if self.a4.blank_tab() && !self.a4.project_io_busy() {
                    self.a4 = next;
                } else {
                    self.add_tab(next);
                }
                self.active_tab
            };
            if session
                .active
                .as_ref()
                .is_some_and(|a| workspace_session::same_path(a, &path))
            {
                selected = Some(index);
            }
        }
        if let Some(index) = priority.or(selected) {
            self.switch_tab(index);
        }
        if !failed.is_empty() {
            self.a4.workspace_status(format!(
                "Some project tabs could not be reopened (missing or invalid): {}",
                failed.join(", ")
            ));
        }
    }
    fn switch_tab(&mut self, index: usize) {
        if index == self.active_tab {
            return;
        }
        if let Some(next) = self.tabs[index].take() {
            let previous = std::mem::replace(&mut self.a4, next);
            self.tabs[self.active_tab] = Some(previous);
            self.active_tab = index;
        }
    }
    fn add_tab(&mut self, mut next: a4_ui::A4Studio) {
        if self.empty_workspace && self.a4.blank_tab() && !self.a4.project_io_busy() {
            next.set_reopen_tabs(self.a4.reopen_tabs_enabled());
            self.a4 = next;
            self.empty_workspace = false;
            return;
        }
        self.empty_workspace = false;
        next.set_reopen_tabs(self.a4.reopen_tabs_enabled());
        let previous = std::mem::replace(&mut self.a4, next);
        self.tabs[self.active_tab] = Some(previous);
        self.active_tab = self.tabs.len();
        self.tabs.push(None);
    }
    fn handle_tabs(&mut self, ctx: &egui::Context) {
        if self.a4.tab_closed && self.closing_all {
            if let Some(index) = self
                .tabs
                .iter()
                .enumerate()
                .find(|(index, t)| {
                    Some(*index) != self.keep_tab && t.as_ref().is_some_and(|d| !d.tab_closed)
                })
                .map(|(index, _)| index)
            {
                self.switch_tab(index);
                self.a4.request_tab_close(ctx);
            } else {
                if self.close_tabs_only {
                    let kept = self.keep_tab.take();
                    if let Some(index) = kept {
                        self.switch_tab(index);
                    } else {
                        self.a4 = a4_ui::A4Studio::fresh_tab();
                        self.empty_workspace = true;
                    }
                    self.tabs = vec![None];
                    self.active_tab = 0;
                    self.exit_active = None;
                    self.closing_all = false;
                    self.close_tabs_only = false;
                    self.persist_workspace();
                    self.a4.workspace_status(
                        if kept.is_some() {
                            "Close Others complete."
                        } else {
                            "Close All complete. All project tabs are closed."
                        }
                        .into(),
                    );
                } else {
                    self.persist_workspace();
                    if let Err(error) = self.updates.launch_if_pending(&self.workspace_snapshot()) {
                        self.a4
                            .workspace_status(format!("Update postponed: {error}"));
                        self.a4.close_canceled = true;
                        self.cancel_close_all_if_needed();
                        return;
                    }
                    self.exit_approved = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        } else if self.a4.tab_closed {
            self.tabs.remove(self.active_tab);
            if self.tabs.is_empty() {
                self.a4 = a4_ui::A4Studio::fresh_tab();
                        self.empty_workspace = true;
                self.tabs.push(None);
                self.active_tab = 0;
            } else {
                self.active_tab = self.active_tab.min(self.tabs.len() - 1);
                self.a4 = self.tabs[self.active_tab].take().unwrap();
            }
        }
        if let Some(action) = self.a4.workspace_action.take() {
            match action {
                a4_ui::WorkspaceAction::CloseAll => {
                    if !self.closing_all && self.save_all.is_none() && !self.a4.tab_busy() {
                        self.closing_all = true;
                        self.close_tabs_only = true;
                        self.keep_tab = None;
                        self.exit_active = Some(self.active_tab);
                        self.a4.request_tab_close(ctx);
                    }
                }
                a4_ui::WorkspaceAction::VectorWorkspace => self.a4_mode = false,
                a4_ui::WorkspaceAction::SaveAll => {
                    if !self.closing_all && self.save_all.is_none() {
                        self.save_all = Some(SaveAll {
                            next: 0,
                            saved: 0,
                            original: self.active_tab,
                            prepared: false,
                        });
                    }
                }
                a4_ui::WorkspaceAction::Recover(next) => self.add_tab(*next),
                a4_ui::WorkspaceAction::New => {
                    let mut next = a4_ui::A4Studio::fresh_tab();
                    next.show_new_tab_dialog();
                    self.pending_new = Some(next);
                }
                a4_ui::WorkspaceAction::Open(path) => {
                    if self.a4.same_project(&path) {
                        return;
                    }
                    if let Some(index) = self
                        .tabs
                        .iter()
                        .position(|t| t.as_ref().is_some_and(|d| d.same_project(&path)))
                    {
                        self.switch_tab(index);
                    } else {
                        let mut next = a4_ui::A4Studio::fresh_tab();
                        if next.open_tab_file(path, ctx) {
                            self.add_tab(next);
                        } else {
                            self.a4.workspace_status(
                                "Could not open project. Check the file and try again.".into(),
                            );
                        }
                    }
                }
            }
        }
        if self.closing_all {
            ctx.request_repaint();
        }
    }
    fn tab_command(&mut self, index: usize, command: TabCommand, ctx: &egui::Context) {
        if self.a4.tab_busy()
            || self.closing_all
            || self.save_all.is_some()
            || index >= self.tabs.len()
        {
            return;
        }
        self.switch_tab(index);
        match command {
            TabCommand::Save => {
                self.a4.save_current_tab();
                self.persist_workspace();
            }
            TabCommand::Close => self.a4.request_tab_close(ctx),
            TabCommand::CloseAll => {
                self.a4.workspace_action = Some(a4_ui::WorkspaceAction::CloseAll);
                ctx.request_repaint();
            }
            TabCommand::CloseOthers => {
                if let Some(other) = (0..self.tabs.len()).find(|i| *i != index) {
                    self.keep_tab = Some(index);
                    self.exit_active = Some(index);
                    self.closing_all = true;
                    self.close_tabs_only = true;
                    self.switch_tab(other);
                    self.a4.request_tab_close(ctx);
                    ctx.request_repaint();
                }
            }
        }
    }
    fn tab_shortcuts(&mut self, ctx: &egui::Context) {
        if !self.a4_mode
            || self.a4.tab_navigation_blocked()
            || self.closing_all
            || self.save_all.is_some()
            || self.tab_drag.is_some()
        {
            return;
        }
        let direction = ctx.input_mut(|input| {
            if input.modifiers.alt {
                return 0;
            }
            if input.consume_key(
                egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                egui::Key::Tab,
            ) {
                -1
            } else if input.consume_key(egui::Modifiers::CTRL, egui::Key::Tab) {
                1
            } else {
                0
            }
        });
        if direction == 0 || self.tabs.len() < 2 {
            return;
        }
        let next = if direction < 0 {
            (self.active_tab + self.tabs.len() - 1) % self.tabs.len()
        } else {
            (self.active_tab + 1) % self.tabs.len()
        };
        self.switch_tab(next);
        if let Some(id) = ctx.memory(|memory| memory.focused()) {
            ctx.memory_mut(|memory| memory.surrender_focus(id));
        }
        ctx.memory_mut(|memory| memory.close_popup());
        self.persist_workspace();
        ctx.request_repaint();
    }
    fn reorder_tab(&mut self, from: usize, to: usize) {
        if from == to
            || from >= self.tabs.len()
            || to >= self.tabs.len()
            || self.a4.tab_busy()
            || self.closing_all
            || self.save_all.is_some()
        {
            return;
        }
        let moved = self.tabs.remove(from);
        self.tabs.insert(to, moved);
        self.active_tab = if self.active_tab == from {
            to
        } else if from < self.active_tab && to >= self.active_tab {
            self.active_tab - 1
        } else if from > self.active_tab && to <= self.active_tab {
            self.active_tab + 1
        } else {
            self.active_tab
        };
        self.persist_workspace();
    }
    fn tab_bar(&mut self, ctx: &egui::Context) {
        let mut selected = None;
        let mut close = None;
        let mut new = false;
        let mut context_command = None;
        let mut tab_rects = Vec::new();
        let busy = self.a4.tab_busy() || self.closing_all || self.save_all.is_some() || self.pending_new.is_some();
        let bar = egui::TopBottomPanel::top("project-tabs")
            .frame(egui::Frame::side_top_panel(&ctx.style()).fill(ui_theme::TAB_STRIP))
            .show(ctx, |ui| {
            egui::ScrollArea::horizontal()
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for index in 0..if self.empty_workspace { 0 } else { self.tabs.len() } {
                            let label = if index == self.active_tab {
                                self.a4.tab_label()
                            } else {
                                self.tabs[index].as_ref().unwrap().tab_label()
                            };
                            ui.push_id(index, |ui| {
                                let active = index == self.active_tab;
                                let fill = if active { ui.visuals().panel_fill } else { egui::Color32::TRANSPARENT };
                                let tab = egui::Frame::NONE.fill(fill).inner_margin(egui::Margin::symmetric(4, 1)).show(ui, |ui| {
                                  ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.;
                                    let response = ui.add_enabled(
                                        !busy,
                                        egui::Button::new(label)
                                            .frame(false)
                                            .sense(egui::Sense::click_and_drag()),
                                    );
                                    if response.drag_started_by(egui::PointerButton::Primary)
                                        && !busy
                                    {
                                        self.tab_drag = Some(index);
                                    }
                                    if response.clicked() {
                                        selected = Some(index);
                                    }
                                    let response = response
                                        .on_hover_cursor(egui::CursorIcon::Grab)
                                        .on_hover_text(
                                            "Drag to reorder; Ctrl+Tab / Ctrl+Shift+Tab to switch; right-click for actions",
                                        );
                                    response.context_menu(|ui| {
                                        ui.add_enabled_ui(!busy, |ui| {
                                            if ui.button("Save").clicked() {
                                                context_command = Some((index, TabCommand::Save));
                                                ui.close_menu();
                                            }
                                            ui.separator();
                                            if ui.button("Close").clicked() {
                                                context_command = Some((index, TabCommand::Close));
                                                ui.close_menu();
                                            }
                                            if ui
                                                .add_enabled(
                                                    self.tabs.len() > 1,
                                                    egui::Button::new("Close Others"),
                                                )
                                                .clicked()
                                            {
                                                context_command =
                                                    Some((index, TabCommand::CloseOthers));
                                                ui.close_menu();
                                            }
                                            if ui.button("Close All").clicked() {
                                                context_command =
                                                    Some((index, TabCommand::CloseAll));
                                                ui.close_menu();
                                            }
                                        });
                                    });
                                    if ui
                                        .add_enabled_ui(!busy, ui_theme::close_icon)
                                        .inner
                                        .on_hover_text("Close project (Ctrl+W)")
                                        .clicked()
                                    {
                                        close = Some(index);
                                    }
                                });
                                  });
                                if active {
                                    ui_theme::underline_tab(ui, tab.response.rect);
                                }
                                tab_rects.push(tab.response.rect);
                            });
                            ui.separator();
                        }
                        new = ui.add_enabled(!busy, egui::Button::new("+").frame(false))
                            .on_hover_text("New document (Ctrl+N)")
                            .clicked();
                    });
                });
        });
        if busy || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.tab_drag = None;
        }
        if let Some(from) = self.tab_drag {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            if let Some(pointer) = ctx.pointer_interact_pos() {
                if bar.response.rect.contains(pointer) && !tab_rects.is_empty() {
                    let slot = tab_rects
                        .iter()
                        .take_while(|r| pointer.x >= r.center().x)
                        .count();
                    let x = if slot < tab_rects.len() {
                        tab_rects[slot].left() - 3.
                    } else {
                        tab_rects.last().unwrap().right() + 3.
                    };
                    ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("tab-insertion"),
                    ))
                    .line_segment(
                        [
                            egui::pos2(x, bar.response.rect.top() + 2.),
                            egui::pos2(x, bar.response.rect.bottom() - 2.),
                        ],
                        egui::Stroke::new(3_f32, egui::Color32::from_rgb(0, 137, 118)),
                    );
                    if ctx.input(|i| i.pointer.button_released(egui::PointerButton::Primary)) {
                        let to = slot.saturating_sub(usize::from(from < slot));
                        self.reorder_tab(from, to.min(self.tabs.len() - 1));
                        selected = None;
                        close = None;
                        context_command = None;
                    }
                }
            }
            if !ctx.input(|i| i.pointer.primary_down()) {
                self.tab_drag = None;
            }
        }
        if let Some(index) = selected {
            self.switch_tab(index);
        }
        if let Some(index) = close {
            self.switch_tab(index);
            self.a4.request_tab_close(ctx);
        }
        if let Some((index, command)) = context_command {
            self.tab_command(index, command, ctx);
        }
        if new {
            let mut next = a4_ui::A4Studio::fresh_tab();
            next.show_new_tab_dialog();
            self.pending_new = Some(next);
        }
    }
}

impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.a4.poll_project_io(ctx);
        for tab in self.tabs.iter_mut().flatten(){tab.poll_project_io(ctx);}
        if self.a4.project_io_busy() {
            if ctx.input(|i|i.viewport().close_requested()){ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);}
            self.a4.project_io_progress(ctx);
            return;
        }
        if let Some(directory) = self.update_cleanup.take() {
            update_install::cleanup_success(directory);
        }
        if self.restore_pending && self.session.saved.is_some() {
            self.a4.suppress_single_project_startup();
        }
        self.a4.reopen_on_startup(ctx);
        self.a4.workspace_enabled = true;
        self.begin_update_if_ready(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.exit_approved {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.save_all = None;
            self.close_tabs_only = false;
            self.keep_tab = None;
            if !self.closing_all {
                self.exit_active = Some(self.active_tab);
            }
            self.closing_all = true;
            if !self.a4.tab_busy() {
                self.a4.request_tab_close(ctx);
            }
        }
        self.cancel_close_all_if_needed();
        self.handle_tabs(ctx);
        self.restore_workspace(ctx);
        self.tick_save_all_with(ctx, a4_ui::A4Studio::save_for_workspace);
        if !self.a4.blank_tab() { self.empty_workspace = false; }
        self.tab_shortcuts(ctx);
        if !self.restore_pending && !self.closing_all && !self.exit_approved {
            self.persist_workspace();
        }
        let reopen = self.a4.reopen_tabs_enabled();
        for tab in self.tabs.iter_mut().flatten() {
            tab.set_reopen_tabs(reopen);
        }
        if self.a4_mode
            && self.save_all.is_none()
            && !self.a4.tab_busy()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::W))
        {
            self.a4.request_tab_close(ctx);
        }
        for tab in self.tabs.iter_mut().flatten() {
            tab.tick_autosave(ctx);
        }
        self.a4.tick_autosave(ctx);
        let dropped = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));

        if let Some(p) = dropped.filter(|_| !self.a4_mode) {
            self.load(p);
        }

        if self.a4_mode {
            self.a4.apply_palette_state(self.palette.state);
            self.a4.document_menu(ctx, &mut self.updates, frame);
            self.a4.standard_toolbar(ctx, frame);
            if !self.empty_workspace && self.save_all.is_none() {
                self.a4.tool_options_bar(ctx);
            }
            self.tab_bar(ctx);
            if self.save_all.is_some() {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(format!(
                            "Saving all project tabs...\n{}",
                            self.a4.tab_label()
                        ));
                    });
                });
                return;
            }
            if let Some(mut next) = self.pending_new.take() {
                if next.new_document_dialog(ctx) {
                    next.apply_palette_state(self.palette.state);
                    self.add_tab(next);
                } else if next.new_document_dialog_open() {
                    self.pending_new = Some(next);
                }
            }
            if self.empty_workspace {
                if let Some(preset) = self.a4.home_workspace(ctx, self.pending_new.is_none()) {
                    let mut next = a4_ui::A4Studio::fresh_tab();
                    next.select_new_paper(preset);
                    next.show_new_tab_dialog();
                    self.pending_new = Some(next);
                    ctx.request_repaint();
                }
                if self.pending_new.is_none() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::N)) {
                    self.a4.workspace_action = Some(a4_ui::WorkspaceAction::New);
                }
                self.a4.settings_dialog(ctx);
            } else {
                self.a4.ui(ctx);
            }
            self.a4.web_image_dialog(ctx);
            if !ctx.input(|i| i.pointer.any_down()) {
                if let Err(error) = self.palette.persist(self.a4.palette_state()) {
                    self.a4
                        .workspace_status(format!("Could not save palette position: {error}"));
                }
            }
            self.updates.show(ctx);
            self.a4.unsaved_dialog(ctx);
            return;
        }

        egui::TopBottomPanel::top("vector-file-menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                if icon_button(ui, Icon::Outline, "Return to Print & Cut workspace") {
                    self.a4_mode = true;
                }
                ui.menu_button("File", |ui| {
                    if ui.button("Save All").clicked() {
                        self.a4.workspace_action = Some(a4_ui::WorkspaceAction::SaveAll);
                        ui.close_menu();
                    }
                    if ui.button("Close All").clicked() {
                        self.a4.workspace_action = Some(a4_ui::WorkspaceAction::CloseAll);
                        ui.close_menu();
                    }
                    if self.a4.recent_projects_menu(ui, ctx) {
                        self.a4_mode = true;
                    }
                    if ui.button("Settings...").clicked() {
                        self.a4.open_settings();
                        ui.close_menu();
                    }
                });
                self.updates.menu(ui, ctx);
            });
        });
        self.a4.settings_dialog(ctx);
        egui::SidePanel::left("tools").exact_width(300.).show(ctx,|ui|{

            ui.add_space(12.);if icon_button(ui, Icon::Open, "Open SVG / PLT"){if let Some(p)=rfd::FileDialog::new().add_filter("Vector designs",&["svg","plt"]).pick_file(){self.load(p);}}

            if icon_button(ui, Icon::Rectangle, "New 10 mm test square"){self.drawing=Drawing::square();self.settings.width=10.;self.settings.height=10.;self.name="10 mm calibration square".into();}

            ui.add_space(12.);ui.strong(&self.name);ui.label(format!("{} paths · {} points",self.drawing.paths.len(),self.drawing.paths.iter().map(Vec::len).sum::<usize>()));ui.separator();

            ui.label(format!("Output size ({})",self.a4.display_unit().label()));

            ui.horizontal(|ui|{ui.label("Width");if ui.add(self.a4.display_unit().distance(&mut self.settings.width).speed(0.1).range(0.1..=630.)).changed()&&self.lock{self.settings.height=self.settings.width/self.ratio;}});

            ui.horizontal(|ui|{ui.label("Length");if ui.add(self.a4.display_unit().distance(&mut self.settings.height).speed(0.1).range(0.1..=10000.)).changed()&&self.lock{self.settings.width=self.settings.height*self.ratio;}});

            if ui.checkbox(&mut self.lock,"Keep proportions").changed()&&self.lock{self.ratio=self.settings.width/self.settings.height;}

            ui.checkbox(&mut self.settings.mirror,"Mirror horizontally");

            if ui.checkbox(&mut self.settings.rotate,"Rotate 90 degrees").changed(){std::mem::swap(&mut self.settings.width,&mut self.settings.height);self.ratio=1./self.ratio;}

            ui.separator();ui.label("Calibration: plotter units per mm");ui.add(egui::DragValue::new(&mut self.settings.units).speed(0.1).range(1.0..=1000.));ui.small("40 is an initial assumption. Export the 10 mm square and measure it before production. For PLT import, set this value before opening.");

            ui.add_space(12.);if icon_button(ui, Icon::Export, "Export PLT"){match mc630_plt_studio::export(&self.drawing,&self.settings){Ok(text)=>{if let Some(p)=rfd::FileDialog::new().add_filter("Plotter file",&["plt"]).set_file_name("CUT001.plt").save_file(){let p=if p.extension().is_none(){p.with_extension("plt")}else{p};self.status=match std::fs::write(&p,text){Ok(())=>format!("Saved: {}",p.display()),Err(e)=>format!("Save failed: {e}")};}},Err(e)=>self.status=e}}

            ui.add_space(12.);ui.small("Profile matches supplied PLT structure: TB25,...;CT1;PA;PU/PD;!PG; The TB/CT values are retained as a compatibility template; their firmware-specific meaning is unverified.");

            for note in &self.drawing.notes{ui.add_space(8.);ui.small(note);}

            ui.add_space(12.);ui.label("SVG text: convert to paths first. Photos: trace to vectors first.");

        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add_space(5.);

            ui.label(&self.status);

            ui.add_space(5.);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("CUT PATH PREVIEW  ·  origin at lower left");

            let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());

            let painter = ui.painter_at(rect);

            painter.rect_filled(rect, 0., egui::Color32::from_gray(210));

            match mc630_plt_studio::transformed(&self.drawing, &self.settings) {
                Ok(paths) => {
                    let scale = ((rect.width() - 60.) / self.settings.width as f32)
                        .min((rect.height() - 60.) / self.settings.height as f32)
                        .max(0.001);

                    let size = egui::vec2(
                        self.settings.width as f32 * scale,
                        self.settings.height as f32 * scale,
                    );

                    let base = rect.center() + egui::vec2(-size.x / 2., size.y / 2.);

                    let map =
                        |p: [f64; 2]| base + egui::vec2(p[0] as f32 * scale, -p[1] as f32 * scale);

                    for path in paths {
                        painter.add(egui::Shape::line(
                            path.into_iter().map(map).collect(),
                            egui::Stroke::new(1.4_f32, egui::Color32::BLACK),
                        ));
                    }

                    painter.circle_filled(base, 4., egui::Color32::from_rgb(255, 190, 80));
                }

                Err(e) => {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        e,
                        egui::FontId::proportional(16.),
                        egui::Color32::LIGHT_RED,
                    );
                }
            }
        });
        self.updates.show(ctx);
        self.a4.unsaved_dialog(ctx);
    }
}

fn main() -> eframe::Result {
    if update_install::run_helper() {
        return Ok(());
    }
    let update_resume = update_install::resume();
    if std::env::args_os().any(|a| a == "--register-file-association") {
        let result = std::env::current_exe()
            .map_err(|e| e.to_string())
            .and_then(|exe| file_association::register(&exe));
        return result.map_err(|e| eframe::Error::AppCreation(Box::new(std::io::Error::other(e))));
    }
    let launch_project = file_association::project_argument(std::env::args_os().skip(1));
    let icon = image::load_from_memory(include_bytes!("../assets/studio-icon.png"))
        .expect("valid embedded Studio icon")
        .into_rgba8();
    let (width, height) = icon.dimensions();
    eframe::run_native(
        "MC630 PLT Studio",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_icon(egui::IconData {
                    rgba: icon.into_raw(),
                    width,
                    height,
                })
                .with_inner_size([1360., 900.])
                .with_min_inner_size([1080., 740.]),

            ..Default::default()
        },
        Box::new(move |cc| {
            ui_theme::apply(&cc.egui_ctx);

            let mut studio = Studio::default();
            if let Some(resume) = update_resume {
                studio.session.saved = Some(resume.session);
                studio.update_resume = true;
                studio.update_cleanup = resume.cleanup_directory;
            }
            if let Some(path) = launch_project {
                studio.launch_requested = true;
                studio.a4.queue_startup_project(path);
            }
            Ok(Box::new(studio))
        }),
    )
}

#[cfg(test)]
mod workspace_tests {
    fn wait_all_io(studio:&mut Studio,ctx:&egui::Context){
        studio.a4.wait_project_io(ctx);
        for tab in studio.tabs.iter_mut().flatten(){tab.wait_project_io(ctx);}
    }

    #[test]
    fn startup_has_no_document_and_new_waits_for_confirmation() {
        let ctx = egui::Context::default();
        let mut studio = Studio::default();
        assert!(studio.empty_workspace);
        studio.a4.workspace_action = Some(a4_ui::WorkspaceAction::New);
        studio.handle_tabs(&ctx);
        assert!(studio.empty_workspace);
        assert!(studio.pending_new.as_ref().unwrap().new_document_dialog_open());
        studio.pending_new = None; // Cancel leaves the workspace empty.
        assert!(studio.empty_workspace);
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        assert!(!studio.empty_workspace);
        assert_eq!(studio.tabs.len(), 1);
        studio.a4.request_tab_close(&ctx);
        studio.handle_tabs(&ctx);
        assert!(studio.empty_workspace);
    }
    #[test]
    fn canceling_update_close_keeps_all_dirty_project_tabs() {
        let ctx = egui::Context::default();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.a4 = a4_ui::A4Studio::fresh_tab();
        studio.a4.edit_workspace_fixture(21);
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        studio.a4.edit_workspace_fixture(22);
        studio.updates = updater::UpdateDialog::ready_for_test();
        studio.begin_update_if_ready(&ctx);
        assert!(studio.closing_all && studio.a4.tab_busy());
        studio.a4.choose_close_fixture("cancel", &ctx);
        studio.cancel_close_all_if_needed();
        assert!(!studio.closing_all && !studio.exit_approved);
        assert_eq!(studio.tabs.len(), 2);
        assert!(!studio.updates.wants_install());
        assert!(!studio.tabs[0].as_ref().unwrap().tab_closed);
    }
    #[test]
    fn failed_update_helper_start_reopens_tabs_instead_of_exiting() {
        let ctx = egui::Context::default();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.a4 = a4_ui::A4Studio::fresh_tab();
        studio.updates = updater::UpdateDialog::ready_for_test();
        studio.begin_update_if_ready(&ctx);
        studio.handle_tabs(&ctx);
        assert!(!studio.exit_approved);
        assert!(!studio.closing_all);
        assert!(!studio.a4.tab_closed);
    }
    fn press_tab(studio: &mut Studio, ctx: &egui::Context, shift: bool) {
        let modifiers = if shift {
            egui::Modifiers::CTRL | egui::Modifiers::SHIFT
        } else {
            egui::Modifiers::CTRL
        };
        let _ = ctx.run(
            egui::RawInput {
                modifiers,
                events: vec![egui::Event::Key {
                    key: egui::Key::Tab,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            },
            |ctx| {
                studio.tab_shortcuts(ctx);
            },
        );
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Tab,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            },
            |_| {},
        );
    }
    #[test]
    fn ctrl_tab_cycles_in_current_order_wraps_and_preserves_edits() {
        let ctx = egui::Context::default();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        studio.a4.edit_workspace_fixture(3);
        studio.switch_tab(1);
        press_tab(&mut studio, &ctx, false);
        assert_eq!(studio.active_tab, 2);
        assert!(studio.a4.tab_label().ends_with(" *"));
        press_tab(&mut studio, &ctx, false);
        assert_eq!(studio.active_tab, 0);
        press_tab(&mut studio, &ctx, true);
        assert_eq!(studio.active_tab, 2);
        press_tab(&mut studio, &ctx, true);
        assert_eq!(studio.active_tab, 1);
        studio.reorder_tab(2, 0);
        press_tab(&mut studio, &ctx, false);
        assert_eq!(studio.active_tab, 0);
        assert!(studio.a4.tab_label().ends_with(" *"));
    }
    #[test]
    fn ctrl_tab_does_not_switch_during_unsaved_prompt_or_batch_save() {
        let ctx = egui::Context::default();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        studio.a4.edit_workspace_fixture(3);
        studio.a4.request_tab_close(&ctx);
        press_tab(&mut studio, &ctx, false);
        assert_eq!(studio.active_tab, 1);
        assert!(studio.a4.tab_busy());
        studio.a4.choose_close_fixture("cancel", &ctx);
        studio.cancel_close_all_if_needed();
        studio.save_all = Some(SaveAll {
            next: 0,
            saved: 0,
            original: 1,
            prepared: false,
        });
        press_tab(&mut studio, &ctx, true);
        assert_eq!(studio.active_tab, 1);
    }

    #[test]
    fn reordered_tabs_restore_in_order_with_the_same_active_document() {
        let root = std::env::temp_dir().join(format!("mc630-reorder-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let paths: Vec<_> = (0..3).map(|i| root.join(format!("{i}.mc630"))).collect();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        for (i, path) in paths.iter().enumerate() {
            let mut doc = a4_ui::A4Studio::fresh_tab();
            doc.save_workspace_fixture(path.clone());
            if i == 0 {
                studio.a4 = doc;
            } else {
                studio.add_tab(doc);
            }
        }
        studio.switch_tab(1);
        studio.reorder_tab(2, 0);
        assert_eq!(studio.active_tab, 2);
        assert_eq!(studio.a4.project_file(), Some(paths[1].clone()));
        studio.reorder_tab(2, 1);
        assert_eq!(studio.active_tab, 1);
        assert!(studio.tabs[1].is_none());
        assert_eq!(
            studio.workspace_snapshot().projects,
            vec![paths[2].clone(), paths[1].clone(), paths[0].clone()]
        );
        studio.reorder_tab(0, 2);
        assert_eq!(studio.active_tab, 0);
        assert_eq!(
            studio.workspace_snapshot().projects,
            vec![paths[1].clone(), paths[0].clone(), paths[2].clone()]
        );
        let mut reopened = Studio { empty_workspace: false, ..Studio::default() };
        reopened.a4 = a4_ui::A4Studio::fresh_tab();
        reopened.a4.set_reopen_tabs(true);
        reopened.session.saved = studio.session.saved.clone();
        reopened.restore_workspace(&egui::Context::default());
        wait_all_io(&mut reopened,&egui::Context::default());
        assert_eq!(reopened.workspace_snapshot(), studio.workspace_snapshot());
        for path in paths {
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn reordering_is_blocked_while_closing_and_preserves_unsaved_state() {
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        studio.a4.edit_workspace_fixture(3);
        studio.closing_all = true;
        studio.reorder_tab(1, 0);
        assert_eq!(studio.active_tab, 1);
        studio.closing_all = false;
        studio.reorder_tab(1, 0);
        assert_eq!(studio.active_tab, 0);
        assert!(studio.a4.tab_label().ends_with(" *"));
        assert!(studio.tabs[0].is_none());
        assert!(studio.tabs[1].is_some());
    }

    #[test]
    fn close_others_retains_target_tab_and_cancel_keeps_all_edits() {
        let ctx = egui::Context::default();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        studio.a4.edit_workspace_fixture(3);
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        studio.a4.edit_workspace_fixture(4);
        studio.tab_command(2, TabCommand::CloseOthers, &ctx);
        studio.handle_tabs(&ctx);
        assert_eq!(studio.active_tab, 1);
        assert!(studio.a4.tab_busy());
        studio.a4.choose_close_fixture("cancel", &ctx);
        studio.cancel_close_all_if_needed();
        studio.handle_tabs(&ctx);
        assert_eq!(studio.tabs.len(), 3);
        assert!(studio.keep_tab.is_none());
        studio.tab_command(2, TabCommand::CloseOthers, &ctx);
        studio.handle_tabs(&ctx);
        studio.a4.choose_close_fixture("discard", &ctx);
        studio.handle_tabs(&ctx);
        assert_eq!(studio.tabs.len(), 1);
        assert!(studio.a4.tab_label().ends_with(" *"));
        assert!(!studio.exit_approved);
        assert!(!studio.a4.tab_closed);
    }
    #[test]
    fn context_save_targets_clicked_tab_and_context_close_uses_unsaved_guard() {
        let ctx = egui::Context::default();
        let path =
            std::env::temp_dir().join(format!("mc630-context-save-{}.mc630", std::process::id()));
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.a4.save_workspace_fixture(path.clone());
        studio.a4.edit_workspace_fixture(3);
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        studio.tab_command(0, TabCommand::Save, &ctx);
        wait_all_io(&mut studio,&ctx);
        assert_eq!(
            project::Project::read(&path).unwrap().settings.layout.count,
            3
        );
        assert!(!studio.a4.tab_label().ends_with(" *"));
        studio.a4.edit_workspace_fixture(4);
        studio.switch_tab(1);
        studio.tab_command(0, TabCommand::Close, &ctx);
        assert!(studio.a4.tab_busy());
        assert!(!studio.a4.tab_closed);
        assert_eq!(studio.active_tab, 0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn close_all_keeps_app_open_and_clears_restored_session() {
        let ctx = egui::Context::default();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        studio.a4.workspace_action = Some(a4_ui::WorkspaceAction::CloseAll);
        for _ in 0..4 {
            studio.handle_tabs(&ctx);
        }
        assert!(!studio.exit_approved);
        assert!(!studio.closing_all);
        assert_eq!(studio.tabs.len(), 1);
        assert!(studio.a4.blank_tab());
        assert!(studio.session.saved.as_ref().unwrap().projects.is_empty());
    }
    #[test]
    fn close_all_cancel_retains_every_tab_and_discard_finishes() {
        let ctx = egui::Context::default();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        studio.a4.edit_workspace_fixture(3);
        studio.switch_tab(0);
        studio.a4.workspace_action = Some(a4_ui::WorkspaceAction::CloseAll);
        studio.handle_tabs(&ctx);
        studio.handle_tabs(&ctx);
        assert_eq!(studio.active_tab, 1);
        assert!(studio.a4.tab_busy());
        studio.a4.choose_close_fixture("cancel", &ctx);
        studio.cancel_close_all_if_needed();
        studio.handle_tabs(&ctx);
        assert!(!studio.closing_all);
        assert_eq!(studio.tabs.len(), 2);
        assert!(studio.a4.tab_label().ends_with(" *"));
        assert!(!studio.tabs[0].as_ref().unwrap().tab_closed);
        studio.a4.workspace_action = Some(a4_ui::WorkspaceAction::CloseAll);
        studio.handle_tabs(&ctx);
        studio.a4.choose_close_fixture("discard", &ctx);
        for _ in 0..3 {
            studio.handle_tabs(&ctx);
        }
        assert!(studio.a4.blank_tab());
        assert!(!studio.exit_approved);
        assert_eq!(studio.tabs.len(), 1);
    }
    #[test]
    fn close_all_save_writes_edits_before_closing() {
        let ctx = egui::Context::default();
        let path =
            std::env::temp_dir().join(format!("mc630-close-all-save-{}.mc630", std::process::id()));
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.a4.save_workspace_fixture(path.clone());
        studio.a4.edit_workspace_fixture(3);
        studio.a4.workspace_action = Some(a4_ui::WorkspaceAction::CloseAll);
        studio.handle_tabs(&ctx);
        assert!(studio.a4.tab_busy());
        studio.a4.choose_close_fixture("save", &ctx);
        wait_all_io(&mut studio,&ctx);
        studio.handle_tabs(&ctx);
        assert_eq!(
            project::Project::read(&path).unwrap().settings.layout.count,
            3
        );
        assert!(studio.a4.blank_tab());
        assert!(!studio.exit_approved);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn save_all_writes_every_project_and_restores_selected_tab() {
        let root = std::env::temp_dir().join(format!("mc630-save-all-ok-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.a4 = a4_ui::A4Studio::fresh_tab();
        let mut paths = Vec::new();
        for index in 0..3 {
            let mut doc = a4_ui::A4Studio::fresh_tab();
            let path = root.join(format!("{index}.mc630"));
            doc.save_workspace_fixture(path.clone());
            doc.edit_workspace_fixture(index + 1);
            if index == 0 {
                studio.a4 = doc;
            } else {
                studio.add_tab(doc);
            }
            paths.push(path);
        }
        studio.switch_tab(1);
        studio.save_all = Some(SaveAll {
            next: 0,
            saved: 0,
            original: 1,
            prepared: false,
        });
        let ctx = egui::Context::default();
        for _ in 0..30 {
            studio.tick_save_all_with(&ctx, a4_ui::A4Studio::save_for_workspace);
            wait_all_io(&mut studio,&ctx);
            if studio.save_all.is_none(){break;}
        }
        assert!(studio.save_all.is_none());
        assert_eq!(studio.active_tab, 1);
        for (index, path) in paths.iter().enumerate() {
            assert_eq!(
                project::Project::read(path).unwrap().settings.layout.count,
                index + 1
            );
            let doc = if index == studio.active_tab {
                &studio.a4
            } else {
                studio.tabs[index].as_ref().unwrap()
            };
            assert!(!doc.tab_label().ends_with(" *"));
        }
        for path in paths {
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn save_all_cancel_stops_at_that_tab_and_keeps_remaining_edits() {
        let ctx = egui::Context::default();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        studio.a4.edit_workspace_fixture(3);
        studio.save_all = Some(SaveAll {
            next: 0,
            saved: 0,
            original: 2,
            prepared: false,
        });
        let mut calls = 0;
        for _ in 0..8 {
            studio.tick_save_all_with(&ctx, |_| {
                calls += 1;
                if calls == 2 {
                    Err("Save canceled".into())
                } else {
                    Ok(())
                }
            });
        }
        assert_eq!(calls, 2);
        assert!(studio.save_all.is_none());
        assert_eq!(studio.active_tab, 1);
        assert!(studio.tabs[2].as_ref().unwrap().tab_label().ends_with(" *"));
        assert_eq!(studio.tabs.len(), 3);
    }

    #[test]
    fn session_restores_order_selection_and_exit_keeps_all_tabs() {
        let root = std::env::temp_dir().join(format!("mc630-session-tabs-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let a = root.join("first.mc630");
        let b = root.join("second.mc630");
        let missing = root.join("missing.mc630");
        let mut fixture = a4_ui::A4Studio::fresh_tab();
        fixture.save_workspace_fixture(a.clone());
        fixture.save_workspace_fixture(b.clone());
        let ctx = egui::Context::default();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.a4 = a4_ui::A4Studio::fresh_tab();
        studio.a4.set_reopen_tabs(true);
        studio
            .session
            .persist(workspace_session::Session {
                projects: vec![a.clone(), missing, b.clone()],
                active: Some(a.clone()),
            })
            .unwrap();
        studio.restore_workspace(&ctx);
        wait_all_io(&mut studio,&ctx);
        assert_eq!(studio.tabs.len(), 2);
        assert_eq!(studio.active_tab, 0);
        assert_eq!(
            studio.workspace_snapshot().projects,
            vec![a.clone(), b.clone()]
        );
        studio.closing_all = true;
        studio.exit_active = Some(0);
        studio.a4.request_tab_close(&ctx);
        studio.handle_tabs(&ctx);
        studio.handle_tabs(&ctx);
        assert!(studio.exit_approved);
        assert_eq!(
            studio.session.saved.as_ref().unwrap().projects,
            vec![a.clone(), b.clone()]
        );
        assert_eq!(
            studio.session.saved.as_ref().unwrap().active,
            Some(a.clone())
        );
        // An explicit tab close removes only that project from the next session.
        studio.closing_all = false;
        studio.exit_approved = false;
        studio.exit_active = None;
        for tab in studio.tabs.iter_mut().flatten() {
            tab.tab_closed = false;
        }
        studio.handle_tabs(&ctx);
        studio.persist_workspace();
        assert_eq!(studio.session.saved.as_ref().unwrap().projects.len(), 1);
        std::fs::remove_file(a).unwrap();
        std::fs::remove_file(b).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn disabled_restore_preserves_blank_startup() {
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.a4 = a4_ui::A4Studio::fresh_tab();
        studio.a4.set_reopen_tabs(false);
        studio
            .session
            .persist(workspace_session::Session {
                projects: vec![std::env::temp_dir().join("not-opened.mc630")],
                active: None,
            })
            .unwrap();
        studio.restore_workspace(&egui::Context::default());
        assert!(!studio.restore_pending);
        assert!(studio.a4.blank_tab());
        assert_eq!(studio.tabs.len(), 1);
    }

    use super::*;
    #[test]
    fn switching_and_closing_keeps_other_documents() {
        let ctx = egui::Context::default();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.a4.workspace_status("first".into());
        studio.add_tab(a4_ui::A4Studio::fresh_tab());
        assert_eq!(studio.active_tab, 1);
        assert!(studio.tabs[0].is_some());
        studio.switch_tab(0);
        assert_eq!(studio.active_tab, 0);
        assert!(studio.tabs[0].is_none());
        assert!(studio.tabs[1].is_some());
        studio.a4.request_tab_close(&ctx);
        studio.handle_tabs(&ctx);
        assert_eq!(studio.tabs.len(), 1);
        assert!(!studio.a4.tab_closed);
        studio.a4.request_tab_close(&ctx);
        studio.handle_tabs(&ctx);
        assert_eq!(studio.tabs.len(), 1);
        assert_eq!(studio.a4.tab_label(), "Untitled");
    }
    #[test]
    fn new_and_open_requests_leave_existing_tab_intact() {
        let ctx = egui::Context::default();
        let mut studio = Studio { empty_workspace: false, ..Studio::default() };
        studio.a4.workspace_action = Some(a4_ui::WorkspaceAction::New);
        studio.handle_tabs(&ctx);
        assert_eq!(studio.tabs.len(), 1);
        assert!(studio.pending_new.is_some());
        studio.a4.workspace_action =
            Some(a4_ui::WorkspaceAction::Open(PathBuf::from("missing.mc630")));
        studio.handle_tabs(&ctx);
        assert_eq!(studio.tabs.len(), 1);
        studio.pending_new = None;
        studio.closing_all = true;
        studio.a4.request_tab_close(&ctx);
        studio.handle_tabs(&ctx);
        studio.handle_tabs(&ctx);
        assert!(studio.exit_approved);
    }
}
