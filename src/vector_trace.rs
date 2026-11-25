use image::RgbaImage;

#[derive(Clone, PartialEq)]
pub struct Options {
    pub colors: usize,
    pub speckle: usize,
    pub corners: bool,
    pub remove_white: bool,
    pub detail: u32,
    pub color_merge: u8,
    pub smoothness: u8,
}
impl Default for Options {
    fn default()->Self {Self {colors:8,speckle:2,corners:true,remove_white:false,detail:1536,color_merge:18,smoothness:30}}
}
pub struct Traced {pub svg:String,pub preview:RgbaImage,pub paths:usize,pub colors:usize}

fn distance(a:[u8;3],b:[u8;3])->u32 {
    (0..3).map(|i|(a[i] as i32-b[i] as i32).pow(2) as u32).sum()
}

// Merge only genuinely similar palette entries. Keep their population weights:
// a handful of antialiased edge pixels must not recolor a large solid logo.
fn merge_palette(image:&RgbaImage,colors:Vec<[u8;3]>,tolerance:u8)->Vec<[u8;3]> {
    if tolerance==0 || colors.len()<2 {return colors;}
    let mut weights=vec![0_u64;colors.len()];
    let mut assignments=std::collections::HashMap::<[u8;3],usize>::new();
    for p in image.pixels().filter(|p|p[3]>0) {
        let rgb=[p[0],p[1],p[2]];
        let i=if let Some(i)=assignments.get(&rgb) {*i}else {
            let i=(0..colors.len()).min_by_key(|&i|distance(rgb,colors[i])).unwrap();
            if assignments.len()<65_536 {assignments.insert(rgb,i);}i
        };
        weights[i]+=1;
    }
    let mut groups:Vec<_>=colors.into_iter().zip(weights).filter(|(_,weight)|*weight>0).collect();
    let limit=tolerance as u32*tolerance as u32;
    loop {
        let closest=(0..groups.len()).flat_map(|a|(a+1..groups.len()).map(move |b|(a,b)))
            .map(|(a,b)|(distance(groups[a].0,groups[b].0),a,b)).min();
        let Some((_,a,b))=closest.filter(|(d,_,_)|*d<=limit) else {break};
        let (right,weight)=groups.remove(b);let (left,left_weight)=&mut groups[a];
        for axis in 0..3 {left[axis]=((left[axis] as u64* *left_weight+right[axis] as u64*weight)/(*left_weight+weight)) as u8;}
        *left_weight+=weight;
    }
    groups.into_iter().map(|(color,_)|color).collect()
}

// Flood from the canvas edge rather than deleting every white pixel, preserving
// enclosed white lettering/highlights inside the artwork.
fn remove_white_background(image:&mut RgbaImage) {
    let (w,h)=image.dimensions();let mut seen=vec![false;(w as usize)*(h as usize)];
    let mut queue=std::collections::VecDeque::new();
    for x in 0..w {queue.push_back((x,0));queue.push_back((x,h-1));}
    for y in 0..h {queue.push_back((0,y));queue.push_back((w-1,y));}
    while let Some((x,y))=queue.pop_front() {
        let index=(y*w+x) as usize;if seen[index] {continue;}seen[index]=true;
        let p=image.get_pixel(x,y);
        if p[3]>0 && !(p[0]>=245 && p[1]>=245 && p[2]>=245) {continue;}
        image.put_pixel(x,y,image::Rgba([0,0,0,0]));
        if x>0 {queue.push_back((x-1,y));}if x+1<w {queue.push_back((x+1,y));}
        if y>0 {queue.push_back((x,y-1));}if y+1<h {queue.push_back((x,y+1));}
    }
}

pub fn prepare_input(input:&RgbaImage,detail:u32)->RgbaImage {
    let detail=detail.clamp(1,2048);
    if input.width().max(input.height())<=detail {return input.clone();}
    // Resize premultiplied RGB, so transparent black/white RGB values cannot
    // bleed a fringe into the colors used to build the palette.
    let mut premultiplied=input.clone();
    for p in premultiplied.pixels_mut() {for a in 0..3 {p[a]=((p[a] as u16*p[3] as u16+127)/255) as u8;}}
    let mut resized=image::DynamicImage::ImageRgba8(premultiplied).resize(detail,detail,image::imageops::FilterType::Lanczos3).into_rgba8();
    for p in resized.pixels_mut().filter(|p|p[3]>0) {for a in 0..3 {p[a]=((p[a] as u32*255+p[3] as u32/2)/p[3] as u32).min(255) as u8;}}
    resized
}

