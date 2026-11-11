use eframe::egui;

#[derive(Clone, Copy)]
enum Anchor {
    Pointer(egui::Pos2),
    Center,
    Fit,
}

#[derive(Default)]
pub struct CanvasCamera {
    target: Option<(f32, Anchor)>,
    pan_drag: Option<(egui::Pos2, egui::Vec2)>,
}

impl CanvasCamera {
    pub fn origin(view: egui::Rect, zoom: f32, offset: egui::Vec2) -> egui::Pos2 {
        // Keep the coordinate basis used by saved Main canvas pan values.
        view.center() + offset + view.size() * ((zoom - 1.).max(0.) * 0.5)
    }

    pub fn is_panning(&self) -> bool {
        self.pan_drag.is_some()
    }

    pub fn cancel_zoom(&mut self) {
        self.target = None;
    }

    pub fn target_zoom(&self, zoom: f32) -> f32 {
        self.target.map_or(zoom, |(target, _)| target)
    }

    pub fn zoom_to(&mut self, ctx: &egui::Context, zoom: f32) {
        self.target = Some((zoom.clamp(0.5, 16.), Anchor::Center));
        ctx.request_repaint();
    }

    pub fn fit(&mut self, ctx: &egui::Context) {
        self.target = Some((1., Anchor::Fit));
        ctx.request_repaint();
    }

    pub fn update(
        &mut self,
        ui: &egui::Ui,
        view: egui::Rect,
        offset: egui::Vec2,
        zoom: &mut f32,
        pan: &mut egui::Vec2,
        allow_navigation: bool,
    ) -> bool {
        let ctx = ui.ctx();
        let over = ui.rect_contains_pointer(view);
        let space = !ctx.wants_keyboard_input() && ui.input(|i| i.key_down(egui::Key::Space));
        let (pointer, pressed, down) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
            )
        });
        if over
            && space
            && pressed
            && allow_navigation
            && let Some(pointer) = pointer
        {
            self.target = None;
            self.pan_drag = Some((pointer, *pan));
        }
        let dragging = self.pan_drag.is_some();
        if let Some((start, initial_pan)) = self.pan_drag {
            if let Some(pointer) = pointer {
                *pan = initial_pan + (pointer - start);
            }
            if !down {
                self.pan_drag = None;
            }
        }
        if dragging || (space && over && allow_navigation) {
            ctx.set_cursor_icon(if dragging {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
        }
        if over && !dragging && allow_navigation {
            let (ctrl, raw, smooth) = ui.input_mut(|i| {
                let result = (i.modifiers.ctrl, i.raw_scroll_delta, i.smooth_scroll_delta);
                i.raw_scroll_delta = egui::Vec2::ZERO;
                i.smooth_scroll_delta = egui::Vec2::ZERO;
                result
            });
            if ctrl && raw.y != 0. {
                let factor = (raw.y.clamp(-240., 240.) * 0.002).exp();
                let target = (self.target_zoom(*zoom) * factor).clamp(0.5, 16.);
                self.target = Some((target, Anchor::Pointer(pointer.unwrap_or(view.center()))));
            } else if !ctrl && smooth != egui::Vec2::ZERO {
                if matches!(self.target, Some((_, Anchor::Fit))) {
                    self.cancel_zoom();
                }
                *pan += smooth;
            }
        }
        if !allow_navigation {
            // A resize or shape drag needs a stationary coordinate transform.
            self.target = None;
        } else if let Some((target, anchor)) = self.target {
            let dt = ui.input(|i| i.stable_dt).clamp(0., 0.05);
            let blend = 1. - (-22. * dt).exp();
            let remaining = (target / *zoom).ln();
            let settled_zoom = remaining.abs() < 0.001;
            let next = if settled_zoom {
                target
            } else {
                *zoom * (remaining * blend).exp()
            };
            match anchor {
                Anchor::Fit => {
                    let pan_settled = pan.length_sq() < 0.04;
                    *pan = if pan_settled {
                        egui::Vec2::ZERO
                    } else {
                        *pan * (1. - blend)
                    };
                    if settled_zoom && pan_settled {
                        self.target = None;
                    }
                }
                _ => {
                    let focus = match anchor {
                        Anchor::Pointer(pointer) => pointer,
                        _ => view.center(),
                    };
                    *pan = anchored_pan(view, offset, *zoom, next, *pan, focus);
                    if settled_zoom {
                        self.target = None;
                    }
                }
            }
            *zoom = next;
            if self.target.is_some() {
                ctx.request_repaint();
            }
        }
        dragging || (space && over && allow_navigation)
    }
}

fn anchored_pan(
    view: egui::Rect,
    offset: egui::Vec2,
    before: f32,
    after: f32,
    pan: egui::Vec2,
    focus: egui::Pos2,
) -> egui::Vec2 {
    let center = CanvasCamera::origin(view, before, offset) + pan;
    let desired_center = focus + (center - focus) * (after / before);
    desired_center - CanvasCamera::origin(view, after, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_anchor_stays_fixed_when_zoom_crosses_fit_scale() {
        let view = egui::Rect::from_min_size(egui::pos2(40., 150.), egui::vec2(900., 600.));
        let offset = egui::vec2(14., 12.);
        let focus = egui::pos2(400., 370.);
        let mut pan = egui::vec2(60., -35.);
        let mut zoom = 0.8;
        let point = (focus - (CanvasCamera::origin(view, zoom, offset) + pan)) / zoom;
        for next in [0.95, 1., 1.2, 2., 1.5, 0.8] {
            pan = anchored_pan(view, offset, zoom, next, pan, focus);
            zoom = next;
            let actual = CanvasCamera::origin(view, zoom, offset) + pan + point * zoom;
            assert!(actual.distance(focus) < 0.001);
        }
    }
}
