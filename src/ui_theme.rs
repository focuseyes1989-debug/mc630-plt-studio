use eframe::egui;

pub const BUTTON_RADIUS: egui::CornerRadius = egui::CornerRadius::same(5);
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0, 103, 192);
pub const CANVAS: egui::Color32 = egui::Color32::from_rgb(230, 233, 237);
pub const TAB_STRIP: egui::Color32 = egui::Color32::from_rgb(243, 243, 243);

/// Compact editor chrome; dialog and sidebar spacing stays unchanged.
pub fn compact_bar(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(8, 2))
}

pub fn compact_spacing(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing = egui::vec2(4., 2.);
    ui.spacing_mut().button_padding = egui::vec2(6., 3.);
    ui.spacing_mut().interact_size = egui::vec2(26., 26.);
}

pub fn underline_tab(ui: &egui::Ui, rect: egui::Rect) {
    let y = rect.bottom() - 1.;
    ui.painter().line_segment(
        [
            egui::pos2(rect.left() + 4., y),
            egui::pos2(rect.right() - 4., y),
        ],
        egui::Stroke::new(2_f32, ACCENT),
    );
}

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
    // Use the installed Windows font without distributing system font files.
    // egui's bundled fonts remain fallbacks on other platforms.
    let mut fonts = egui::FontDefinitions::default();
    #[cfg(windows)]
    if let Some(data) = [r"C:\Windows\Fonts\segoeui.ttf"]
        .iter()
        .find_map(|path| std::fs::read(path).ok())
    {
        fonts
            .font_data
            .insert("windows-ui".into(), egui::FontData::from_owned(data).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "windows-ui".into());
    }
    let icon_family = egui::FontFamily::Name("windows-icons".into());
    let mut native_icons = false;
    #[cfg(windows)]
    {
        let font_dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows")).join("Fonts");
        if let Some(data) = ["SegoeIcons.ttf", "segmdl2.ttf"].iter()
            .find_map(|name| std::fs::read(font_dir.join(name)).ok())
        {
            fonts.font_data.insert("windows-icons".into(), egui::FontData::from_owned(data).into());
            fonts.families.insert(icon_family, vec!["windows-icons".into()]);
            native_icons = true;
        }
    }
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("windows-icons-loaded"), native_icons));
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Light);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.animation_time = 0.10;
    let visuals = &mut style.visuals;
    visuals.panel_fill = egui::Color32::from_rgb(243, 243, 243);
    visuals.window_fill = egui::Color32::from_rgb(249, 249, 249);
    visuals.extreme_bg_color = egui::Color32::WHITE;
    visuals.override_text_color = Some(egui::Color32::from_rgb(32, 32, 32));
    visuals.selection.bg_fill = egui::Color32::from_rgb(229, 241, 251);
    visuals.selection.stroke = egui::Stroke::new(1.5_f32, ACCENT);
    visuals.slider_trailing_fill = true;

    let border = egui::Stroke::new(1_f32, egui::Color32::from_rgb(215, 215, 215));
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
    visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(243, 248, 252);
    visuals.widgets.hovered.bg_stroke =
        egui::Stroke::new(1_f32, egui::Color32::from_rgb(164, 194, 219));
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(243, 248, 252);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(229, 241, 251);
    visuals.widgets.open.bg_fill = egui::Color32::from_rgb(229, 241, 251);
    visuals.widgets.open.weak_bg_fill = egui::Color32::from_rgb(229, 241, 251);
    visuals.widgets.active.bg_stroke = visuals.selection.stroke;
    visuals.widgets.active.weak_bg_fill = visuals.selection.bg_fill;
    visuals.widgets.open.bg_stroke = visuals.selection.stroke;
    visuals.widgets.noninteractive.bg_stroke = border;
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(235, 235, 235);
    visuals.window_corner_radius = egui::CornerRadius::same(10);
    visuals.window_stroke = border;
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 20,
        spread: 0,
        color: egui::Color32::from_black_alpha(28),
    };
    style.spacing.item_spacing = egui::vec2(8., 8.);
    style.spacing.button_padding = egui::vec2(10., 5.);
    style.spacing.interact_size = egui::vec2(30., 28.);
    style.spacing.window_margin = egui::Margin::same(14);
    style.spacing.slider_width = 148.;
    style.spacing.slider_rail_height = 4.;
    visuals.handle_shape = egui::style::HandleShape::Circle;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(2_f32, ACCENT);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(2_f32, ACCENT);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(2_f32, ACCENT);
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
    Registration,
    Visibility,
    Link,
    Unlink,
    MarginLeft,
    MarginRight,
    MarginTop,
    MarginBottom,
    Portrait,
    Landscape,
    Crop,
    Grid,
    List,
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
    FlipHorizontal,
    FlipVertical,
    Refresh,
    Swap,
    Check,
    OpenPath,
    ClosedPath,
}

pub fn icon_button(ui: &mut egui::Ui, icon: Icon, hint: &str) -> bool {
    icon_toggle(ui, icon, false, hint)
}

