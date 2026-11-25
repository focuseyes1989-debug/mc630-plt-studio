//! Native SVG silhouette extraction. Curves are flattened in physical millimetres,
//! independently of preview pixels and the optional raster smoothing controls.
use crate::{Point, svg_artwork::SvgArtwork};
use geo::{BooleanOps, Buffer, Simplify, LineString, MultiPolygon, Polygon};
use geo::algorithm::bool_ops::{FillRule, unary_union};

pub const CURVE_TOLERANCE_MM:f64=0.01;

impl SvgArtwork {
    pub fn vector_cut_paths(&self,tile:[u32;4],width:f64,height:f64,offset:f64,holes:bool)->Result<Vec<Vec<Point>>,String> {
        if !width.is_finite() || !height.is_finite() || width<=0. || height<=0. || !offset.is_finite() || offset.abs()>5. {return Err("Invalid SVG cutline size or offset.".into());}
        let asset=self.cropped(tile)?;
        let tree=crate::svg_artwork::tree(&asset.source)?;
        let size=tree.size();let viewport=asset.viewport;
        let map=|p:resvg::tiny_skia::Point|[(p.x as f64/size.width() as f64-viewport[0])/viewport[2]*width,(p.y as f64/size.height() as f64-viewport[1])/viewport[3]*height];
        let scale=(width/(size.width() as f64*viewport[2])).max(height/(size.height() as f64*viewport[3]));
        let mut polygons=Vec::new();let mut budget=100_000;
        collect(tree.root(),&map,scale,&mut polygons,&mut budget)?;
        if polygons.is_empty() {return Err("SVG has no visible vector paths to cut.".into());}
        // Union visible painted regions. Internal color boundaries are not cutlines.
        let silhouette=unary_union(&polygons);
        let frame=Polygon::new(LineString::from(vec![(0.,0.),(width,0.),(width,height),(0.,height),(0.,0.)]),vec![]);
        // Remove only sub-tolerance Boolean seams before offsetting. This is
        // numerical geometry cleanup, not user-adjustable contour smoothing.
        let mut result=silhouette.intersection(&frame).simplify(CURVE_TOLERANCE_MM*0.25);
        if offset!=0. {
            use geo::algorithm::buffer::{BufferStyle,LineJoin};
            result=result.buffer_with_style(BufferStyle::new(offset).line_join(LineJoin::Miter(0.75)));
        }
        result=result.simplify(CURVE_TOLERANCE_MM*0.25);
        let mut output=Vec::new();
        for polygon in result.0 {
            if holes {for ring in polygon.interiors() {output.push(ring.0.iter().map(|p|[p.x,p.y]).collect());}}
            output.push(polygon.exterior().0.iter().map(|p|[p.x,p.y]).collect());
        }
        if output.is_empty() {return Err("SVG outline distance removes the entire cutline.".into());}
        Ok(output)
    }
}

fn collect(group:&usvg::Group,map:&impl Fn(resvg::tiny_skia::Point)->Point,scale:f64,polygons:&mut Vec<Polygon<f64>>,budget:&mut usize)->Result<(),String> {
    if group.opacity().get()==0. {return Ok(());}
    if group.clip_path().is_some() || group.mask().is_some() || !group.filters().is_empty() {
        return Err("This SVG uses clips, masks or filters. Convert these effects to paths in the source editor, or draw a manual cutline.".into());
    }
    for node in group.children() {
        match node {
            usvg::Node::Group(group)=>collect(group,map,scale,polygons,budget)?,
            usvg::Node::Text(text)=>collect(text.flattened(),map,scale,polygons,budget)?,
            usvg::Node::Image(image) if image.is_visible()=>return Err("This SVG contains raster images. Use a manual cutline or convert the image to vector paths first.".into()),
            usvg::Node::Image(_)=>{},
            usvg::Node::Path(path)=>{
                if !path.is_visible() {continue;}
                if let Some(fill)=path.fill().filter(|fill|fill.opacity().get()>0.) {
                    if matches!(fill.paint(),usvg::Paint::Pattern(_)) {return Err("SVG pattern fills require a manual cutline or conversion to plain paths.".into());}
                    let data=path.data().clone().transform(path.abs_transform()).ok_or("Invalid SVG path transform")?;
                    let rule=match fill.rule() {usvg::FillRule::EvenOdd=>FillRule::EvenOdd,usvg::FillRule::NonZero=>FillRule::NonZero};
                    polygons.extend(filled(&data,map,rule,budget)?.0);
                }
                if let Some(stroke)=path.stroke().filter(|stroke|stroke.opacity().get()>0.) {
                    if matches!(stroke.paint(),usvg::Paint::Pattern(_)) {return Err("SVG pattern strokes require a manual cutline or conversion to plain paths.".into());}
                    let transform=path.abs_transform();
                    let transform_scale=(transform.sx as f64).hypot(transform.ky as f64).max((transform.kx as f64).hypot(transform.sy as f64));
                    let resolution=(scale*transform_scale/CURVE_TOLERANCE_MM).clamp(1.,10000.) as f32;
                    let stroke=stroke.to_tiny_skia();
                    let dashed=if let Some(dash)=&stroke.dash {Some(path.data().dash(dash,resolution).ok_or("Cannot expand SVG dashed stroke")?)}else{None};
                    let outline=dashed.as_ref().unwrap_or(path.data());
                    let data=outline.stroke(&stroke,resolution).ok_or("Cannot expand SVG stroke")?.transform(transform).ok_or("Invalid SVG stroke transform")?;
                    polygons.extend(filled(&data,map,FillRule::NonZero,budget)?.0);
                }
                if polygons.len()>4096 {return Err("SVG cutline geometry is too complex.".into());}
            }
        }
    }
    Ok(())
}

