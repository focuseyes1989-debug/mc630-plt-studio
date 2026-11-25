use eframe::egui;
use mc630_plt_studio::{svg_artwork::SvgArtwork, vector_trace::{self,Options,Traced}};

pub struct Dialog {
    pub index:usize,
    pub source_digest:u64,
    pub tile:[u32;4],
    pub size_mm:[f64;2],
    original:image::RgbaImage,
    texture:egui::TextureHandle,
    preview:Option<egui::TextureHandle>,
    options:Options,
    traced_options:Option<Options>,
    result:Option<Traced>,
    pending:Option<std::sync::mpsc::Receiver<Result<Traced,String>>>,
    started:std::time::Instant,
    error:Option<String>,
    mode:usize,
    notice:Option<String>,
    zoom:f32,
    center:egui::Vec2,
    vector_renderer:crate::design_preview::Preview,
    preview_layout:Option<mc630_plt_studio::sheet::Layout>,
}
pub enum Action {Close,Apply(SvgArtwork)}
fn texture(ctx:&egui::Context,name:&str,image:&image::RgbaImage)->egui::TextureHandle {
    ctx.load_texture(name,egui::ColorImage::from_rgba_unmultiplied([image.width() as usize,image.height() as usize],image.as_raw()),egui::TextureOptions::LINEAR)
}
impl Dialog {
    pub fn new(ctx:&egui::Context,index:usize,source_digest:u64,tile:[u32;4],size_mm:[f64;2],original:&image::RgbaImage)->Self {
        let original=vector_trace::prepare_input(original,2048);
        let thumb=&original;
        Self {index,source_digest,tile,size_mm,texture:texture(ctx,"trace-original",thumb),original,preview:None,options:Default::default(),traced_options:None,result:None,pending:None,started:std::time::Instant::now(),error:None,mode:0,notice:None,zoom:1.,center:egui::vec2(0.5,0.5),vector_renderer:Default::default(),preview_layout:None}
    }
    fn start(&mut self,ctx:&egui::Context) {
        if self.pending.is_some() {return;}
        let source=self.original.clone();let options=self.options.clone();
        self.traced_options=Some(options.clone());self.error=None;self.notice=None;self.started=std::time::Instant::now();
        let (tx,rx)=std::sync::mpsc::channel();self.pending=Some(rx);
        let ctx=ctx.clone();
        std::thread::spawn(move || {
            let result=std::panic::catch_unwind(||vector_trace::trace(&source,&options)).unwrap_or_else(|_|Err("Cannot trace this image. Try fewer colors or a smaller detail size.".into()));
            let _=tx.send(result);ctx.request_repaint();
        });
    }
    pub fn show(&mut self,ctx:&egui::Context)->Option<Action> {
        if let Some(rx)=&self.pending {
            match rx.try_recv() {
                Ok(result)=>{self.pending=None;match result {Ok(result)=>{self.accept_result(ctx,result);},Err(error)=>{self.error=Some(error);self.result=None;self.preview=None;self.preview_layout=None;}}},
                Err(std::sync::mpsc::TryRecvError::Disconnected)=>{self.pending=None;self.error=Some("Trace worker stopped. Try again.".into());},
                Err(std::sync::mpsc::TryRecvError::Empty)=>{ctx.request_repaint_after(std::time::Duration::from_millis(100));}
            }
        }
        let mut action=None;let busy=self.pending.is_some();
        crate::ui_theme::dialog_modal(ctx,"vector-trace-dialog").show(ctx,|ui| {
            ui.set_width(740.);
            if crate::ui_theme::dialog_header(ui,"Vector Trace",true) {action=Some(Action::Close);}
            ui.add_enabled_ui(!busy,|ui| {
                ui.horizontal(|ui| {
                    ui.label("Mode");
                    let before=self.mode;
                    ui.selectable_value(&mut self.mode,0,"Logo");ui.selectable_value(&mut self.mode,1,"Color Artwork");
                    if self.mode!=before {self.options.corners=self.mode==0;self.options.colors=if self.mode==0 {8}else{16};self.options.color_merge=if self.mode==0 {18}else{4};self.options.smoothness=if self.mode==0 {30}else{50};}
                    ui.separator();ui.checkbox(&mut self.options.corners,"Preserve corners");
                    ui.checkbox(&mut self.options.remove_white,"Remove white background");
                });
                ui.horizontal(|ui| {
                    ui.add(crate::ui_theme::small_slider(egui::Slider::new(&mut self.options.colors,2..=32).text("Max colors")));
                    ui.add(crate::ui_theme::small_slider(egui::Slider::new(&mut self.options.speckle,0..=16).text("Remove speckles")));
                });
                ui.horizontal(|ui| {
                    ui.add(crate::ui_theme::small_slider(egui::Slider::new(&mut self.options.color_merge,0..=64).text("Merge similar colors"))).on_hover_text("Reduce small shade patches in solid logos. Set 0 to keep separate palette colors.");
                    ui.add(crate::ui_theme::small_slider(egui::Slider::new(&mut self.options.smoothness,0..=100).text("Curve smoothing"))).on_hover_text("Higher values fit longer, smoother curves; Preserve corners keeps distinct corners.");
                });
                ui.horizontal(|ui| {
                    ui.label("Detail");
                    for (label,value) in [("Fast",768),("Standard",1536),("High",2048)] {ui.selectable_value(&mut self.options.detail,value,label);}
                    if crate::ui_theme::primary_button(ui,"Trace / Update Preview").clicked() {self.start(ctx);}
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Fit").clicked() {self.zoom=1.;self.center=egui::vec2(0.5,0.5);}
                ui.add(crate::ui_theme::small_slider(egui::Slider::new(&mut self.zoom,1. ..=8.).text("Preview zoom").custom_formatter(|v,_|format!("{:.0}%",v*100.))));
                ui.small("Drag to pan · Scroll to zoom");
            });
            ui.columns(2,|columns| {
                for (index,column) in columns.iter_mut().enumerate() {
                    let (label,image)=if index==0 {("Original",Some(&self.texture))}else{("Vector Preview",self.preview.as_ref())};
                    column.label(label);
                    let (rect,response)=column.allocate_exact_size(egui::vec2(358.,270.),egui::Sense::drag());
                    let source_size=egui::vec2(self.original.width() as f32,self.original.height() as f32);
                    let fitted=source_size*(rect.width()/source_size.x).min(rect.height()/source_size.y);
                    if response.dragged() {self.center-=column.input(|i|i.pointer.delta())/(fitted*self.zoom);}
                    if response.hovered() {
                        let wheel=column.input(|i|i.smooth_scroll_delta.y);
                        let next=(self.zoom*(wheel/300.).exp()).clamp(1.,8.);
                        if next!=self.zoom {
                            if let Some(pointer)=response.hover_pos() {self.center+=(pointer-rect.center())/fitted*(1./self.zoom-1./next);}
                            self.zoom=next;
                        }
                        column.ctx().set_cursor_icon(if response.dragged(){egui::CursorIcon::Grabbing}else{egui::CursorIcon::Grab});
                    }
                    self.center=self.center.clamp(egui::Vec2::ZERO,egui::vec2(1.,1.));
                    let painter=column.painter_at(rect);
                    for row in 0..18 {for col in 0..24 {
                        let cell=egui::Rect::from_min_size(rect.min+egui::vec2(col as f32*16.,row as f32*16.),egui::vec2(16.,16.)).intersect(rect);
                        painter.rect_filled(cell,0.,if (col+row)%2==0 {egui::Color32::WHITE}else{egui::Color32::from_gray(232)});
                    }}
                    if let Some(texture)=image {
                        let image_rect=egui::Rect::from_center_size(rect.center()+(egui::vec2(0.5,0.5)-self.center)*fitted*self.zoom,fitted*self.zoom);
                        if index==1 && let Some(layout)=&self.preview_layout {
                            if let Err(error)=self.vector_renderer.paint(ctx,&painter,image_rect,layout) {self.error=Some(error);}
                        }
                        if index==0 || !self.vector_renderer.has_base() {painter.image(texture.id(),image_rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);}
                    }else {painter.text(rect.center(),egui::Align2::CENTER_CENTER,"Click Trace to preview",egui::FontId::proportional(14.),egui::Color32::DARK_GRAY);}
                }
            });
            if self.pending.is_some() {ui.label(format!("Tracing colors and curves… {:.1}s",self.started.elapsed().as_secs_f32()));}
            else if let Some(result)=&self.result {ui.label(format!("{} colors · {} vector paths · {:.1} KB",result.colors,result.paths,result.svg.len() as f64/1024.));}
            if self.result.is_some() && self.traced_options.as_ref()!=Some(&self.options) {ui.colored_label(crate::ui_theme::ACCENT,"Settings changed. Update Preview before applying or exporting.");}
            if let Some(notice)=&self.notice {ui.label(notice);}
            if let Some(error)=&self.error {ui.colored_label(egui::Color32::DARK_RED,error);}
            ui.small("Alpha below 50% is removed. White removal clears only the outside background; enclosed white details are kept.");
            ui.separator();
            let ready=self.pending.is_none() && self.result.is_some() && self.traced_options.as_ref()==Some(&self.options);
            crate::ui_theme::dialog_actions(ui,|ui| {
                ui.add_enabled_ui(ready,|ui| {
                    if crate::ui_theme::primary_button(ui,"Use Vector").clicked() {
                        let result=self.result.as_ref().unwrap();
                        action=Some(Action::Apply(SvgArtwork {source:result.svg.clone(),raster_rect:self.tile,viewport:[0.,0.,1.,1.]}));
                    }
                    if ui.button("Export SVG…").clicked() {
                        if let Some(path)=rfd::FileDialog::new().add_filter("Vector SVG",&["svg"]).set_file_name("Traced-logo.svg").save_file() {
                            let path=if path.extension().is_none(){path.with_extension("svg")}else{path};
                            let result=vector_trace::export_svg(&self.result.as_ref().unwrap().svg,self.size_mm).and_then(|svg|std::fs::write(&path,svg).map_err(|e|e.to_string()));
                            match result {Ok(())=>{self.notice=Some(format!("SVG exported: {}",path.display()));self.error=None;},Err(error)=>self.error=Some(error)}
                        }
                    }
                });
                if ui.button("Close").clicked() {action=Some(Action::Close);}
            });
        });
        action
    }

    fn accept_result(&mut self,ctx:&egui::Context,result:Traced) {
        use mc630_plt_studio::sheet::{Layout,Placement};
        let tile=[0,0,result.preview.width(),result.preview.height()];
        let aspect=tile[2] as f64/tile[3] as f64;
        let width=100_f64.max(50.*aspect).clamp(50.,630.);
        let height=(width/aspect).clamp(50.,2000.);
        let mut layout=Layout {page_width:width,page_height:height,width,height,count:1,page_margins:Some([0.;4]),..Default::default()};
        layout.custom.insert(0,Placement {x:0.,y:0.,width,height,orientation:Default::default()});
        layout.svg_sources.insert(0,SvgArtwork {source:result.svg.clone(),raster_rect:tile,viewport:[0.,0.,1.,1.]});
        self.preview_layout=Some(layout);self.vector_renderer=Default::default();
        self.preview=Some(texture(ctx,"trace-vector",&result.preview));self.result=Some(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trace_dialog_draws_original_and_vector_preview_without_applying_changes() {
        let ctx=egui::Context::default();
        let image=image::RgbaImage::from_pixel(40,30,image::Rgba([0,100,200,255]));
        let mut dialog=Dialog::new(&ctx,0,0,[0,0,40,30],[40.,30.],&image);
        let result=vector_trace::trace(&dialog.original,&dialog.options).unwrap();
        dialog.accept_result(&ctx,result);dialog.traced_options=Some(dialog.options.clone());
        dialog.zoom=4.;dialog.center=egui::vec2(0.7,0.4);
        let mut action=None;
        let output=ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1100.,800.))),..Default::default()},|ctx|{action=dialog.show(ctx);});
        assert!(action.is_none());assert!(dialog.result.is_some());assert!(!output.shapes.is_empty());
    }
}
