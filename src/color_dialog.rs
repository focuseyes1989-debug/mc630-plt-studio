use eframe::egui::{self, Color32};

#[derive(Clone)]
struct Picker {
    original: Color32,
    color: Color32,
    hsv: egui::ecolor::Hsva,
    hex: String,
}

impl Picker {
    fn new(color: Color32) -> Self {
        let mut picker = Self { original: color, color, hsv: color.into(), hex: String::new() };
        picker.sync_hex();
        picker
    }
    fn sync_hex(&mut self) {
        self.hex = format!("{:02X}{:02X}{:02X}", self.color.r(), self.color.g(), self.color.b());
    }
    fn from_hsv(&mut self) { self.color = self.hsv.into(); self.sync_hex(); }
}

fn parse_hex(text: &str, alpha: u8) -> Option<Color32> {
    let text = text.trim().trim_start_matches('#');
    if text.len() != 6 || !text.is_ascii() { return None; }
    let value = u32::from_str_radix(text, 16).ok()?;
    Some(Color32::from_rgba_unmultiplied((value >> 16) as u8, (value >> 8) as u8, value as u8, alpha))
}

/// Transactional color editor: Cancel and the title-bar close keep the original color.
pub fn button(ui: &mut egui::Ui, title: &str, color: &mut Color32) -> egui::Response {
    let id = ui.make_persistent_id(("studio-color-picker", title));
    // Keep the swatch identity stable when selection inserts/removes toolbar controls.
    let mut response = ui.push_id(id.with("swatch"), |ui| {
        ui.add(egui::Button::new("     ").fill(*color).min_size(egui::vec2(34., 24.)))
    }).inner.on_hover_text(title);
    let armed_id = id.with("pressed-on-swatch");
    if ui.input(|i| i.pointer.primary_pressed()) {
        let armed = response.hovered() && ui.input(|i| i.pointer.press_origin().is_some_and(|p| response.rect.contains(p)));
        ui.ctx().data_mut(|d| d.insert_temp(armed_id, armed));
    }
    let armed = ui.ctx().data(|d| d.get_temp::<bool>(armed_id).unwrap_or(false));
    if response.clicked() && armed { ui.ctx().data_mut(|d| d.insert_temp(id, Picker::new(*color))); }
    if ui.input(|i| i.pointer.primary_released() || !i.pointer.primary_down()) {
        ui.ctx().data_mut(|d| d.remove::<bool>(armed_id));
    }
    let Some(mut picker) = ui.ctx().data_mut(|d| d.get_temp::<Picker>(id)) else { return response; };
    let mut accept = false;
    let mut cancel = false;
    let modal = egui::Modal::new(id)
        .frame(egui::Frame::window(&ui.ctx().style()).inner_margin(egui::Margin::same(14)))
        .show(ui.ctx(), |ui| {
            ui.set_width(338.);
            cancel |= crate::ui_theme::dialog_header(ui,&format!("Color Picker — {title}"),true);
            ui.spacing_mut().item_spacing = egui::vec2(8., 5.);
            ui.horizontal_top(|ui| {
                let size = 210.;
                let (rect, area) = ui.allocate_exact_size(egui::vec2(size,size), egui::Sense::click_and_drag());
                #[cfg(test)] ui.ctx().data_mut(|d|d.insert_temp(id.with("sv-rect"),rect));
                let mut mesh = egui::Mesh::default();
                for y in 0..=24 { for x in 0..=24 {
                    let s=x as f32/24.; let v=1.-y as f32/24.;
                    mesh.colored_vertex(rect.min+egui::vec2(s,1.-v)*size, egui::ecolor::Hsva::new(picker.hsv.h,s,v,1.).into());
                }}
                for y in 0..24 { for x in 0..24 { let i=y*25+x;
                    mesh.add_triangle(i,i+1,i+25); mesh.add_triangle(i+1,i+26,i+25);
                }}
                ui.painter().add(egui::Shape::mesh(mesh));
                if (area.clicked() || area.dragged() || area.is_pointer_button_down_on()) && let Some(p)=area.interact_pointer_pos() {
                    picker.hsv.s=((p.x-rect.left())/size).clamp(0.,1.);
                    picker.hsv.v=(1.-(p.y-rect.top())/size).clamp(0.,1.); picker.from_hsv();
                }
                let pos=rect.min+egui::vec2(picker.hsv.s,1.-picker.hsv.v)*size;
                ui.painter().circle_stroke(pos,5.,egui::Stroke::new(2_f32,Color32::BLACK));
                ui.painter().circle_stroke(pos,4.,egui::Stroke::new(1_f32,Color32::WHITE));
                let (hue, slider)=ui.allocate_exact_size(egui::vec2(18.,size),egui::Sense::click_and_drag());
                #[cfg(test)] ui.ctx().data_mut(|d|d.insert_temp(id.with("hue-rect"),hue));
                for y in 0..210 {
                    ui.painter().rect_filled(egui::Rect::from_min_size(hue.min+egui::vec2(0.,y as f32),egui::vec2(18.,1.1)),0.,Color32::from(egui::ecolor::Hsva::new(y as f32/210.,1.,1.,1.)));
                }
                if (slider.clicked() || slider.dragged() || slider.is_pointer_button_down_on()) && let Some(p)=slider.interact_pointer_pos() {
                    picker.hsv.h=((p.y-hue.top())/size).clamp(0.,1.); picker.from_hsv();
                }
                let y=hue.top()+picker.hsv.h*size;
                ui.painter().rect_stroke(egui::Rect::from_min_size(egui::pos2(hue.left()-2.,y-2.),egui::vec2(22.,4.)),0.,egui::Stroke::new(1_f32,Color32::BLACK),egui::StrokeKind::Outside);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| { ui.small("New"); ui.add_space(4.); ui.small("Current"); });
                    let (preview,_) = ui.allocate_exact_size(egui::vec2(86.,36.),egui::Sense::hover());
                    for y in 0..4 { for x in 0..9 {
                        ui.painter().rect_filled(egui::Rect::from_min_size(preview.min+egui::vec2(x as f32*10.,y as f32*10.),egui::vec2(10.,10.)).intersect(preview),0.,if (x+y)%2==0 {Color32::WHITE}else{Color32::from_gray(215)});
                    }}
                    ui.painter().rect_filled(egui::Rect::from_min_size(preview.min,egui::vec2(43.,36.)),0.,picker.color);
                    ui.painter().rect_filled(egui::Rect::from_min_size(preview.min+egui::vec2(43.,0.),egui::vec2(43.,36.)),0.,picker.original);
                    ui.add_space(4.);
                    let mut rgba=picker.color.to_srgba_unmultiplied();
                    let mut changed=false;
                    egui::Grid::new(id.with("channels")).show(ui,|ui| {
                        for (label,value) in ["R","G","B","A"].into_iter().zip(rgba.iter_mut()) {
                            ui.label(label); changed |= ui.add_sized([62.,24.],egui::DragValue::new(value).range(0..=255)).changed(); ui.end_row();
                        }
                    });
                    if changed {picker.color=Color32::from_rgba_unmultiplied(rgba[0],rgba[1],rgba[2],rgba[3]);picker.hsv=picker.color.into();picker.sync_hex();}
                    ui.horizontal(|ui| {ui.label("#");
                        if ui.add(egui::TextEdit::singleline(&mut picker.hex).desired_width(65.).char_limit(7)).changed()
                            && let Some(c)=parse_hex(&picker.hex,picker.color.a()) {picker.color=c;picker.hsv=c.into();}
                    });
                });
            });
            ui.separator();
            crate::ui_theme::dialog_actions(ui,|ui| {
                accept=crate::ui_theme::primary_button(ui,"OK").clicked();
                cancel |= ui.add_sized([80.,28.],egui::Button::new("Cancel")).clicked();
            });
        });
    if accept { *color=picker.color; response.mark_changed(); }
    if accept || cancel || modal.should_close() { ui.ctx().data_mut(|d| d.remove::<Picker>(id)); }
    else { ui.ctx().data_mut(|d| d.insert_temp(id,picker)); }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appearing_swatch_does_not_capture_canvas_release_but_real_click_opens() {
        let ctx = egui::Context::default();
        let mut color = Color32::BLUE;
        let mut rect = egui::Rect::NOTHING;
        let mut picker_id = egui::Id::NULL;
        let mut frame = |show: bool, events: Vec<egui::Event>| {
            let _ = ctx.run(egui::RawInput { events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    picker_id = ui.make_persistent_id(("studio-color-picker", "Test"));
                    if show { rect = button(ui,"Test",&mut color).rect; }
                    else { ui.allocate_exact_size(egui::vec2(34.,24.),egui::Sense::click()); }
                });
            });
            (rect, picker_id)
        };
        frame(false,vec![]);
        let point=egui::pos2(20.,20.);
        let event=|pressed| egui::Event::PointerButton {pos:point,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()};
        frame(false,vec![egui::Event::PointerMoved(point),event(true)]);
        let (_,id)=frame(true,vec![event(false)]);
        assert!(ctx.data(|d|d.get_temp::<Picker>(id)).is_none());
        frame(true,vec![egui::Event::PointerMoved(point),event(true)]);
        let (_,id)=frame(true,vec![event(false)]);
        assert!(ctx.data(|d|d.get_temp::<Picker>(id)).is_some());
        assert_eq!(color,Color32::BLUE);
    }
    #[test]
    fn modal_accepts_saturation_and_hue_pointer_input_without_committing_early() {
        let ctx=egui::Context::default();let mut color=Color32::from_rgb(0,103,192);
        let frame=|color:&mut Color32,events:Vec<egui::Event>,seed:bool| {
            let mut id=egui::Id::NULL;
            let _=ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(900.,700.))),events,..Default::default()},|ctx| {
                egui::CentralPanel::default().show(ctx,|ui| {
                    id=ui.make_persistent_id(("studio-color-picker","Test"));
                    if seed {ui.ctx().data_mut(|d|d.insert_temp(id,Picker::new(*color)));}
                    button(ui,"Test",color);
                });
            });id
        };
        frame(&mut color,vec![],true);
        for _ in 0..5 {frame(&mut color,vec![],false);}
        let id=frame(&mut color,vec![],false);
        let rect=ctx.data(|d|d.get_temp::<egui::Rect>(id.with("sv-rect")).unwrap());
        let p=rect.min+egui::vec2(rect.width()*0.4,rect.height()*0.3);
        let event=|p,pressed|egui::Event::PointerButton {pos:p,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()};
        frame(&mut color,vec![egui::Event::PointerMoved(p),event(p,true)],false);
        frame(&mut color,vec![event(p,false)],false);
        let picker=ctx.data(|d|d.get_temp::<Picker>(id).unwrap());
        assert!((picker.hsv.s-0.4).abs()<0.02);
        assert!((picker.hsv.v-0.7).abs()<0.02,"value={} saturation={}",picker.hsv.v,picker.hsv.s);
        let hue=ctx.data(|d|d.get_temp::<egui::Rect>(id.with("hue-rect")).unwrap());
        let p=egui::pos2(hue.center().x,hue.top()+hue.height()*0.15);
        frame(&mut color,vec![egui::Event::PointerMoved(p),event(p,true)],false);
        frame(&mut color,vec![event(p,false)],false);
        let picker=ctx.data(|d|d.get_temp::<Picker>(id).unwrap());
        assert!((picker.hsv.h-0.15).abs()<0.02);
        assert_ne!(picker.color,picker.original);
        assert_eq!(color,picker.original);
    }
    #[test]
    fn hex_accepts_rgb_preserving_alpha_and_rejects_invalid_input() {
        assert_eq!(parse_hex("#518BE2",128).unwrap().to_srgba_unmultiplied(),[81,139,226,128]);
        assert!(parse_hex("xyz123",255).is_none());
        assert!(parse_hex("FFF",255).is_none());
        assert!(parse_hex("ကက",255).is_none());
    }
}
