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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Ellipse,
    Rectangle,
}

#[derive(Clone, Copy)]
pub struct Anchor {
    pub point: Point,
    pub tangent: Point,
}

pub struct Draft {
    pub tool: Tool,
    pub anchors: Vec<Anchor>,
    pub drag_start: Option<Point>,
}

impl Draft {
    pub fn new(tool: Tool) -> Self {
        Self {
            tool,
            anchors: vec![],
            drag_start: None,
        }
    }
}

pub fn pen_path(anchors: &[Anchor], close: bool) -> Vec<Point> {
    let Some(first) = anchors.first() else {
        return vec![];
    };
    let mut path = vec![first.point];
    for (a, b) in anchors
        .iter()
        .zip(anchors.iter().skip(1).chain(close.then_some(first)))
    {
        if a.tangent == [0.; 2] && b.tangent == [0.; 2] {
            path.push(b.point);
        } else {
            append_cubic(
                &mut path,
                [
                    a.point,
                    [a.point[0] + a.tangent[0], a.point[1] + a.tangent[1]],
                    [b.point[0] - b.tangent[0], b.point[1] - b.tangent[1]],
                    b.point,
                ],
                0.02,
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
        Tool::Pen => return vec![],
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
                    tangent: [0., -5.],
                },
                Anchor {
                    point: [20., 10.],
                    tangent: [0., 5.],
                },
            ],
            false,
        );
        assert!(pen.len() > 2 && pen.iter().any(|p| p[1] < 8.));
    }
}
