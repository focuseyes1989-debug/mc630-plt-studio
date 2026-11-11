use eframe::egui;

pub const BUTTON_RADIUS: egui::CornerRadius = egui::CornerRadius::same(5);
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(21, 116, 196);
pub const CANVAS: egui::Color32 = egui::Color32::from_rgb(235, 233, 230);

pub fn close_icon(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(22., 22.), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Close project")
    });
    if ui.is_rect_visible(rect) {
        let color = if !ui.is_enabled() {
            ui.visuals().weak_text_color()
        } else if response.hovered() {
            egui::Color32::from_rgb(190, 50, 50)
        } else {
            ui.visuals().text_color()
        };
        let center = rect.center();
        let stroke = egui::Stroke::new(1.4_f32, color);
        for direction in [-1., 1.] {
            ui.painter().line_segment(
                [
                    center + egui::vec2(-3.5, -3.5 * direction),
                    center + egui::vec2(3.5, 3.5 * direction),
                ],
                stroke,
            );
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.animation_time = 0.;
    let visuals = &mut style.visuals;
    visuals.panel_fill = egui::Color32::from_rgb(247, 246, 244);
    visuals.window_fill = visuals.panel_fill;
    visuals.extreme_bg_color = egui::Color32::WHITE;
    visuals.override_text_color = Some(egui::Color32::from_rgb(42, 44, 46));
    visuals.selection.bg_fill = egui::Color32::from_rgb(210, 232, 251);
    visuals.selection.stroke = egui::Stroke::new(1.5_f32, ACCENT);
    visuals.slider_trailing_fill = true;

    let border = egui::Stroke::new(1_f32, egui::Color32::from_rgb(207, 204, 200));
    for widgets in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widgets.corner_radius = BUTTON_RADIUS;
        widgets.bg_stroke = border;
        widgets.bg_fill = egui::Color32::WHITE;
        widgets.weak_bg_fill = egui::Color32::WHITE;
        widgets.expansion = 0.;
    }
    visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(237, 246, 254);
    visuals.widgets.hovered.bg_stroke =
        egui::Stroke::new(1_f32, egui::Color32::from_rgb(122, 170, 212));
    visuals.widgets.active.bg_stroke = visuals.selection.stroke;
    visuals.widgets.active.weak_bg_fill = visuals.selection.bg_fill;
    visuals.widgets.open.bg_stroke = visuals.selection.stroke;
    visuals.widgets.noninteractive.bg_stroke = border;
    visuals.widgets.noninteractive.bg_fill = CANVAS;
    visuals.window_corner_radius = egui::CornerRadius::same(8);
    visuals.window_stroke = border;
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: egui::Color32::from_black_alpha(32),
    };
    style.spacing.item_spacing = egui::vec2(7., 8.);
    style.spacing.button_padding = egui::vec2(9., 5.);
    style.spacing.interact_size = egui::vec2(30., 28.);
    style.spacing.window_margin = egui::Margin::same(14);
    style.spacing.slider_width = 148.;
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(13.));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(13.));
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(11.5));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(16.));
    ctx.set_style(style);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Import,
    Settings,
    Print,
    Outline,
    Grid,
    Fit,
    New,
    Open,
    Save,
    Artwork,
    AddImage,
    Export,
    Pen,
    Ellipse,
    Rectangle,
    Select,
    Close,
    Delete,
    Copy,
    Duplicate,
    Paste,
    RotateLeft,
    RotateRight,
    Refresh,
    Swap,
    Check,
    Undo,
    OpenPath,
    ClosedPath,
    Help,
}

pub fn icon_button(ui: &mut egui::Ui, icon: Icon, hint: &str) -> bool {
    icon_toggle(ui, icon, false, hint)
}

