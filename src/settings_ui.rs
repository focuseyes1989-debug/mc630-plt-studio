impl A4Studio {
    pub fn open_settings(&mut self) {
        self.show_settings = true;
    }
    pub fn display_unit(&self) -> Unit {
        self.unit
    }
    fn preferences(&self) -> Preferences {
        let mut layout = self.layout.clone();
        layout.custom.clear();
        layout.hidden_images.clear();
        layout.guides = Default::default();
        layout.manual_cut_paths.clear();
        layout.use_manual_cut_paths = false;
        if !layout.image_tiles.is_empty() {
            layout.image_tiles.clear();
            layout.count = 1;
        }
        Preferences {
            reopen_last_project: self.reopen_last_project,
            unit: self.unit,
            layout,
            ccd: self.ccd_trial,
            profile: self.ccd_profile.clone(),
            cut_shape: self.cut_shape,
            remove_white: self.remove_white,
            threshold: self.threshold,
            background_tolerance: self.background_tolerance,
            print_without_background: self.print_without_background,
            rulers: self.rulers,
            grid: self.grid,
            show_cut: self.show_cut,
            lock: self.lock,
            preview_origin: self.preview_mark_origin,
            nudge_fine: self.nudge_fine,
            nudge_coarse: self.nudge_coarse,
            zoom: self.zoom,
            contour_rgba: self.contour_color.to_array(),
            autosave_enabled: self.autosave_enabled,
            autosave_seconds: self.autosave_seconds,
        }
    }
    fn apply_preferences(&mut self, p: Preferences) {
        self.reopen_last_project = p.reopen_last_project;
        self.autosave_enabled = p.autosave_enabled;
        self.autosave_seconds = p.autosave_seconds;
        self.unit = p.unit;
        self.layout = p.layout;
        self.layout.custom.clear();
        self.ccd_trial = p.ccd;
        self.ccd_profile = p.profile;
        self.cut_shape = p.cut_shape;
        self.remove_white = p.remove_white;
        self.threshold = p.threshold;
        self.background_tolerance = p.background_tolerance;
        self.print_without_background = p.print_without_background;
        self.rulers = p.rulers;
        self.grid = p.grid;
        self.show_cut = p.show_cut;
        self.lock = p.lock;
        self.preview_mark_origin = p.preview_origin;
        self.nudge_fine = p.nudge_fine;
        self.nudge_coarse = p.nudge_coarse;
        self.ratio = self.layout.width / self.layout.height;
        self.zoom = p.zoom;
        self.contour_color = if p.contour_rgba == [0, 150, 100, 255] {
            egui::Color32::RED
        } else {
            egui::Color32::from_rgba_unmultiplied(
                p.contour_rgba[0],
                p.contour_rgba[1],
                p.contour_rgba[2],
                p.contour_rgba[3],
            )
        };
        self.new_width = self.layout.page_width;
        self.new_height = self.layout.page_height;
    }
    pub fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let mut close = false;
        let mut retrace = false;
        const SECTIONS: [&str; 9] = [
            "General",
            "Auto Save",
            "Paper",
            "Layout",
            "Artwork",
            "Cut Outline",
            "Registration",
            "Canvas",
            "Printing",
        ];
        let tab_id = egui::Id::new("studio-settings-section");
        let mut tab = ctx
            .data(|data| data.get_temp::<usize>(tab_id).unwrap_or(0))
            .min(SECTIONS.len() - 1);
        egui::Window::new("Settings")
            .open(&mut open)
            .default_width(560.)
            .min_width(480.)
            .collapsible(false)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(126., 0.),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                        ui.spacing_mut().item_spacing.y = 4.;
                        for (index, name) in SECTIONS.into_iter().enumerate() {
                            let button = egui::Button::new(name)
                                .selected(tab == index)
                                .corner_radius(crate::ui_theme::BUTTON_RADIUS);
                            if ui.add_sized([126., 29.], button).clicked() {
                                tab = index;
                            }
                        }
                        },
                    );
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_width(300.);
                        ui.heading(SECTIONS[tab]);
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .id_salt(("studio-settings-content", tab))
                            .max_height((ctx.screen_rect().height() - 150.).clamp(220., 420.))
                            .show(ui, |ui| match tab {
0 => {
ui.label("Measurement units");
egui::ComboBox::from_id_salt("settings-unit").selected_text(match self.unit{Unit::Inches=>"Inches (default)",Unit::Cm=>"Centimetres",Unit::Mm=>"Millimetres"}).show_ui(ui,|ui|{
                        ui.selectable_value(&mut self.unit,Unit::Inches,"Inches (in) - default");ui.selectable_value(&mut self.unit,Unit::Cm,"Centimetres (cm)");ui.selectable_value(&mut self.unit,Unit::Mm,"Millimetres (mm)");
                    });
                    ui.checkbox(&mut self.reopen_last_project,"Reopen all project tabs on startup");
                    if icon_button(ui, Icon::Open, "Set Studio as default for .mc630"){
                        self.status=match std::env::current_exe().map_err(|e|e.to_string()).and_then(|exe|crate::file_association::configure_default(&exe)){Ok(())=>"Project icon registered. In Windows Default apps, choose MC630 PLT Studio for .mc630 files.".into(),Err(e)=>format!("Could not register project files: {e}")};
                    }
                    ui.horizontal(|ui|{ui.label("Arrow key step");ui.add(self.unit.distance(&mut self.nudge_fine).range(0.001..=100.));});
                    ui.horizontal(|ui|{ui.label("Shift + Arrow step");ui.add(self.unit.distance(&mut self.nudge_coarse).range(0.001..=100.));});
}
1 => {
ui.checkbox(&mut self.autosave_enabled,"Enable Auto Save recovery");
                    ui.horizontal(|ui|{ui.label("Maximum interval");ui.add(egui::DragValue::new(&mut self.autosave_seconds).range(5..=300).suffix(" seconds"));});
                    ui.small(format!("Recovery folder: {}",crate::recovery::root().display()));
                    if let Some(at)=self.autosave.succeeded_at{ui.small(format!("Last successful Auto Save: {} seconds ago",at.elapsed().as_secs()));}
                    if let Some(problem)=&self.autosave.problem{ui.colored_label(egui::Color32::from_rgb(180,60,30),problem);}
}
2 => {
egui::ComboBox::from_id_salt("settings-paper").selected_text(self.layout.paper_label()).show_ui(ui,|ui|{
                        for &(name,w,h) in sheet::PAPER_PRESETS {
                            if ui.selectable_label(false,format!("{name}  {}",self.unit.size(w,h))).clicked(){self.layout.page_width=w;self.layout.page_height=h;}
                        }
                    });
                    ui.horizontal(|ui|{if icon_button(ui, Icon::Swap, "Swap portrait / landscape"){std::mem::swap(&mut self.layout.page_width,&mut self.layout.page_height);}});
                    ui.horizontal(|ui|{ui.label("Paper width");ui.add(self.unit.distance(&mut self.layout.page_width).range(50.0..=630.));});
                    ui.horizontal(|ui|{ui.label("Paper height");ui.add(self.unit.distance(&mut self.layout.page_height).range(50.0..=2000.));});
}
3 => {
egui::Grid::new("settings-layout").num_columns(2).show(ui,|ui| {
                        ui.label("Image width");ui.add(self.unit.distance(&mut self.layout.width).range(1.0..=self.layout.page_width));ui.end_row();
                        ui.label("Image height");ui.add(self.unit.distance(&mut self.layout.height).range(1.0..=self.layout.page_height));ui.end_row();
                        ui.label("Copies");ui.add_enabled(self.layout.image_tiles.is_empty(), egui::DragValue::new(&mut self.layout.count).range(1..=1000));ui.end_row();
                        ui.label("Column spacing");ui.add(self.unit.distance(&mut self.layout.column_gap).range(0.0..=100.));ui.end_row();
                        ui.label("Row spacing");ui.add(self.unit.distance(&mut self.layout.row_gap).range(0.0..=100.));ui.end_row();
                        self.edit_margin_fields(ui);
                    });
                    ui.checkbox(&mut self.lock,"Lock proportions for resize");
}
4 => {
egui::ComboBox::from_id_salt("settings-shape").selected_text(self.cut_shape.label()).show_ui(ui,|ui| {
                        for shape in [sheet::CutShape::AlphaOutline,sheet::CutShape::ImageFrame,sheet::CutShape::PlainBackgroundObject,sheet::CutShape::Ellipse] {retrace|=ui.selectable_value(&mut self.cut_shape,shape,shape.label()).changed();}
                    });
                    retrace|=ui.checkbox(&mut self.remove_white,"Remove white background").changed();
                    retrace|=ui.add(egui::Slider::new(&mut self.threshold,180..=254).text("White threshold")).changed();
                    retrace|=ui.add(egui::Slider::new(&mut self.background_tolerance,0..=100).text("Background tolerance")).changed();
                    retrace|=ui.checkbox(&mut self.print_without_background,"Print without background").changed();
}
5 => {
ui.horizontal(|ui|{ui.label("Outline smoothing");ui.add(self.unit.slider(&mut self.layout.smoothing,0.0..=4.0));});
                    ui.horizontal(|ui|{ui.label("Outline distance");ui.add(self.unit.distance(&mut self.layout.contour_offset).range(-5.0..=5.0));});
                    ui.checkbox(&mut self.layout.cut_inner_holes,"Cut inner holes");ui.checkbox(&mut self.show_cut,"Show cut outlines");
}
6 => {
ui.checkbox(&mut self.ccd_trial,"CCD camera - four dots");
                    ui.horizontal(|ui|{ui.label("Dot diameter");ui.add(self.unit.distance(&mut self.ccd_profile.diameter).range(1.0..=10.0));});
                    ui.horizontal(|ui|{ui.label("Dot centre gap");ui.add(self.unit.distance(&mut self.ccd_profile.inset).range(5.0..=30.0));});
                    ui.add_enabled_ui(!self.ccd_trial,|ui| {
                        ui.horizontal(|ui|{ui.label("X offset");ui.add(self.unit.distance(&mut self.layout.offset_x).range(-self.layout.page_width..=self.layout.page_width));});
                        ui.horizontal(|ui|{ui.label("Y offset");ui.add(self.unit.distance(&mut self.layout.offset_y).range(-self.layout.page_height..=self.layout.page_height));});
                        ui.checkbox(&mut self.layout.flip_x,"Flip X");ui.checkbox(&mut self.layout.flip_y,"Flip Y");
                        ui.horizontal(|ui|{ui.label("Plotter units per mm");ui.add(egui::DragValue::new(&mut self.layout.units).range(1.0..=1000.));});
                    });
                    ui.checkbox(&mut self.preview_mark_origin,"Preview origin at registration mark");
}
7 => {
ui.checkbox(&mut self.rulers,"Show rulers");ui.checkbox(&mut self.grid,"Show grid");
                    ui.add(egui::Slider::new(&mut self.zoom,0.5..=16.0).custom_formatter(|v,_|format!("{:.0}%",v*100.)).text("Zoom"));
                    ui.horizontal(|ui| { ui.label("Cut line color"); ui.color_edit_button_srgba(&mut self.contour_color); });
                    if icon_button(ui, Icon::Fit, "Fit page / reset view"){self.zoom=1.;self.canvas_pan=egui::Vec2::ZERO;}
}
8 => {
if icon_button(ui, Icon::Print, "Open Print dialog") {
    close = true;
    self.show_print_dialog = true;
}
}
                                _ => {}
                            });
                    });
                });
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Close, "Close settings") {
                        close = true;
                    }
                    if icon_button(ui, Icon::Save, "Save settings") {
                        self.status = match self.preferences().save() {
                            Ok(()) => "Settings saved.".into(),
                            Err(error) => format!("Cannot save settings: {error}"),
                        };
                    }
                });
            });
        ctx.data_mut(|data| data.insert_temp(tab_id, tab));
        if retrace && !self.source.is_empty() {
            self.retrace(ctx);
        }
        self.show_settings = open && !close;
    }
}