pub fn icon_toggle(ui: &mut egui::Ui, kind: Icon, selected: bool, hint: &str) -> bool {
    let (r, response) = ui.allocate_exact_size(egui::Vec2::splat(ui.spacing().interact_size.y.min(32.)), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, hint)
    });
    let painter = ui.painter();
    if ui.is_enabled() && (selected || response.hovered() || response.has_focus()) {
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
    paint_icon(ui, r, kind);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint)
        .on_disabled_hover_text(hint)
        .clicked()
}

pub fn margin_indicator(ui: &mut egui::Ui, icon: Icon, hint: &str) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(26., 28.), egui::Sense::hover());
    paint_icon(ui, rect, icon);
    response.on_hover_text(hint);
}

pub fn tool_indicator(ui: &mut egui::Ui, kind: Icon, label: &str) {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(ui.spacing().interact_size.y.min(32.)), egui::Sense::hover());
    paint_icon(ui, rect, kind);
    response.labelled_by(ui.label(label).id);
}

fn paint_icon(ui: &egui::Ui, r: egui::Rect, kind: Icon) {
    let painter = ui.painter();
    if let Some(glyph) = windows_icon(kind)
        && ui.ctx().data(|data| data.get_temp::<bool>(egui::Id::new("windows-icons-loaded")).unwrap_or(false))
    {
        painter.text(r.center(), egui::Align2::CENTER_CENTER, glyph,
            egui::FontId::new(17., egui::FontFamily::Name("windows-icons".into())),
            if ui.is_enabled() { ui.visuals().text_color() } else { ui.visuals().weak_text_color() });
        return;
    }
    let ink = egui::Stroke::new(
        1.4_f32,
        if ui.is_enabled() {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        },
    );
    let p = |x: f32, y: f32| r.min + egui::vec2(x, y) * (r.width() / 32.);
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
                    ink.color,
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
        Icon::Portrait | Icon::Landscape => {
            let size = if kind == Icon::Portrait { egui::vec2(15.,22.) } else { egui::vec2(22.,15.) };
            let rect = egui::Rect::from_center_size(p(16.,16.),size * (r.width()/32.));
            painter.rect(rect,2.,ui.visuals().extreme_bg_color,ink,egui::StrokeKind::Inside);
            let inset = rect.shrink(4.);
            for t in [0.25,0.5,0.75] {
                let y = inset.top()+inset.height()*t;
                painter.line_segment([egui::pos2(inset.left(),y),egui::pos2(inset.right(),y)],
                    egui::Stroke::new(1_f32,ui.visuals().weak_text_color()));
            }
        }
        Icon::MarginLeft | Icon::MarginRight | Icon::MarginTop | Icon::MarginBottom => {
            painter.rect_stroke(egui::Rect::from_min_max(p(6.,5.),p(26.,27.)),2.,
                egui::Stroke::new(1_f32,egui::Color32::from_gray(160)),egui::StrokeKind::Inside);
            let (a,b) = match kind {
                Icon::MarginLeft => (p(11.,8.),p(11.,24.)),
                Icon::MarginRight => (p(21.,8.),p(21.,24.)),
                Icon::MarginTop => (p(10.,10.),p(22.,10.)),
                _ => (p(10.,22.),p(22.,22.)),
            };
            painter.line_segment([a,b],egui::Stroke::new(2.5_f32,ACCENT));
        }
        Icon::Link | Icon::Unlink => {
            for rect in [egui::Rect::from_min_max(p(5.,10.),p(17.,20.)), egui::Rect::from_min_max(p(15.,10.),p(27.,20.))] {
                painter.rect_stroke(rect,4.,ink,egui::StrokeKind::Inside);
            }
            if kind == Icon::Unlink {
                painter.line_segment([p(10.,26.),p(22.,5.)],egui::Stroke::new(3_f32,ui.visuals().panel_fill));
                line((10.,26.),(22.,5.));
            }
        }
        Icon::Visibility => {
            painter.add(egui::Shape::closed_line(vec![p(4.,16.), p(10.,10.), p(22.,10.), p(28.,16.), p(22.,22.), p(10.,22.)], ink));
            painter.circle_stroke(p(16.,16.), 3., ink);
        }
        Icon::Crop => {
            line((9., 4.), (9., 23.)); line((9., 23.), (28., 23.));
            line((4., 9.), (23., 9.)); line((23., 9.), (23., 28.));
        }
        Icon::Registration => {
            painter.circle_stroke(p(16., 16.), 7., ink);
            line((16., 4.), (16., 12.));
            line((16., 20.), (16., 28.));
            line((4., 16.), (12., 16.));
            line((20., 16.), (28., 16.));
        }
        Icon::Outline => {
            painter.add(egui::Shape::closed_line(
                vec![p(6., 20.), p(10., 8.), p(22., 6.), p(27., 19.), p(18., 27.)],
                ink,
            ));
        }
        Icon::List => {
            for y in [8.,16.,24.] { line((11.,y),(26.,y)); painter.circle_filled(p(6.,y),1.5,ink.color); }
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
                vec![p(9., 8.), p(23., 18.), p(17., 19.), p(14., 24.)],
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
        Icon::FlipHorizontal | Icon::FlipVertical => {
            let transform = |(x, y)| {
                if kind == Icon::FlipVertical {
                    (y, x)
                } else {
                    (x, y)
                }
            };
            for (a, b) in [
                ((16., 6.), (16., 26.)),
                ((7., 10.), (7., 22.)),
                ((7., 10.), (13., 16.)),
                ((7., 22.), (13., 16.)),
                ((25., 10.), (25., 22.)),
                ((25., 10.), (19., 16.)),
                ((25., 22.), (19., 16.)),
            ] {
                line(transform(a), transform(b));
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
}

pub const CUT_COLORS: [(&str, egui::Color32); 7] = [
    ("Red", egui::Color32::RED),
    ("Black", egui::Color32::BLACK),
    ("Gray", egui::Color32::GRAY),
    ("Green", egui::Color32::from_rgb(0, 135, 85)),
    ("Blue", egui::Color32::BLUE),
    ("Purple", egui::Color32::from_rgb(130, 50, 170)),
    ("Orange", egui::Color32::from_rgb(218, 113, 34)),
];

pub fn paint_workspace(painter: &egui::Painter, area: egui::Rect) {
    let visible = painter.clip_rect().intersect(area);
    painter.rect_filled(visible, 0., CANVAS);
    if !visible.is_positive() {
        return;
    }
    let mut dots = egui::Mesh::default();
    let spacing = 22.;
    let ink = egui::Color32::from_rgb(195, 199, 205);
    let mut y = (visible.top() / spacing).ceil() * spacing;
    while y < visible.bottom() {
        let mut x = (visible.left() / spacing).ceil() * spacing;
        while x < visible.right() {
            dots.add_colored_rect(
                egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(1.4, 1.4)),
                ink,
            );
            x += spacing;
        }
        y += spacing;
    }
    painter.add(egui::Shape::mesh(dots));
}

pub fn progress_bar(progress: f32) -> egui::ProgressBar {
    egui::ProgressBar::new(progress).desired_height(6.).corner_radius(3).fill(ACCENT)
}

pub fn spin_box(ui: &mut egui::Ui, value: &mut f64, step: f64, range: std::ops::RangeInclusive<f64>, suffix: &str) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.;
        let mut response = ui.add(egui::DragValue::new(value).speed(step).range(range.clone())
            .update_while_editing(false).suffix(suffix));
        let mut delta = 0.;
        if ui.small_button("-").on_hover_text("Decrease").clicked() { delta -= step; }
        if ui.small_button("+").on_hover_text("Increase").clicked() { delta += step; }
        if delta != 0. {
            let next = (*value + delta).clamp(*range.start(), *range.end());
            if next != *value { *value = next; response.mark_changed(); }
        }
        response
    }).inner
}

