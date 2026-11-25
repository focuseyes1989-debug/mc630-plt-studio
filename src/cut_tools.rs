use mc630_plt_studio::{Point, append_cubic};

pub fn bounds(path: &[Point]) -> [f64; 4] {
    path.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |[x0, y0, x1, y1], &[x, y]| [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
    )
}

pub fn hit(path: &[Point], point: Point, tolerance: f64) -> bool {
    let [x, y] = point;
    let mut inside = false;
    for edge in path.windows(2) {
        let [[ax, ay], [bx, by]] = [edge[0], edge[1]];
        let (dx, dy) = (bx - ax, by - ay);
        let length = dx * dx + dy * dy;
        let t = if length > 0. {
            (((x - ax) * dx + (y - ay) * dy) / length).clamp(0., 1.)
        } else {
            0.
        };
        if (x - ax - t * dx).hypot(y - ay - t * dy) <= tolerance {
            return true;
        }
        if (ay > y) != (by > y) && x < ax + (y - ay) * dx / dy {
            inside = !inside;
        }
    }
    path.first() == path.last() && inside
}

pub fn intersects_box(path:&[Point],rect:[f64;4])->bool {
    for pair in path.windows(2) {
        let (mut lo,mut hi)=(0_f64,1_f64);
        for axis in 0..2 {
            let delta=pair[1][axis]-pair[0][axis];
            if delta.abs()<1e-12 {
                if pair[0][axis]<rect[axis] || pair[0][axis]>rect[axis+2] {hi=-1.;break;}
            }else {
                let a=(rect[axis]-pair[0][axis])/delta;let b=(rect[axis+2]-pair[0][axis])/delta;
                lo=lo.max(a.min(b));hi=hi.min(a.max(b));
            }
        }
        if lo<=hi {return true;}
    }
    path.len()>2 && path.first()==path.last() && hit(path,[(rect[0]+rect[2])/2.,(rect[1]+rect[3])/2.],0.)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Line,
    Ellipse,
    Rectangle,
}

/// Keep a constrained straight line inside the page without changing its angle.
pub fn clip_line_end(start:Point,end:Point,page:Point)->Point {
    let delta=[end[0]-start[0],end[1]-start[1]];
    let mut fraction=1_f64;
    for axis in 0..2 {
        if delta[axis]>0. {fraction=fraction.min((page[axis]-start[axis])/delta[axis]);}
        else if delta[axis]<0. {fraction=fraction.min(-start[axis]/delta[axis]);}
    }
    let fraction=fraction.clamp(0.,1.);
    [start[0]+delta[0]*fraction,start[1]+delta[1]*fraction]
}

#[derive(Clone, Copy, PartialEq)]
pub struct Anchor {
    pub point: Point,
    pub tangent: Point,
    pub break_outgoing: bool,
    pub incoming: Option<Point>,
}

pub struct Draft {
    pub tool: Tool,
    pub anchors: Vec<Anchor>,
    pub drag_start: Option<Point>,
    pub edit_drag: Option<usize>,
    preview_cache: Option<(Vec<Anchor>, f64, Vec<Point>)>,
}