// Weighted median-cut palette; alpha is a mask, never flattened onto white.
fn palette(image:&RgbaImage,count:usize)->Vec<[u8;3]> {
    let mut hist=std::collections::BTreeMap::<[u8;3],(u32,[u64;3])>::new();
    for p in image.pixels().filter(|p|p[3]>=128) {
        let entry=hist.entry([p[0]>>3,p[1]>>3,p[2]>>3]).or_default();entry.0+=1;
        for a in 0..3 {entry.1[a]+=p[a] as u64;}
    }
    let mut boxes:Vec<Vec<([u8;3],u32)>>=vec![hist.into_values().map(|(n,sum)|(std::array::from_fn(|a|(sum[a]/n as u64) as u8),n)).collect::<Vec<_>>()];
    while boxes.len()<count {
        let candidate=boxes.iter().enumerate().filter(|(_,b)|b.len()>1).map(|(i,b)| {
            let ranges:[u8;3]=std::array::from_fn(|a|b.iter().map(|(p,_)|p[a]).max().unwrap()-b.iter().map(|(p,_)|p[a]).min().unwrap());
            let axis=(0..3).max_by_key(|&a|ranges[a]).unwrap();
            (i,axis,ranges[axis] as u64*b.iter().map(|(_,n)|*n as u64).sum::<u64>())
        }).max_by_key(|v|v.2);
        let Some((index,axis,_))=candidate else {break};
        let mut b=boxes.remove(index);b.sort_by_key(|(p,_)|p[axis]);
        let half=b.iter().map(|(_,n)|*n as u64).sum::<u64>()/2;
        let mut sum=0;let split=b.iter().position(|(_,n)|{sum+=*n as u64;sum>=half}).unwrap_or(0)+1;
        let tail=b.split_off(split.clamp(1,b.len()-1));boxes.push(b);boxes.push(tail);
    }
    boxes.into_iter().filter(|b|!b.is_empty()).map(|b| {
        let total=b.iter().map(|(_,n)|*n as u64).sum::<u64>();
        std::array::from_fn(|a|(b.iter().map(|(p,n)|p[a] as u64* *n as u64).sum::<u64>()/total) as u8)
    }).collect()
}

pub fn trace(input:&RgbaImage,options:&Options)->Result<Traced,String> {
    if input.width()==0 || input.height()==0 || !(2..=32).contains(&options.colors) || !(256..=2048).contains(&options.detail) || options.speckle>16 || options.color_merge>64 || options.smoothness>100 {return Err("Invalid vector trace settings.".into());}
    let mut image=prepare_input(input,options.detail);
    for p in image.pixels_mut() {
        if p[3]<128 {*p=image::Rgba([0,0,0,0]);}else{p[3]=255;}
    }
    if options.remove_white {remove_white_background(&mut image);}
    let colors=merge_palette(&image,palette(&image,options.colors),options.color_merge);
    if colors.is_empty() {return Err("No visible artwork to trace. Check transparency and Remove white background.".into());}
    let mut color_cache=std::collections::HashMap::<[u8;3],[u8;3]>::new();
    for p in image.pixels_mut().filter(|p|p[3]>0) {
        let rgb=[p[0],p[1],p[2]];
        let color=if let Some(color)=color_cache.get(&rgb) {*color}else {
            let color=*colors.iter().min_by_key(|&&c|distance(rgb,c)).unwrap();
            if color_cache.len()<65_536 {color_cache.insert(rgb,color);}color
        };
        p.0=[color[0],color[1],color[2],255];
    }
    let (w,h)=image.dimensions();
    // A transparent border forces reliable alpha keying, even for edge-to-edge artwork.
    let mut padded=RgbaImage::new(w+2,h+2);image::imageops::overlay(&mut padded,&image,1,1);
    let mut config=vtracer::Config::default();
    config.color_precision=8;config.layer_difference=0;config.filter_speckle=options.speckle;
    config.corner_threshold=if options.corners {45}else{80};
    config.length_threshold=1.+options.smoothness as f64/20.;config.path_precision=Some(4);
    let mut traced=vtracer::convert(vtracer::ColorImage {width:(w+2) as usize,height:(h+2) as usize,pixels:padded.into_raw()},config)?;
    // Hierarchical clustering averages regions; snap those averages back to the
    // requested palette so the exported SVG honors the color limit too.
    for path in &mut traced.paths {
        let rgb=[path.color.r,path.color.g,path.color.b];
        let c=colors.iter().min_by_key(|c|(0..3).map(|a|(rgb[a] as i32-c[a] as i32).pow(2)).sum::<i32>()).unwrap();
        path.color.r=c[0];path.color.g=c[1];path.color.b=c[2];
    }
    let result=traced.to_string();
    let doc=roxmltree::Document::parse(&result).map_err(|e|e.to_string())?;
    let root=doc.root_element();let text=&result[root.range()];
    let inner=&text[text.find('>').ok_or("Invalid trace output")?+1..text.rfind("</svg>").ok_or("Invalid trace output")?];
    let svg=format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="1 1 {w} {h}">{inner}</svg>"#);
    if svg.len()>30_000_000 {return Err("Trace is too complex. Reduce colors or detail.".into());}
    let tree=crate::svg_artwork::tree(&svg)?;
    let canonical=tree.to_string(&usvg::WriteOptions::default());
    let asset=crate::svg_artwork::SvgArtwork::from_bytes(canonical.as_bytes(),[w,h])?;
    let preview=asset.raster([0,0,w,h],w,h,Default::default())?;
    let paths=canonical.matches("<path ").count();
    let doc=roxmltree::Document::parse(&canonical).map_err(|e|e.to_string())?;
    let used_colors=doc.descendants().filter(|node|node.has_tag_name("path")).map(|node|node.attribute("fill").unwrap_or("black")).collect::<std::collections::BTreeSet<_>>().len();
    Ok(Traced {svg:canonical,preview,paths,colors:used_colors})
}

