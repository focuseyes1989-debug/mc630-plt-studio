use eframe::egui;
use geo::{BooleanOps, LineString, MultiPolygon, Polygon};
use mc630_plt_studio::Point;

#[derive(Clone,Copy,PartialEq,Eq)]
pub enum Target {Design(usize), Cut(usize)}
#[derive(Clone,Copy)]
pub enum Operation {Weld,Trim,Intersect}
impl Operation {
    pub fn label(self)->&'static str {match self {Self::Weld=>"Weld",Self::Trim=>"Trim",Self::Intersect=>"Intersect"}}
}

pub fn combine(paths:&[Vec<Point>],operation:Operation)->Result<Vec<(Vec<Point>,bool)>,String> {
    if paths.len()<2 {return Err("Shift+click two or more closed shapes.".into());}
    let mut polygons=Vec::new();
    for path in paths {
        if path.len()<4 || path.first()!=path.last() || path.iter().flatten().any(|v|!v.is_finite()) {
            return Err("Shaping requires closed Pen, Ellipse or Rectangle paths.".into());
        }
        polygons.push(Polygon::new(LineString::from(path.iter().map(|p|(p[0],p[1])).collect::<Vec<_>>()),vec![]));
    }
    let mut result=MultiPolygon(vec![polygons.remove(0)]);
    for polygon in polygons {
        result=match operation {Operation::Weld=>result.union(&polygon),Operation::Trim=>result.difference(&polygon),Operation::Intersect=>result.intersection(&polygon)};
    }
    if result.0.is_empty() {return Err("The operation leaves no shape. The original shapes were kept.".into());}
    let mut paths:Vec<(Vec<Point>,bool)>=Vec::new();
    for polygon in result.0 {
        paths.push((polygon.exterior().0.iter().map(|p|[p.x,p.y]).collect(),false));
        for ring in polygon.interiors() {paths.push((ring.0.iter().map(|p|[p.x,p.y]).collect(),true));}
    }
    if paths.len()>100 || paths.iter().any(|(path,_)|path.len()>4000) {return Err("Shaping result is too complex.".into());}
    Ok(paths)
}

pub fn button(ui:&mut egui::Ui,operation:Operation)->bool {
    let (rect,response)=ui.allocate_exact_size(egui::vec2(28.,28.),egui::Sense::click());
    let painter=ui.painter();
    if response.hovered() {painter.rect_filled(rect,4.,ui.visuals().selection.bg_fill);}
    let a=egui::Rect::from_min_size(rect.min+egui::vec2(4.,4.),egui::vec2(13.,13.));
    let b=a.translate(egui::vec2(6.,6.));
    let ink=ui.visuals().text_color();let selected=crate::ui_theme::ACCENT;
    match operation {
        Operation::Weld=>{painter.rect_filled(a,0.,selected);painter.rect_filled(b,0.,selected);}
        Operation::Trim=>{painter.rect_filled(a,0.,selected);painter.rect_filled(b,0.,ui.visuals().window_fill);}
        Operation::Intersect=>{painter.rect_filled(a.intersect(b),0.,selected);}
    }
    for rect in [a,b] {painter.rect_stroke(rect,0.,egui::Stroke::new(1_f32,ink),egui::StrokeKind::Inside);}
    response.widget_info(||egui::WidgetInfo::labeled(egui::WidgetType::Button,ui.is_enabled(),operation.label()));
    response.on_hover_text(format!("{} — Shift+click shapes; Trim subtracts later selections from the first",operation.label())).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn square(x:f64,y:f64,size:f64)->Vec<Point>{vec![[x,y],[x+size,y],[x+size,y+size],[x,y+size],[x,y]]}
    fn area(paths:Vec<(Vec<Point>,bool)>)->f64 {
        paths.iter().map(|(p,hole)|p.windows(2).map(|p|p[0][0]*p[1][1]-p[1][0]*p[0][1]).sum::<f64>().abs()/2.*if *hole{-1.}else{1.}).sum()
    }
    #[test]
    fn shaping_preserves_concavities_and_reports_holes_and_empty_intersections() {
        let paths=vec![square(0.,0.,20.),square(10.,10.,20.)];
        assert_eq!(area(combine(&paths,Operation::Weld).unwrap()),700.);
        assert_eq!(area(combine(&paths,Operation::Trim).unwrap()),300.);
        assert_eq!(area(combine(&paths,Operation::Intersect).unwrap()),100.);
        assert!(combine(&[square(0.,0.,20.),square(5.,5.,5.)],Operation::Trim).unwrap().iter().any(|(_,hole)|*hole));
        assert!(combine(&[square(0.,0.,5.),square(50.,50.,5.)],Operation::Intersect).is_err());
    }
}
