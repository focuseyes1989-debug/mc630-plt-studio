use crate::sheet::{Orientation, Placement};

#[derive(Clone,Debug,PartialEq,serde::Serialize,serde::Deserialize)]
pub struct SvgArtwork {
    pub source:String,
    pub raster_rect:[u32;4],
    pub viewport:[f64;4],
}

pub fn tree(source:&str)->Result<usvg::Tree,String> {
    if source.len()>30_000_000 {return Err("SVG exceeds 30 MB.".into());}
    let document=roxmltree::Document::parse(source).map_err(|e|e.to_string())?;
    if document.root_element().tag_name().name()!="svg" {return Err("Not an SVG image.".into());}
    let mut options=usvg::Options::default();
    options.image_href_resolver.resolve_string=Box::new(|_,_|None);
    let resolve=usvg::ImageHrefResolver::default_data_resolver();
    options.image_href_resolver.resolve_data=Box::new(move |mime,data,options| {
        let format=image::guess_format(&data).ok()?;
        if !matches!(format,image::ImageFormat::Png|image::ImageFormat::Jpeg|image::ImageFormat::Gif|image::ImageFormat::WebP) {return None;}
        let (w,h)=image::ImageReader::with_format(std::io::Cursor::new(data.as_slice()),format).into_dimensions().ok()?;
        if w>4000 || h>4000 {return None;}
        resolve(mime,data,options)
    });
    options.fontdb_mut().load_system_fonts();
    usvg::Tree::from_str(source,&options).map_err(|e|e.to_string())
}

impl SvgArtwork {
    pub fn from_bytes(bytes:&[u8],raster_size:[u32;2])->Result<Self,String> {
        let tree=tree(std::str::from_utf8(bytes).map_err(|e|e.to_string())?)?;
        Ok(Self {source:tree.to_string(&usvg::WriteOptions::default()),raster_rect:[0,0,raster_size[0],raster_size[1]],viewport:[0.,0.,1.,1.]})
    }
    pub fn cropped(&self,tile:[u32;4])->Result<Self,String> {
        let [x,y,w,h]=self.raster_rect;
        if w==0 || h==0 || tile[0]<x || tile[1]<y || tile[0] as u64+tile[2] as u64>x as u64+w as u64 || tile[1] as u64+tile[3] as u64>y as u64+h as u64 {
            return Err("SVG crop is outside its original image.".into());
        }
        let mut asset=self.clone();
        asset.viewport=[self.viewport[0]+(tile[0]-x) as f64/w as f64*self.viewport[2],self.viewport[1]+(tile[1]-y) as f64/h as f64*self.viewport[3],tile[2] as f64/w as f64*self.viewport[2],tile[3] as f64/h as f64*self.viewport[3]];
        asset.raster_rect=tile;Ok(asset)
    }
    pub fn markup(&self,placement:Placement,tile:[u32;4],index:usize)->Result<String,String> {
        let asset=self.cropped(tile)?;
        let tree=tree(&asset.source)?;
        let size=tree.size();
        let canonical=tree.to_string(&usvg::WriteOptions {id_prefix:Some(format!("asset{index}-")),..Default::default()});
        let document=roxmltree::Document::parse(&canonical).map_err(|e|e.to_string())?;
        let root=&canonical[document.root_element().range()];
        let inner=&root[root.find('>').ok_or("Invalid SVG")?+1..root.rfind("</svg>").ok_or("Invalid SVG")?];
        let [origin,right,_,bottom]=placement.orientation.unit_corners();
        let viewport=asset.viewport;
        let vx=viewport[0]*size.width() as f64;let vy=viewport[1]*size.height() as f64;
        let vw=viewport[2]*size.width() as f64;let vh=viewport[3]*size.height() as f64;
        Ok(format!(r#"<g transform="matrix({} {} {} {} {} {})"><defs><clipPath id="svg-clip-{index}"><rect width="1" height="1"/></clipPath></defs><g clip-path="url(#svg-clip-{index})"><g transform="matrix({} 0 0 {} {} {})">{}</g></g></g>"#,
            (right[0]-origin[0])*placement.width,(right[1]-origin[1])*placement.height,
            (bottom[0]-origin[0])*placement.width,(bottom[1]-origin[1])*placement.height,
            placement.x+origin[0]*placement.width,placement.y+origin[1]*placement.height,
            1./vw,1./vh,-vx/vw,-vy/vh,inner))
    }
    pub fn raster(&self,tile:[u32;4],width:u32,height:u32,orientation:Orientation)->Result<image::RgbaImage,String> {
        let placement=Placement {x:0.,y:0.,width:1.,height:1.,orientation};
        render(&self.markup(placement,tile,0)?,width,height,[0.,0.,1.,1.])
    }
}

