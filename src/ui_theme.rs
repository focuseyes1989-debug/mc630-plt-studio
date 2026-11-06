use eframe::egui;

pub const BUTTON_RADIUS: egui::CornerRadius = egui::CornerRadius::same(4);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.animation_time = 0.;
    style.visuals.panel_fill = egui::Color32::from_rgb(245, 246, 248);
    style.visuals.window_fill = egui::Color32::from_rgb(246, 247, 249);
    style.visuals.extreme_bg_color = egui::Color32::WHITE;
    style.visuals.override_text_color = Some(egui::Color32::from_rgb(38, 43, 50));
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(219, 232, 248);
    style.visuals.selection.stroke =
        egui::Stroke::new(1_f32, egui::Color32::from_rgb(56, 100, 157));
    for widgets in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widgets.corner_radius = BUTTON_RADIUS;
        widgets.bg_stroke = egui::Stroke::new(1_f32, egui::Color32::from_rgb(201, 207, 215));
    }
    style.visuals.widgets.inactive.bg_fill = egui::Color32::WHITE;
    style.visuals.widgets.inactive.weak_bg_fill = egui::Color32::WHITE;
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(235, 241, 248);
    style.visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(235, 241, 248);
    style.visuals.widgets.hovered.bg_stroke =
        egui::Stroke::new(1_f32, egui::Color32::from_rgb(122, 149, 182));
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(219, 232, 248);
    style.visuals.widgets.active.weak_bg_fill = style.visuals.widgets.active.bg_fill;
    style.visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1_f32, egui::Color32::from_rgb(218, 222, 228));
    style.visuals.window_corner_radius = egui::CornerRadius::same(7);
    style.visuals.window_stroke = egui::Stroke::new(1_f32, egui::Color32::from_rgb(195, 202, 212));
    style.visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 3],
        blur: 10,
        spread: 0,
        color: egui::Color32::from_black_alpha(28),
    };
    style.spacing.item_spacing = egui::vec2(6., 7.);
    style.spacing.button_padding = egui::vec2(8., 5.);
    style.spacing.interact_size = egui::vec2(30., 28.);
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
