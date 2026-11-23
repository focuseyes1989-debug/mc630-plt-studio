use eframe::egui;
use mc630_plt_studio::sheet::{Layout, DesignShape};
use std::{sync::mpsc, time::{Duration, Instant}};

struct Cached { generation:u64, bounds:Option<[f64;4]>, region: [f64;4], size: [usize;2], texture: egui::TextureHandle }
struct Job {
    generation: u64,
    bounds: Option<[f64;4]>,
    base: bool,
    region: [f64;4],
    size: [usize;2],
    result: mpsc::Receiver<Result<egui::ColorImage,String>>,
}

#[derive(Default)]
pub struct Preview {
    shapes: Vec<DesignShape>,
    page_size: [f64;2],
    generation: u64,
    base: Option<Cached>,
    detail: Option<Cached>,
    job: Option<Job>,
    view: Option<(egui::Rect, f32)>,
    moved: Option<Instant>,
}

impl Preview {
    pub fn paint(&mut self,ctx:&egui::Context,painter:&egui::Painter,page:egui::Rect,layout:&Layout)->Result<(),String> {
        let dimensions=[layout.page_width,layout.page_height];
        if self.page_size!=dimensions || self.shapes!=layout.design_shapes {
            self.shapes.clone_from(&layout.design_shapes);
            self.page_size=dimensions; self.generation+=1;
            // Keep the last complete preview visible until its replacement is ready.
        }
        let now=Instant::now();
        let view=(page,ctx.pixels_per_point());
        if self.view!=Some(view) {self.view=Some(view);self.moved=Some(now);}
        if let Some(job)=&self.job {
            match job.result.try_recv() {
                Ok(result)=>{
                    let job=self.job.take().unwrap();
                    if job.generation==self.generation {
                        let image=result?;
                        let texture=ctx.load_texture("design-preview",image,egui::TextureOptions::LINEAR);
                        let cache=Cached {generation:job.generation,bounds:job.bounds,region:job.region,size:job.size,texture};
                        if job.base {self.base=Some(cache);self.detail=None;} else {self.detail=Some(cache);}
                    }
                }
                Err(mpsc::TryRecvError::Disconnected)=>{self.job=None;return Err("Design preview worker stopped.".into());}
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
        let visible=page.intersect(painter.clip_rect());
        if !visible.is_positive() {return Ok(());}
        let idle=self.moved.is_none_or(|time|now.duration_since(time)>=Duration::from_millis(140));
        if self.job.is_none() {
            if self.base.as_ref().is_none_or(|cache|cache.generation!=self.generation) {
                let scale=1000./dimensions[0].max(dimensions[1]);
                self.start(ctx,layout,true,[0.,0.,dimensions[0],dimensions[1]],[(dimensions[0]*scale).ceil() as usize,(dimensions[1]*scale).ceil() as usize]);
            }else if idle {
                let density=(page.width() as f64/dimensions[0]*ctx.pixels_per_point() as f64*1.25).max(0.01);
                let density=2_f64.powf(density.log2().ceil());
                let tile=192./density;
                let x0=((((visible.left()-page.left())/page.width()) as f64*dimensions[0]/tile).floor()-1.).max(0.)*tile;
                let y0=((((visible.top()-page.top())/page.height()) as f64*dimensions[1]/tile).floor()-1.).max(0.)*tile;
                let x1=(((((visible.right()-page.left())/page.width()) as f64*dimensions[0]/tile).ceil()+1.)*tile).min(dimensions[0]);
                let y1=(((((visible.bottom()-page.top())/page.height()) as f64*dimensions[1]/tile).ceil()+1.)*tile).min(dimensions[1]);
                let region=[x0,y0,x1-x0,y1-y0];
                let limit=ctx.input(|i|i.max_texture_side).min(3072) as f64;
                let scale=density.min(limit/region[2].max(region[3]));
                let size=[(region[2]*scale).ceil().max(1.) as usize,(region[3]*scale).ceil().max(1.) as usize];
                if self.detail.as_ref().is_none_or(|cache|cache.region!=region || cache.size!=size) {
                    self.start(ctx,layout,false,region,size);
                }
            }
        }
        for (is_base,cache) in [(true,&self.base),(false,&self.detail)].into_iter().filter_map(|(base,cache)|cache.as_ref().map(|cache|(base,cache))) {
            let r=display_region(cache,layout);
            let min=page.min+egui::vec2((r[0]/dimensions[0]) as f32*page.width(),(r[1]/dimensions[1]) as f32*page.height());
            let rect=egui::Rect::from_min_size(min,egui::vec2((r[2]/dimensions[0]) as f32*page.width(),(r[3]/dimensions[1]) as f32*page.height()));
            // Replace the base in this region; translucent fills must not be composited twice.
            if is_base {
                if let Some(detail)=&self.detail {
                    let d=display_region(detail,layout);
                    let cut=egui::Rect::from_min_max(page.min+egui::vec2((d[0]/dimensions[0]) as f32*page.width(),(d[1]/dimensions[1]) as f32*page.height()),page.min+egui::vec2(((d[0]+d[2])/dimensions[0]) as f32*page.width(),((d[1]+d[3])/dimensions[1]) as f32*page.height()));
                    for clip in [egui::Rect::from_min_max(rect.min,egui::pos2(rect.right(),cut.top())),egui::Rect::from_min_max(egui::pos2(rect.left(),cut.bottom()),rect.max),egui::Rect::from_min_max(egui::pos2(rect.left(),cut.top()),egui::pos2(cut.left(),cut.bottom())),egui::Rect::from_min_max(egui::pos2(cut.right(),cut.top()),egui::pos2(rect.right(),cut.bottom()))] {
                        if clip.is_positive() {painter.with_clip_rect(painter.clip_rect().intersect(clip)).image(cache.texture.id(),rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);}
                    }
                    continue;
                }
            }
            painter.image(cache.texture.id(),rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);
        }
        if self.job.is_some() || !idle {ctx.request_repaint_after(Duration::from_millis(16));}
        Ok(())
    }

    fn start(&mut self,ctx:&egui::Context,layout:&Layout,base:bool,region:[f64;4],size:[usize;2]) {
        let layout=layout.clone();let ctx=ctx.clone();let (sender,result)=mpsc::channel();
        std::thread::spawn(move || {
            let image=layout.render_design_region(size[0] as u32,size[1] as u32,region).map(|pixels|egui::ColorImage::from_rgba_unmultiplied(size,pixels.as_raw()));
            let _=sender.send(image);ctx.request_repaint();
        });
        self.job=Some(Job {generation:self.generation,bounds:single_bounds(&self.shapes),base,region,size,result});
    }
}

fn single_bounds(shapes:&[DesignShape])->Option<[f64;4]> {
    (shapes.len()==1).then(||crate::cut_tools::bounds(&shapes[0].path))
}

fn display_region(cache:&Cached,layout:&Layout)->[f64;4] {
    if let (Some(old),Some(new))=(cache.bounds,single_bounds(&layout.design_shapes)) {
        let sx=(new[2]-new[0])/(old[2]-old[0]).max(0.001);
        let sy=(new[3]-new[1])/(old[3]-old[1]).max(0.001);
        return [new[0]+(cache.region[0]-old[0])*sx,new[1]+(cache.region[1]-old[1])*sy,cache.region[2]*sx,cache.region[3]*sy];
    }
    cache.region
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_keeps_visible_texture_and_transforms_it_while_worker_is_pending() {
        let ctx=egui::Context::default();
        let mut layout=Layout::default();
        let original=DesignShape {path:vec![[20.,20.],[40.,20.],[40.,40.],[20.,20.]],fill:Some([0,103,192,255]),stroke:[0,0,0,255],stroke_width:0.3,print:true,cut:false};
        layout.design_shapes=vec![original.clone()];
        let texture=ctx.load_texture("test-preview",egui::ColorImage::new([1,1],egui::Color32::BLUE),egui::TextureOptions::LINEAR);
        let id=texture.id();
        let mut preview=Preview::default();
        preview.shapes=vec![original];preview.page_size=[layout.page_width,layout.page_height];preview.generation=1;
        preview.base=Some(Cached {generation:1,bounds:Some([20.,20.,40.,40.]),region:[0.,0.,210.,297.],size:[1,1],texture});
        let (_sender,result)=mpsc::channel();
        preview.job=Some(Job {generation:1,bounds:None,base:true,region:[0.,0.,210.,297.],size:[1,1],result});
        for point in &mut layout.design_shapes[0].path {point[0]+=10.;point[1]+=5.;}
        let output=ctx.run(egui::RawInput::default(),|ctx| {
            egui::CentralPanel::default().show(ctx,|ui| {
                preview.paint(ctx,ui.painter(),egui::Rect::from_min_size(egui::pos2(10.,10.),egui::vec2(420.,594.)),&layout).unwrap();
            });
        });
        assert_eq!(preview.base.as_ref().unwrap().texture.id(),id);
        assert_eq!(display_region(preview.base.as_ref().unwrap(),&layout),[10.,5.,210.,297.]);
        assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.texture_id==id)));
    }
    #[test]
    fn navigation_reuses_base_and_defers_detail_until_idle() {
        let ctx=egui::Context::default();let mut preview=Preview::default();
        let mut layout=Layout::default();
        layout.design_shapes.push(DesignShape {path:vec![[20.,20.],[40.,20.],[40.,40.],[20.,20.]],fill:Some([0,103,192,128]),stroke:[0,0,0,255],stroke_width:0.3,print:true,cut:false});
        let frame=|preview:&mut Preview,page:egui::Rect| {
            let _=ctx.run(egui::RawInput::default(),|ctx| {
                egui::CentralPanel::default().show(ctx,|ui|preview.paint(ctx,ui.painter(),page,&layout).unwrap());
            });
        };
        let page=egui::Rect::from_min_size(egui::pos2(10.,10.),egui::vec2(420.,594.));
        frame(&mut preview,page);
        assert!(preview.job.as_ref().unwrap().base);
        let deadline=Instant::now()+Duration::from_secs(5);
        while preview.base.is_none() {
            assert!(Instant::now()<deadline);
            preview.moved=Some(Instant::now());
            frame(&mut preview,page);
            std::thread::sleep(Duration::from_millis(2));
        }
        let texture=preview.base.as_ref().unwrap().texture.id();
        for n in 1..20 {
            frame(&mut preview,page.translate(egui::vec2(n as f32*4.,0.)));
            assert!(preview.job.is_none());
            assert_eq!(preview.base.as_ref().unwrap().texture.id(),texture);
        }
        preview.moved=Some(Instant::now()-Duration::from_millis(200));
        let moved=page.translate(egui::vec2(76.,0.));
        frame(&mut preview,moved);
        assert!(!preview.job.as_ref().unwrap().base);
    }
}