impl Draft {
    pub fn segment_at(&self, point: Point, tolerance: f64) -> Option<(usize, f64)> {
        let mut best = tolerance;
        let mut found = None;
        for (index, pair) in self.anchors.windows(2).enumerate() {
            let control = control_points(pair[0], pair[1]);
            let mut previous = control[0];
            for step in 1..=64 {
                let end = cubic_point(control, step as f64 / 64.);
                let d = [end[0]-previous[0],end[1]-previous[1]];
                let length=d[0]*d[0]+d[1]*d[1];
                let fraction=if length>0. {((point[0]-previous[0])*d[0]+(point[1]-previous[1])*d[1])/length} else {0.};
                let fraction=fraction.clamp(0.,1.);
                let distance=(point[0]-previous[0]-fraction*d[0]).hypot(point[1]-previous[1]-fraction*d[1]);
                let t=((step-1) as f64+fraction)/64.;
                if distance<best && t>0.001 && t<0.999 { best=distance;found=Some((index,t)); }
                previous=end;
            }
        }
        found
    }
    pub fn insert_anchor(&mut self, index: usize, t: f64) -> Option<usize> {
        if self.anchors.len()>=256 || index+1>=self.anchors.len() || !(0. ..1.).contains(&t) { return None; }
        let a=self.anchors[index]; let b=self.anchors[index+1];
        let c=control_points(a,b);
        let lerp=|a:Point,b:Point| [a[0]+(b[0]-a[0])*t,a[1]+(b[1]-a[1])*t];
        let p01=lerp(c[0],c[1]);let p12=lerp(c[1],c[2]);let p23=lerp(c[2],c[3]);
        let p012=lerp(p01,p12);let p123=lerp(p12,p23);let point=lerp(p012,p123);
        // Preserve existing curves exactly when subdividing, including asymmetric handles.
        let old_in=a.incoming.unwrap_or(a.tangent);
        self.anchors[index].incoming=Some(old_in);
        self.anchors[index].tangent=[p01[0]-a.point[0],p01[1]-a.point[1]];
        self.anchors[index].break_outgoing=false;
        self.anchors[index+1].incoming=Some([b.point[0]-p23[0],b.point[1]-p23[1]]);
        let straight=c[0]==c[1] && c[2]==c[3];
        self.anchors.insert(index+1,Anchor { point,
            tangent:if straight {[0.;2]}else{[p123[0]-point[0],p123[1]-point[1]]},
            incoming:Some(if straight {[0.;2]}else{[point[0]-p012[0],point[1]-p012[1]]}),break_outgoing:false });
        Some(index+1)
    }
    pub fn preview_path(&mut self, tolerance: f64) -> &[Point] {
        let stale = self.preview_cache.as_ref().is_none_or(|(anchors, old_tolerance, _)|
            anchors != &self.anchors || *old_tolerance != tolerance);
        if stale {
            self.preview_cache = Some((self.anchors.clone(), tolerance,
                pen_path_with_tolerance(&self.anchors, false, tolerance)));
        }
        &self.preview_cache.as_ref().unwrap().2
    }
    pub fn new(tool: Tool) -> Self {
        Self {
            tool,
            anchors: vec![],
            drag_start: None,
            edit_drag: None,
            preview_cache: None,
        }
    }
}

pub fn pen_path(anchors: &[Anchor], close: bool) -> Vec<Point> {
    pen_path_with_tolerance(anchors, close, 0.02)
}

pub fn pen_path_with_tolerance(anchors: &[Anchor], close: bool, tolerance: f64) -> Vec<Point> {
    let Some(first) = anchors.first() else {
        return vec![];
    };
    let mut path = vec![first.point];
    for (a, b) in anchors
        .iter()
        .zip(anchors.iter().skip(1).chain(close.then_some(first)))
    {
        let outgoing = if a.break_outgoing { [0.; 2] } else { a.tangent };
        let incoming = b.incoming.unwrap_or(b.tangent);
        if outgoing == [0.; 2] && incoming == [0.; 2] {
            path.push(b.point);
        } else {
            append_cubic(
                &mut path,
                [
                    a.point,
                    [a.point[0] + outgoing[0], a.point[1] + outgoing[1]],
                    [b.point[0] - incoming[0], b.point[1] - incoming[1]],
                    b.point,
                ],
                tolerance,
            );
        }
    }
    path
}

