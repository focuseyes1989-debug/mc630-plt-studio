use eframe::egui;
use mc630_plt_studio::sheet::Layout;
use std::{collections::HashMap, path::PathBuf, sync::mpsc};

struct Preview {
    pixels: egui::ColorImage,
    layout: Layout,
    size: u64,
    dimensions: [u32; 2],
}
struct Card {
    texture: Option<egui::TextureHandle>,
    layout: Option<Layout>,
    size: u64,
    dimensions: [u32; 2],
    error: Option<String>,
}
#[derive(Default)]
pub struct Gallery {
    pub remove_requested: Option<PathBuf>,
    filter: String,
    list: bool,
    sort: usize,
    reverse: bool,
    cards: HashMap<PathBuf, Card>,
    pending: Option<(PathBuf, mpsc::Receiver<Result<Preview, String>>)>,
}
fn preview(path: &std::path::Path) -> Result<Preview, String> {
    use base64::Engine as _;
    let project = crate::project::Project::read(path)?;
    project.settings.layout.capacity()?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&project.image_base64)
        .map_err(|e| e.to_string())?;
    let image = if bytes.is_empty() {
        image::RgbaImage::new(1, 1)
    } else {
        mc630_plt_studio::sheet::decode_image(&bytes)?.into_rgba8()
    };
    let thumb = image::imageops::thumbnail(&image, 512, 512);
    let pixels = egui::ColorImage::from_rgba_unmultiplied(
        [thumb.width() as usize, thumb.height() as usize],
        thumb.as_raw(),
    );
    Ok(Preview {
        pixels,
        layout: project.settings.layout,
        size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        dimensions: [image.width(), image.height()],
    })
}
impl Gallery {
    pub fn invalidate(&mut self) {
        self.cards.clear();
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, entries: &[PathBuf], enabled: bool) -> Option<PathBuf> {
        let entries = &entries[..entries.len().min(6)];
        self.cards.retain(|path, _| entries.contains(path));
        if let Some((path, rx)) = &self.pending {
            if let Ok(result) = rx.try_recv() {
                let card = match result {
                    Ok(preview) => Card {
                        texture: Some(ui.ctx().load_texture(
                            format!("recent-{}", path.display()),
                            preview.pixels,
                            egui::TextureOptions::LINEAR,
                        )),
                        layout: Some(preview.layout),
                        size: preview.size,
                        dimensions: preview.dimensions,
                        error: None,
                    },
                    Err(error) => Card {
                        texture: None,
                        layout: None,
                        size: 0,
                        dimensions: [1, 1],
                        error: Some(error),
                    },
                };
                self.cards.insert(path.clone(), card);
                self.pending = None;
            }
        }
        self.cards.retain(|path, _| entries.contains(path));
        if self.pending.is_none()
            && let Some(path) = entries.iter().find(|p| !self.cards.contains_key(*p))
        {
            let path = path.clone();
            let job = path.clone();
            let ctx = ui.ctx().clone();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(|| preview(&job))
                    .unwrap_or_else(|_| Err("Preview unavailable".into()));
                let _ = tx.send(result);
                ctx.request_repaint();
            });
            self.pending = Some((path, rx));
        }
        ui.horizontal(|ui| {
            ui.heading("Recent");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::ui_theme::icon_toggle(
                    ui,
                    crate::ui_theme::Icon::Grid,
                    !self.list,
                    "Grid view",
                ) {
                    self.list = false;
                }
                if crate::ui_theme::icon_toggle(
                    ui,
                    crate::ui_theme::Icon::List,
                    self.list,
                    "List view",
                ) {
                    self.list = true;
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Sort");
            egui::ComboBox::from_id_salt("recent-sort")
                .width(90.)
                .selected_text(["Recent", "Name", "File size"][self.sort])
                .show_ui(ui, |ui| {
                    for (i, label) in ["Recent", "Name", "File size"].iter().enumerate() {
                        ui.selectable_value(&mut self.sort, i, *label);
                    }
                });
            if crate::ui_theme::icon_toggle(
                ui,
                crate::ui_theme::Icon::Swap,
                self.reverse,
                "Reverse sort order",
            ) {
                self.reverse = !self.reverse;
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter recent projects")
                    .desired_width(180.),
            );
        });
        ui.add_space(12.);
        let filter = self.filter.to_lowercase();
        let mut paths: Vec<_> = entries
            .iter()
            .filter(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&filter)
            })
            .collect();
        if self.sort == 1 {
            paths.sort_by_key(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase()
            });
        }
        if self.sort == 2 {
            paths.sort_by_key(|p| std::cmp::Reverse(self.cards.get(*p).map_or(0, |c| c.size)));
        }
        if self.reverse {
            paths.reverse();
        }
        if paths.is_empty() {
            ui.weak(if entries.is_empty() {
                "No recent projects"
            } else {
                "No matching projects"
            });
        }
        let mut selected = None;
        let mut removed = None;
        egui::ScrollArea::vertical()
            .id_salt("recent-project-scroll")
            .max_height(ui.available_height().max(0.))
            .auto_shrink([false, false])
            .show(ui, |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            if self.list {
                for path in &paths {
                    let (open, remove) = self.card(ui, path, true, 0.);
                    if open { selected = Some((*path).clone()); }
                    if remove { removed = Some((*path).clone()); }
                }
            } else {
                // A plain row avoids egui Grid retaining old column widths after resize.
                let card_width = ((ui.available_width() - 16. * 5.) / 6.).clamp(1., 176.);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.;
                    for path in &paths {
                        let (open, remove) = self.card(ui, path, false, card_width);
                        if open { selected = Some((*path).clone()); }
                        if remove { removed = Some((*path).clone()); }
                    }
                });
            }
        });
        });
        self.remove_requested = removed;
        selected
    }
    fn card(&self, ui: &mut egui::Ui, path: &PathBuf, list: bool, width: f32) -> (bool, bool) {
        let size = if list {
            egui::vec2(ui.available_width(), 72.)
        } else {
            egui::vec2(width, 210.)
        };
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect(
            rect,
            6.,
            if response.hovered() {
                egui::Color32::from_rgb(229, 241, 251)
            } else {
                egui::Color32::from_rgb(250, 250, 250)
            },
            egui::Stroke::new(1_f32, egui::Color32::from_gray(211)),
            egui::StrokeKind::Inside,
        );
        let preview = if list {
            egui::Rect::from_min_size(rect.min + egui::vec2(8., 6.), egui::vec2(60., 60.))
        } else {
            egui::Rect::from_min_size(rect.min + egui::vec2(8., 8.), egui::vec2((rect.width()-16.).max(1.), 155.))
        };
        if let Some(card) = self.cards.get(path)
            && let Some(layout) = &card.layout
        {
            let scale = (preview.width() / layout.page_width as f32)
                .min(preview.height() / layout.page_height as f32);
            let page = egui::Rect::from_center_size(
                preview.center(),
                egui::vec2(layout.page_width as f32, layout.page_height as f32) * scale,
            );
            painter.rect_filled(page, 0., egui::Color32::WHITE);
            if let Some(texture) = &card.texture
                && let Ok(items) = layout.editor_items()
            {
                let painter = painter.with_clip_rect(page.intersect(preview));
                for (index, item) in items.iter().enumerate() {
                    if layout.hidden_images.contains(&index) {
                        continue;
                    }
                    // Atlas coordinates are normalized using original source dimensions stored below.
                    let uv = if let Some(tile) = layout.image_tiles.get(index) {
                        let w = card.dimensions[0].max(1) as f32;
                        let h = card.dimensions[1].max(1) as f32;
                        egui::Rect::from_min_max(
                            egui::pos2(tile[0] as f32 / w, tile[1] as f32 / h),
                            egui::pos2(
                                (tile[0] + tile[2]) as f32 / w,
                                (tile[1] + tile[3]) as f32 / h,
                            ),
                        )
                    } else {
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.))
                    };
                    let r = egui::Rect::from_min_size(
                        page.min + egui::vec2(item.x as f32, item.y as f32) * scale,
                        egui::vec2(item.width as f32, item.height as f32) * scale,
                    );
                    super::a4_ui::paint_oriented_image(
                        &painter,
                        texture.id(),
                        r,
                        uv,
                        item.orientation,
                        egui::Color32::WHITE,
                    );
                }
            }
        } else {
            painter.text(
                preview.center(),
                egui::Align2::CENTER_CENTER,
                "MC630",
                egui::FontId::proportional(16.),
                egui::Color32::GRAY,
            );
        }
        let text_pos = if list {
            rect.min + egui::vec2(80., 18.)
        } else {
            rect.min + egui::vec2(10., 172.)
        };
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let mut job = egui::text::LayoutJob::simple(
            name.into_owned(),
            egui::FontId::proportional(13.),
            ui.visuals().text_color(),
            if list { rect.width() - 90. } else { (rect.width()-20.).max(1.) },
        );
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let galley = painter.layout_job(job);
        painter.galley(text_pos, galley, ui.visuals().text_color());
        let card = self.cards.get(path);
        let detail = if card.is_some_and(|c| c.error.is_some()) {
            "Preview unavailable".into()
        } else if let Some(c) = card {
            format!("{:.1} MB", c.size as f64 / 1_000_000.)
        } else {
            "Loading preview…".into()
        };
        painter.text(
            text_pos + egui::vec2(0., 21.),
            egui::Align2::LEFT_TOP,
            detail,
            egui::FontId::proportional(11.),
            ui.visuals().weak_text_color(),
        );
        let mut remove = false;
        response.context_menu(|ui| {
            if ui.button("Remove from Recent").clicked() { remove = true; ui.close_menu(); }
        });
        let close_rect = egui::Rect::from_min_size(rect.right_top() + egui::vec2(-25., 4.), egui::vec2(21.,21.));
        let close = ui.put(close_rect, egui::Button::new("×").frame(false))
            .on_hover_text("Remove from Recent");
        remove |= close.clicked();
        (response.on_hover_text(path.display().to_string()).clicked() && !remove, remove)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recent_grid_and_list_render_with_missing_file_and_filtered_results() {
        let ctx = egui::Context::default();
        let mut gallery = Gallery::default();
        let path = PathBuf::from("missing-gallery-test.mc630");
        gallery.cards.insert(
            path.clone(),
            Card {
                texture: None,
                layout: None,
                size: 0,
                dimensions: [1, 1],
                error: Some("Missing".into()),
            },
        );
        for list in [false, true] {
            gallery.list = list;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600., 500.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        assert!(gallery.ui(ui, std::slice::from_ref(&path), true).is_none());
                    });
                },
            );
            assert!(!output.shapes.is_empty());
        }
        gallery.filter = "no matches".into();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                gallery.ui(ui, std::slice::from_ref(&path), true);
            });
        });
    }
}

#[cfg(test)]
mod resize_tests {
    use super::*;
    #[test]
    fn six_cards_fit_after_maximize_and_restore_without_stretching() {
        let ctx=egui::Context::default();
        let mut gallery=Gallery::default();
        let paths:Vec<_>=(0..6).map(|i|PathBuf::from(format!("recent-{i}.mc630"))).collect();
        for path in &paths {gallery.cards.insert(path.clone(),Card{texture:None,layout:None,size:0,dimensions:[1,1],error:Some("Unavailable".into())});}
        for width in [850.,2300.,850.] {
            for _ in 0..2 {
                let output=ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(width,700.))),..Default::default()},|ctx| {
                    egui::CentralPanel::default().show(ctx,|ui|{gallery.ui(ui,&paths,true);});
                });
                let cards:Vec<_>=output.shapes.iter().filter_map(|shape| {
                    if let egui::Shape::Rect(r)=&shape.shape {if r.fill==egui::Color32::from_rgb(250,250,250) {return Some(r.rect);}}
                    None
                }).collect();
                assert_eq!(cards.len(),6);
                assert!(cards.iter().all(|r|r.width()<=176.1 && r.right()<=width));
            }
        }
    }
}