fn filled(path:&resvg::tiny_skia::Path,map:&impl Fn(resvg::tiny_skia::Point)->Point,rule:FillRule,budget:&mut usize)->Result<MultiPolygon<f64>,String> {
    use resvg::tiny_skia::PathSegment;
    let mut rings=Vec::new();let mut current=Vec::new();let mut last=[0.,0.];
    let flush=|current:&mut Vec<Point>,rings:&mut Vec<Polygon<f64>>| {
        current.dedup();
        if current.len()>=3 {
            if current.first()!=current.last() {current.push(current[0]);}
            let polygon=Polygon::new(LineString::from(current.iter().map(|p|(p[0],p[1])).collect::<Vec<_>>()),vec![]);
            rings.push(polygon);
        }
        current.clear();
    };
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p)=>{flush(&mut current,&mut rings);last=map(p);push(&mut current,last,budget)?;}
            PathSegment::LineTo(p)=>{last=map(p);push(&mut current,last,budget)?;}
            PathSegment::QuadTo(a,b)=>{
                let a=map(a);let b=map(b);
                let controls=[last,[last[0]+(a[0]-last[0])*2./3.,last[1]+(a[1]-last[1])*2./3.],[b[0]+(a[0]-b[0])*2./3.,b[1]+(a[1]-b[1])*2./3.],b];
                cubic(controls,0,&mut current,budget)?;last=b;
            }
            PathSegment::CubicTo(a,b,c)=>{let c=map(c);cubic([last,map(a),map(b),c],0,&mut current,budget)?;last=c;}
            PathSegment::Close=>{if let Some(first)=current.first() {last=*first;}flush(&mut current,&mut rings);}
        }
    }
    flush(&mut current,&mut rings);
    Ok(MultiPolygon(rings).union_with_fill_rule(&MultiPolygon::<f64>(vec![]),rule))
}

fn push(points:&mut Vec<Point>,p:Point,budget:&mut usize)->Result<(),String> {
    if *budget==0 || !p.iter().all(|v|v.is_finite()) {return Err("SVG cutline exceeds geometry limits.".into());}
    *budget-=1;points.push(p);Ok(())
}

