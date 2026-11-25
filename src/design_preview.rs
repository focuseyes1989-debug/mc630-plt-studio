use eframe::egui;
use mc630_plt_studio::sheet::{Layout, DesignShape};
use std::{sync::mpsc, time::{Duration, Instant}};

fn convex_ring(path:&[[f64;2]])->bool {
    if path.len()<4 || path.first()!=path.last() {return false;}
    let ring=&path[..path.len()-1];let mut sign=0.;let mut turn=0.;
    for i in 0..ring.len() {
        let a=ring[i];let b=ring[(i+1)%ring.len()];let c=ring[(i+2)%ring.len()];
        let u=[b[0]-a[0],b[1]-a[1]];let v=[c[0]-b[0],c[1]-b[1]];
        let cross=u[0]*v[1]-u[1]*v[0];
        if cross.abs()>1e-9 {if sign!=0. && sign*cross<0. {return false;}sign=cross.signum();}
        turn+=cross.atan2(u[0]*v[0]+u[1]*v[1]);
    }
    sign!=0. && (turn.abs()-std::f64::consts::TAU).abs()<0.001
}

/// Common editor shapes can be painted immediately as vector geometry. Preserve
/// the SVG fallback for concave fills, without partially painting a mixed stack.
impl Preview {
pub fn paint_simple(&mut self,painter:&egui::Painter,page:egui::Rect,layout:&Layout)->bool {
    if self.simple_shapes!=layout.design_shapes {
        self.simple_supported=!layout.design_shapes.iter().filter(|shape|shape.print).any(|shape|shape.fill.is_some() && shape.path.first()==shape.path.last() && !convex_ring(&shape.path));
        self.simple_shapes.clone_from(&layout.design_shapes);self.simple_page=None;
    }
    if !self.simple_supported {return false;}
    let view=(page,painter.ctx().pixels_per_point());
    if self.simple_page!=Some(view) {
    let mut shapes=Vec::new();
    let printed=||layout.design_shapes.iter().filter(|shape|shape.print);
    let scale=page.width()/layout.page_width as f32;
    let color=|c:[u8;4]|egui::Color32::from_rgba_unmultiplied(c[0],c[1],c[2],c[3]);
    for shape in printed() {
        let closed=shape.path.len()>2 && shape.path.first()==shape.path.last();
        let stroke=egui::Stroke::new(shape.stroke_width as f32*scale,color(shape.stroke));
        let mut points:Vec<_>=shape.path.iter().map(|p|page.min+egui::vec2(p[0] as f32*scale,p[1] as f32*scale)).collect();
        if closed && let Some(fill)=shape.fill {
            points.pop();shapes.push(egui::Shape::convex_polygon(points,color(fill),stroke));
        }else {shapes.push(egui::Shape::line(points,stroke));}
    }
    self.simple_meshes=painter.ctx().tessellate(shapes.into_iter().map(|shape|egui::epaint::ClippedShape {clip_rect:egui::Rect::EVERYTHING,shape}).collect(),view.1)
        .into_iter().filter_map(|primitive|match primitive.primitive {egui::epaint::Primitive::Mesh(mesh)=>Some(std::sync::Arc::new(mesh)),_=>None}).collect();
    self.simple_page=Some(view);
    }
    for mesh in &self.simple_meshes {painter.add(egui::Shape::Mesh(mesh.clone()));}
    true
}
}

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
    simple_shapes:Vec<DesignShape>,
    simple_supported:bool,
    simple_page:Option<(egui::Rect,f32)>,
    simple_meshes:Vec<std::sync::Arc<egui::Mesh>>,
    shapes: Vec<DesignShape>,
    svg_layout: Option<Layout>,
    page_size: [f64;2],
    generation: u64,
    base: Option<Cached>,
    detail: Option<Cached>,
    job: Option<Job>,
    view: Option<(egui::Rect, f32)>,
    moved: Option<Instant>,
}