// Documented Segoe Fluent / MDL2 code points shared by Windows 10 and 11.
// Drawing-specific symbols retain their precise vector silhouettes.
fn windows_icon(kind: Icon) -> Option<char> {
    Some(match kind {
        Icon::Visibility => '\u{E890}',
        Icon::Crop => '\u{E7A8}',
        Icon::New => '\u{E7C3}',
        Icon::Open => '\u{E8E5}',
        Icon::Save => '\u{E74E}',
        Icon::Import => '\u{E898}',
        Icon::Export => '\u{E896}',
        Icon::Settings => '\u{E713}',
        Icon::Print => '\u{E749}',
        Icon::Artwork => '\u{EB9F}',
        Icon::AddImage => '\u{E710}',
        Icon::Grid => '\u{E80A}',
        Icon::Fit => '\u{E740}',
        Icon::Close => '\u{E711}',
        Icon::Delete => '\u{E74D}',
        Icon::Copy => '\u{E8C8}',
        Icon::Duplicate => '\u{E8A7}',
        Icon::Paste => '\u{E77F}',
        Icon::RotateRight => '\u{E7AD}',
        Icon::Refresh => '\u{E72C}',
        Icon::Check => '\u{E73E}',
        Icon::Pen => '\u{ED63}',
        _ => return None,
    })
}

/// The nib tip is the exact anchor hotspot; outlined for light and dark art.
pub fn paint_pen_cursor(ctx: &egui::Context, tip: egui::Pos2, closing: bool) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("pen-cursor")));
    let points = vec![tip, tip + egui::vec2(3.,12.),tip + egui::vec2(12.,16.),
        tip + egui::vec2(16.,12.),tip + egui::vec2(12.,3.)];
    painter.add(egui::Shape::convex_polygon(points.clone(),egui::Color32::WHITE,
        egui::Stroke::new(3_f32,egui::Color32::WHITE)));
    painter.add(egui::Shape::convex_polygon(points,egui::Color32::WHITE,
        egui::Stroke::new(1_f32,egui::Color32::BLACK)));
    painter.line_segment([tip,tip+egui::vec2(8.,8.)],egui::Stroke::new(1_f32,egui::Color32::BLACK));
    painter.circle_filled(tip+egui::vec2(8.,8.),1.5,egui::Color32::BLACK);
    if closing {
        painter.circle(tip+egui::vec2(20.,2.),3.,egui::Color32::WHITE,egui::Stroke::new(1_f32,egui::Color32::BLACK));
    }
}
