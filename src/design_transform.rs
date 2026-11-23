use eframe::egui;
use mc630_plt_studio::Point;

pub fn cursor(corner:usize)->egui::CursorIcon {
    if corner%2==0 {egui::CursorIcon::ResizeNwSe}else{egui::CursorIcon::ResizeNeSw}
}

pub fn transform(path:&[Point],mut delta:[f64;2],corner:Option<usize>,shift:bool,alt:bool,page:[f64;2])->Vec<Point> {
    let [x0,y0,x1,y1]=crate::cut_tools::bounds(path);
    let Some(corner)=corner else {
        if shift {if delta[0].abs()>=delta[1].abs() {delta[1]=0.;}else{delta[0]=0.;}}
        delta[0]=delta[0].clamp(-x0,(page[0]-x1).max(-x0));
        delta[1]=delta[1].clamp(-y0,(page[1]-y1).max(-y0));
        return path.iter().map(|p|[p[0]+delta[0],p[1]+delta[1]]).collect();
    };
    let signs=[if corner==0 || corner==3 {-1.}else{1.},if corner<2 {-1.}else{1.}];
    let min=[x0,y0];let max=[x1,y1];
    let anchor=if alt {[(x0+x1)/2.,(y0+y1)/2.]}else{[if signs[0]<0. {x1}else{x0},if signs[1]<0. {y1}else{y0}]};
    let mut factor=[1.;2];let mut limit=[1.;2];let mut lower=[1.;2];
    for axis in 0..2 {
        let extent=((max[axis]-min[axis])*if alt {0.5}else{1.}).max(0.001);
        factor[axis]=1.+delta[axis]*signs[axis]/extent;
        lower[axis]=0.1/(max[axis]-min[axis]).max(0.001);
        limit[axis]=if alt {anchor[axis].min(page[axis]-anchor[axis])/extent}else if signs[axis]<0. {anchor[axis]/extent}else{(page[axis]-anchor[axis])/extent};
        limit[axis]=limit[axis].max(lower[axis]);
    }
    if shift {
        let factor=if (factor[0]-1.).abs()>=(factor[1]-1.).abs() {factor[0]}else{factor[1]};
        let factor=factor.clamp(lower[0].max(lower[1]),limit[0].min(limit[1]).max(lower[0].max(lower[1])));
        path.iter().map(|p|[anchor[0]+(p[0]-anchor[0])*factor,anchor[1]+(p[1]-anchor[1])*factor]).collect()
    }else{
        for axis in 0..2 {factor[axis]=factor[axis].clamp(lower[axis],limit[axis]);}
        path.iter().map(|p|[anchor[0]+(p[0]-anchor[0])*factor[0],anchor[1]+(p[1]-anchor[1])*factor[1]]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
