use mc630_plt_studio::sheet::Layout;
use std::sync::Mutex;
#[derive(Clone)]
pub struct ArtworkCopy { pub source: Vec<u8>, pub layout: Layout, pub indices: Vec<usize> }
static COPIED: Mutex<Option<(String,ArtworkCopy)>>=Mutex::new(None);

const SHAPE_PREFIX: &str = "MC630-STUDIO-SHAPES:1:";
#[derive(Clone,serde::Serialize,serde::Deserialize)]
pub struct ShapeCopy {
    pub designs: Vec<mc630_plt_studio::sheet::DesignShape>,
    pub paths: Vec<Vec<mc630_plt_studio::Point>>,
}
impl ShapeCopy {
    pub fn encode(&self)->Result<String,String> {
        let layout=Layout {page_width:630.,page_height:2000.,design_shapes:self.designs.clone(),manual_cut_paths:self.paths.clone(),..Default::default()};
        layout.validate_manual_cut_paths()?;
        serde_json::to_string(self).map(|s|format!("{SHAPE_PREFIX}{s}")).map_err(|e|e.to_string())
    }
    pub fn decode(text:&str)->Result<Option<Self>,String> {
        let Some(json)=text.strip_prefix(SHAPE_PREFIX) else{return Ok(None)};
        if json.len()>16_000_000 {return Err("Copied shapes are too large.".into());}
        let copy:Self=serde_json::from_str(json).map_err(|e|e.to_string())?;
        copy.encode()?;Ok(Some(copy))
    }
    pub fn compose(&self,destination:&Layout)->Result<Layout,String> {
        let points:Vec<_>=self.designs.iter().flat_map(|s|s.path.iter()).chain(self.paths.iter().flatten()).collect();
        if points.is_empty() {return Err("No shapes to paste.".into());}
        let x0=points.iter().map(|p|p[0]).fold(f64::INFINITY,f64::min);
        let y0=points.iter().map(|p|p[1]).fold(f64::INFINITY,f64::min);
        let x1=points.iter().map(|p|p[0]).fold(f64::NEG_INFINITY,f64::max);
        let y1=points.iter().map(|p|p[1]).fold(f64::NEG_INFINITY,f64::max);
        if x1-x0>destination.page_width || y1-y0>destination.page_height {return Err("Copied shapes do not fit this page. Increase the page size first.".into());}
        let dx=5_f64.clamp(-x0,destination.page_width-x1);
        let dy=5_f64.clamp(-y0,destination.page_height-y1);
        let translate=|path:&Vec<mc630_plt_studio::Point>|path.iter().map(|p|[p[0]+dx,p[1]+dy]).collect();
        let mut layout=destination.clone();
        layout.design_shapes.extend(self.designs.iter().cloned().map(|mut shape|{shape.path=translate(&shape.path);shape}));
        layout.manual_cut_paths.extend(self.paths.iter().map(translate));
        layout.validate_manual_cut_paths()?;
        Ok(layout)
    }
}
pub fn store_shapes(copy:&ShapeCopy)->Result<(),String> {
    arboard::Clipboard::new().map_err(|e|e.to_string())?.set_text(copy.encode()?).map_err(|e|e.to_string())
}
pub fn read_shapes()->Result<Option<ShapeCopy>,String> {
    let text=match arboard::Clipboard::new().and_then(|mut c|c.get_text()) {Ok(text)=>text,Err(_)=>return Ok(None)};
    ShapeCopy::decode(&text)
}
pub fn store(copy: ArtworkCopy)->Result<(),String> {
    let token=format!("MC630-STUDIO-ARTWORK:{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos());
    arboard::Clipboard::new().map_err(|e|e.to_string())?.set_text(token.clone()).map_err(|e|e.to_string())?;
    *COPIED.lock().map_err(|_|"Clipboard unavailable")?=Some((token,copy));
    Ok(())
}
pub fn read()->Option<ArtworkCopy> {
    let token=arboard::Clipboard::new().ok()?.get_text().ok()?;
    let data=COPIED.lock().ok()?;
    data.as_ref().filter(|(old,_)|old==&token).map(|(_,copy)|copy.clone())
}
impl ArtworkCopy {
    pub fn compose(&self,destination:&Layout,source:&[u8],inset:f64)->Result<(Vec<u8>,Layout),String> {
        let image=mc630_plt_studio::sheet::decode_image(&self.source)?.into_rgba8();
        let items=self.layout.editor_items()?;
        let mut layout=destination.clone();let mut source=source.to_vec();
        for &old in &self.indices {
            let item=*items.get(old).ok_or("Copied image is missing")?;
            let [x,y,w,h]=self.layout.image_tiles.get(old).copied().unwrap_or([0,0,image.width(),image.height()]);
            if w==0 || h==0 || x.saturating_add(w)>image.width() || y.saturating_add(h)>image.height() {return Err("Invalid copied image bounds".into());}
            let pixels=image::imageops::crop_imm(&image,x,y,w,h).to_image();
            let mut png=std::io::Cursor::new(Vec::new());
            pixels.write_to(&mut png,image::ImageFormat::Png).map_err(|e|e.to_string())?;
            let new;
            if source.is_empty() {
                let [left,right,top,bottom]=layout.margins().map(|v|v.max(inset));
                if item.width>layout.page_width-left-right || item.height>layout.page_height-top-bottom {return Err("Copied image does not fit this page.".into());}
                layout.count=1;layout.custom.clear();layout.image_tiles.clear();layout.image_groups.clear();layout.hidden_images.clear();layout.image_cut_paths.clear();
                layout.width=item.width;layout.height=item.height;
                layout.custom.insert(0,mc630_plt_studio::sheet::Placement{x:left,y:top,..item});
                source=png.into_inner();new=0;
            } else {
                let (next,next_layout)=crate::photo_size::append_image_sized(&layout,&source,&png.into_inner(),inset,Some([item.width,item.height]))?;
                source=next;layout=next_layout;new=layout.count-1;
                layout.custom.get_mut(&new).unwrap().orientation=item.orientation;
            }
            if let Some(paths)=self.layout.image_cut_paths_for(old) {
                layout.image_cut_paths.insert(new,paths.clone());
                layout.use_manual_cut_paths=self.layout.use_manual_cut_paths;
            }
        }
        Ok((source,layout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paste_copies_linked_outline_and_rotation_to_existing_and_blank_project() {
        let mut layout=Layout{count:1,width:30.,height:40.,use_manual_cut_paths:true,..Default::default()};
        let item=mc630_plt_studio::sheet::Placement{x:20.,y:20.,width:30.,height:40.,orientation:Default::default()}.with_rotation(90.);
        layout.custom.insert(0,item);
        layout.link_cut_paths(0,vec![vec![[20.,25.],[45.,25.],[45.,45.],[20.,45.],[20.,25.]]]).unwrap();
        let bytes=include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        let copied=ArtworkCopy{source:bytes.clone(),layout:layout.clone(),indices:vec![0]};
        let (source,pasted)=copied.compose(&layout,&bytes,0.).unwrap();
        assert_eq!(pasted.count,2);
        assert_eq!(pasted.image_cut_paths[&1],layout.image_cut_paths[&0]);
        assert_eq!(pasted.custom[&1].orientation,item.orientation);
        assert_eq!((pasted.custom[&1].width,pasted.custom[&1].height),(item.width,item.height));
        assert_eq!(pasted.manual_page_cut_paths().unwrap().len(),2);
        assert!(pasted.trace_images(&source,false,245,mc630_plt_studio::sheet::CutShape::AlphaOutline,20,false).is_ok());
        let (_,blank)=copied.compose(&Layout::default(),&[],0.).unwrap();
        assert_eq!(blank.count,1);
        assert_eq!(blank.image_cut_paths[&0],layout.image_cut_paths[&0]);
        assert!(blank.use_manual_cut_paths);
        assert_eq!(blank.custom[&0].orientation,item.orientation);
    }
}

#[cfg(test)]
mod shape_tests {
    use super::*;
    #[test]
    fn vector_clipboard_roundtrip_keeps_style_cut_flags_and_relative_positions() {
        let shape=mc630_plt_studio::sheet::DesignShape {path:vec![[20.,20.],[40.,20.],[40.,40.],[20.,20.]],fill:Some([255,103,0,128]),stroke:[0,0,0,255],stroke_width:0.7,print:true,cut:true};
        let copy=ShapeCopy {designs:vec![shape.clone()],paths:vec![vec![[50.,20.],[60.,30.]]]};
        let decoded=ShapeCopy::decode(&copy.encode().unwrap()).unwrap().unwrap();
        let pasted=decoded.compose(&Layout::default()).unwrap();
        let result=&pasted.design_shapes[0];
        assert_eq!(result.fill,shape.fill);assert_eq!(result.stroke_width,0.7);assert!(result.print && result.cut);
        assert_eq!(result.path[0],[25.,25.]);assert_eq!(pasted.manual_cut_paths[0][0],[55.,25.]);
        assert!(ShapeCopy::decode("ordinary clipboard text").unwrap().is_none());
        assert!(ShapeCopy::decode("MC630-STUDIO-SHAPES:1:bad json").is_err());
        assert!(decoded.compose(&Layout {page_width:10.,page_height:10.,..Default::default()}).is_err());
        let wide=ShapeCopy {designs:vec![],paths:vec![vec![[300.,50.],[350.,100.]]]};
        assert!(ShapeCopy::decode(&wide.encode().unwrap()).unwrap().is_some());
    }
}

#[cfg(test)]
mod clipboard_profile {
    use super::*;
    #[test]
    #[ignore = "manual clipboard stage profiling using MC630_PROFILE_RECOVERY"]
    fn profile_open_document_image_paste() {
        let path=std::env::var("MC630_PROFILE_RECOVERY").expect("Set MC630_PROFILE_RECOVERY");
        let record:crate::recovery::Record=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let source=record.project.recovery_source().unwrap();let prefs=&record.project.settings;
        let pixels=mc630_plt_studio::sheet::decode_image(&source).unwrap();
        let copied=ArtworkCopy {source:source.clone(),layout:prefs.layout.clone(),indices:vec![0]};
        let start=std::time::Instant::now();
        let (combined,layout)=copied.compose(&prefs.layout,&source,0.).unwrap();
        let compose_ms=start.elapsed().as_secs_f64()*1000.;
        let trace=||layout.trace_images(&combined,prefs.remove_white,prefs.threshold,prefs.cut_shape,prefs.background_tolerance,prefs.print_without_background).unwrap();
        let start=std::time::Instant::now();let _=trace();let first_trace_ms=start.elapsed().as_secs_f64()*1000.;
        let start=std::time::Instant::now();let _=trace();let second_trace_ms=start.elapsed().as_secs_f64()*1000.;
        let report=serde_json::json!({"source_pixels":[pixels.width(),pixels.height()],"source_bytes":source.len(),"compose_ms":compose_ms,"validation_trace_ms":first_trace_ms,"retrace_ms":second_trace_ms,"total_measured_ms":compose_ms+first_trace_ms+second_trace_ms,"build":"debug"});
        std::fs::write("target/clipboard-profile.json",serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        println!("PROFILE {report}");
    }
}
