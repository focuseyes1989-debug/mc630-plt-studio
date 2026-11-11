impl A4Studio {
    pub fn select_new_paper(&mut self, preset: usize) {
        self.new_preset = preset;
        if let Some(&(_, width, height)) = sheet::PAPER_PRESETS.get(preset) {
            (self.new_width, self.new_height) = if self.new_landscape {
                (height, width)
            } else {
                (width, height)
            };
        }
    }

    pub fn home_workspace(&mut self, ctx: &egui::Context, enabled: bool) -> Option<usize> {
        let mut preset = None;
        egui::SidePanel::left("home-actions")
            .exact_width(198.)
            .resizable(false)
            .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(14))
            .show(ctx, |ui| {
                ui.heading("Studio");
                ui.add_space(12.);
                ui.add_enabled_ui(enabled, |ui| {
                    if ui
                        .add_sized(
                            [ui.available_width(), 34.],
                            egui::Button::new("New document"),
                        )
                        .on_hover_text("Ctrl+N")
                        .clicked()
                    {
                        self.request_action(DocumentAction::New, ctx);
                    }
                    if ui
                        .add_sized(
                            [ui.available_width(), 34.],
                            egui::Button::new("Open project…"),
                        )
                        .on_hover_text("Ctrl+Shift+O")
                        .clicked()
                    {
                        self.choose_project(ctx);
                    }
                    ui.separator();
                    self.recent_projects_menu(ui, ctx);
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(&ctx.style())
                    .fill(crate::ui_theme::CANVAS)
                    .inner_margin(egui::Margin::symmetric(26, 22)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading(egui::RichText::new("MC630 PLT Studio").size(28.));
                    ui.add_space(16.);
                    ui.strong("Paper presets");
                    ui.add_enabled_ui(enabled, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for name in ["A4", "A3", "A5", "Letter", "Legal"] {
                                let Some(index) = sheet::PAPER_PRESETS
                                    .iter()
                                    .position(|paper| paper.0 == name)
                                else {
                                    continue;
                                };
                                let (_, width, height) = sheet::PAPER_PRESETS[index];
                                if paper_card(
                                    ui,
                                    name,
                                    &format!("{width} × {height} mm"),
                                    Some([width, height]),
                                ) {
                                    preset = Some(index);
                                }
                            }
                            if paper_card(ui, "Custom size", "", None) {
                                preset = Some(sheet::PAPER_PRESETS.len());
                            }
                        });
                        ui.add_space(20.);
                        let height = ui.available_height().clamp(130., 330.);
                        if ui
                            .add_sized(
                                [ui.available_width(), height],
                                egui::Button::new("Drop an image here or click to import")
                                    .fill(egui::Color32::from_rgb(226, 226, 229)),
                            )
                            .on_hover_text("Import PNG or JPEG (Ctrl+O)")
                            .clicked()
                        {
                            self.choose_image(ctx);
                        }
                    });
                });
            });
        if enabled {
            self.handle_dropped_artwork(ctx);
            if ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::O)
            }) {
                self.choose_project(ctx);
            } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::O))
            {
                self.choose_image(ctx);
            }
        }
        preset
    }

    fn navigator(&mut self, ui: &mut egui::Ui) {
        let (area, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 130.),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 5., crate::ui_theme::CANVAS);
        let width = self.layout.page_width as f32;
        let height = self.layout.page_height as f32;
        let scale = ((area.width() - 16.) / width)
            .min((area.height() - 12.) / height)
            .max(0.01);
        let page = egui::Rect::from_center_size(area.center(), egui::vec2(width, height) * scale);
        painter.rect_filled(page, 0., egui::Color32::WHITE);
        if let (Some(art), Some(texture), Ok(items)) =
            (&self.art, &self.texture, self.layout.items())
        {
            for (index, item) in items.into_iter().enumerate() {
                if self.layout.hidden_images.contains(&index) {
                    continue;
                }
                let rect = egui::Rect::from_min_size(
                    page.min + egui::vec2(item.x as f32, item.y as f32) * scale,
                    egui::vec2(item.width as f32, item.height as f32) * scale,
                );
                painter.image(
                    texture.id(),
                    rect,
                    self.image_uv(index, art),
                    egui::Color32::WHITE,
                );
            }
        }
        if let Some((canvas_page, viewport)) = self.navigator_view {
            let scale = page.width() / canvas_page.width();
            let view = egui::Rect::from_min_max(
                page.min + (viewport.min - canvas_page.min) * scale,
                page.min + (viewport.max - canvas_page.min) * scale,
            )
            .intersect(page);
            if view.is_positive() {
                painter.rect_stroke(
                    view,
                    0.,
                    egui::Stroke::new(1_f32, crate::ui_theme::ACCENT),
                    egui::StrokeKind::Inside,
                );
            }
            if (response.clicked() || response.dragged())
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let target = page.clamp(pointer);
                let canvas_point = canvas_page.min + (target - page.min) / scale;
                self.camera.cancel_zoom();
                self.canvas_pan += viewport.center() - canvas_point;
                ui.ctx().request_repaint();
            }
        }
        response.on_hover_text("Click or drag to reposition the canvas");
        ui.horizontal(|ui| {
            if icon_button(ui, Icon::Fit, "Fit page") {
                self.camera.fit(ui.ctx());
            }
            for (label, zoom) in [("100%", 1.), ("150%", 1.5), ("200%", 2.)] {
                if ui
                    .selectable_label((self.zoom - zoom).abs() < 0.001, label)
                    .clicked()
                {
                    self.camera.zoom_to(ui.ctx(), zoom);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Zoom");
            let mut percent = self.camera.target_zoom(self.zoom) * 100.;
            if ui
                .add(
                    egui::Slider::new(&mut percent, 50. ..=1600.)
                        .logarithmic(true)
                        .suffix("%")
                        .fixed_decimals(0),
                )
                .changed()
            {
                self.camera.zoom_to(ui.ctx(), percent / 100.);
            }
        });
    }
}