impl Preview {
    pub fn has_base(&self)->bool {self.base.is_some()}
    pub fn paint(&mut self,ctx:&egui::Context,painter:&egui::Painter,page:egui::Rect,layout:&Layout)->Result<(),String> {
        let dimensions=[layout.page_width,layout.page_height];
        let svg_changed=if layout.svg_sources.is_empty() {self.svg_layout.is_some()}else{self.svg_layout.as_ref().is_none_or(|old|old.svg_sources!=layout.svg_sources || old.custom!=layout.custom || old.image_tiles!=layout.image_tiles || old.hidden_images!=layout.hidden_images || old.count!=layout.count || old.width!=layout.width || old.height!=layout.height)};
        if self.page_size!=dimensions || self.shapes!=layout.design_shapes || svg_changed {
            self.svg_layout=(!layout.svg_sources.is_empty()).then(||layout.clone());
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
        let bounds=content_bounds(layout);
        let layout=layout.clone();let ctx=ctx.clone();let (sender,result)=mpsc::channel();
        std::thread::spawn(move || {
            let image=layout.render_design_region(size[0] as u32,size[1] as u32,region).map(|pixels|egui::ColorImage::from_rgba_unmultiplied(size,pixels.as_raw()));
            let _=sender.send(image);ctx.request_repaint();
        });
        self.job=Some(Job {generation:self.generation,bounds,base,region,size,result});
    }
}

fn content_bounds(layout:&Layout)->Option<[f64;4]> {
    if !layout.svg_sources.is_empty() && layout.design_shapes.is_empty() {
        let items=layout.editor_items().ok()?;
        let mut visible=items.iter().enumerate().filter(|(i,_)|!layout.hidden_images.contains(i) && layout.svg_source_for(*i).is_some());
        let (_,item)=visible.next()?;
        if visible.next().is_none() {return Some([item.x,item.y,item.x+item.width,item.y+item.height]);}
        return None;
    }
    single_bounds(&layout.design_shapes)
}

fn single_bounds(shapes:&[DesignShape])->Option<[f64;4]> {
    (shapes.len()==1).then(||crate::cut_tools::bounds(&shapes[0].path))
}

fn display_region(cache:&Cached,layout:&Layout)->[f64;4] {
    if let (Some(old),Some(new))=(cache.bounds,content_bounds(layout)) {
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
    fn simple_shapes_render_immediately_and_reuse_mesh_until_geometry_changes() {
        let ctx=egui::Context::default();let mut preview=Preview::default();let mut layout=Layout::default();
        layout.design_shapes.push(DesignShape {path:crate::cut_tools::shape_path(crate::cut_tools::Tool::Ellipse,[20.,30.],[60.,70.],false,0.),fill:Some([0,103,192,128]),stroke:[0,0,0,255],stroke_width:0.3,print:true,cut:false});
        let page=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(420.,594.));
        let frame=|preview:&mut Preview,layout:&Layout| {
            let _=ctx.run(egui::RawInput::default(),|ctx| {
                egui::CentralPanel::default().show(ctx,|ui|assert!(preview.paint_simple(ui.painter(),page,layout)));
            });
        };
        frame(&mut preview,&layout);
        assert!(!preview.simple_meshes.is_empty());assert!(preview.job.is_none());
        let mesh=preview.simple_meshes[0].clone();
        frame(&mut preview,&layout);assert!(std::sync::Arc::ptr_eq(&mesh,&preview.simple_meshes[0]));
        for p in &mut layout.design_shapes[0].path {p[0]+=5.;}
        frame(&mut preview,&layout);assert!(!std::sync::Arc::ptr_eq(&mesh,&preview.simple_meshes[0]));
    }
    #[test]
    fn concave_and_self_crossing_fills_keep_the_svg_renderer() {
        let ctx=egui::Context::default();let mut preview=Preview::default();let mut layout=Layout::default();
        let path=vec![[10.,10.],[50.,10.],[50.,20.],[20.,20.],[20.,50.],[10.,50.],[10.,10.]];
        assert!(!convex_ring(&path));
        let mut star:Vec<_>=(0..5).map(|i|{let a=i as f64*std::f64::consts::TAU*2./5.;[20.+10.*a.cos(),20.+10.*a.sin()]}).collect();star.push(star[0]);
        assert!(!convex_ring(&star));
        layout.design_shapes.push(DesignShape {path,fill:Some([10,20,30,255]),stroke:[0,0,0,255],stroke_width:0.3,print:true,cut:false});
        let _=ctx.run(egui::RawInput::default(),|ctx| {
            egui::CentralPanel::default().show(ctx,|ui|assert!(!preview.paint_simple(ui.painter(),ui.max_rect(),&layout)));
        });
        assert!(preview.simple_meshes.is_empty());
    }
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