pub fn render(markup:&str,width:u32,height:u32,region:[f64;4])->Result<image::RgbaImage,String> {
    if width==0 || height==0 || width as u64*height as u64>80_000_000 {return Err("SVG preview exceeds image limits.".into());}
    let source=format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="{} {} {} {}" preserveAspectRatio="none">{}</svg>"#,width,height,region[0],region[1],region[2],region[3],markup);
    let tree=tree(&source)?;
    let mut pixels=resvg::tiny_skia::Pixmap::new(width,height).ok_or("Cannot allocate SVG preview")?;
    resvg::render(&tree,resvg::tiny_skia::Transform::identity(),&mut pixels.as_mut());
    let mut image=image::RgbaImage::new(width,height);
    for (target,pixel) in image.pixels_mut().zip(pixels.pixels()) {let p=pixel.demultiply();*target=image::Rgba([p.red(),p.green(),p.blue(),p.alpha()]);}
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn asset()->SvgArtwork {
        SvgArtwork::from_bytes(br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="10" height="10" fill="red"/><rect x="10" width="10" height="10" fill="blue"/></svg>"#,[20,10]).unwrap()
    }
    #[test]
    fn vector_crop_rotation_and_serialization_keep_colors_at_high_resolution() {
        let asset=asset();
        let pixels=asset.raster([0,0,20,10],1000,500,Orientation::default()).unwrap();
        assert_eq!(pixels.get_pixel(250,250).0,[255,0,0,255]);
        assert_eq!(pixels.get_pixel(750,250).0,[0,0,255,255]);
        let cropped=asset.raster([10,0,10,10],800,800,Orientation::default()).unwrap();
        assert_eq!(cropped.get_pixel(400,400).0,[0,0,255,255]);
        let orientation=Placement {x:0.,y:0.,width:20.,height:10.,orientation:Default::default()}.with_rotation(90.).orientation;
        let rotated=asset.raster([0,0,20,10],500,1000,orientation).unwrap();
        assert_eq!(rotated.get_pixel(250,250).0,[255,0,0,255]);
        assert_eq!(rotated.get_pixel(250,750).0,[0,0,255,255]);
        let restored:SvgArtwork=serde_json::from_str(&serde_json::to_string(&asset).unwrap()).unwrap();
        assert_eq!(restored,asset);
    }
    #[test]
    fn print_html_and_native_print_use_vectors_in_layer_order() {
        use crate::sheet::{Artwork,Layout};
        let mut layout=Layout {count:2,width:20.,height:10.,page_width:50.,page_height:50.,..Default::default()};
        let item=Placement {x:15.,y:15.,width:20.,height:10.,orientation:Default::default()};
        layout.custom.insert(0,item);layout.custom.insert(1,item);
        layout.svg_sources.insert(0,asset());
        layout.image_tiles=vec![[0,0,20,10],[20,0,20,10]];
        let tile=Artwork {pixels:image::RgbaImage::from_pixel(20,10,image::Rgba([0,255,0,255])),outlines:vec![],holes:vec![],tiles:vec![],ellipse_bounds:None};
        let tile=std::sync::Arc::new(tile);
        let art=Artwork {pixels:tile.pixels.clone(),outlines:vec![],holes:vec![],tiles:vec![tile.clone(),tile],ellipse_bounds:None};
        let page=layout.print_raster(&art,None).unwrap();
        assert_eq!(page.get_pixel(295,236).0,[0,255,0,255]);
        layout.hidden_images.insert(1);
        let page=layout.print_raster(&art,None).unwrap();
        assert_eq!(page.get_pixel(220,236).0,[255,0,0,255]);
        let html=layout.print_html(&art).unwrap();
        assert!(html.contains("svg-clip-0"));
        assert!(!html.contains("data:image/png"));
    }
}