pub fn icon_toggle(ui: &mut egui::Ui, kind: Icon, selected: bool, hint: &str) -> bool {
    let (r, response) = ui.allocate_exact_size(egui::vec2(32., 32.), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, hint)
    });
    let painter = ui.painter();
    if selected || response.hovered() || response.has_focus() {
        painter.rect_filled(
            r,
            BUTTON_RADIUS,
            if selected {
                ui.visuals().selection.bg_fill
            } else {
                ui.visuals().widgets.hovered.weak_bg_fill
            },
        );
    }
    if response.has_focus() {
        painter.rect_stroke(
            r,
            BUTTON_RADIUS,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Inside,
        );
    }
    let ink = egui::Stroke::new(
        1.4_f32,
        if ui.is_enabled() {
            egui::Color32::from_gray(55)
        } else {
            egui::Color32::from_gray(150)
        },
    );
    let p = |x: f32, y: f32| r.min + egui::vec2(x, y);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([p(a.0, a.1), p(b.0, b.1)], ink);
    };
    match kind {
        Icon::Import => {
            line((8., 22.), (8., 26.));
            line((8., 26.), (24., 26.));
            line((24., 26.), (24., 22.));
            line((16., 22.), (16., 7.));
            line((10., 13.), (16., 7.));
            line((16., 7.), (22., 13.));
        }
        Icon::Settings => {
            for (y, x) in [(9., 12.), (16., 21.), (23., 15.)] {
                line((6., y), (26., y));
                painter.rect_filled(
                    egui::Rect::from_center_size(p(x, y), egui::vec2(4., 6.)),
                    1.,
                    egui::Color32::from_gray(60),
                );
            }
        }
        Icon::Print => {
            line((10., 6.), (23., 6.));
            line((10., 6.), (10., 12.));
            line((23., 6.), (23., 12.));
            for (a, b) in [
                ((6., 12.), (27., 12.)),
                ((6., 12.), (6., 23.)),
                ((27., 12.), (27., 23.)),
                ((6., 23.), (10., 23.)),
                ((23., 23.), (27., 23.)),
                ((10., 19.), (23., 19.)),
                ((10., 19.), (10., 28.)),
                ((23., 19.), (23., 28.)),
                ((10., 28.), (23., 28.)),
            ] {
                line(a, b);
            }
        }
        Icon::Outline => {
            painter.add(egui::Shape::closed_line(
                vec![p(6., 20.), p(10., 8.), p(22., 6.), p(27., 19.), p(18., 27.)],
                ink,
            ));
        }
        Icon::Grid => {
            for n in [7., 16., 25.] {
                line((n, 7.), (n, 25.));
                line((7., n), (25., n));
            }
        }
        Icon::New => {
            painter.add(egui::Shape::closed_line(
                vec![p(9., 5.), p(19., 5.), p(24., 10.), p(24., 27.), p(9., 27.)],
                ink,
            ));
            line((13., 17.), (20., 17.));
            line((16.5, 13.5), (16.5, 20.5));
        }
        Icon::Open => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(5., 24.),
                    p(5., 8.),
                    p(13., 8.),
                    p(16., 11.),
                    p(27., 11.),
                    p(24., 24.),
                ],
                ink,
            ));
            line((5., 24.), (9., 14.));
            line((9., 14.), (26., 14.));
        }
        Icon::Save => {
            painter.rect_stroke(
                egui::Rect::from_min_max(p(6., 6.), p(26., 26.)),
                2.,
                ink,
                egui::StrokeKind::Inside,
            );
            painter.rect_stroke(
                egui::Rect::from_min_max(p(10., 6.), p(21., 13.)),
                0.,
                ink,
                egui::StrokeKind::Inside,
            );
            painter.rect_stroke(
                egui::Rect::from_min_max(p(10., 18.), p(22., 26.)),
                0.,
                ink,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Artwork | Icon::AddImage => {
            let right = if kind == Icon::AddImage { 21. } else { 26. };
            painter.rect_stroke(
                egui::Rect::from_min_max(p(6., 7.), p(right, 25.)),
                2.,
                ink,
                egui::StrokeKind::Inside,
            );
            painter.circle_stroke(p(11., 12.), 1.6, ink);
            line((7., 22.), (13., 17.));
            line((13., 17.), (17., 20.));
            line((17., 20.), (right - 1., 16.));
            if kind == Icon::AddImage {
                line((22., 13.), (30., 13.));
                line((26., 9.), (26., 17.));
            }
        }
        Icon::Export => {
            line((7., 23.), (7., 27.));
            line((7., 27.), (25., 27.));
            line((25., 27.), (25., 23.));
            line((16., 6.), (16., 22.));
            line((11., 17.), (16., 22.));
            line((16., 22.), (21., 17.));
        }
        Icon::Pen => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(7., 25.),
                    p(10., 14.),
                    p(21., 7.),
                    p(25., 11.),
                    p(18., 22.),
                ],
                ink,
            ));
            line((7., 25.), (15., 17.));
            painter.circle_filled(p(15., 17.), 1.5, ink.color);
        }
        Icon::Ellipse => {
            painter.circle_stroke(p(16., 16.), 9., ink);
        }
        Icon::Select => {
            painter.add(egui::Shape::closed_line(
                vec![p(8., 5.), p(24., 18.), p(17., 19.), p(14., 27.)],
                ink,
            ));
        }
        Icon::Rectangle => {
            painter.rect_stroke(
                egui::Rect::from_min_max(p(6., 8.), p(26., 24.)),
                4.,
                ink,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Close => {
            line((9., 9.), (23., 23.));
            line((23., 9.), (9., 23.));
        }
        Icon::Delete => {
            line((7., 10.), (25., 10.));
            line((12., 7.), (20., 7.));
            line((10., 10.), (11., 25.));
            line((22., 10.), (21., 25.));
            line((11., 25.), (21., 25.));
            for x in [14., 18.] {
                line((x, 13.), (x, 22.));
            }
        }
        Icon::Copy | Icon::Duplicate => {
            line((7., 20.), (7., 6.));
            line((7., 6.), (21., 6.));
            painter.rect_stroke(
                egui::Rect::from_min_max(p(11., 10.), p(25., 26.)),
                2.,
                ink,
                egui::StrokeKind::Inside,
            );
            if kind == Icon::Duplicate {
                line((15., 18.), (21., 18.));
                line((18., 15.), (18., 21.));
            }
        }
        Icon::Paste => {
            painter.rect_stroke(
                egui::Rect::from_min_max(p(8., 8.), p(24., 26.)),
                2.,
                ink,
                egui::StrokeKind::Inside,
            );
            painter.rect(
                egui::Rect::from_min_max(p(12., 5.), p(20., 10.)),
                1.,
                ui.visuals().panel_fill,
                ink,
                egui::StrokeKind::Inside,
            );
            line((12., 15.), (20., 15.));
            line((12., 20.), (20., 20.));
        }
        Icon::RotateLeft | Icon::RotateRight | Icon::Refresh => {
            let direction = if kind == Icon::RotateLeft { -1. } else { 1. };
            let arc = (0..=20)
                .map(|i| {
                    let angle = -1.2 + i as f32 * 0.255;
                    p(16. + direction * 8. * angle.cos(), 16. + 8. * angle.sin())
                })
                .collect();
            painter.add(egui::Shape::line(arc, ink));
            let tip = (16. - direction * 6., 10.);
            line(tip, (tip.0, 4.));
            line(tip, (tip.0 + direction * 6., 10.));
            if kind != Icon::Refresh {
                line((14., 16.), (18., 16.));
                line((16., 16.), (16., 20.));
            }
        }
        Icon::Swap => {
            line((7., 11.), (25., 11.));
            line((25., 11.), (21., 7.));
            line((25., 11.), (21., 15.));
            line((25., 21.), (7., 21.));
            line((7., 21.), (11., 17.));
            line((7., 21.), (11., 25.));
        }
        Icon::Check => {
            line((8., 16.), (13., 21.));
            line((13., 21.), (24., 10.));
        }
        Icon::Undo => {
            line((6., 13.), (12., 7.));
            line((6., 13.), (12., 19.));
            painter.add(egui::Shape::line(
                vec![
                    p(6., 13.),
                    p(21., 13.),
                    p(25., 16.),
                    p(25., 22.),
                    p(21., 25.),
                    p(15., 25.),
                ],
                ink,
            ));
        }
        Icon::OpenPath => {
            painter.add(egui::Shape::line(
                vec![p(7., 23.), p(14., 10.), p(24., 18.)],
                ink,
            ));
            painter.circle_filled(p(24., 18.), 2., ink.color);
        }
        Icon::ClosedPath => {
            painter.add(egui::Shape::closed_line(
                vec![p(7., 23.), p(14., 8.), p(24., 19.)],
                ink,
            ));
        }
        Icon::Help => {
            painter.circle_stroke(p(16., 16.), 10., ink);
            painter.text(
                p(16., 16.),
                egui::Align2::CENTER_CENTER,
                "?",
                egui::FontId::proportional(15.),
                ink.color,
            );
        }
        Icon::Fit => {
            for (x, y, dx, dy) in [
                (7., 7., 7., 7.),
                (25., 7., -7., 7.),
                (7., 25., 7., -7.),
                (25., 25., -7., -7.),
            ] {
                line((x, y), (x + dx, y));
                line((x, y), (x, y + dy));
            }
        }
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint)
        .on_disabled_hover_text(hint)
        .clicked()
}