pub fn export_svg(source:&str,size_mm:[f64;2])->Result<String,String> {
    if size_mm.iter().any(|v|!v.is_finite() || *v<=0.) {return Err("Invalid SVG export size.".into());}
    let tree=crate::svg_artwork::tree(source)?;
    let canonical=tree.to_string(&usvg::WriteOptions::default());
    let doc=roxmltree::Document::parse(&canonical).map_err(|e|e.to_string())?;
    let root=&canonical[doc.root_element().range()];
    let inner=&root[root.find('>').ok_or("Invalid SVG")?+1..root.rfind("</svg>").ok_or("Invalid SVG")?];
    Ok(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}mm" height="{}mm" viewBox="0 0 {} {}">{inner}</svg>"#,size_mm[0],size_mm[1],tree.size().width(),tree.size().height()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_trace_has_real_paths_transparency_and_no_embedded_bitmap() {
        let mut image=RgbaImage::new(80,60);
        for y in 8..52 {for x in 8..72 {image.put_pixel(x,y,image::Rgba(if x<40 {[230,20,30,255]}else{[20,40,230,255]}));}}
        let result=trace(&image,&Options::default()).unwrap();
        assert!(result.paths>=2);assert_eq!(result.colors,2);
        assert!(!result.svg.contains("<image"));assert_eq!(result.preview.dimensions(),(80,60));
        assert_eq!(result.preview.get_pixel(0,0)[3],0);
        assert!(result.preview.get_pixel(20,30)[0]>200);
        assert!(result.preview.get_pixel(60,30)[2]>200);
        let exported=export_svg(&result.svg,[50.,37.5]).unwrap();
        assert!(exported.contains("width=\"50mm\""));
        assert!(!exported.contains("<image"));
        assert!(!exported.contains("clipPath"));
        crate::svg_artwork::tree(&exported).unwrap();
        let asset=crate::svg_artwork::SvgArtwork::from_bytes(exported.as_bytes(),[80,60]).unwrap();
        assert!(!asset.vector_cut_paths([0,0,80,60],50.,37.5,0.,false).unwrap().is_empty());
    }
    #[test]
    fn empty_trace_is_rejected_and_white_removal_is_optional() {
        let image=RgbaImage::from_pixel(30,30,image::Rgba([255,255,255,255]));
        assert!(trace(&image,&Options {remove_white:true,..Default::default()}).is_err());
        assert!(trace(&image,&Options::default()).is_ok());
    }
    #[test]
    fn white_background_removal_keeps_enclosed_white_logo_details() {
        let mut input=RgbaImage::from_pixel(64,64,image::Rgba([255,255,255,255]));
        for y in 8..56 {for x in 8..56 {input.put_pixel(x,y,image::Rgba([0,60,180,255]));}}
        for y in 24..40 {for x in 24..40 {input.put_pixel(x,y,image::Rgba([255,255,255,255]));}}
        let traced=trace(&input,&Options {remove_white:true,..Default::default()}).unwrap();
        assert_eq!(traced.preview.get_pixel(0,0)[3],0);
        assert_eq!(traced.preview.get_pixel(30,30).0,[255,255,255,255]);
        assert!(traced.preview.get_pixel(12,12)[2]>150);
    }
    #[test]
    fn shade_merge_removes_flat_logo_noise_but_preserves_small_contrasting_detail() {
        let mut input=RgbaImage::new(80,64);
        for y in 4..60 {for x in 4..76 {
            input.put_pixel(x,y,image::Rgba(if (x/8+y/8)%2==0 {[0,45,120,255]}else{[4,48,124,255]}));
        }}
        for y in 28..36 {for x in 36..44 {input.put_pixel(x,y,image::Rgba([230,20,30,255]));}}
        let merged=trace(&input,&Options::default()).unwrap();
        let separate=trace(&input,&Options {color_merge:0,..Default::default()}).unwrap();
        assert_eq!(merged.colors,2);assert!(merged.paths<separate.paths);
        assert!(merged.preview.get_pixel(40,32)[0]>200);
        assert_eq!(merged.preview.get_pixel(0,0)[3],0);
    }
    #[test]
    fn downsampling_transparency_does_not_add_black_edge_fringe() {
        let mut input=RgbaImage::new(800,500);
        for y in 57..443 {for x in 133..667 {input.put_pixel(x,y,image::Rgba([255,30,10,255]));}}
        let resized=prepare_input(&input,256);
        assert!(resized.pixels().any(|p|p[3]>64 && p[3]<192));
        for p in resized.pixels().filter(|p|p[3]>64 && p[3]<192) {assert!(p[0]>=250,"dark fringe: {p:?}");}
    }
    #[test]
    fn color_artwork_retains_contrasting_colors_and_silhouette() {
        let mut input=RgbaImage::new(200,160);
        for y in 12..148 {for x in 12..188 {
            let color=if x<65 {[230,20,30,255]}else if x<130 {[10,150,30,255]}else{[20,40,230,255]};
            input.put_pixel(x,y,image::Rgba(color));
        }}
        let result=trace(&input,&Options {colors:16,corners:false,color_merge:4,smoothness:50,..Default::default()}).unwrap();
        assert_eq!(result.colors,3);
        assert!(result.preview.get_pixel(30,80)[0]>200);
        assert!(result.preview.get_pixel(95,80)[1]>120);
        assert!(result.preview.get_pixel(160,80)[2]>200);
        let (mut intersection,mut union)=(0.,0.);
        for (a,b) in input.pixels().zip(result.preview.pixels()) {
            if a[3]>=128 && b[3]>=128 {intersection+=1.;}
            if a[3]>=128 || b[3]>=128 {union+=1.;}
        }
        assert!(intersection/union>0.99,"silhouette overlap: {}",intersection/union);
    }
    #[test]
    #[ignore = "Local image trace diagnostic; set MC630_TRACE_IMAGE"]
    fn trace_real_logo() {
        let path=std::env::var("MC630_TRACE_IMAGE").unwrap();
        let input=image::open(path).unwrap().into_rgba8();let start=std::time::Instant::now();
        let result=trace(&input,&Options::default()).unwrap();
        std::fs::create_dir_all("target/vector-trace-check").unwrap();
        result.preview.save("target/vector-trace-check/preview.png").unwrap();
        let svg=export_svg(&result.svg,[81.196,78.404]).unwrap();
        std::fs::write("target/vector-trace-check/MERTON-traced.svg",&svg).unwrap();
        assert!(!svg.contains("<image"));
        let asset=crate::svg_artwork::SvgArtwork::from_bytes(svg.as_bytes(),[input.width(),input.height()]).unwrap();
        let outlines=asset.vector_cut_paths(asset.raster_rect,81.196,78.404,0.,false).unwrap();
        println!("Trace {:?}: {} colors, {} paths, {} SVG bytes, {} outer cut paths",start.elapsed(),result.colors,result.paths,svg.len(),outlines.len());
        let (mut intersection,mut union)=(0.,0.);
        for (a,b) in input.pixels().zip(result.preview.pixels()) {
            if a[3]>=128 && b[3]>=128 {intersection+=1.;}
            if a[3]>=128 || b[3]>=128 {union+=1.;}
        }
        println!("Original silhouette overlap: {:.4}%",100.*intersection/union);
        if let Ok(before)=std::fs::read("target/vector-trace-check/MERTON-before-quality.svg") {
            let before=crate::svg_artwork::SvgArtwork::from_bytes(&before,[input.width(),input.height()]).unwrap();
            before.raster(before.raster_rect,input.width(),input.height(),Default::default()).unwrap().save("target/vector-trace-check/before-quality.png").unwrap();
        }
    }

}