fn paper_card(ui: &mut egui::Ui, name: &str, size: &str, dimensions: Option<[f64; 2]>) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(112., 106.), egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name));
    let style = ui.style().interact(&response);
    let painter = ui.painter();
    painter.rect(
        rect,
        7.,
        style.weak_bg_fill,
        style.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let center = egui::pos2(rect.center().x, rect.top() + 32.);
    let color = ui.visuals().text_color();
    if let Some([width, height]) = dimensions {
        let scale = 45. / width.max(height);
        let paper = egui::Rect::from_center_size(
            center,
            egui::vec2((width * scale) as f32, (height * scale) as f32),
        );
        painter.rect_filled(
            paper.translate(egui::vec2(1., 2.)),
            2.,
            egui::Color32::from_black_alpha(24),
        );
        painter.rect(
            paper,
            2.,
            egui::Color32::WHITE,
            egui::Stroke::new(1_f32, egui::Color32::from_gray(205)),
            egui::StrokeKind::Inside,
        );
    } else {
        let pen = egui::Stroke::new(1.5_f32, color);
        painter.rect_stroke(
            egui::Rect::from_center_size(center, egui::vec2(31., 40.)),
            3.,
            pen,
            egui::StrokeKind::Inside,
        );
        painter.line_segment(
            [center - egui::vec2(6., 0.), center + egui::vec2(6., 0.)],
            pen,
        );
        painter.line_segment(
            [center - egui::vec2(0., 6.), center + egui::vec2(0., 6.)],
            pen,
        );
    }
    painter.text(
        egui::pos2(center.x, rect.top() + 67.),
        egui::Align2::CENTER_CENTER,
        name,
        egui::FontId::proportional(13.),
        color,
    );
    if !size.is_empty() {
        painter.text(
            egui::pos2(center.x, rect.top() + 86.),
            egui::Align2::CENTER_CENTER,
            size,
            egui::FontId::proportional(11.),
            ui.visuals().weak_text_color(),
        );
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}