pub fn shape_path(
    tool: Tool,
    start: Point,
    mut end: Point,
    equal: bool,
    radius: f64,
) -> Vec<Point> {
    if tool == Tool::Line {
        if equal {end=constrain_pen(start,end);}
        return if (end[0]-start[0]).hypot(end[1]-start[1])>=0.2 {vec![start,end]}else{vec![]};
    }
    if equal {
        let dx = end[0] - start[0];
        let dy = end[1] - start[1];
        let size = dx.abs().min(dy.abs());
        end = [start[0] + size.copysign(dx), start[1] + size.copysign(dy)];
    }
    let [x, y] = [start[0].min(end[0]), start[1].min(end[1])];
    let [w, h] = [(start[0] - end[0]).abs(), (start[1] - end[1]).abs()];
    if w < 0.2 || h < 0.2 {
        return vec![];
    }
    let arc_steps =
        |r: f64| ((std::f64::consts::FRAC_PI_2 * (r / 0.16).sqrt()).ceil() as usize).clamp(4, 128);
    let mut path = match tool {
        Tool::Pen | Tool::Line => return vec![],
        Tool::Ellipse => {
            let steps = 4 * arc_steps(w.max(h) / 2.);
            (0..steps)
                .map(|i| {
                    let (sin, cos) = (std::f64::consts::TAU * i as f64 / steps as f64).sin_cos();
                    [x + (1. + cos) * w / 2., y + (1. + sin) * h / 2.]
                })
                .collect::<Vec<_>>()
        }
        Tool::Rectangle => {
            let r = radius.clamp(0., w.min(h) / 2.);
            if r == 0. {
                vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
            } else {
                let steps = arc_steps(r);
                let mut points = Vec::new();
                for (i, [cx, cy]) in [
                    [x + w - r, y + r],
                    [x + w - r, y + h - r],
                    [x + r, y + h - r],
                    [x + r, y + r],
                ]
                .into_iter()
                .enumerate()
                {
                    for j in 0..=steps {
                        let (sin, cos) = (std::f64::consts::FRAC_PI_2
                            * (i as f64 - 1. + j as f64 / steps as f64))
                            .sin_cos();
                        points.push([cx + r * cos, cy + r * sin]);
                    }
                }
                points
            }
        }
    };
    for p in &mut path {
        p[0] = p[0].clamp(x, x + w);
        p[1] = p[1].clamp(y, y + h);
    }
    path.push(path[0]);
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marquee_intersects_actual_segments_and_closed_interiors() {
        let line=vec![[0.,0.],[100.,100.]];
        assert!(!intersects_box(&line,[0.,80.,10.,90.]));
        assert!(intersects_box(&line,[45.,45.,55.,55.]));
        let closed=vec![[0.,0.],[100.,0.],[100.,100.],[0.,100.],[0.,0.]];
        assert!(intersects_box(&closed,[40.,40.,60.,60.]));
        assert!(!intersects_box(&closed,[110.,110.,120.,120.]));
    }
    #[test]
    fn line_tool_keeps_open_horizontal_vertical_and_constrained_segments() {
        assert_eq!(shape_path(Tool::Line,[10.,20.],[50.,20.],false,0.),vec![[10.,20.],[50.,20.]]);
        assert_eq!(shape_path(Tool::Line,[10.,20.],[10.,60.],false,0.),vec![[10.,20.],[10.,60.]]);
        let diagonal=shape_path(Tool::Line,[10.,20.],[35.,43.],true,0.);
        assert!(((diagonal[1][0]-10.)-(diagonal[1][1]-20.)).abs()<1e-8);
        assert!(shape_path(Tool::Line,[10.,20.],[10.01,20.01],false,0.).is_empty());
        let end=clip_line_end([200.,290.],[220.,310.],[210.,297.]);
        assert_eq!(end,[207.,297.]);
    }
    #[test]
    fn shapes_close_and_pen_drag_creates_a_curve() {
        let square = shape_path(Tool::Rectangle, [10., 10.], [30., 25.], true, 0.);
        assert_eq!(
            square,
            vec![[10., 10.], [25., 10.], [25., 25.], [10., 25.], [10., 10.]]
        );
        let rounded = shape_path(Tool::Rectangle, [10., 10.], [30., 25.], false, 2.);
        assert_eq!(rounded.first(), rounded.last());
        assert!(rounded.len() > square.len() && !rounded.contains(&[10., 10.]));
        let ellipse = shape_path(Tool::Ellipse, [10., 10.], [30., 25.], false, 0.);
        assert_eq!(ellipse.first(), ellipse.last());
        assert!(
            ellipse
                .iter()
                .all(|p| ((p[0] - 20.) / 10.).powi(2) + ((p[1] - 17.5) / 7.5).powi(2) <= 1.00001)
        );
        let pen = pen_path(
            &[
                Anchor {
                    point: [10., 10.],
                    tangent: [0., -5.], break_outgoing: false, incoming: None,
                },
                Anchor {
                    point: [20., 10.],
                    tangent: [0., 5.], break_outgoing: false, incoming: None,
                },
            ],
            false,
        );
        assert!(pen.len() > 2 && pen.iter().any(|p| p[1] < 8.));
    }
}

/// Photoshop-style Shift constraint to 45-degree increments.
pub fn constrain_pen(origin: Point, point: Point) -> Point {
    let (dx,dy) = (point[0]-origin[0],point[1]-origin[1]);
    let angle = (dy.atan2(dx)/std::f64::consts::FRAC_PI_4).round()*std::f64::consts::FRAC_PI_4;
    let length = dx*angle.cos()+dy*angle.sin();
    [origin[0]+length*angle.cos(),origin[1]+length*angle.sin()]
}

#[cfg(test)]
mod pen_constraint_tests {
    use super::*;
    #[test]
    fn shift_locks_horizontal_vertical_and_diagonal() {
        let a = constrain_pen([10.,20.],[30.,22.]);
        assert!((a[1]-20.).abs()<1e-8);
        let b = constrain_pen([10.,20.],[12.,40.]);
        assert!((b[0]-10.).abs()<1e-8);
        let c = constrain_pen([10.,20.],[30.,38.]);
        assert!(((c[0]-10.)-(c[1]-20.)).abs()<1e-8);
    }
}