fn cubic(p:[Point;4],depth:u8,out:&mut Vec<Point>,budget:&mut usize)->Result<(),String> {
    let dx=p[3][0]-p[0][0];let dy=p[3][1]-p[0][1];let denominator=dx*dx+dy*dy;
    let distance=|q:Point| {let t=if denominator>0. {(((q[0]-p[0][0])*dx+(q[1]-p[0][1])*dy)/denominator).clamp(0.,1.)}else{0.};(q[0]-p[0][0]-t*dx).hypot(q[1]-p[0][1]-t*dy)};
    if distance(p[1]).max(distance(p[2]))<=CURVE_TOLERANCE_MM {return push(out,p[3],budget);}
    if depth>=24 {return Err("SVG curve could not meet the cutline accuracy limit.".into());}
    let mid=|a:Point,b:Point|[(a[0]+b[0])/2.,(a[1]+b[1])/2.];
    let a=mid(p[0],p[1]);let b=mid(p[1],p[2]);let c=mid(p[2],p[3]);let d=mid(a,b);let e=mid(b,c);let f=mid(d,e);
    cubic([p[0],a,d,f],depth+1,out,budget)?;cubic([f,e,c,p[3]],depth+1,out,budget)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn asset(source:&str,size:[u32;2])->SvgArtwork {SvgArtwork::from_bytes(source.as_bytes(),size).unwrap()}
    #[test]
    fn circle_cutline_is_dpi_independent_and_meets_physical_curve_tolerance() {
        let source=r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><circle cx="50" cy="50" r="40" fill="red"/><circle cx="50" cy="50" r="12" fill="yellow"/></svg>"#;
        let low=asset(source,[100,100]);let high=asset(source,[4000,4000]);
        let a=low.vector_cut_paths(low.raster_rect,100.,100.,0.,true).unwrap();
        let b=high.vector_cut_paths(high.raster_rect,100.,100.,0.,true).unwrap();
        assert_eq!(a,b);assert_eq!(a.len(),1);assert_eq!(a[0].first(),a[0].last());
        for edge in a[0].windows(2) {
            let point=[(edge[0][0]+edge[1][0])/2.-50.,(edge[0][1]+edge[1][1])/2.-50.];
            assert!((point[0].hypot(point[1])-40.).abs()<0.025);
        }
        let offset=low.vector_cut_paths(low.raster_rect,100.,100.,1.,false).unwrap();
        let max=offset[0].iter().map(|p|p[0]).fold(f64::NEG_INFINITY,f64::max);
        assert!((max-91.).abs()<0.025);
    }
    #[test]
    fn compound_fill_rules_strokes_crops_and_transforms_follow_svg_geometry() {
        let source=r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><g transform="translate(10 5)"><path fill-rule="evenodd" d="M0 0H80V80H0Z M20 20H60V60H20Z"/></g></svg>"#;
        let asset=asset(source,[100,100]);
        assert_eq!(asset.vector_cut_paths(asset.raster_rect,100.,100.,0.,true).unwrap().len(),2);
        assert_eq!(asset.vector_cut_paths(asset.raster_rect,100.,100.,0.,false).unwrap().len(),1);
        let cropped=asset.vector_cut_paths([50,0,50,100],50.,100.,0.,true).unwrap();
        assert!(cropped.iter().flatten().all(|p|p[0]>=-1e-8 && p[0]<=50.00001));
        let stroke=super::tests::asset(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><path d="M20 50H80" fill="none" stroke="black" stroke-width="10" stroke-linecap="round"/></svg>"#,[100,100]);
        let paths=stroke.vector_cut_paths(stroke.raster_rect,100.,100.,0.,false).unwrap();
        let min=paths.iter().flatten().map(|p|p[0]).fold(f64::INFINITY,f64::min);
        assert!((min-15.).abs()<0.02);
    }
    #[test]
    fn native_svg_pipeline_preserves_corners_and_supports_optional_smoothing() {
        use crate::sheet::{Artwork,Layout,Placement};
        let asset=asset(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><path d="M10 10H90V90H10Z"/></svg>"#,[100,100]);
        let mut layout=Layout {count:1,width:50.,height:50.,smoothing:0.,..Default::default()};
        layout.svg_sources.insert(0,asset);
        layout.custom.insert(0,Placement {x:30.,y:30.,width:50.,height:50.,orientation:Default::default()}.with_rotation(90.));
        let art=Artwork {pixels:image::RgbaImage::new(100,100),outlines:vec![],holes:vec![],tiles:vec![],ellipse_bounds:None};
        let mut cache=crate::sheet::OutlineCache::default();
        let original=layout.page_cut_paths_cached(&art,&mut cache).unwrap();
        layout.smoothing=10.;let smooth=layout.page_cut_paths_cached(&art,&mut cache).unwrap();
        assert_eq!(original,smooth);assert_eq!(original[0].len(),5);
        layout.preserve_corners=false;layout.smoothing=0.5;
        let rounded=layout.page_cut_paths_cached(&art,&mut cache).unwrap();
        assert_ne!(rounded,original);assert_eq!(rounded[0].first(),rounded[0].last());
        layout.smoothing=0.;assert_eq!(layout.page_cut_paths_cached(&art,&mut cache).unwrap(),original);
        assert!(original[0].iter().all(|p|p[0]>=34.999 && p[0]<=75.001 && p[1]>=34.999 && p[1]<=75.001));
    }
}
