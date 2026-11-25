use eframe::egui;
use mc630_plt_studio::Point;

pub fn cursor(corner:usize)->egui::CursorIcon {
    match corner {4|6=>egui::CursorIcon::ResizeVertical,5|7=>egui::CursorIcon::ResizeHorizontal,0|2=>egui::CursorIcon::ResizeNwSe,_=>egui::CursorIcon::ResizeNeSw}
}

// Corners first, followed by top, right, bottom and left edge midpoints.
pub fn handles([x0,y0,x1,y1]:[f64;4])->[Point;8] {
    let cx=(x0+x1)/2.;let cy=(y0+y1)/2.;
    [[x0,y0],[x1,y0],[x1,y1],[x0,y1],[cx,y0],[x1,cy],[cx,y1],[x0,cy]]
}

pub fn transform(path:&[Point],mut delta:[f64;2],corner:Option<usize>,shift:bool,alt:bool,page:[f64;2])->Vec<Point> {
    let [x0,y0,x1,y1]=crate::cut_tools::bounds(path);
    let Some(corner)=corner else {
        if shift {if delta[0].abs()>=delta[1].abs() {delta[1]=0.;}else{delta[0]=0.;}}
        delta[0]=delta[0].clamp(-x0,(page[0]-x1).max(-x0));
        delta[1]=delta[1].clamp(-y0,(page[1]-y1).max(-y0));
        return path.iter().map(|p|[p[0]+delta[0],p[1]+delta[1]]).collect();
    };
    let signs=match corner {0=>[-1.,-1.],1=>[1.,-1.],2=>[1.,1.],3=>[-1.,1.],4=>[0.,-1.],5=>[1.,0.],6=>[0.,1.],_=>[-1.,0.]};
    let min=[x0,y0];let max=[x1,y1];
    let anchor=if alt {[(x0+x1)/2.,(y0+y1)/2.]}else{[if signs[0]==0. {(x0+x1)/2.}else if signs[0]<0. {x1}else{x0},if signs[1]==0. {(y0+y1)/2.}else if signs[1]<0. {y1}else{y0}]};
    let mut factor=[1.;2];let mut limit=[1.;2];let mut lower=[1.;2];
    for axis in 0..2 {
        let extent=((max[axis]-min[axis])*if alt || signs[axis]==0. {0.5}else{1.}).max(0.001);
        factor[axis]=1.+delta[axis]*signs[axis]/extent;
        lower[axis]=if max[axis]-min[axis]<=0.001 {0.}else{0.1/(max[axis]-min[axis])};
        limit[axis]=if alt || signs[axis]==0. {anchor[axis].min(page[axis]-anchor[axis])/extent}else if signs[axis]<0. {anchor[axis]/extent}else{(page[axis]-anchor[axis])/extent};
        limit[axis]=if max[axis]-min[axis]<=0.001 {f64::INFINITY}else{limit[axis].max(lower[axis])};
    }
    if shift {
        let factor=if (factor[0]-1.).abs()>=(factor[1]-1.).abs() {factor[0]}else{factor[1]};
        let factor=factor.clamp(lower[0].max(lower[1]),limit[0].min(limit[1]).max(lower[0].max(lower[1])));
        path.iter().map(|p|[anchor[0]+(p[0]-anchor[0])*factor,anchor[1]+(p[1]-anchor[1])*factor]).collect()
    }else{
        for axis in 0..2 {if signs[axis]!=0. {factor[axis]=factor[axis].clamp(lower[axis],limit[axis]);}}
        path.iter().map(|p|[anchor[0]+(p[0]-anchor[0])*factor[0],anchor[1]+(p[1]-anchor[1])*factor[1]]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edge_handles_resize_one_axis_and_support_center_and_ratio() {
        let path=vec![[20.,30.],[60.,30.],[60.,50.],[20.,50.],[20.,30.]];
        for (handle,delta,expected) in [
            (4,[9.,-10.],[20.,20.,60.,50.]),
            (5,[20.,9.],[20.,30.,80.,50.]),
            (6,[9.,10.],[20.,30.,60.,60.]),
            (7,[-10.,9.],[10.,30.,60.,50.]),
        ] {
            assert_eq!(crate::cut_tools::bounds(&transform(&path,delta,Some(handle),false,false,[210.,297.])),expected);
        }
        assert_eq!(crate::cut_tools::bounds(&transform(&path,[10.,9.],Some(5),false,true,[210.,297.])),[10.,30.,70.,50.]);
        assert_eq!(crate::cut_tools::bounds(&transform(&path,[20.,0.],Some(5),true,false,[210.,297.])),[20.,25.,80.,55.]);
        assert_eq!(cursor(4),egui::CursorIcon::ResizeVertical);
        assert_eq!(cursor(5),egui::CursorIcon::ResizeHorizontal);
        assert_eq!(handles([20.,30.,60.,50.])[4..],[[40.,30.],[60.,40.],[40.,50.],[20.,40.]]);
    }
    #[test]
    fn modifiers_preserve_ratio_center_axis_and_page_bounds() {
        let path=vec![[20.,30.],[60.,30.],[60.,50.],[20.,50.],[20.,30.]];
        let resized=transform(&path,[20.,5.],Some(2),true,false,[210.,297.]);
        assert_eq!(crate::cut_tools::bounds(&resized),[20.,30.,80.,60.]);
        let centered=transform(&path,[20.,10.],Some(2),true,true,[210.,297.]);
        assert_eq!(crate::cut_tools::bounds(&centered),[0.,20.,80.,60.]);
        let moved=transform(&path,[10.,3.],None,true,false,[210.,297.]);
        assert_eq!(crate::cut_tools::bounds(&moved),[30.,30.,70.,50.]);
        for corner in 0..4 {
            let result=transform(&path,[1000.,1000.],Some(corner),true,true,[210.,297.]);
            let [x0,y0,x1,y1]=crate::cut_tools::bounds(&result);
            assert!(x0>=-1e-8 && y0>=-1e-8 && x1<=210.00001 && y1<=297.00001);
        }
        assert_eq!(cursor(0),egui::CursorIcon::ResizeNwSe);
        assert_eq!(cursor(1),egui::CursorIcon::ResizeNeSw);
    }
}
