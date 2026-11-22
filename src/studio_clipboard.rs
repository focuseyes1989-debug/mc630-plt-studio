use mc630_plt_studio::sheet::Layout;
use std::sync::Mutex;
#[derive(Clone)]
pub struct ArtworkCopy { pub source: Vec<u8>, pub layout: Layout, pub indices: Vec<usize> }
static COPIED: Mutex<Option<(String,ArtworkCopy)>>=Mutex::new(None);
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