#[cfg(test)]
mod preview_cache_tests {
    use super::*;
    #[test]
    fn cached_preview_matches_curve_and_invalidates_on_edit_and_zoom() {
        let mut draft = Draft::new(Tool::Pen);
        draft.anchors = vec![Anchor { point: [0.,0.], tangent: [10.,15.], break_outgoing: false, incoming: None }, Anchor { point: [40.,20.], tangent: [5.,-10.], break_outgoing: false, incoming: None }];
        let expected = pen_path_with_tolerance(&draft.anchors, false, 0.1);
        assert_eq!(draft.preview_path(0.1), expected);
        let address = draft.preview_path(0.1).as_ptr();
        assert_eq!(draft.preview_path(0.1).as_ptr(), address);
        draft.anchors[1].point = [50.,30.];
        assert_eq!(draft.preview_path(0.1).last(), Some(&[50.,30.]));
        let expected = pen_path_with_tolerance(&draft.anchors, false, 0.01);
        assert_eq!(draft.preview_path(0.01), expected);
        assert_eq!(pen_path(&draft.anchors,false),pen_path_with_tolerance(&draft.anchors,false,0.02));
    }
}

#[cfg(test)]
mod corner_handle_tests {
    use super::*;
    #[test]
    fn breaking_outgoing_preserves_incoming_curve() {
        let a = Anchor { point: [0.,0.], tangent: [0.,0.], break_outgoing: false, incoming: None };
        let mut b = Anchor { point: [20.,20.], tangent: [10.,0.], break_outgoing: false, incoming: None };
        let c = Anchor { point: [40.,20.], tangent: [0.,0.], break_outgoing: false, incoming: None };
        let incoming = pen_path(&[a,b],false);
        b.break_outgoing = true;
        assert_eq!(pen_path(&[a,b],false),incoming);
        assert_eq!(pen_path(&[b,c],false),vec![b.point,c.point]);
    }
}

fn control_points(a: Anchor, b: Anchor) -> [Point;4] {
    let out=if a.break_outgoing {[0.;2]}else{a.tangent};
    let incoming=b.incoming.unwrap_or(b.tangent);
    [a.point,[a.point[0]+out[0],a.point[1]+out[1]],
        [b.point[0]-incoming[0],b.point[1]-incoming[1]],b.point]
}
fn cubic_point(p: [Point;4], t: f64) -> Point {
    let u=1.-t;
    [0,1].map(|i| u*u*u*p[0][i]+3.*u*u*t*p[1][i]+3.*u*t*t*p[2][i]+t*t*t*p[3][i])
}

#[cfg(test)]
mod insert_tests {
    use super::*;
    fn anchor(point: Point,tangent: Point)->Anchor { Anchor{point,tangent,incoming:None,break_outgoing:false} }
    #[test]
    fn subdivision_keeps_original_curve_and_can_be_bent_afterward() {
        let a=anchor([0.,0.],[15.,25.]);let b=anchor([60.,20.],[20.,-10.]);
        let original=control_points(a,b);
        let mut draft=Draft::new(Tool::Pen);draft.anchors=vec![a,b];
        assert_eq!(draft.insert_anchor(0,0.4),Some(1));
        for n in 0..=100 {
            let t=n as f64/100.;
            let expected=cubic_point(original,t);
            let actual=if t<=0.4 {cubic_point(control_points(draft.anchors[0],draft.anchors[1]),t/0.4)}
                else {cubic_point(control_points(draft.anchors[1],draft.anchors[2]),(t-0.4)/0.6)};
            assert!((actual[0]-expected[0]).hypot(actual[1]-expected[1])<1e-8);
        }
        let before=pen_path(&draft.anchors,false);
        draft.anchors[1].tangent=[0.,20.];draft.anchors[1].incoming=None;
        assert_ne!(pen_path(&draft.anchors,false),before);
    }
    #[test]
    fn segment_hit_ignores_distant_pointer_and_inserts_on_straight_line() {
        let mut draft=Draft::new(Tool::Pen);
        draft.anchors=vec![anchor([0.,0.],[0.;2]),anchor([100.,0.],[0.;2])];
        assert!(draft.segment_at([50.,20.],2.).is_none());
        let (index,t)=draft.segment_at([50.,0.5],2.).unwrap();
        draft.insert_anchor(index,t).unwrap();
        assert!((draft.anchors[1].point[0]-50.).abs()<0.1);
        assert_eq!(draft.anchors[1].point[1],0.);
    }
}
