use eframe::egui;

use mc630_plt_studio::sheet::{self, Artwork, Layout};

use crate::cut_tools::{self, Anchor, Draft, Tool};
use crate::preferences::{Preferences, Unit};
use std::path::PathBuf;

fn group_move_delta(
    members: &[(usize, sheet::Placement)],
    dx: f64,
    dy: f64,
    inset: f64,
    page: [f64; 2],
) -> (f64, f64) {
    let mut low = [f64::NEG_INFINITY; 2];
    let mut high = [f64::INFINITY; 2];
    for (_, p) in members {
        low[0] = low[0].max(inset - p.x);
        low[1] = low[1].max(inset - p.y);
        high[0] = high[0].min(page[0] - inset - p.x - p.width);
        high[1] = high[1].min(page[1] - inset - p.y - p.height);
    }
    let limit = |v: f64, a: f64, b: f64| if a <= b { v.clamp(a, b) } else { 0. };
    (limit(dx, low[0], high[0]), limit(dy, low[1], high[1]))
}
fn group_bounds(members: &[(usize, sheet::Placement)]) -> Option<sheet::Placement> {
    let mut b = members.first()?.1;
    for (_, p) in &members[1..] {
        let right = (b.x + b.width).max(p.x + p.width);
        let bottom = (b.y + b.height).max(p.y + p.height);
        b.x = b.x.min(p.x);
        b.y = b.y.min(p.y);
        b.width = right - b.x;
        b.height = bottom - b.y;
    }
    Some(b)
}
fn group_resize(
    members: &[(usize, sheet::Placement)],
    corner: usize,
    delta: [f64; 2],
    lock: bool,
    inset: f64,
    page: [f64; 2],
) -> Vec<(usize, sheet::Placement)> {
    let Some(b) = group_bounds(members) else {
        return vec![];
    };
    let target = b.resized_on(corner, delta[0], delta[1], lock, inset, page);
    let left = corner == 0 || corner == 3;
    let top = corner == 0 || corner == 1;
    let anchor = [
        if left { b.x + b.width } else { b.x },
        if top { b.y + b.height } else { b.y },
    ];
    let minx = members.iter().map(|(_, p)| 1. / p.width).fold(0., f64::max);
    let miny = members
        .iter()
        .map(|(_, p)| 1. / p.height)
        .fold(0., f64::max);
    let maxx = (if left {
        anchor[0] - inset
    } else {
        page[0] - inset - anchor[0]
    }) / b.width;
    let maxy = (if top {
        anchor[1] - inset
    } else {
        page[1] - inset - anchor[1]
    }) / b.height;
    if minx > maxx || miny > maxy || (lock && minx.max(miny) > maxx.min(maxy)) {
        return members.to_vec();
    }
    let (sx, sy) = if lock {
        let scale = (target.width / b.width).clamp(minx.max(miny), maxx.min(maxy));
        (scale, scale)
    } else {
        (
            (target.width / b.width).clamp(minx, maxx),
            (target.height / b.height).clamp(miny, maxy),
        )
    };
    let x = if left {
        anchor[0] - b.width * sx
    } else {
        anchor[0]
    };
    let y = if top {
        anchor[1] - b.height * sy
    } else {
        anchor[1]
    };
    members
        .iter()
        .map(|(i, p)| {
            (
                *i,
                sheet::Placement {
                    x: x + (p.x - b.x) * sx,
                    y: y + (p.y - b.y) * sy,
                    width: p.width * sx,
                    height: p.height * sy,
                },
            )
        })
        .collect()
}
struct CanvasDrag {
    pointer: egui::Pos2,
    corner: Option<usize>,
    before: std::collections::BTreeMap<usize, sheet::Placement>,
    members: Vec<(usize, sheet::Placement)>,
    duplicate_target: Option<usize>,
    axis_locked: bool,
    additive_click: bool,
}
impl CanvasDrag {
    fn locked_axis(&self, delta: egui::Vec2) -> Option<usize> {
        self.axis_locked
            .then_some(usize::from(delta.y.abs() > delta.x.abs()))
    }
    fn duplicate_offset(&self, delta: egui::Vec2, inset: f64, page: [f64; 2]) -> (f64, f64) {
        let (dx, dy) = group_move_delta(&self.members, delta.x.into(), delta.y.into(), inset, page);
        match self.locked_axis(delta) {
            Some(0) => (dx, 0.),
            Some(_) => (0., dy),
            None => (dx, dy),
        }
    }
}
enum ObjectEdit {
    Positions(std::collections::BTreeMap<usize, sheet::Placement>),
    Layers(Box<Layout>),
    LastImage(Box<Layout>, Vec<u8>, String),
    Visibility(std::collections::BTreeSet<usize>),
    ManualCuts(Vec<Vec<mc630_plt_studio::Point>>, bool),
}
struct GuideDrag {
    axis: usize,
    index: Option<usize>,
    value: f64,
    grab_offset: f64,
}
struct CutShapeDrag {
    index: usize,
    start: mc630_plt_studio::Point,
    path: Vec<mc630_plt_studio::Point>,
    corner: Option<usize>,
}
struct PreviewCache {
    layout: Layout,
    ccd: bool,
    profile: sheet::CcdTrial,
    paths: Result<Vec<Vec<[f64; 2]>>, String>,
}
#[derive(Clone, Debug, PartialEq)]
enum DocumentAction {
    New,
    OpenImage(PathBuf),
    OpenImageGroup(Vec<PathBuf>),
    OpenProject(PathBuf),
    CloseImage,
    CloseProject,
    #[cfg(test)]
    Quit,
}
#[derive(Clone, Copy)]
enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}
pub enum WorkspaceAction {
    SaveAll,
    CloseAll,
    VectorWorkspace,
    New,
    Open(PathBuf),
    Recover(Box<A4Studio>),
}

pub struct A4Studio {
    pub workspace_enabled: bool,
    pub workspace_action: Option<WorkspaceAction>,
    pub tab_closed: bool,
    pub close_canceled: bool,
    reopen_last_project: bool,
    startup_pending: bool,
    startup_project: Option<PathBuf>,
    recent_projects: crate::recent::RecentProjects,
    autosave: crate::recovery::AutoSave,
    autosave_enabled: bool,
    autosave_seconds: u64,
    saved_signature: u64,
    source_digest: u64,
    pending_action: Option<DocumentAction>,
    close_approved: bool,

    unit: Unit,
    show_settings: bool,
    show_properties: bool,
    palette_geometry: Option<crate::palette_state::Geometry>,
    section_palettes: [crate::palette_state::Section; 3],
    show_margin_guides: bool,
    nudge_fine: f64,
    nudge_coarse: f64,
    preview_cache: Option<PreviewCache>,
    outline_cache: sheet::OutlineCache,
    capacity_cache: Option<(Layout, Result<usize, String>)>,
    art: Option<Artwork>,

    texture: Option<egui::TextureHandle>,

    source: Vec<u8>,

    name: String,

    layout: Layout,

    remove_white: bool,
    cut_shape: sheet::CutShape,
    background_tolerance: u8,
    print_without_background: bool,

    threshold: u8,

    status: String,

    show_cut: bool,
    cut_draft: Option<Draft>,
    cut_corner_radius: f64,
    pick_cut_shapes: bool,
    selected_cut_shape: Option<usize>,
    cut_shape_drag: Option<CutShapeDrag>,
    show_print_dialog: bool,
    zoom: f32,
    canvas_pan: egui::Vec2,
    panning: bool,
    rulers: bool,
    guide_drag: Option<GuideDrag>,
    selected_guide: Option<(usize, usize)>,
    snap_guides: bool,
    grid: bool,
    show_guide: bool,
    show_new: bool,
    new_preset: usize,
    new_width: f64,
    new_height: f64,
    new_landscape: bool,
    contour_color: egui::Color32,
    selected: Option<usize>,
    selection: std::collections::BTreeSet<usize>,
    selection_anchor: Option<usize>,
    drag: Option<CanvasDrag>,
    undo: Vec<ObjectEdit>,
    redo: Vec<ObjectEdit>,
    duplicate_step: Option<(f64, f64)>,

    ratio: f64,

    lock: bool,

    last_print: Option<PathBuf>,
    project_path: Option<PathBuf>,

    preview_mark_origin: bool,

    ccd_trial: bool,

    ccd_profile: sheet::CcdTrial,

    #[cfg(windows)]
    print_panel: crate::native_print::PrintPanel,
}

impl Default for A4Studio {
    fn default() -> Self {
        #[allow(unused_mut)]
        let mut app=Self{workspace_enabled:false,workspace_action:None,tab_closed:false,close_canceled:false,reopen_last_project:false,startup_pending:true,startup_project:None,recent_projects:Default::default(),autosave:Default::default(),autosave_enabled:true,autosave_seconds:15,saved_signature:0,source_digest:0,pending_action:None,close_approved:false,unit:Unit::Inches,show_settings:false,show_properties:false,palette_geometry:None,section_palettes:Default::default(),show_margin_guides:true,nudge_fine:0.1,nudge_coarse:1.,preview_cache:None,outline_cache:Default::default(),capacity_cache:None,art:None,texture:None,source:vec![],name:"No image selected".into(),layout:Layout{offset_x:-7.,offset_y:-7.,..Layout::default()},remove_white:false,cut_shape:sheet::CutShape::AlphaOutline,background_tolerance:20,print_without_background:true,threshold:245,status:"Choose a transparent PNG. Repeat it on A4 and export matching PRINT.html + CUT.plt.".into(),show_cut:true,cut_draft:None,cut_corner_radius:0.,pick_cut_shapes:false,selected_cut_shape:None,cut_shape_drag:None,show_print_dialog:false,zoom:1.,canvas_pan:egui::Vec2::ZERO,panning:false,rulers:true,guide_drag:None,selected_guide:None,snap_guides:true,grid:false,show_guide:false,show_new:false,new_preset:0,new_width:210.,new_height:297.,new_landscape:false,contour_color:egui::Color32::RED,selected:None,selection:Default::default(),selection_anchor:None,drag:None,duplicate_step:None,undo:vec![],redo:vec![],ratio:1.,lock:true,last_print:None,project_path:None,preview_mark_origin:true,ccd_trial:false,ccd_profile:sheet::CcdTrial::default(),#[cfg(windows)] print_panel:crate::native_print::PrintPanel::default()};
        #[cfg(not(test))]
        if let Ok(p) = Preferences::load() {
            app.apply_preferences(p);
        }
        app.mark_saved();
        #[cfg(not(test))]
        app.autosave.start(&crate::recovery::root());
        #[cfg(not(test))]
        {
            app.recent_projects =
                crate::recent::RecentProjects::load(crate::recent::storage_path());
        }
        app
    }
}

impl A4Studio {
    fn snap_move(
        &self,
        members: &[(usize, sheet::Placement)],
        delta: (f64, f64),
        scale: f32,
        axis: Option<usize>,
    ) -> (f64, f64) {
        if delta == (0., 0.) || !self.snap_guides || self.layout.guides.iter().all(Vec::is_empty) {
            return delta;
        }
        let mut adjusted = [delta.0, delta.1];
        let threshold = 6. / f64::from(scale);
        for (i, offset) in adjusted.iter_mut().enumerate() {
            if axis.is_some_and(|axis| axis != i) {
                continue;
            }
            let mut best: Option<f64> = None;
            for (_, item) in members {
                let (start, size) = if i == 0 {
                    (item.x, item.width)
                } else {
                    (item.y, item.height)
                };
                for anchor in [start, start + size / 2., start + size] {
                    for &guide in &self.layout.guides[i] {
                        let shift = guide - (anchor + *offset);
                        if shift.abs() <= threshold && best.is_none_or(|v| shift.abs() < v.abs()) {
                            best = Some(shift);
                        }
                    }
                }
            }
            *offset += best.unwrap_or(0.);
        }
        let (x, y) = group_move_delta(
            members,
            adjusted[0],
            adjusted[1],
            self.edit_inset(),
            [self.layout.page_width, self.layout.page_height],
        );
        (
            if axis == Some(1) { 0. } else { x },
            if axis == Some(0) { 0. } else { y },
        )
    }

    fn update_ruler_guides(
        &mut self,
        ui: &egui::Ui,
        view: egui::Rect,
        page: egui::Rect,
        scale: f32,
        panning: bool,
    ) -> bool {
        let (pointer, pressed, released) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.button_released(egui::PointerButton::Primary),
            )
        });
        let ruler_axis = pointer.filter(|p| view.contains(*p)).and_then(|p| {
            if p.y <= view.top() + 23. && p.x > view.left() + 28. {
                Some(1)
            } else if p.x <= view.left() + 28. && p.y > view.top() + 23. {
                Some(0)
            } else {
                None
            }
        });
        let over_ruler = self.rulers && ruler_axis.is_some();
        let coordinate = |p: egui::Pos2, axis| {
            f64::from(if axis == 0 {
                p.x - page.left()
            } else {
                p.y - page.top()
            }) / f64::from(scale)
        };
        let hit = pointer
            .filter(|p| view.contains(*p) && !over_ruler)
            .and_then(|p| {
                self.layout
                    .guides
                    .iter()
                    .enumerate()
                    .flat_map(|(axis, guides)| {
                        guides.iter().enumerate().map(move |(index, &value)| {
                            (axis, index, value, (coordinate(p, axis) - value).abs())
                        })
                    })
                    .filter(|&(_, _, _, distance)| distance * f64::from(scale) <= 5.)
                    .min_by(|a, b| a.3.total_cmp(&b.3))
            });
        if pressed
            && self.drag.is_none()
            && !panning
            && ui.rect_contains_pointer(view)
            && let Some(p) = pointer.filter(|p| view.contains(*p))
        {
            if over_ruler {
                self.clear_selection();
                self.selected_guide = None;
                self.guide_drag = ruler_axis.map(|axis| GuideDrag {
                    axis,
                    index: None,
                    value: coordinate(p, axis),
                    grab_offset: 0.,
                });
            } else if let Some((axis, index, value, _)) = hit {
                self.clear_selection();
                self.selected_guide = Some((axis, index));
                self.guide_drag = Some(GuideDrag {
                    axis,
                    index: Some(index),
                    value,
                    grab_offset: value - coordinate(p, axis),
                });
            } else {
                self.selected_guide = None;
            }
        }
        let active = self.guide_drag.is_some();
        if let (Some(drag), Some(pointer)) = (&mut self.guide_drag, pointer) {
            drag.value = coordinate(pointer, drag.axis) + drag.grab_offset;
        }
        if !panning
            && self.drag.is_none()
            && let Some(axis) = self
                .guide_drag
                .as_ref()
                .map(|drag| drag.axis)
                .or(hit.map(|h| h.0))
        {
            ui.ctx().set_cursor_icon(if axis == 0 {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::ResizeVertical
            });
        }
        if released
            && let Some(GuideDrag {
                axis, index, value, ..
            }) = self.guide_drag.take()
        {
            let guides = &mut self.layout.guides[axis];
            let length = [self.layout.page_width, self.layout.page_height][axis];
            if pointer.is_some_and(|p| view.contains(p))
                && !over_ruler
                && (0. ..=length).contains(&value)
            {
                if let Some(index) = index {
                    if (guides[index] - value).abs() > 1e-8 {
                        guides[index] = value;
                        self.status = format!(
                            "Guide moved to {} {}",
                            self.unit.number(value),
                            self.unit.label()
                        );
                    }
                } else if guides.len() < 100 && !guides.iter().any(|v| (v - value).abs() < 0.01) {
                    guides.push(value);
                    self.selected_guide = Some((axis, guides.len() - 1));
                    self.status = format!(
                        "Guide added at {} {}",
                        self.unit.number(value),
                        self.unit.label()
                    );
                }
            }
        }
        active || (over_ruler && self.drag.is_none())
    }

    fn delete_selected_guide(&mut self) -> bool {
        let Some((axis, index)) = self.selected_guide.take() else {
            return false;
        };
        let guides = &mut self.layout.guides[axis];
        if index >= guides.len() {
            return false;
        }
        guides.remove(index);
        self.guide_drag = None;
        self.status = "Guide deleted.".into();
        true
    }

    fn draw_fixed_rulers(
        &self,
        painter: &egui::Painter,
        area: egui::Rect,
        page: egui::Rect,
        scale: f32,
    ) {
        let ruler = egui::Color32::from_rgb(246, 246, 246);
        let ink = egui::Color32::from_gray(85);
        painter.rect_filled(
            egui::Rect::from_min_max(area.min, egui::pos2(area.right(), area.top() + 23.)),
            0.,
            ruler,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(area.min, egui::pos2(area.left() + 28., area.bottom())),
            0.,
            ruler,
        );
        let ticks = crate::ruler::RulerScale::new(self.unit, scale as f64);
        let clip = painter.clip_rect().intersect(area);
        for tick in ticks.visible(
            ((clip.left().max(area.left() + 28.) - page.left()) / scale) as f64,
            ((clip.right() - page.left()) / scale) as f64,
            self.layout.page_width,
        ) {
            let mm = tick as f64 * ticks.step_mm;
            let x = page.left() + mm as f32 * scale;
            painter.line_segment(
                [
                    egui::pos2(x, area.top() + 23. - ticks.length(tick)),
                    egui::pos2(x, area.top() + 23.),
                ],
                egui::Stroke::new(0.7_f32, ink),
            );
            if ticks.labeled(tick) {
                painter.text(
                    egui::pos2(x + 2., area.top() + 2.),
                    egui::Align2::LEFT_TOP,
                    self.unit.number(mm),
                    egui::FontId::proportional(9.),
                    ink,
                );
            }
        }
        for tick in ticks.visible(
            ((clip.top().max(area.top() + 23.) - page.top()) / scale) as f64,
            ((clip.bottom() - page.top()) / scale) as f64,
            self.layout.page_height,
        ) {
            let mm = tick as f64 * ticks.step_mm;
            let y = page.top() + mm as f32 * scale;
            painter.line_segment(
                [
                    egui::pos2(area.left() + 28. - ticks.length(tick), y),
                    egui::pos2(area.left() + 28., y),
                ],
                egui::Stroke::new(0.7_f32, ink),
            );
            if ticks.labeled(tick) {
                painter.text(
                    egui::pos2(area.left() + 2., y - 2.),
                    egui::Align2::LEFT_BOTTOM,
                    self.unit.number(mm),
                    egui::FontId::proportional(9.),
                    ink,
                );
            }
        }
        painter.text(
            area.min + egui::vec2(3., 3.),
            egui::Align2::LEFT_TOP,
            self.unit.label(),
            egui::FontId::proportional(9.),
            ink,
        );
    }

    fn draw_ruler_guides(
        &self,
        painter: &egui::Painter,
        view: egui::Rect,
        page: egui::Rect,
        scale: f32,
    ) {
        let painter = painter.with_clip_rect(view);
        for axis in 0..2 {
            let preview = self.guide_drag.as_ref().filter(|drag| drag.axis == axis);
            let lines = self.layout.guides[axis]
                .iter()
                .copied()
                .enumerate()
                .filter(|(index, _)| preview.and_then(|drag| drag.index) != Some(*index))
                .map(|(index, value)| (value, self.selected_guide == Some((axis, index))))
                .chain(preview.map(|drag| (drag.value, true)));
            for (value, selected) in lines {
                let points = if axis == 0 {
                    let x = page.left() + value as f32 * scale;
                    [egui::pos2(x, view.top()), egui::pos2(x, view.bottom())]
                } else {
                    let y = page.top() + value as f32 * scale;
                    [egui::pos2(view.left(), y), egui::pos2(view.right(), y)]
                };
                painter.line_segment(
                    points,
                    if selected {
                        egui::Stroke::new(2_f32, egui::Color32::from_rgb(25, 93, 213))
                    } else {
                        egui::Stroke::new(1_f32, egui::Color32::from_rgb(0, 149, 201))
                    },
                );
            }
        }
    }

    fn retrace(&mut self, ctx: &egui::Context) {
        self.refresh_source_digest();
        self.preview_cache = None;
        self.outline_cache = Default::default();
        match self.layout.trace_images(
            &self.source,
            self.remove_white,
            self.threshold,
            self.cut_shape,
            self.background_tolerance,
            self.print_without_background,
        ) {
            Ok(a) => {
                let thumb = image::imageops::thumbnail(&a.pixels, 1500, 1500);

                let size = [thumb.width() as usize, thumb.height() as usize];

                let image = egui::ColorImage::from_rgba_unmultiplied(size, thumb.as_raw());

                self.texture =
                    Some(ctx.load_texture("sticker", image, egui::TextureOptions::LINEAR));

                self.status = format!(
                    "{} outer contours, {} inner holes. Check cut paths before export.",
                    a.outlines.len(),
                    a.holes.len()
                );

                if self.cut_shape == sheet::CutShape::AlphaOutline
                    && a.pixels.pixels().all(|p| p[3] >= 128)
                {
                    self.status="Image is fully opaque: outline is the rectangular image frame. Use a transparent PNG or enable white-background removal.".into();
                }

                self.art = Some(a);
            }

            Err(e) => {
                self.art = None;

                self.texture = None;

                self.status = e;
            }
        }
    }

    fn close_image(&mut self) {
        self.autosave.clear();
        self.preview_cache = None;
        self.art = None;
        self.texture = None;
        self.source.clear();
        self.source_digest = 0;
        self.name = "No image selected".into();
        self.layout.custom.clear();
        self.layout.image_tiles.clear();
        self.layout.hidden_images.clear();
        self.clear_selection();
        self.drag = None;
        self.duplicate_step = None;
        self.undo.clear();
        self.redo.clear();
        self.last_print = None;
        self.ratio = 1.;
        self.status =
            "Image closed. Open another image; paper and cut settings are preserved.".into();
    }

    fn load(&mut self, path: PathBuf, ctx: &egui::Context) {
        match std::fs::read(&path) {
            Ok(bytes) => {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.install_single_image(bytes, name, ctx);
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn install_single_image(&mut self, bytes: Vec<u8>, name: String, ctx: &egui::Context) {
        if bytes.len() > 30_000_000 {
            self.status = "Image too large (30 MB maximum).".into();
            return;
        }
        if let Err(error) = image::load_from_memory(&bytes) {
            self.status = format!("Cannot open image: {error}");
            return;
        }
        self.close_image();
        self.source = bytes;
        self.name = name;
        self.retrace(ctx);
        if let Some(art) = &self.art {
            let [x_dpi, y_dpi] = crate::photo_size::dpi(&self.source).unwrap_or([300.; 2]);
            self.layout.width = f64::from(art.pixels.width()) * 25.4 / x_dpi;
            self.layout.height = f64::from(art.pixels.height()) * 25.4 / y_dpi;
            self.layout.count = 1;
            self.ratio = self.layout.width / self.layout.height;
        }
    }

    fn cut_file_data(&self) -> Result<String, String> {
        match &self.art {
            None => self.layout.manual_design_plt(),
            Some(art) if self.ccd_trial => {
                self.ccd_profile.pair(&self.layout, art).map(|(_, plt)| plt)
            }
            Some(art) => self.layout.plt(art),
        }
    }

    fn export_cut_file(&mut self, frame: &eframe::Frame) {
        match self.cut_file_data() {
            Err(error) => {
                self.status = format!("Export failed: {error}");
                rfd::MessageDialog::new()
                    .set_parent(frame)
                    .set_title("Cannot export PLT")
                    .set_description(&error)
                    .set_level(rfd::MessageLevel::Error)
                    .show();
            }
            Ok(data) => {
                if let Some(mut path) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .set_title("Export PLT")
                    .add_filter("Plotter file (*.plt)", &["plt"])
                    .set_file_name(if self.ccd_trial && self.art.is_some() {
                        "CCD_TEST.plt"
                    } else {
                        "CUT.plt"
                    })
                    .save_file()
                {
                    if path.extension().is_none() {
                        path.set_extension("plt");
                    }
                    self.status = match std::fs::write(&path, data) {
                        Ok(()) => format!("Saved {}", path.display()),
                        Err(error) => format!("Export failed: {error}"),
                    };
                }
            }
        }
    }

    fn export_pair(&mut self, frame: &eframe::Frame) {
        let Some(a) = &self.art else {
            self.export_cut_file(frame);
            return;
        };

        let output = if self.ccd_trial {
            self.ccd_profile.pair(&self.layout, a)
        } else {
            (|| Ok::<_, String>((self.layout.print_html(a)?, self.layout.plt(a)?)))()
        };

        match output {
            Err(e) => self.status = e,

            Ok((html, plt)) => {
                if let Some(parent) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .set_title("Choose folder for a new print & cut job")
                    .pick_folder()
                {
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis();

                    let folder = parent.join(format!("SHEET-{stamp}"));

                    let result = (|| {
                        std::fs::create_dir(&folder)?;

                        std::fs::write(folder.join("PRINT.html"), html)?;

                        let cut_name = if self.ccd_trial {
                            "CCD_TEST.plt"
                        } else {
                            "CUT.plt"
                        };

                        std::fs::write(folder.join(cut_name), plt)?;

                        std::fs::write(
                            folder.join("SETUP.txt"),
                            format!(
                                "A4 portrait 210 x 297 mm\nCopies: {}\nImage size: {} x {} mm\nMargin: {} mm, column gap: {} mm, row gap: {} mm\nUnits/mm: {}\nCut offsets: {}, {} mm\nFlip X: {}, Flip Y: {}\n\nPRINT.html: Open and print at 100% / Actual size, no margins, no headers/footers. Do not use Fit to page.\nCUT.plt: Copy to USB stick. Coordinates use the A4 lower-left corner as origin before flips/offsets. Manually align sheet and blade origin; use a pen test first.\nThe four crosses are visual alignment references, NOT a verified MC630 automatic registration format. Scale/rotation/skew compensation is not automatic. Re-export both files after changing layout.\nTrace: outer alpha silhouettes, alpha >=128; white removal: {}, white threshold: {}. Interior holes follow the Cut inner holes setting below. No image bleed is generated. See Cut border distance below.\n",
                                self.layout.count,
                                self.layout.width,
                                self.layout.height,
                                self.layout.margin,
                                self.layout.column_gap,
                                self.layout.row_gap,
                                self.layout.units,
                                self.layout.offset_x,
                                self.layout.offset_y,
                                self.layout.flip_x,
                                self.layout.flip_y,
                                self.remove_white,
                                self.threshold
                            ),
                        )?;

                        if self.ccd_trial {
                            std::fs::write(
                                folder.join("SETUP.txt"),
                                format!(
                                    "EXPERIMENTAL MC630 CCD FOUR-DOT TRIAL - NOT VERIFIED ON Ver5.022\nPrint: PRINT.html; cutter: CCD_TEST.plt\nCopies: {}; image {} x {} mm\nBlack circle diameter: {} mm (trial value, inferred from video, NOT a firmware specification)\nDot center gap outside occupied image frames: {} mm (trial value)\nTB25 spans (horizontal x vertical): {} x {} mm; feed span exported first; units/mm: {}\nCut coordinates relative to TOP-LEFT mark; page X right, Y down; PLT exports feed Y first, carriage X second; manual X/Y corrections ignored in this mode; Reverse X/Y must be off.\n\nREMOVE BLADE before loading the PLT. The file contains full contours and may cut after a successful scan. Print fresh at A4 100%, no margins, no headers. Do not use a manual-cross print with this file. Start near the first dot using the machine's CCD workflow. The first-dot corner, feed orientation, TB25 argument order and CT1 semantics remain unverified for Ver5.022. Record scan result; do not treat a successful export as a successful scan.\n\nEvidence: official MC User Manual and U-disc contour video linked from https://www.mycutcutter.com/info/new-ccd-camear-contour-cut-pluginin-for-coreld-45966569.html\nTB25 mark-span reference: https://github.com/inkcut/inkcut/issues/379 (different cutter, corroboration only).\n",
                                    self.layout.count,
                                    self.layout.width,
                                    self.layout.height,
                                    self.ccd_profile.diameter,
                                    self.ccd_profile.inset,
                                    self.ccd_profile.marks(&self.layout)[1][0]
                                        - self.ccd_profile.marks(&self.layout)[0][0],
                                    self.ccd_profile.marks(&self.layout)[2][1]
                                        - self.ccd_profile.marks(&self.layout)[0][1],
                                    self.layout.units
                                ),
                            )?;
                        }

                        use std::io::Write as _;
                        writeln!(
                            std::fs::OpenOptions::new()
                                .append(true)
                                .open(folder.join("SETUP.txt"))?,
                            "\nCut border: {:+.2} mm (positive outside, negative inside; print size unchanged).",
                            self.layout.contour_offset
                        )?;
                        writeln!(
                            std::fs::OpenOptions::new()
                                .append(true)
                                .open(folder.join("SETUP.txt"))?,
                            "Cut inner holes: {}\nOutline smoothing: {:.2} mm",
                            self.layout.cut_inner_holes,
                            self.layout.smoothing
                        )?;
                        writeln!(
                            std::fs::OpenOptions::new()
                                .append(true)
                                .open(folder.join("SETUP.txt"))?,
                            "Cut shape: {}\nBackground tolerance: {}\nPrint without background: {}",
                            self.cut_shape.label(),
                            self.background_tolerance,
                            self.print_without_background
                        )?;
                        let mut placement_csv =
                            String::from("image,x_mm,y_mm,width_mm,height_mm\n");
                        for (index, p) in self
                            .layout
                            .items()
                            .map_err(std::io::Error::other)?
                            .iter()
                            .enumerate()
                        {
                            if self.layout.hidden_images.contains(&index) {
                                continue;
                            }
                            placement_csv.push_str(&format!(
                                "{},{:.4},{:.4},{:.4},{:.4}\n",
                                index + 1,
                                p.x,
                                p.y,
                                p.width,
                                p.height
                            ));
                        }
                        std::fs::write(folder.join("LAYOUT.csv"), placement_csv)?;
                        if !self.layout.custom.is_empty() {
                            writeln!(
                                std::fs::OpenOptions::new()
                                    .append(true)
                                    .open(folder.join("SETUP.txt"))?,
                                "Individually edited layout: actual positions and sizes are in LAYOUT.csv. Summary image size is the repeat default. Print the new sheet after each layout edit."
                            )?;
                        }

                        let setup = folder.join("SETUP.txt");
                        let text = std::fs::read_to_string(&setup)?
                            .replace(
                                "A4 portrait 210 x 297 mm",
                                &format!(
                                    "{} {:.1} x {:.1} mm",
                                    self.layout.paper_label(),
                                    self.layout.page_width,
                                    self.layout.page_height
                                ),
                            )
                            .replace("A4 portrait", &self.layout.paper_label())
                            .replace("A4 lower-left", "page lower-left")
                            .replace(
                                "Print fresh at A4 100%",
                                &format!("Print fresh at {} 100%", self.layout.paper_label()),
                            );
                        std::fs::write(setup, text)?;
                        Ok::<_, std::io::Error>(())
                    })();

                    match result {
                        Ok(()) => {
                            self.last_print = Some(folder.join("PRINT.html"));

                            self.status =
                                format!("Saved matching print/cut files in {}", folder.display());
                        }

                        Err(e) => self.status = format!("Export incomplete: {e}"),
                    }
                }
            }
        }
    }

    fn can_export_cut(&self) -> bool {
        self.art.is_some() || !self.layout.manual_cut_paths.is_empty()
    }

    fn new_document_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_new {
            return;
        }
        let mut open = true;
        let mut create = false;
        egui::Window::new("New document").open(&mut open).collapsible(false).resizable(false).default_width(450.).show(ctx,|ui|{
            ui.heading("Create a new print & cut document");ui.add_space(10.);
            let previous=self.new_preset;
            egui::ComboBox::from_id_salt("new-paper-preset").selected_text(sheet::PAPER_PRESETS.get(self.new_preset).map(|p|p.0).unwrap_or("Custom")).width(280.).show_ui(ui,|ui|{
                for (i,(name,w,h)) in sheet::PAPER_PRESETS.iter().enumerate(){ui.selectable_value(&mut self.new_preset,i,format!("{name}   {}",self.unit.size(*w,*h)));}
                ui.selectable_value(&mut self.new_preset,sheet::PAPER_PRESETS.len(),"Custom size");
            });
            if previous!=self.new_preset{if let Some(&(_,w,h))=sheet::PAPER_PRESETS.get(self.new_preset){(self.new_width,self.new_height)=if self.new_landscape{(h,w)}else{(w,h)};}}
            ui.add_space(8.);let orientation=self.new_landscape;
            ui.horizontal(|ui|{ui.label("Orientation");ui.selectable_value(&mut self.new_landscape,false,"Portrait");ui.selectable_value(&mut self.new_landscape,true,"Landscape");});
            if orientation!=self.new_landscape{std::mem::swap(&mut self.new_width,&mut self.new_height);}
            ui.add_space(8.);
            egui::Grid::new("new-paper-dimensions").num_columns(2).spacing([24.,10.]).show(ui,|ui|{
                ui.label("Page width");if ui.add(self.unit.distance(&mut self.new_width).range(50.0..=2000.).speed(0.1)).changed(){self.new_preset=sheet::PAPER_PRESETS.len();self.new_landscape=self.new_width>self.new_height;}ui.end_row();
                ui.label("Page height");if ui.add(self.unit.distance(&mut self.new_height).range(50.0..=2000.).speed(0.1)).changed(){self.new_preset=sheet::PAPER_PRESETS.len();self.new_landscape=self.new_width>self.new_height;}ui.end_row();
            });
            ui.add_space(12.);
            let valid=self.new_width.is_finite()&&self.new_height.is_finite()&&(50. ..=630.).contains(&self.new_width)&&(50. ..=2000.).contains(&self.new_height);
            if !valid{ui.colored_label(egui::Color32::from_rgb(170,65,20),"MC630 document width: 50-630 mm. Height: 50-2000 mm. Choose the other orientation or a smaller custom size.");}
            ui.small("Creates a blank document and replaces the current unsaved layout. Printer preferences stay as configured.");
            ui.small("For direct printing, select the same paper size and orientation in Printing Preferences.");
            ui.add_space(12.);ui.horizontal(|ui|{
                if ui.add_enabled(valid,egui::Button::new("Create document").min_size(egui::vec2(160.,32.))).clicked(){create=true;}
                if ui.button("Cancel").clicked(){self.show_new=false;}
            });
        });
        if !open {
            self.show_new = false;
        }
        if create {
            self.layout = Layout {
                page_width: self.new_width,
                page_height: self.new_height,
                units: self.layout.units,
                offset_x: -7.,
                offset_y: -7.,
                ..Default::default()
            };
            self.guide_drag = None;
            self.selected_guide = None;
            self.cut_draft = None;
            self.pick_cut_shapes = false;
            self.selected_cut_shape = None;
            self.cut_shape_drag = None;
            let size = (self.new_width - 30.)
                .min(self.new_height - 30.)
                .clamp(1., 35.);
            self.layout.width = size;
            self.layout.height = size;
            if let Ok((c, r)) = self.layout.capacity() {
                self.layout.count = (c * r).clamp(1, 20);
            }
            self.preview_cache = None;
            self.art = None;
            self.texture = None;
            self.source.clear();
            self.source_digest = 0;
            self.name = format!("Untitled {}", self.layout.paper_label());
            self.project_path = None;
            self.last_print = None;
            self.clear_selection();
            self.drag = None;
            self.duplicate_step = None;
            self.undo.clear();
            self.redo.clear();
            self.ratio = 1.;
            self.zoom = 1.;
            self.canvas_pan = egui::Vec2::ZERO;
            self.show_new = false;
            self.status = format!(
                "New document: {} / {:.1} x {:.1} mm. Import an image to begin.",
                self.layout.paper_label(),
                self.new_width,
                self.new_height
            );
            self.mark_saved();
        }
    }
    fn clear_selection(&mut self) {
        self.selection.clear();
        self.selected = None;
        self.selection_anchor = None;
    }
    fn sync_selection(&mut self) {
        self.selected = if self.selection.len() == 1 {
            self.selection.first().copied()
        } else {
            None
        };
    }
    fn select_all(&mut self) {
        if self.art.is_some() {
            self.selected_guide = None;
            self.selection = (0..self.layout.count).collect();
            self.selection_anchor = Some(0);
            self.sync_selection();
        }
    }
    fn select_target(&mut self, index: usize, ctrl: bool, shift: bool) {
        self.selected_guide = None;
        if shift {
            let anchor = self
                .selection_anchor
                .unwrap_or(index)
                .min(self.layout.count.saturating_sub(1));
            if !ctrl {
                self.selection.clear();
            }
            self.selection.extend(anchor.min(index)..=anchor.max(index));
            self.selection_anchor = Some(anchor);
        } else if ctrl {
            if !self.selection.remove(&index) {
                self.selection.insert(index);
            }
            self.selection_anchor = Some(index);
        } else {
            if !self.selection.contains(&index) {
                self.selection.clear();
                self.selection.insert(index);
            }
            self.selection_anchor = Some(index);
        }
        self.sync_selection();
    }
    fn record_object_edit(&mut self, before: std::collections::BTreeMap<usize, sheet::Placement>) {
        if before != self.layout.custom {
            self.undo.push(ObjectEdit::Positions(before));
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.status="Object layout changed. Reprint and export matching cut files; CCD dots follow the layout.".into();
        }
    }
    fn swap_object_edit(&mut self, edit: ObjectEdit, ctx: &egui::Context) -> ObjectEdit {
        self.drag = None;
        match edit {
            ObjectEdit::ManualCuts(paths, enabled) => {
                self.cut_draft = None;
                self.selected_cut_shape = None;
                self.cut_shape_drag = None;
                ObjectEdit::ManualCuts(
                    std::mem::replace(&mut self.layout.manual_cut_paths, paths),
                    std::mem::replace(&mut self.layout.use_manual_cut_paths, enabled),
                )
            }
            ObjectEdit::Visibility(hidden) => {
                ObjectEdit::Visibility(std::mem::replace(&mut self.layout.hidden_images, hidden))
            }
            ObjectEdit::Positions(positions) => {
                ObjectEdit::Positions(std::mem::replace(&mut self.layout.custom, positions))
            }
            ObjectEdit::Layers(mut layout) => {
                std::mem::swap(&mut self.layout.count, &mut layout.count);
                std::mem::swap(&mut self.layout.custom, &mut layout.custom);
                std::mem::swap(&mut self.layout.image_tiles, &mut layout.image_tiles);
                std::mem::swap(&mut self.layout.hidden_images, &mut layout.hidden_images);
                self.clear_selection();
                self.retrace(ctx);
                ObjectEdit::Layers(layout)
            }
            ObjectEdit::LastImage(layout, source, name) => {
                let previous = std::mem::replace(&mut self.layout, *layout);
                let source = std::mem::replace(&mut self.source, source);
                let name = std::mem::replace(&mut self.name, name);
                self.clear_selection();
                if self.source.is_empty() {
                    self.art = None;
                    self.texture = None;
                    self.preview_cache = None;
                    self.refresh_source_digest();
                } else {
                    self.retrace(ctx);
                }
                ObjectEdit::LastImage(Box::new(previous), source, name)
            }
        }
    }
    fn undo_object(&mut self, ctx: &egui::Context) {
        if let Some(before) = self.undo.pop() {
            let edit = self.swap_object_edit(before, ctx);
            self.redo.push(edit);
            self.last_print = None;
        }
    }
    fn redo_object(&mut self, ctx: &egui::Context) {
        if let Some(next) = self.redo.pop() {
            let edit = self.swap_object_edit(next, ctx);
            self.undo.push(edit);
            self.last_print = None;
        }
    }
    fn record_layer_edit(&mut self, edit: ObjectEdit) {
        self.undo.push(edit);
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.last_print = None;
    }

    fn record_manual_cut_edit(&mut self) {
        self.record_layer_edit(ObjectEdit::ManualCuts(
            self.layout.manual_cut_paths.clone(),
            self.layout.use_manual_cut_paths,
        ));
    }

    fn finish_manual_cut(&mut self, close: bool) {
        let required = if close { 3 } else { 2 };
        if let Some(draft) = self.cut_draft.as_ref()
            && draft.tool == Tool::Pen
            && draft.anchors.len() >= required
        {
            self.commit_manual_cut(cut_tools::pen_path(&draft.anchors, close));
        }
    }

    fn activate_cut_tool(&mut self, tool: Tool) {
        self.open_sidebar_section(2);
        if self
            .cut_draft
            .as_ref()
            .is_some_and(|draft| draft.tool == tool)
        {
            return;
        }
        self.clear_selection();
        self.selected_guide = None;
        self.pick_cut_shapes = false;
        self.selected_cut_shape = None;
        self.cut_shape_drag = None;
        self.cut_draft = Some(Draft::new(tool));
    }

    fn activate_cut_picker(&mut self) {
        self.open_sidebar_section(2);
        self.cut_draft = None;
        self.clear_selection();
        self.selected_guide = None;
        self.pick_cut_shapes = true;
        self.show_cut = true;
    }

    fn cut_picker_owns_gesture(&self, ctx: &egui::Context, page: egui::Rect, scale: f32) -> bool {
        if !self.pick_cut_shapes {
            return false;
        }
        if self.cut_shape_drag.is_some() {
            return true;
        }
        let Some(pointer) = ctx.input(|i| i.pointer.press_origin().or(i.pointer.interact_pos()))
        else {
            return false;
        };
        let p = (pointer - page.min) / scale;
        let point = [f64::from(p.x), f64::from(p.y)];
        let tolerance = 8. / f64::from(scale);
        if let Some(path) = self
            .selected_cut_shape
            .and_then(|i| self.layout.manual_cut_paths.get(i))
        {
            let [x0, y0, x1, y1] = cut_tools::bounds(path);
            if [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
                .iter()
                .any(|&[x, y]| (x - point[0]).hypot(y - point[1]) <= tolerance)
            {
                return true;
            }
        }
        self.layout
            .manual_cut_paths
            .iter()
            .any(|path| cut_tools::hit(path, point, 6. / f64::from(scale)))
    }

    fn remove_selected_cut_shape(&mut self) -> bool {
        if self.cut_shape_drag.is_some() {
            return true;
        }
        let Some(index) = self.selected_cut_shape.take() else {
            return false;
        };
        if index >= self.layout.manual_cut_paths.len() {
            return false;
        }
        self.record_manual_cut_edit();
        self.layout.manual_cut_paths.remove(index);
        self.layout.use_manual_cut_paths &= !self.layout.manual_cut_paths.is_empty();
        self.status = "Cut shape removed.".into();
        true
    }

    fn commit_manual_cut(&mut self, mut path: Vec<mc630_plt_studio::Point>) {
        if path.len() < 2 {
            return;
        }
        let limits = [self.layout.page_width, self.layout.page_height];
        for p in &mut path {
            for axis in 0..2 {
                if !(-1e-7..=limits[axis] + 1e-7).contains(&p[axis]) {
                    self.status = "Keep the cut line within the page.".into();
                    return;
                }
                p[axis] = p[axis].clamp(0., limits[axis]);
            }
        }
        let vertices = self
            .layout
            .manual_cut_paths
            .iter()
            .map(Vec::len)
            .sum::<usize>();
        if self.layout.manual_cut_paths.len() >= 100
            || path.len() > 4000
            || vertices + path.len() > 10_000
        {
            self.status = "Manual cut line limit reached.".into();
            return;
        }
        self.cut_draft = None;
        self.record_manual_cut_edit();
        self.layout.manual_cut_paths.push(path);
        self.layout.use_manual_cut_paths = true;
        self.pick_cut_shapes = true;
        self.selected_cut_shape = Some(self.layout.manual_cut_paths.len() - 1);
        self.show_cut = true;
        self.status = "Manual cut line added.".into();
    }

    fn manual_cut_canvas(
        &mut self,
        response: &egui::Response,
        painter: &egui::Painter,
        page: egui::Rect,
        view: egui::Rect,
        scale: f32,
        panning: bool,
    ) {
        let Some(draft) = &mut self.cut_draft else {
            return;
        };
        let canvas = if self.rulers {
            egui::Rect::from_min_max(view.min + egui::vec2(28., 23.), view.max)
        } else {
            view
        };
        let screen =
            |[x, y]: mc630_plt_studio::Point| page.min + egui::vec2(x as f32, y as f32) * scale;
        let to_page = |p: egui::Pos2| {
            let q = (p - page.min) / scale;
            [
                f64::from(q.x).clamp(0., self.layout.page_width),
                f64::from(q.y).clamp(0., self.layout.page_height),
            ]
        };
        let pointer = response
            .interact_pointer_pos()
            .or_else(|| response.hover_pos())
            .filter(|p| !panning && canvas.contains(*p) && page.contains(*p));
        if pointer.is_some() {
            response.ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let primary = egui::PointerButton::Primary;
        if !panning && response.drag_started_by(primary) {
            draft.drag_start = response
                .ctx
                .input(|i| i.pointer.press_origin())
                .filter(|p| canvas.contains(*p) && page.contains(*p))
                .map(to_page);
        }
        let stop = response.drag_stopped_by(primary);
        let stroke = egui::Stroke::new(1.5_f32, self.contour_color);
        if draft.tool != Tool::Pen {
            if let (Some(start), Some(pointer)) = (draft.drag_start, pointer) {
                let equal = response.ctx.input(|i| i.modifiers.shift);
                let path = cut_tools::shape_path(
                    draft.tool,
                    start,
                    to_page(pointer),
                    equal,
                    self.cut_corner_radius,
                );
                if stop && !path.is_empty() {
                    self.commit_manual_cut(path);
                    return;
                }
                for edge in path.windows(2) {
                    painter.line_segment([screen(edge[0]), screen(edge[1])], stroke);
                }
            }
            if stop {
                draft.drag_start = None;
            }
            return;
        }
        let pending_anchor = draft.drag_start.zip(pointer).map(|(point, pointer)| {
            let end = to_page(pointer);
            Anchor {
                point,
                tangent: [end[0] - point[0], end[1] - point[1]],
            }
        });
        if stop {
            draft.drag_start = None;
            if let Some(anchor) = pending_anchor
                && draft.anchors.len() < 256
            {
                draft.anchors.push(anchor);
            }
        } else if response.clicked()
            && let Some(p) = pointer
        {
            if draft.anchors.len() >= 3 && screen(draft.anchors[0].point).distance(p) <= 7. {
                self.finish_manual_cut(true);
                return;
            }
            if draft.anchors.len() < 256
                && draft
                    .anchors
                    .last()
                    .is_none_or(|last| screen(last.point).distance(p) >= 2.)
            {
                draft.anchors.push(Anchor {
                    point: to_page(p),
                    tangent: [0.; 2],
                });
            }
        }
        let mut anchors = draft.anchors.clone();
        if !stop && let Some(anchor) = pending_anchor {
            anchors.push(anchor);
            painter.line_segment(
                [
                    screen(anchor.point),
                    screen([
                        anchor.point[0] + anchor.tangent[0],
                        anchor.point[1] + anchor.tangent[1],
                    ]),
                ],
                stroke,
            );
        }
        for edge in cut_tools::pen_path(&anchors, false).windows(2) {
            painter.line_segment([screen(edge[0]), screen(edge[1])], stroke);
        }
        for anchor in &anchors {
            painter.circle_filled(screen(anchor.point), 3., self.contour_color);
        }
        if pending_anchor.is_none()
            && let (Some(last), Some(pointer)) = (anchors.last(), pointer)
        {
            painter.line_segment([screen(last.point), pointer], stroke);
        }
    }

    fn cut_shape_canvas(
        &mut self,
        response: &egui::Response,
        painter: &egui::Painter,
        page: egui::Rect,
        view: egui::Rect,
        scale: f32,
        panning: bool,
    ) {
        let screen =
            |[x, y]: mc630_plt_studio::Point| page.min + egui::vec2(x as f32, y as f32) * scale;
        let physical = |p: egui::Pos2| {
            let d = (p - page.min) / scale;
            [f64::from(d.x), f64::from(d.y)]
        };
        let corners = |path: &[mc630_plt_studio::Point]| {
            let [x0, y0, x1, y1] = cut_tools::bounds(path);
            [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(screen)
        };
        let content = if self.rulers {
            egui::Rect::from_min_max(view.min + egui::vec2(28., 23.), view.max)
        } else {
            view
        };
        let primary = egui::PointerButton::Primary;
        if self.pick_cut_shapes
            && !panning
            && response.drag_started_by(primary)
            && let Some(start) = response
                .ctx
                .input(|i| i.pointer.press_origin())
                .filter(|p| content.contains(*p) && page.contains(*p))
        {
            let handle = self.selected_cut_shape.and_then(|i| {
                let path = self.layout.manual_cut_paths.get(i)?;
                corners(path)
                    .iter()
                    .position(|p| p.distance(start) <= 8.)
                    .map(|corner| (i, corner))
            });
            let hit = handle.map(|(i, _)| i).or_else(|| {
                self.layout
                    .manual_cut_paths
                    .iter()
                    .rposition(|p| cut_tools::hit(p, physical(start), 6. / f64::from(scale)))
            });
            self.selected_cut_shape = hit;
            if hit.is_some() {
                self.clear_selection();
            }
            self.cut_shape_drag = hit.map(|index| CutShapeDrag {
                index,
                start: physical(start),
                path: self.layout.manual_cut_paths[index].clone(),
                corner: handle.map(|(_, corner)| corner),
            });
        }
        if !panning
            && self.pick_cut_shapes
            && response.clicked()
            && let Some(pointer) = response
                .interact_pointer_pos()
                .filter(|p| content.contains(*p))
        {
            self.selected_cut_shape = self
                .layout
                .manual_cut_paths
                .iter()
                .rposition(|p| cut_tools::hit(p, physical(pointer), 6. / f64::from(scale)));
            if self.selected_cut_shape.is_some() {
                self.clear_selection();
            }
        }
        if let Some(drag) = &self.cut_shape_drag
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let [x, y] = physical(pointer);
            let (dx, dy) = (x - drag.start[0], y - drag.start[1]);
            let [x0, y0, x1, y1] = cut_tools::bounds(&drag.path);
            let size = [self.layout.page_width, self.layout.page_height];
            let old = sheet::Placement {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
            };
            let next =
                if let Some(corner) = drag.corner.filter(|_| old.width >= 1. && old.height >= 1.) {
                    let lock = response.ctx.input(|i| i.modifiers.shift);
                    old.resized_on(corner, dx, dy, lock, 0., size)
                } else {
                    old.moved_on(dx, dy, 0., size)
                };
            let (sx, sy) = (
                if old.width > 0. {
                    next.width / old.width
                } else {
                    1.
                },
                if old.height > 0. {
                    next.height / old.height
                } else {
                    1.
                },
            );
            self.layout.manual_cut_paths[drag.index] = drag
                .path
                .iter()
                .map(|&[x, y]| {
                    [
                        (next.x + (x - x0) * sx).clamp(0., size[0]),
                        (next.y + (y - y0) * sy).clamp(0., size[1]),
                    ]
                })
                .collect();
        }
        if response.drag_stopped_by(primary)
            && let Some(drag) = self.cut_shape_drag.take()
        {
            let index = drag.index;
            if self.layout.manual_cut_paths[index] != drag.path {
                let current =
                    std::mem::replace(&mut self.layout.manual_cut_paths[index], drag.path);
                self.record_manual_cut_edit();
                self.layout.manual_cut_paths[index] = current;
            }
        }
        if self.art.is_none() && self.show_cut
            || self.pick_cut_shapes && !self.layout.use_manual_cut_paths
        {
            let stroke = egui::Stroke::new(1.3_f32, self.contour_color);
            for path in &self.layout.manual_cut_paths {
                painter.add(egui::Shape::line(
                    path.iter().copied().map(screen).collect(),
                    stroke,
                ));
            }
        }
        if self.pick_cut_shapes
            && let Some(path) = self
                .selected_cut_shape
                .and_then(|i| self.layout.manual_cut_paths.get(i))
        {
            let corners = corners(path);
            let ink = egui::Color32::from_rgb(30, 95, 180);
            let stroke = egui::Stroke::new(1_f32, ink);
            painter.add(egui::Shape::closed_line(corners.to_vec(), stroke));
            for p in corners {
                let handle = egui::Rect::from_center_size(p, egui::vec2(7., 7.));
                painter.rect(
                    handle,
                    1.,
                    egui::Color32::WHITE,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    fn duplicate_images(
        &mut self,
        members: &[(usize, sheet::Placement)],
        delta: (f64, f64),
        ctx: &egui::Context,
    ) {
        let Ok(items) = self.layout.items() else {
            return;
        };
        if members.is_empty() {
            return;
        }
        let limit = if self.layout.image_tiles.is_empty() {
            1000
        } else {
            24
        };
        if items.len() + members.len() > limit {
            self.status = format!("This sheet supports up to {limit} images.");
            return;
        }
        let (dx, dy) = delta;
        if dx.abs() < 1e-8 && dy.abs() < 1e-8 {
            self.status = "No room to repeat in that direction.".into();
            return;
        }
        let before = self.layout.clone();
        let first = items.len();
        self.layout.custom = items
            .into_iter()
            .chain(members.iter().map(|(_, p)| sheet::Placement {
                x: p.x + dx,
                y: p.y + dy,
                ..*p
            }))
            .enumerate()
            .collect();
        if !before.image_tiles.is_empty() {
            self.layout
                .image_tiles
                .extend(members.iter().map(|(i, _)| before.image_tiles[*i]));
        }
        self.layout.count = first + members.len();
        self.selection = (first..self.layout.count).collect();
        self.selection_anchor = Some(first);
        self.sync_selection();
        self.duplicate_step = Some(delta);
        self.record_layer_edit(ObjectEdit::Layers(Box::new(before)));
        if !self.layout.image_tiles.is_empty() {
            self.retrace(ctx);
        }
        self.status = format!("Duplicated {} image(s).", members.len());
    }

    fn repeat_duplicate(&mut self, ctx: &egui::Context) {
        let Ok(items) = self.layout.items() else {
            return;
        };
        let members: Vec<_> = self
            .selection
            .iter()
            .filter(|i| !self.layout.hidden_images.contains(i))
            .filter_map(|i| items.get(*i).map(|p| (*i, *p)))
            .collect();
        let (dx, dy) = self.duplicate_step.unwrap_or((5., 5.));
        let actual = group_move_delta(
            &members,
            dx,
            dy,
            self.edit_inset(),
            [self.layout.page_width, self.layout.page_height],
        );
        if (actual.0 - dx).abs() > 1e-8 || (actual.1 - dy).abs() > 1e-8 {
            self.status = "The repeated placement would go outside the sheet.".into();
            return;
        }
        self.duplicate_images(&members, actual, ctx);
    }

    fn delete_selected_images(&mut self, ctx: &egui::Context) {
        let Ok(items) = self.layout.items() else {
            return;
        };
        if self.selection.is_empty() || self.source.is_empty() {
            return;
        }
        let before = self.layout.clone();
        let keep: Vec<_> = items
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !self.selection.contains(i))
            .collect();
        let removed = before.count - keep.len();
        self.clear_selection();
        if keep.is_empty() {
            let source = std::mem::take(&mut self.source);
            let name = std::mem::replace(&mut self.name, "No image selected".into());
            self.layout.custom.clear();
            self.layout.image_tiles.clear();
            self.layout.hidden_images.clear();
            self.layout.count = 1;
            self.art = None;
            self.texture = None;
            self.preview_cache = None;
            self.refresh_source_digest();
            self.record_layer_edit(ObjectEdit::LastImage(Box::new(before), source, name));
        } else {
            self.layout.count = keep.len();
            self.layout.custom = keep.iter().map(|(_, p)| *p).enumerate().collect();
            self.layout.hidden_images = keep
                .iter()
                .enumerate()
                .filter_map(|(new, (old, _))| before.hidden_images.contains(old).then_some(new))
                .collect();
            if !before.image_tiles.is_empty() {
                self.layout.image_tiles =
                    keep.iter().map(|(i, _)| before.image_tiles[*i]).collect();
                self.retrace(ctx);
            }
            self.record_layer_edit(ObjectEdit::Layers(Box::new(before)));
        }
        self.status = format!("Deleted {removed} image(s). Ctrl+Z to undo.");
    }
    fn reflow(&mut self) {
        if !self.layout.image_tiles.is_empty() {
            return;
        }
        let before = self.layout.custom.clone();
        self.layout.custom.clear();
        self.clear_selection();
        self.record_object_edit(before);
    }
    fn nudge_selection(&mut self, dx: f64, dy: f64) {
        if self.selection.is_empty() || self.drag.is_some() {
            return;
        }
        let Ok(items) = self.layout.items() else {
            return;
        };
        let members: Vec<_> = self
            .selection
            .iter()
            .filter(|i| !self.layout.hidden_images.contains(i))
            .filter_map(|i| items.get(*i).map(|p| (*i, *p)))
            .collect();
        if members.is_empty() {
            return;
        }
        let (dx, dy) = group_move_delta(
            &members,
            dx,
            dy,
            self.edit_inset(),
            [self.layout.page_width, self.layout.page_height],
        );
        if dx.abs() < 1e-10 && dy.abs() < 1e-10 {
            return;
        }
        let before = self.layout.custom.clone();
        // Freeze implicit copies too, so free-slot packing cannot move unselected images.
        for (i, p) in items.into_iter().enumerate() {
            self.layout.custom.insert(i, p);
        }
        for (i, p) in members {
            self.layout.custom.insert(
                i,
                sheet::Placement {
                    x: p.x + dx,
                    y: p.y + dy,
                    ..p
                },
            );
        }
        self.record_object_edit(before);
    }
    fn edit_inset(&self) -> f64 {
        if self.ccd_trial {
            self.ccd_profile.inset + self.ccd_profile.diameter / 2. + 3.
        } else {
            0.
        }
    }
    fn selected_properties(&mut self, ui: &mut egui::Ui) {
        let Some(index) = self.selected else {
            return;
        };
        let Ok(items) = self.layout.items() else {
            return;
        };
        let Some(original) = items.get(index).copied() else {
            self.clear_selection();
            return;
        };
        let mut p = original;
        ui.strong(format!("Image {}", index + 1)).on_hover_text(
            "Arrow keys: move selection. Configure normal and Shift steps in File > Settings.",
        );
        ui.label("X");
        let x = ui
            .add(self.unit.distance(&mut p.x).speed(0.1).fixed_decimals(2))
            .changed();
        ui.label("Y");
        let y = ui
            .add(self.unit.distance(&mut p.y).speed(0.1).fixed_decimals(2))
            .changed();
        ui.label("W");
        let w = ui
            .add(
                self.unit
                    .distance(&mut p.width)
                    .speed(0.1)
                    .fixed_decimals(2)
                    .range(1.0..=self.layout.page_width),
            )
            .changed();
        ui.label("H");
        let h = ui
            .add(
                self.unit
                    .distance(&mut p.height)
                    .speed(0.1)
                    .fixed_decimals(2)
                    .range(1.0..=self.layout.page_height),
            )
            .changed();
        if x || y || w || h {
            self.apply_selected_dimensions(index, original, p, w, h);
        }
    }
    fn apply_selected_dimensions(
        &mut self,
        index: usize,
        original: sheet::Placement,
        mut p: sheet::Placement,
        w: bool,
        h: bool,
    ) {
        let before = self.layout.custom.clone();
        let inset = self.edit_inset();
        if self.lock {
            if w {
                p.height = p.width * original.height / original.width;
            } else if h {
                p.width = p.height * original.width / original.height;
            }
        }
        let factor = ((self.layout.page_width - 2. * inset) / p.width)
            .min((self.layout.page_height - 2. * inset) / p.height)
            .min(1.);
        p.width *= factor;
        p.height *= factor;
        p = p.moved_on(
            0.,
            0.,
            inset,
            [self.layout.page_width, self.layout.page_height],
        );
        self.layout.custom.insert(index, p);
        self.record_object_edit(before);
    }
    fn canvas_edit(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        page: egui::Rect,
        scale: f32,
    ) {
        if self.art.is_none() {
            return;
        }
        let Ok(items) = self.layout.items() else {
            return;
        };
        let screen = |p: sheet::Placement| {
            egui::Rect::from_min_size(
                page.min + egui::vec2(p.x as f32 * scale, p.y as f32 * scale),
                egui::vec2(p.width as f32 * scale, p.height as f32 * scale),
            )
        };
        let corners = |r: egui::Rect| {
            [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
            ]
        };
        let pointer = ui.input(|i| i.pointer.interact_pos());
        // Capture press coordinates even if press, motion and release arrive in one frame.
        let press = ui.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } => Some(*pos),
                _ => None,
            })
        });
        let start = press;
        if self.drag.is_none() {
            if let Some(start) = start.filter(|p| {
                response.rect.contains(*p)
                    && (response.hovered()
                        || response.is_pointer_button_down_on()
                        || response.drag_started())
            }) {
                let members: Vec<_> = self
                    .selection
                    .iter()
                    .filter(|i| !self.layout.hidden_images.contains(i))
                    .map(|i| (*i, items[*i]))
                    .collect();
                let handle = group_bounds(&members).and_then(|bounds| {
                    corners(screen(bounds))
                        .iter()
                        .position(|p| p.distance(start) <= 8.)
                        .map(|corner| (members[0].0, corner))
                });
                let target = handle.map(|(index, _)| index).or_else(|| {
                    items
                        .iter()
                        .enumerate()
                        .rfind(|(i, p)| {
                            !self.layout.hidden_images.contains(i) && screen(**p).contains(start)
                        })
                        .map(|(i, _)| i)
                });
                let modifiers = ui.input(|i| i.modifiers);
                if let Some(index) = target {
                    self.selected_cut_shape = None;
                    let duplicate = modifiers.ctrl || modifiers.shift;
                    if !duplicate {
                        self.select_target(index, modifiers.ctrl, modifiers.shift);
                    }
                    let members = if duplicate && !self.selection.contains(&index) {
                        vec![(index, items[index])]
                    } else {
                        self.selection
                            .iter()
                            .filter(|i| !self.layout.hidden_images.contains(i))
                            .map(|i| (*i, items[*i]))
                            .collect()
                    };
                    self.drag = Some(CanvasDrag {
                        pointer: start,
                        corner: handle.map(|(_, c)| c),
                        before: self.layout.custom.clone(),
                        members,
                        duplicate_target: duplicate.then_some(index),
                        axis_locked: modifiers.shift,
                        additive_click: modifiers.ctrl,
                    });
                } else if !modifiers.ctrl && !modifiers.shift {
                    self.clear_selection();
                }
            }
        }
        if ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary))
            && let Some(drag) = &mut self.drag
            && drag.corner.is_none()
            && drag.duplicate_target.is_none()
        {
            self.layout.custom.clone_from(&drag.before);
            drag.duplicate_target = drag.members.first().map(|(index, _)| *index);
        }
        if let (Some(drag), Some(pointer)) = (&self.drag, pointer) {
            let delta = (pointer - drag.pointer) / scale;
            let inset = self.edit_inset();
            if drag.duplicate_target.is_some() {
                if let (Some(art), Some(texture)) = (&self.art, &self.texture)
                    && pointer.distance(drag.pointer) >= 4.
                {
                    let offset = drag.duplicate_offset(
                        delta,
                        inset,
                        [self.layout.page_width, self.layout.page_height],
                    );
                    let (dx, dy) =
                        self.snap_move(&drag.members, offset, scale, drag.locked_axis(delta));
                    for (i, p) in &drag.members {
                        let rect = screen(sheet::Placement {
                            x: p.x + dx,
                            y: p.y + dy,
                            ..*p
                        });
                        ui.painter().image(
                            texture.id(),
                            rect,
                            self.image_uv(*i, art),
                            egui::Color32::from_white_alpha(150),
                        );
                    }
                }
            } else if let Some(corner) = drag.corner {
                for (index, p) in group_resize(
                    &drag.members,
                    corner,
                    [delta.x as f64, delta.y as f64],
                    self.lock,
                    inset,
                    [self.layout.page_width, self.layout.page_height],
                ) {
                    let original = drag.members.iter().find(|(i, _)| *i == index).unwrap().1;
                    if p != original || drag.before.contains_key(&index) {
                        self.layout.custom.insert(index, p);
                    } else {
                        self.layout.custom.remove(&index);
                    }
                }
            } else {
                let offset = group_move_delta(
                    &drag.members,
                    delta.x as f64,
                    delta.y as f64,
                    inset,
                    [self.layout.page_width, self.layout.page_height],
                );
                let (dx, dy) = self.snap_move(&drag.members, offset, scale, None);
                if (dx.abs() > 1e-8 || dy.abs() > 1e-8)
                    && let (Some(art), Some(texture)) = (&self.art, &self.texture)
                {
                    for (i, original) in &drag.members {
                        let rect = screen(*original);
                        ui.painter().image(
                            texture.id(),
                            rect,
                            self.image_uv(*i, art),
                            egui::Color32::from_white_alpha(80),
                        );
                        ui.painter().rect_stroke(
                            rect,
                            0.,
                            egui::Stroke::new(1_f32, egui::Color32::from_gray(140)),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                for (index, start) in &drag.members {
                    let p = sheet::Placement {
                        x: start.x + dx,
                        y: start.y + dy,
                        ..*start
                    };
                    if p != *start || drag.before.contains_key(index) {
                        self.layout.custom.insert(*index, p);
                    } else {
                        self.layout.custom.remove(index);
                    }
                }
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary)) {
            if let Some(drag) = self.drag.take() {
                if let Some(index) = drag.duplicate_target {
                    if let Some(pointer) = pointer.filter(|p| p.distance(drag.pointer) >= 4.) {
                        let delta = (pointer - drag.pointer) / scale;
                        let actual = drag.duplicate_offset(
                            delta,
                            self.edit_inset(),
                            [self.layout.page_width, self.layout.page_height],
                        );
                        let actual =
                            self.snap_move(&drag.members, actual, scale, drag.locked_axis(delta));
                        self.duplicate_images(&drag.members, actual, ui.ctx());
                        return;
                    }
                    self.select_target(index, drag.additive_click, drag.axis_locked);
                } else {
                    self.record_object_edit(drag.before);
                }
            }
        }
        if let Some(pointer) = pointer {
            if response.hovered() {
                let members: Vec<_> = self
                    .selection
                    .iter()
                    .filter(|i| !self.layout.hidden_images.contains(i))
                    .map(|i| (*i, items[*i]))
                    .collect();
                if let Some(bounds) = group_bounds(&members) {
                    if let Some(corner) = corners(screen(bounds))
                        .iter()
                        .position(|p| p.distance(pointer) <= 8.)
                    {
                        ui.ctx().set_cursor_icon(if corner % 2 == 0 {
                            egui::CursorIcon::ResizeNwSe
                        } else {
                            egui::CursorIcon::ResizeNeSw
                        });
                        return;
                    }
                }
                if items.iter().enumerate().any(|(i, p)| {
                    !self.layout.hidden_images.contains(&i) && screen(*p).contains(pointer)
                }) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
            }
        }
    }
    fn draw_selection(&self, painter: &egui::Painter, page: egui::Rect, scale: f32) {
        let Ok(items) = self.layout.items() else {
            return;
        };
        for index in self
            .selection
            .iter()
            .copied()
            .filter(|i| !self.layout.hidden_images.contains(i))
        {
            let Some(item) = items.get(index) else {
                continue;
            };
            let r = egui::Rect::from_min_size(
                page.min + egui::vec2(item.x as f32 * scale, item.y as f32 * scale),
                egui::vec2(item.width as f32 * scale, item.height as f32 * scale),
            );
            let colour = egui::Color32::from_rgb(30, 95, 180);
            let points = vec![
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
            ];
            painter.add(egui::Shape::closed_line(
                points.clone(),
                egui::Stroke::new(1.2_f32, colour),
            ));
            if self.selection.len() == 1 {
                for p in points {
                    let h = egui::Rect::from_center_size(p, egui::vec2(8., 8.));
                    painter.rect_filled(h, 0., egui::Color32::WHITE);
                    painter.add(egui::Shape::closed_line(
                        vec![
                            h.left_top(),
                            h.right_top(),
                            h.right_bottom(),
                            h.left_bottom(),
                        ],
                        egui::Stroke::new(1.0_f32, colour),
                    ));
                }
            }
            if self.selection.len() == 1 {
                painter.text(
                    r.left_bottom() + egui::vec2(0., 12.),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "Image {}  {}",
                        index + 1,
                        self.unit.size(item.width, item.height)
                    ),
                    egui::FontId::proportional(10.),
                    colour,
                );
            }
        }
        if self.selection.len() > 1 {
            let members: Vec<_> = self
                .selection
                .iter()
                .filter(|i| !self.layout.hidden_images.contains(i))
                .filter_map(|i| items.get(*i).map(|p| (*i, *p)))
                .collect();
            if let Some(b) = group_bounds(&members) {
                let r = egui::Rect::from_min_size(
                    page.min + egui::vec2(b.x as f32 * scale, b.y as f32 * scale),
                    egui::vec2(b.width as f32 * scale, b.height as f32 * scale),
                );
                let colour = egui::Color32::from_rgb(30, 95, 180);
                let corners = vec![
                    r.left_top(),
                    r.right_top(),
                    r.right_bottom(),
                    r.left_bottom(),
                ];
                painter.add(egui::Shape::closed_line(
                    corners.clone(),
                    egui::Stroke::new(1.5_f32, colour),
                ));
                for p in corners {
                    let h = egui::Rect::from_center_size(p, egui::vec2(9., 9.));
                    painter.rect_filled(h, 0., egui::Color32::WHITE);
                    painter.add(egui::Shape::closed_line(
                        vec![
                            h.left_top(),
                            h.right_top(),
                            h.right_bottom(),
                            h.left_bottom(),
                        ],
                        egui::Stroke::new(1.2_f32, colour),
                    ));
                }
            }
        }
    }
    fn tool_icon(ui: &mut egui::Ui, kind: u8, selected: bool, hint: &str) -> bool {
        let (r, response) = ui.allocate_exact_size(egui::vec2(32., 32.), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, hint)
        });
        let painter = ui.painter();
        if selected || response.hovered() {
            painter.rect_filled(
                r,
                crate::ui_theme::BUTTON_RADIUS,
                if selected {
                    egui::Color32::from_gray(176)
                } else {
                    egui::Color32::from_gray(220)
                },
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
            0 => {
                line((8., 22.), (8., 26.));
                line((8., 26.), (24., 26.));
                line((24., 26.), (24., 22.));
                line((16., 22.), (16., 7.));
                line((10., 13.), (16., 7.));
                line((16., 7.), (22., 13.));
            }
            1 => {
                for (y, x) in [(9., 12.), (16., 21.), (23., 15.)] {
                    line((6., y), (26., y));
                    painter.rect_filled(
                        egui::Rect::from_center_size(p(x, y), egui::vec2(4., 6.)),
                        1.,
                        egui::Color32::from_gray(60),
                    );
                }
            }
            2 => {
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
            3 => {
                painter.add(egui::Shape::closed_line(
                    vec![p(6., 20.), p(10., 8.), p(22., 6.), p(27., 19.), p(18., 27.)],
                    ink,
                ));
            }
            4 => {
                for n in [7., 16., 25.] {
                    line((n, 7.), (n, 25.));
                    line((7., n), (25., n));
                }
            }
            6 => {
                painter.add(egui::Shape::closed_line(
                    vec![p(9., 5.), p(19., 5.), p(24., 10.), p(24., 27.), p(9., 27.)],
                    ink,
                ));
                line((13., 17.), (20., 17.));
                line((16.5, 13.5), (16.5, 20.5));
            }
            7 => {
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
            8 => {
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
            9 | 10 => {
                let right = if kind == 10 { 21. } else { 26. };
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
                if kind == 10 {
                    line((22., 13.), (30., 13.));
                    line((26., 9.), (26., 17.));
                }
            }
            11 => {
                line((7., 23.), (7., 27.));
                line((7., 27.), (25., 27.));
                line((25., 27.), (25., 23.));
                line((16., 6.), (16., 22.));
                line((11., 17.), (16., 22.));
                line((16., 22.), (21., 17.));
            }
            12 => {
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
            13 => {
                painter.circle_stroke(p(16., 16.), 9., ink);
            }
            15 => {
                painter.add(egui::Shape::closed_line(
                    vec![p(8., 5.), p(24., 18.), p(17., 19.), p(14., 27.)],
                    ink,
                ));
            }
            14 => {
                painter.rect_stroke(
                    egui::Rect::from_min_max(p(6., 8.), p(26., 24.)),
                    4.,
                    ink,
                    egui::StrokeKind::Inside,
                );
            }
            _ => {
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
        response.on_hover_text(hint).clicked()
    }
    fn choose_image(&mut self, ctx: &egui::Context) {
        if !self.source.is_empty() {
            self.choose_add_image(ctx);
            return;
        }
        if let Some(mut paths) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg"])
            .pick_files()
        {
            let action = if paths.len() == 1 {
                DocumentAction::OpenImage(paths.remove(0))
            } else {
                DocumentAction::OpenImageGroup(paths)
            };
            self.request_action(action, ctx);
        }
    }

    fn choose_add_image(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg"])
            .pick_file()
        {
            self.add_image(&path, ctx);
        }
    }

    fn add_image(&mut self, path: &std::path::Path, ctx: &egui::Context) {
        let inset = self.layout.margin.max(self.edit_inset());
        let result = crate::photo_size::append_image(&self.layout, &self.source, path, inset);
        self.finish_add_image(result, ctx);
    }

    fn paste_image(&mut self, ctx: &egui::Context) {
        let result = (|| -> Result<Vec<u8>, String> {
            let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
            let image = clipboard.get_image().map_err(|error| match error {
                arboard::Error::ContentNotAvailable => {
                    "No image on the clipboard. Copy pixels or use Copy Merged in Photoshop.".into()
                }
                _ => error.to_string(),
            })?;
            if image.width > 8000
                || image.height > 8000
                || image.width.saturating_mul(image.height) > 32_000_000
            {
                return Err("Clipboard image is too large.".into());
            }
            let pixels = image::RgbaImage::from_raw(
                image.width as u32,
                image.height as u32,
                image.bytes.into_owned(),
            )
            .ok_or("Clipboard image data is invalid.")?;
            let mut png = std::io::Cursor::new(Vec::new());
            pixels
                .write_to(&mut png, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            Ok(png.into_inner())
        })();
        match result {
            Ok(bytes) if self.source.is_empty() => {
                self.install_single_image(bytes, "Clipboard image".into(), ctx);
                if self.art.is_some() {
                    let inset = self.layout.margin.max(self.edit_inset());
                    let max_width = self.layout.page_width - 2. * inset;
                    let max_height = self.layout.page_height - 2. * inset;
                    let fit = (max_width / self.layout.width)
                        .min(max_height / self.layout.height)
                        .min(1.);
                    if fit > 0. && fit < 1. {
                        self.layout.margin = inset;
                        self.layout.width *= fit;
                        self.layout.height *= fit;
                        self.status
                            .push_str(" Pasted image was scaled to fit the sheet.");
                    }
                }
            }
            Ok(bytes) => {
                let inset = self.layout.margin.max(self.edit_inset());
                let result = crate::photo_size::append_image_bytes(
                    &self.layout,
                    &self.source,
                    &bytes,
                    inset,
                );
                self.finish_add_image(result, ctx);
            }
            Err(error) => self.status = format!("Cannot paste image: {error}"),
        }
    }

    fn finish_add_image(&mut self, result: Result<(Vec<u8>, Layout), String>, ctx: &egui::Context) {
        let result = result.and_then(|(source, layout)| {
            layout.trace_images(
                &source,
                self.remove_white,
                self.threshold,
                self.cut_shape,
                self.background_tolerance,
                self.print_without_background,
            )?;
            Ok((source, layout))
        });
        match result {
            Ok((source, layout)) => {
                self.source = source;
                self.layout = layout;
                self.name = format!("{} images", self.layout.count);
                self.clear_selection();
                self.select_target(self.layout.count - 1, false, false);
                self.drag = None;
                // Object undo snapshots refer to the old number of placements.
                self.undo.clear();
                self.redo.clear();
                self.last_print = None;
                self.retrace(ctx);
                self.status = format!(
                    "Image {} added. Check preview before printing or exporting.",
                    self.layout.count
                );
            }
            Err(error) => self.status = format!("Cannot add image: {error}"),
        }
    }

    fn load_image_group(&mut self, paths: &[PathBuf], ctx: &egui::Context) {
        let inset = self.layout.margin.max(self.edit_inset());
        let result = crate::photo_size::image_group(
            paths,
            [
                self.layout.page_width - 2. * inset,
                self.layout.page_height - 2. * inset,
            ],
        );
        match result {
            Ok(group) => {
                let [width, height] = group.size;
                let mut layout = self.layout.clone();
                layout.count = group.tiles.len();
                layout.image_tiles = group.tiles;
                layout.custom = group
                    .placements
                    .into_iter()
                    .enumerate()
                    .map(|(i, mut p)| {
                        p.x += inset;
                        p.y += inset;
                        (i, p)
                    })
                    .collect();
                layout.margin = inset;
                layout.width = width;
                layout.height = height;
                if let Err(error) = layout.trace_images(
                    &group.png,
                    self.remove_white,
                    self.threshold,
                    sheet::CutShape::AlphaOutline,
                    self.background_tolerance,
                    self.print_without_background,
                ) {
                    self.status = error;
                    return;
                }
                self.close_image();
                self.source = group.png;
                self.name = format!("{} images", paths.len());
                self.cut_shape = sheet::CutShape::AlphaOutline;
                self.layout = layout;
                self.ratio = width / height;
                self.retrace(ctx);
            }
            Err(error) => self.status = error,
        }
    }
    fn cached_capacity(&mut self) -> Result<usize, String> {
        if self
            .capacity_cache
            .as_ref()
            .is_none_or(|(layout, _)| layout != &self.layout)
        {
            self.capacity_cache = Some((self.layout.clone(), self.layout.available_capacity()));
        }
        self.capacity_cache.as_ref().unwrap().1.clone()
    }
    pub fn document_menu(
        &mut self,
        ctx: &egui::Context,
        updates: &mut crate::updater::UpdateDialog,
        frame: &eframe::Frame,
    ) {
        egui::TopBottomPanel::top("document-menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open project...   Ctrl+Shift+O").clicked() {
                        ui.close_menu();
                        self.choose_project(ctx);
                    }
                    self.recent_projects_menu(ui, ctx);
                    if ui.button("Save   Ctrl+S").clicked() {
                        ui.close_menu();
                        self.save_project(false);
                    }
                    if ui
                        .button("Save All")
                        .on_hover_text("Save every open project tab")
                        .clicked()
                    {
                        self.workspace_action = Some(WorkspaceAction::SaveAll);
                        ui.close_menu();
                    }
                    if ui.button("Close project   Ctrl+W").clicked() {
                        self.request_tab_close(ctx);
                        ui.close_menu();
                    }
                    if ui
                        .button("Close All")
                        .on_hover_text("Close all project tabs and keep the app open")
                        .clicked()
                    {
                        self.workspace_action = Some(WorkspaceAction::CloseAll);
                        ui.close_menu();
                    }
                    if ui.button("Save As...   Ctrl+Shift+S").clicked() {
                        ui.close_menu();
                        self.save_project(true);
                    }
                    ui.separator();
                    if ui.button("Settings...").clicked() {
                        self.show_settings = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("New document...   Ctrl+N").clicked() {
                        self.request_action(DocumentAction::New, ctx);
                        ui.close_menu();
                    }
                    ui.separator();

                    if ui.button("Import image...   Ctrl+O").clicked() {
                        ui.close_menu();
                        self.choose_image(ctx);
                    }
                    if ui
                        .add_enabled(!self.source.is_empty(), egui::Button::new("Close image"))
                        .clicked()
                    {
                        self.request_action(DocumentAction::CloseImage, ctx);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(self.can_export_cut(), egui::Button::new("Export PLT..."))
                        .clicked()
                    {
                        ui.close_menu();
                        self.export_cut_file(frame);
                    }
                    if ui
                        .add_enabled(
                            self.art.is_some(),
                            egui::Button::new("Export print & cut job..."),
                        )
                        .clicked()
                    {
                        ui.close_menu();
                        self.export_pair(frame);
                    }
                    if ui.button("Print...   Ctrl+P").clicked() {
                        self.show_print_dialog = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.button("Paste image   Ctrl+V").clicked() {
                        ui.close_menu();
                        self.paste_image(ctx);
                    }
                    if ui
                        .add_enabled(
                            self.art.is_some(),
                            egui::Button::new("Select all images   Ctrl+A"),
                        )
                        .clicked()
                    {
                        self.select_all();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            !self.selection.is_empty(),
                            egui::Button::new("Repeat duplicate   Ctrl+R"),
                        )
                        .clicked()
                    {
                        self.repeat_duplicate(ctx);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            !self.selection.is_empty(),
                            egui::Button::new("Delete selected images   Del"),
                        )
                        .clicked()
                    {
                        self.delete_selected_images(ctx);
                        ui.close_menu();
                    }

                    if ui
                        .add_enabled(
                            !self.undo.is_empty(),
                            egui::Button::new("Undo object edit   Ctrl+Z"),
                        )
                        .clicked()
                    {
                        self.undo_object(ctx);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            !self.redo.is_empty(),
                            egui::Button::new("Redo object edit   Ctrl+Y"),
                        )
                        .clicked()
                    {
                        self.redo_object(ctx);
                        ui.close_menu();
                    }
                    if ui.button("Deselect   Esc").clicked() {
                        self.clear_selection();
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.rulers, "Rulers");
                    ui.checkbox(&mut self.snap_guides, "Snap to guides");
                    if ui
                        .add_enabled(
                            self.layout.guides.iter().any(|axis| !axis.is_empty()),
                            egui::Button::new("Clear ruler guides"),
                        )
                        .clicked()
                    {
                        self.layout.guides = Default::default();
                        self.guide_drag = None;
                        self.selected_guide = None;
                        ui.close_menu();
                    }
                    ui.checkbox(&mut self.show_margin_guides, "Margin guides");
                    ui.checkbox(&mut self.show_properties, "Properties sidebar");
                    ui.menu_button("Contour colour", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for color in [
                                egui::Color32::BLACK,
                                egui::Color32::GRAY,
                                egui::Color32::from_rgb(0, 135, 85),
                                egui::Color32::BLUE,
                                egui::Color32::RED,
                                egui::Color32::from_rgb(130, 50, 170),
                            ] {
                                if ui
                                    .add_sized([20., 20.], egui::Button::new("").fill(color))
                                    .clicked()
                                {
                                    self.contour_color = color;
                                    ui.close_menu();
                                }
                            }
                        });
                    });
                    ui.separator();
                    if ui.button("Vector document workspace").clicked() {
                        self.workspace_action = Some(WorkspaceAction::VectorWorkspace);
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.checkbox(
                        &mut self.grid,
                        format!("{} {} page grid", self.unit.number(10.), self.unit.label()),
                    );
                    ui.checkbox(&mut self.show_cut, "Cut outlines");
                    if ui.button("Fit page").clicked() {
                        self.zoom = 1.;
                        self.canvas_pan = egui::Vec2::ZERO;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Layout", |ui| {
                    if ui
                        .add_enabled(
                            self.layout.image_tiles.is_empty(),
                            egui::Button::new("Reflow all copies to grid"),
                        )
                        .clicked()
                    {
                        self.reflow();
                        ui.close_menu();
                    }

                    if ui
                        .add_enabled(
                            self.layout.image_tiles.is_empty(),
                            egui::Button::new("Fill sheet to capacity"),
                        )
                        .clicked()
                    {
                        if let Ok(capacity) = self.layout.available_capacity() {
                            self.layout.count = capacity;
                        }
                        ui.close_menu();
                    }
                    if ui.button("Page properties").clicked() {
                        self.show_properties = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Cut", |ui| {
                    ui.checkbox(&mut self.ccd_trial, "CCD four-dot registration");
                    ui.checkbox(&mut self.layout.cut_inner_holes, "Cut inner holes");
                    if ui.button("Contour properties").clicked() {
                        self.show_properties = true;
                        ui.close_menu();
                    }
                });
                if ui.button("Quick guide").clicked() {
                    self.show_guide = true;
                }
                updates.menu(ui, ctx);
            });
        });
    }
    pub fn standard_toolbar(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        egui::TopBottomPanel::top("standard-toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if Self::tool_icon(ui, 6, false, "New project (Ctrl+N)") {
                    self.request_action(DocumentAction::New, ctx);
                }
                if Self::tool_icon(ui, 7, false, "Open project (Ctrl+Shift+O)") {
                    self.choose_project(ctx);
                }
                if Self::tool_icon(ui, 8, false, "Save project (Ctrl+S)") {
                    self.save_project(false);
                }
                ui.separator();
                if Self::tool_icon(ui, 0, false, "Import image (Ctrl+O)") {
                    self.choose_image(ctx);
                }
                if ui
                    .add_enabled_ui(self.art.is_some(), |ui| {
                        Self::tool_icon(ui, 10, false, "Add image")
                    })
                    .inner
                {
                    self.choose_add_image(ctx);
                }
                if Self::tool_icon(ui, 2, self.show_print_dialog, "Print (Ctrl+P)") {
                    self.show_print_dialog = true;
                }
                if ui
                    .add_enabled_ui(self.can_export_cut(), |ui| {
                        Self::tool_icon(ui, 11, false, "Export PLT")
                    })
                    .inner
                {
                    self.export_cut_file(frame);
                }
                ui.separator();
                for (index, (icon, label)) in
                    [(9, "Artwork"), (1, "Size and Repeat"), (3, "Cut Outline")]
                        .into_iter()
                        .enumerate()
                {
                    let visible = self.show_properties
                        && !self.show_print_dialog
                        && self
                            .section_palettes
                            .iter()
                            .position(|section| section.visible)
                            .unwrap_or(0)
                            == index;
                    if Self::tool_icon(ui, icon, visible, label) {
                        self.open_sidebar_section(index);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("MC630  |  Print & Cut").small());
                });
            });
        });
    }
    pub fn ui(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        self.print_panel.poll_job(ctx, &mut self.status);
        if !self.show_print_dialog
            && self.pending_action.is_none()
            && self.autosave.offered.is_empty()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::P))
        {
            self.show_print_dialog = true;
        }
        if !self.show_print_dialog
            && self.pending_action.is_none()
            && self.autosave.offered.is_empty()
        {
            if ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::S)
            }) {
                self.save_project(true);
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::S)) {
                self.save_project(false);
            }
            if ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::O)
            }) {
                self.choose_project(ctx);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::N)) {
                self.request_action(DocumentAction::New, ctx);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::O)) {
                self.choose_image(ctx);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::W)) {
                self.request_action(DocumentAction::CloseImage, ctx);
            }
            self.new_document_dialog(ctx);
            self.settings_dialog(ctx);
        }

        self.layout
            .custom
            .retain(|index, _| *index < self.layout.count);
        self.selection.retain(|i| *i < self.layout.count);
        self.layout.hidden_images.retain(|i| *i < self.layout.count);
        self.sync_selection();
        if !self.show_print_dialog && !ctx.wants_keyboard_input() && self.pending_action.is_none() {
            if !self.show_new && !self.show_settings && self.drag.is_none() {
                // egui-winit consumes Ctrl+V on press for text paste, but forwards release.
                let paste = ctx.input(|input| {
                    input.events.iter().any(|event| {
                        matches!(event, egui::Event::Key {
                        key: egui::Key::V, pressed: false, modifiers, ..
                    } if modifiers.ctrl && !modifiers.alt && !modifiers.shift)
                    })
                });
                if paste {
                    self.paste_image(ctx);
                }
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::R)) {
                    self.repeat_duplicate(ctx);
                }
                if self.guide_drag.is_none()
                    && self.cut_draft.is_none()
                    && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
                    && !self.delete_selected_guide()
                    && !self.remove_selected_cut_shape()
                {
                    self.delete_selected_images(ctx);
                }
            }
            if !self.show_new
                && !self.show_settings
                && !self.selection.is_empty()
                && self.drag.is_none()
                && !self.panning
            {
                let delta = ctx.input_mut(|i| {
                    let mods = i.modifiers;
                    if mods.ctrl || mods.alt || mods.command {
                        return [0., 0.];
                    }
                    let keymods = if mods.shift {
                        egui::Modifiers::SHIFT
                    } else {
                        egui::Modifiers::NONE
                    };
                    let step = if mods.shift {
                        self.nudge_coarse
                    } else {
                        self.nudge_fine
                    };
                    let mut delta = [0., 0.];
                    for (key, axis, direction) in [
                        (egui::Key::ArrowLeft, 0, -1.),
                        (egui::Key::ArrowRight, 0, 1.),
                        (egui::Key::ArrowUp, 1, -1.),
                        (egui::Key::ArrowDown, 1, 1.),
                    ] {
                        if i.consume_key(keymods, key) {
                            delta[axis] += direction * step;
                        }
                    }
                    delta
                });
                if delta != [0., 0.] {
                    self.nudge_selection(delta[0], delta[1]);
                }
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::A)) {
                self.select_all();
            }

            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z)) {
                self.undo_object(ctx);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Y)) {
                self.redo_object(ctx);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.guide_drag = None;
                self.selected_guide = None;
                self.cut_draft = None;
                self.pick_cut_shapes = false;
                self.selected_cut_shape = None;
                if let Some(drag) = self.cut_shape_drag.take() {
                    self.layout.manual_cut_paths[drag.index] = drag.path;
                }
                if let Some(drag) = self.drag.take() {
                    self.layout.custom = drag.before;
                }
                self.clear_selection();
            }
        }

        if let Some(p) = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            if p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("mc630"))
            {
                self.request_action(DocumentAction::OpenProject(p), ctx);
            } else if self.source.is_empty() {
                self.request_action(DocumentAction::OpenImage(p), ctx);
            } else {
                self.add_image(&p, ctx);
            }
        }

        egui::TopBottomPanel::bottom("a4-status").show(ctx, |ui| {
            ui.add_space(1.);

            ui.horizontal(|ui| {
                ui.small("Status");
                ui.separator();
                ui.label(&self.status);
            });

            ui.add_space(1.);
        });

        egui::TopBottomPanel::bottom("property-bar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.small("Control");
                ui.separator();
                ui.label(self.layout.paper_label());
                ui.small(self.unit.size(self.layout.page_width,self.layout.page_height));
                ui.separator();
                if self.selection.len()>1 {
                    ui.strong(format!("{} images selected",self.selection.len()));
                    ui.small("Drag to move / corners to resize").on_hover_text("Arrow keys: move selection. Shift + Arrow: larger step. Configure steps in File > Settings. Ctrl+Z: Undo.");
                    ui.checkbox(&mut self.lock,"Lock ratio");
                } else if self.selected.is_some() {
                    self.selected_properties(ui);
                    ui.checkbox(&mut self.lock, "Lock ratio");
                } else {
                    ui.label("W");
                    if ui
                        .add(
                            self.unit.distance(&mut self.layout.width)
                                .speed(0.1)
                                .range(1.0..=self.layout.page_width - 10.)
                                ,
                        )
                        .changed()
                        && self.lock
                    {
                        self.layout.height = self.layout.width / self.ratio;
                    }
                    ui.label("H");
                    if ui
                        .add(
                            self.unit.distance(&mut self.layout.height)
                                .speed(0.1)
                                .range(1.0..=self.layout.page_height - 10.)
                                ,
                        )
                        .changed()
                        && self.lock
                    {
                        self.layout.width = self.layout.height * self.ratio;
                    }
                }
                ui.label("Copies");
                ui.add_enabled(self.layout.image_tiles.is_empty(), egui::DragValue::new(&mut self.layout.count).update_while_editing(false).range(1..=1000));
                ui.label("Border");
                ui.add(
                    self.unit.distance(&mut self.layout.contour_offset)
                        .speed(0.1)
                        .range(-5.0..=5.0)
                        ,
                );
                ui.separator();
                if ui.small_button("-").on_hover_text("Zoom out").clicked() {
                    self.zoom = (self.zoom - 0.25).max(0.5);
                }
                ui.label(format!("{:.0}%", self.zoom * 100.)).on_hover_text("Ctrl + mouse wheel: zoom at pointer. Space + left drag: pan. Fit page resets the view.");
                if ui.small_button("+").on_hover_text("Zoom in").clicked() {
                    self.zoom = (self.zoom + 0.25).min(16.);
                }
                if ui.button("Fit page").clicked() {
                    self.zoom = 1.;
                    self.canvas_pan = egui::Vec2::ZERO;
                }
            });
        });
        if self.show_guide {
            egui::Window::new("Print & Cut workflow").open(&mut self.show_guide).resizable(false).show(ctx,|ui|{
                ui.label("1. Import an image and choose frame / object / transparent outline.");
                ui.label("2. Set dimensions, copies, spacing and cut border in Properties.");
                ui.label("3. Enable CCD and use the top-left dot as origin.");
                ui.label("4. Open Print / Export; match the document paper size at Actual size (100%).");
                ui.label("5. Export the matching PLT and copy it directly to your USB stick.");
                ui.small("Click an image to select; drag to move; drag a corner to resize. Keep proportions controls aspect ratio. Ctrl+Z / Ctrl+Y undo / redo. Zoom, rulers, grid and palette colours affect the screen only. Force / speed are set on the cutter.");
            });
        }
        egui::SidePanel::left("toolbox")
            .exact_width(82.)
            .resizable(false)
            .show(ctx, |ui| {
                ui.small("Tools");
                ui.separator();
                ui.horizontal(|ui| {
                    if Self::tool_icon(ui, 0, false, "Import image") {
                        self.choose_image(ctx);
                    }
                    if Self::tool_icon(ui, 1, self.show_properties, "Properties sidebar") {
                        self.show_properties = true;
                    }
                });
                ui.horizontal(|ui| {
                    if Self::tool_icon(ui, 2, self.show_print_dialog, "Print (Ctrl+P)") {
                        self.show_print_dialog = true;
                    }
                    if Self::tool_icon(ui, 3, self.show_cut, "Show / hide cut outlines") {
                        self.show_cut = !self.show_cut;
                    }
                });
                ui.horizontal(|ui| {
                    if Self::tool_icon(ui, 4, self.grid, "Show / hide grid") {
                        self.grid = !self.grid;
                    }
                    if Self::tool_icon(ui, 5, false, "Fit page") {
                        self.zoom = 1.;
                        self.canvas_pan = egui::Vec2::ZERO;
                    }
                });
                ui.separator();
                ui.small("Cut");
                let active = self.cut_draft.as_ref().map(|draft| draft.tool);
                {
                    ui.horizontal(|ui| {
                        if Self::tool_icon(ui, 12, active == Some(Tool::Pen), "Pen cut tool") {
                            self.activate_cut_tool(Tool::Pen);
                        }
                        if Self::tool_icon(
                            ui,
                            13,
                            active == Some(Tool::Ellipse),
                            "Circle / Ellipse cut tool",
                        ) {
                            self.activate_cut_tool(Tool::Ellipse);
                        }
                    });
                    if Self::tool_icon(
                        ui,
                        14,
                        active == Some(Tool::Rectangle),
                        "Rectangle / Rounded rectangle cut tool",
                    ) {
                        self.activate_cut_tool(Tool::Rectangle);
                    }
                    if Self::tool_icon(ui, 15, self.pick_cut_shapes, "Select images and cut shapes")
                    {
                        self.activate_cut_picker();
                    }
                }
                ui.separator();
                ui.small("Space + drag")
                    .on_hover_text("Hold Space and drag to pan the page");
                ui.small("Ctrl + wheel")
                    .on_hover_text("Hold Ctrl and use the mouse wheel to zoom");
            });
        self.properties_sidebar(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0))
            .show(ctx, |ui| {
                if let Some(problem) = &self.autosave.problem {
                    ui.colored_label(egui::Color32::DARK_RED, "Auto Save unavailable")
                        .on_hover_text(problem);
                }
                let viewport = ui.available_size();
                let viewport_rect = egui::Rect::from_min_size(ui.cursor().min, viewport);
                let space =
                    ctx.input(|i| i.key_down(egui::Key::Space)) && !ctx.wants_keyboard_input();
                let pointer_over = ui.rect_contains_pointer(viewport_rect);
                let wheel = if pointer_over {
                    ctx.input_mut(|i| {
                        if i.modifiers.ctrl {
                            let delta = i.raw_scroll_delta.y;
                            i.raw_scroll_delta = egui::Vec2::ZERO;
                            i.smooth_scroll_delta = egui::Vec2::ZERO;
                            delta
                        } else {
                            0.
                        }
                    })
                } else {
                    0.
                };
                let old_zoom = self.zoom;
                if wheel != 0. {
                    self.zoom = (self.zoom * (wheel * 0.003).exp()).clamp(0.5, 16.);
                }
                if space
                    && pointer_over
                    && ctx.input(|i| i.pointer.primary_pressed())
                    && self.drag.is_none()
                {
                    self.panning = true;
                }
                let pan_gesture = self.panning || (space && pointer_over);
                if self.panning {
                    self.canvas_pan += ctx.input(|i| i.pointer.delta());
                    if !ctx.input(|i| i.pointer.primary_down()) {
                        self.panning = false;
                    }
                }
                if pan_gesture {
                    ctx.set_cursor_icon(if self.panning {
                        egui::CursorIcon::Grabbing
                    } else {
                        egui::CursorIcon::Grab
                    });
                }
                let canvas = egui::ScrollArea::both()
                    .drag_to_scroll(false)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let canvas_size = viewport * self.zoom.max(1.);
                        let (area, canvas_response) =
                            ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());
                        let painter = ui.painter_at(area);

                        painter.rect_filled(area, 0., egui::Color32::from_rgb(225, 228, 233));

                        let scale = ((viewport.x - 90.) / self.layout.page_width as f32)
                            .min((viewport.y - 80.) / self.layout.page_height as f32)
                            .max(0.01)
                            * self.zoom;
                        if self.zoom != old_zoom {
                            if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                                let old_center = area.min
                                    + viewport * old_zoom.max(1.) * 0.5
                                    + egui::vec2(14., 12.)
                                    + self.canvas_pan;
                                let new_center =
                                    area.center() + egui::vec2(14., 12.) + self.canvas_pan;
                                self.canvas_pan += pointer
                                    + (old_center - pointer) * (self.zoom / old_zoom)
                                    - new_center;
                            }
                        }
                        let page = egui::Rect::from_center_size(
                            area.center() + egui::vec2(14., 12.) + self.canvas_pan,
                            egui::vec2(
                                self.layout.page_width as f32 * scale,
                                self.layout.page_height as f32 * scale,
                            ),
                        );
                        painter.rect_filled(
                            page.translate(egui::vec2(4., 6.)),
                            2.,
                            egui::Color32::from_gray(180),
                        );

                        painter.rect_filled(page, 0., egui::Color32::WHITE);
                        painter.rect_stroke(
                            page,
                            0.,
                            egui::Stroke::new(1_f32, egui::Color32::BLACK),
                            egui::StrokeKind::Outside,
                        );
                        if self.show_margin_guides {
                            let inset = self.layout.margin as f32 * scale;
                            let guide = page.shrink(inset);
                            if guide.is_positive() {
                                painter.rect_stroke(
                                    guide,
                                    0.,
                                    egui::Stroke::new(
                                        0.7_f32,
                                        egui::Color32::from_rgb(174, 126, 174),
                                    ),
                                    egui::StrokeKind::Inside,
                                );
                            }
                        }
                        if self.grid {
                            let grid = egui::Color32::from_rgb(228, 232, 235);
                            for x in (0..=self.layout.page_width as u32).step_by(10) {
                                let px = page.left() + x as f32 * scale;
                                painter.line_segment(
                                    [egui::pos2(px, page.top()), egui::pos2(px, page.bottom())],
                                    egui::Stroke::new(0.5_f32, grid),
                                );
                            }
                            for y in (0..=self.layout.page_height as u32).step_by(10) {
                                let py = page.top() + y as f32 * scale;
                                painter.line_segment(
                                    [egui::pos2(page.left(), py), egui::pos2(page.right(), py)],
                                    egui::Stroke::new(0.5_f32, grid),
                                );
                            }
                        }
                        if self.art.is_none()
                            && self.layout.manual_cut_paths.is_empty()
                            && self.cut_draft.is_none()
                        {
                            painter.text(
                                page.center(),
                                egui::Align2::CENTER_CENTER,
                                "Your artwork goes here\nChoose an image or load the sample",
                                egui::FontId::proportional(15.),
                                egui::Color32::from_rgb(100, 116, 139),
                            );
                        }

                        let pos = |x: f64, y: f64| {
                            page.min + egui::vec2(x as f32 * scale, y as f32 * scale)
                        };

                        let view = viewport_rect;
                        if self.cut_draft.is_none() {
                            let guide_gesture =
                                self.update_ruler_guides(ui, view, page, scale, pan_gesture);
                            if !pan_gesture
                                && !guide_gesture
                                && !self.cut_picker_owns_gesture(ui.ctx(), page, scale)
                            {
                                self.canvas_edit(ui, &canvas_response, page, scale);
                            }
                        }
                        match self.layout.items() {
                            Ok(positions) => {
                                if let (Some(texture), Some(a)) = (&self.texture, &self.art) {
                                    for (i, p) in positions.into_iter().enumerate() {
                                        if self.layout.hidden_images.contains(&i) {
                                            continue;
                                        }
                                        let r = egui::Rect::from_min_size(
                                            pos(p.x, p.y),
                                            egui::vec2(
                                                p.width as f32 * scale,
                                                p.height as f32 * scale,
                                            ),
                                        );

                                        let uv = self.image_uv(i, a);
                                        painter.image(texture.id(), r, uv, egui::Color32::WHITE);
                                    }

                                    if self.show_cut {
                                        let mut preview_layout = self.layout.clone();

                                        if self.ccd_trial {
                                            preview_layout.offset_x =
                                                -self.ccd_profile.origin(&self.layout)[0];

                                            preview_layout.offset_y =
                                                -self.ccd_profile.origin(&self.layout)[1];
                                        }

                                        let stale = self.preview_cache.as_ref().is_none_or(|c| {
                                            c.layout != preview_layout
                                                || c.ccd != self.ccd_trial
                                                || c.profile != self.ccd_profile
                                        });
                                        if stale {
                                            let paths = if self.ccd_trial {
                                                self.ccd_profile.cut_paths_cached(
                                                    &self.layout,
                                                    a,
                                                    &mut self.outline_cache,
                                                )
                                            } else {
                                                preview_layout
                                                    .cut_paths_cached(a, &mut self.outline_cache)
                                            };
                                            self.preview_cache = Some(PreviewCache {
                                                layout: preview_layout,
                                                ccd: self.ccd_trial,
                                                profile: self.ccd_profile.clone(),
                                                paths,
                                            });
                                        }
                                        let origin = if self.preview_mark_origin {
                                            sheet::LOWER_LEFT_MARK
                                        } else {
                                            [0., 0.]
                                        };
                                        let dot = self.ccd_profile.marks(&self.layout)[0];
                                        match &self.preview_cache.as_ref().unwrap().paths {
                                            Ok(paths) => {
                                                for path in paths {
                                                    painter.add(egui::Shape::line(
                                                        path.iter()
                                                            .map(|p| {
                                                                let q = if self.ccd_trial {
                                                                    [dot[0] + p[0], dot[1] + p[1]]
                                                                } else {
                                                                    sheet::cutter_to_page_on(
                                                                        *p,
                                                                        origin,
                                                                        self.layout.page_height,
                                                                    )
                                                                };

                                                                pos(q[0], q[1])
                                                            })
                                                            .collect(),
                                                        egui::Stroke::new(
                                                            1.0_f32,
                                                            self.contour_color,
                                                        ),
                                                    ));
                                                }
                                            }

                                            Err(e) => {
                                                ui.label(
                                                    egui::RichText::new(e)
                                                        .color(egui::Color32::RED),
                                                );
                                            }
                                        }
                                    }
                                }
                            }

                            Err(e) => {
                                painter.text(
                                    area.center(),
                                    egui::Align2::CENTER_CENTER,
                                    e,
                                    egui::FontId::proportional(15.),
                                    egui::Color32::RED,
                                );
                            }
                        }

                        if self.ccd_trial && self.layout.has_visible_images() {
                            for p in self.ccd_profile.marks(&self.layout) {
                                painter.circle_filled(
                                    pos(p[0], p[1]),
                                    self.ccd_profile.diameter as f32 / 2. * scale,
                                    egui::Color32::BLACK,
                                );
                            }
                        } else if !self.ccd_trial {
                            for (x, y) in self.layout.manual_marks() {
                                painter.line_segment(
                                    [pos(x - 2., y), pos(x + 2., y)],
                                    egui::Stroke::new(1.0_f32, egui::Color32::BLACK),
                                );

                                painter.line_segment(
                                    [pos(x, y - 2.), pos(x, y + 2.)],
                                    egui::Stroke::new(1.0_f32, egui::Color32::BLACK),
                                );
                            }
                        }

                        let origin = if self.ccd_trial {
                            self.ccd_profile.origin(&self.layout)
                        } else if self.preview_mark_origin {
                            sheet::LOWER_LEFT_MARK
                        } else {
                            [0., 0.]
                        };

                        let q = sheet::cutter_to_page_on([0., 0.], origin, self.layout.page_height);

                        let origin_pos = pos(q[0], q[1]);

                        painter.circle_stroke(
                            origin_pos,
                            5.,
                            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(240, 145, 0)),
                        );

                        painter.text(
                            origin_pos + egui::vec2(9., -8.),
                            egui::Align2::LEFT_BOTTOM,
                            "Origin (0,0)",
                            egui::FontId::proportional(10.),
                            egui::Color32::from_rgb(170, 95, 0),
                        );
                        self.cut_shape_canvas(
                            &canvas_response,
                            &painter,
                            page,
                            view,
                            scale,
                            pan_gesture,
                        );
                        self.draw_selection(&painter, page, scale);
                        self.manual_cut_canvas(
                            &canvas_response,
                            &painter,
                            page,
                            view,
                            scale,
                            pan_gesture,
                        );
                        self.draw_ruler_guides(&painter, view, page, scale);
                        (page, scale)
                    });
                if self.rulers {
                    let (page, scale) = canvas.inner;
                    let area = viewport_rect;
                    let painter = ui.painter_at(area);
                    self.draw_fixed_rulers(&painter, area, page, scale);
                }
            });
        self.print_dialog(ctx);
    }
}

#[cfg(test)]
mod selection_tests {
    #[test]
    fn export_data_preserves_image_ccd_and_standalone_manual_formats() {
        let ctx = egui::Context::default();
        let mut app = super::A4Studio::default();
        app.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.layout.count = 1;
        app.retrace(&ctx);
        let art = app.art.as_ref().unwrap();
        assert_eq!(app.cut_file_data().unwrap(), app.layout.plt(art).unwrap());
        app.ccd_trial = true;
        assert_eq!(
            app.cut_file_data().unwrap(),
            app.ccd_profile.pair(&app.layout, art).unwrap().1
        );
        app.commit_manual_cut(vec![[20., 20.], [40., 20.], [40., 40.], [20., 20.]]);
        let ccd = app.cut_file_data().unwrap();
        assert_eq!(
            ccd,
            app.ccd_profile
                .pair(&app.layout, app.art.as_ref().unwrap())
                .unwrap()
                .1
        );
        app.art = None;
        assert_eq!(
            app.cut_file_data().unwrap(),
            app.layout.manual_design_plt().unwrap()
        );
        app.layout.manual_cut_paths.clear();
        assert!(app.cut_file_data().is_err());
    }

    #[test]
    fn selection_arrow_routes_images_and_manual_shapes_without_stealing_drags() {
        let ctx = egui::Context::default();
        let mut app = super::A4Studio::default();
        app.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.layout.count = 1;
        app.retrace(&ctx);
        app.activate_cut_picker();
        let page = egui::Rect::from_min_size(egui::pos2(20., 20.), egui::vec2(420., 594.));
        let pointer = egui::pos2(70., 70.);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.allocate_rect(page, egui::Sense::click_and_drag());
            });
        });
        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::PointerButton {
                    pos: pointer,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = ui.allocate_rect(page, egui::Sense::click_and_drag());
                assert!(!app.cut_picker_owns_gesture(ctx, page, 2.));
                app.canvas_edit(ui, &response, page, 2.);
            });
        });
        assert_eq!(app.selected, Some(0));
        app.drag = None;
        app.layout.manual_cut_paths = vec![vec![
            [20., 20.],
            [40., 20.],
            [40., 40.],
            [20., 40.],
            [20., 20.],
        ]];
        assert!(app.cut_picker_owns_gesture(&ctx, page, 2.));
        app.layout.manual_cut_paths[0]
            .iter_mut()
            .for_each(|p| p[0] += 80.);
        assert!(!app.cut_picker_owns_gesture(&ctx, page, 2.));
        app.cut_shape_drag = Some(super::CutShapeDrag {
            index: 0,
            start: [25., 25.],
            path: app.layout.manual_cut_paths[0].clone(),
            corner: None,
        });
        assert!(app.cut_picker_owns_gesture(&ctx, page, 2.));
    }

    #[test]
    fn guides_snap_edges_and_centers_without_breaking_axis_lock() {
        let mut app = super::A4Studio::default();
        app.layout.guides = [vec![45.], vec![56.]];
        let members = [(
            0,
            sheet::Placement {
                x: 25.,
                y: 35.,
                width: 10.,
                height: 20.,
            },
        )];
        assert_eq!(app.snap_move(&members, (9., 0.), 4., Some(0)), (10., 0.));
        assert_eq!(app.snap_move(&members, (0., 10.), 4., Some(1)), (0., 11.));
        assert_eq!(app.snap_move(&members, (14., 10.), 4., None), (15., 11.));
        assert_eq!(app.snap_move(&members, (7., 0.), 4., Some(0)), (7., 0.));
    }

    #[test]
    #[ignore = "manual UI performance measurement"]
    fn profile_ui_latency() {
        let ctx = egui::Context::default();
        let mut app = super::A4Studio::fresh_tab();
        app.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.retrace(&ctx);
        app.layout.count = 20;
        app.layout.custom = app
            .layout
            .items()
            .unwrap()
            .into_iter()
            .enumerate()
            .collect();
        app.layout.smoothing = 0.5;
        app.layout.contour_offset = 0.5;
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1360., 900.),
            )),
            ..Default::default()
        };
        for _ in 0..3 {
            let _ = ctx.run(input(), |ctx| app.ui(ctx));
        }
        for moving in [false, true] {
            let start = std::time::Instant::now();
            for _ in 0..40 {
                if moving {
                    app.layout.custom.get_mut(&0).unwrap().x += 0.001;
                }
                let output = ctx.run(input(), |ctx| {
                    app.tick_autosave(ctx);
                    app.ui(ctx);
                });
                let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
            }
            eprintln!(
                "UI_PROFILE moving={moving} mean_ms={:.3}",
                start.elapsed().as_secs_f64() * 1000. / 40.
            );
        }
    }

    #[test]
    fn workspace_new_preserves_dirty_document_and_close_can_cancel() {
        let ctx = egui::Context::default();
        let mut app = super::A4Studio::fresh_tab();
        app.layout.count += 1;
        app.request_action(super::DocumentAction::New, &ctx);
        assert!(app.is_dirty());
        assert!(app.pending_action.is_none());
        assert!(matches!(
            app.workspace_action.take(),
            Some(super::WorkspaceAction::New)
        ));
        app.request_tab_close(&ctx);
        assert!(!app.tab_closed);
        assert_eq!(
            app.pending_action,
            Some(super::DocumentAction::CloseProject)
        );
        app.finish_unsaved(super::UnsavedChoice::Cancel, &ctx);
        assert!(!app.tab_closed);
        assert!(app.is_dirty());
        assert!(app.close_canceled);
        app.request_tab_close(&ctx);
        app.finish_unsaved(super::UnsavedChoice::Discard, &ctx);
        assert!(app.tab_closed);
    }

    use super::*;
    fn resize_members() -> Vec<(usize, sheet::Placement)> {
        vec![
            (
                0,
                sheet::Placement {
                    x: 20.,
                    y: 30.,
                    width: 20.,
                    height: 30.,
                },
            ),
            (
                1,
                sheet::Placement {
                    x: 50.,
                    y: 40.,
                    width: 30.,
                    height: 20.,
                },
            ),
        ]
    }
    fn project_test_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mc630-{label}-{}-{}.mc630",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    fn wait_autosave(app: &mut A4Studio) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.autosave.busy() {
            app.autosave.poll();
            assert!(std::time::Instant::now() < deadline, "Auto Save timed out");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(app.autosave.problem.is_none(), "{:?}", app.autosave.problem);
    }
    #[test]
    fn explicit_launch_project_overrides_last_project_startup_option() {
        let ctx = egui::Context::default();
        let requested = project_test_path("double click requested");
        let last = project_test_path("last-opened");
        let mut source = A4Studio::default();
        source.layout.width = 31.;
        source.save_project_to(requested.clone()).unwrap();
        source.layout.width = 32.;
        source.save_project_to(last.clone()).unwrap();
        let mut app = A4Studio::default();
        app.reopen_last_project = true;
        app.recent_projects.remember(&last).unwrap();
        app.queue_startup_project(requested.clone());
        app.reopen_on_startup(&ctx);
        assert_eq!(app.project_path, Some(requested.clone()));
        assert_eq!(app.layout.width, 31.);
        assert!(!app.is_dirty());
        std::fs::remove_file(requested).unwrap();
        std::fs::remove_file(last).unwrap();
    }
    #[test]
    fn explicit_launch_waits_for_recovery_then_protects_restored_unsaved_work() {
        let ctx = egui::Context::default();
        let root = project_test_path("launch-recovery").with_extension("recovery");
        let mut old = A4Studio::default();
        old.layout.width = 31.;
        old.autosave.start(&root);
        old.tick_autosave(&ctx);
        wait_autosave(&mut old);
        drop(old);
        let requested = project_test_path("requested-after-recovery");
        let mut saved = A4Studio::default();
        saved.save_project_to(requested.clone()).unwrap();
        let mut app = A4Studio::default();
        app.autosave.start(&root);
        app.queue_startup_project(requested.clone());
        app.reopen_on_startup(&ctx);
        assert!(app.startup_pending);
        assert!(app.project_path.is_none());
        app.restore_recovery(&ctx).unwrap();
        app.reopen_on_startup(&ctx);
        assert_eq!(app.layout.width, 31.);
        assert_eq!(
            app.pending_action,
            Some(DocumentAction::OpenProject(requested.clone()))
        );
        app.finish_unsaved(UnsavedChoice::Cancel, &ctx);
        assert_eq!(app.layout.width, 31.);
        app.autosave.clear();
        drop(app);
        std::fs::remove_dir(root).unwrap();
        std::fs::remove_file(requested).unwrap();
    }
    #[test]
    fn startup_reopens_only_when_enabled_and_preserves_global_choice() {
        let ctx = egui::Context::default();
        let path = project_test_path("startup");
        let mut saved = A4Studio::default();
        saved.layout.width = 31.;
        saved.save_project_to(path.clone()).unwrap();
        let mut app = A4Studio::default();
        app.recent_projects.remember(&path).unwrap();
        app.reopen_on_startup(&ctx);
        assert!(app.project_path.is_none());
        app.startup_pending = true;
        app.reopen_last_project = true;
        assert!(!app.is_dirty());
        app.reopen_on_startup(&ctx);
        assert_eq!(
            std::fs::canonicalize(app.project_path.as_ref().unwrap()).unwrap(),
            std::fs::canonicalize(&path).unwrap()
        );
        assert_eq!(app.layout.width, 31.);
        assert!(app.reopen_last_project);
        assert!(!app.is_dirty());
        app.layout.width = 32.;
        app.reopen_on_startup(&ctx);
        assert_eq!(app.layout.width, 32.);
        let mut json = serde_json::to_value(app.preferences()).unwrap();
        json.as_object_mut().unwrap().remove("reopen_last_project");
        assert!(
            !serde_json::from_value::<Preferences>(json)
                .unwrap()
                .reopen_last_project
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn startup_recovery_has_priority_and_missing_project_keeps_blank_document() {
        let ctx = egui::Context::default();
        let root = project_test_path("startup-recovery").with_extension("recovery");
        let mut old = A4Studio::default();
        old.layout.width = 31.;
        old.autosave.start(&root);
        old.tick_autosave(&ctx);
        wait_autosave(&mut old);
        drop(old);
        let path = project_test_path("startup-existing");
        let mut saved = A4Studio::default();
        saved.save_project_to(path.clone()).unwrap();
        let mut app = A4Studio::default();
        app.reopen_last_project = true;
        app.recent_projects.remember(&path).unwrap();
        app.autosave.start(&root);
        app.reopen_on_startup(&ctx);
        assert!(app.project_path.is_none());
        assert_eq!(app.autosave.offered.len(), 1);
        app.restore_recovery(&ctx).unwrap();
        app.reopen_on_startup(&ctx);
        assert_eq!(app.layout.width, 31.);
        app.autosave.clear();
        drop(app);
        std::fs::remove_dir(root).unwrap();
        std::fs::remove_file(&path).unwrap();
        let mut missing = A4Studio::default();
        missing.reopen_last_project = true;
        missing.recent_projects.remember(&path).unwrap();
        missing.reopen_on_startup(&ctx);
        assert!(missing.project_path.is_none());
        assert!(!missing.is_dirty());
        assert!(missing.status.contains("could not be reopened"));
    }
    #[test]
    fn recent_projects_follow_successful_save_open_and_unsaved_guard() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::default();
        let first = project_test_path("recent-first");
        let second = project_test_path("recent-second");
        app.save_project_to(first.clone()).unwrap();
        app.save_project_to(second.clone()).unwrap();
        assert_eq!(app.recent_projects.entries.len(), 2);
        assert_eq!(
            std::fs::canonicalize(&app.recent_projects.entries[0]).unwrap(),
            std::fs::canonicalize(&second).unwrap()
        );
        app.load_project_from(first.clone(), &ctx).unwrap();
        assert_eq!(
            std::fs::canonicalize(&app.recent_projects.entries[0]).unwrap(),
            std::fs::canonicalize(&first).unwrap()
        );
        app.layout.width = 31.;
        app.request_action(DocumentAction::OpenProject(second.clone()), &ctx);
        assert_eq!(
            app.pending_action,
            Some(DocumentAction::OpenProject(second.clone()))
        );
        app.finish_unsaved(UnsavedChoice::Cancel, &ctx);
        assert_eq!(app.layout.width, 31.);
        assert_eq!(
            std::fs::canonicalize(&app.recent_projects.entries[0]).unwrap(),
            std::fs::canonicalize(&first).unwrap()
        );
        let missing = first.with_extension("missing");
        assert!(app.load_project_from(missing, &ctx).is_err());
        assert_eq!(
            std::fs::canonicalize(&app.recent_projects.entries[0]).unwrap(),
            std::fs::canonicalize(&first).unwrap()
        );
        app.recent_projects.clear().unwrap();
        assert!(first.is_file() && second.is_file());
        std::fs::remove_file(first).unwrap();
        std::fs::remove_file(second).unwrap();
    }
    #[test]
    fn autosave_settings_render_without_waiting_for_printer_discovery() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::default();
        app.show_settings = true;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1360., 900.),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            app.tick_autosave(ctx);
            app.ui(ctx);
            app.unsaved_dialog(ctx);
        });
        assert!(output.shapes.len() > 10);
        assert!(app.autosave_enabled);
        assert_eq!(app.autosave_seconds, 15);
    }
    #[test]
    fn crash_recovery_restores_embedded_image_manual_layout_settings_and_origin() {
        let ctx = egui::Context::default();
        let root = project_test_path("recover").with_extension("recovery");
        let mut app = A4Studio::default();
        app.autosave.start(&root);
        app.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.retrace(&ctx);
        let project_path = root.join("original.mc630");
        app.save_project_to(project_path.clone()).unwrap();
        let saved = std::fs::read(&project_path).unwrap();
        app.unit = Unit::Cm;
        app.ccd_trial = true;
        app.layout.smoothing = 0.3;
        app.layout.custom.insert(
            0,
            sheet::Placement {
                x: 17.,
                y: 16.,
                width: 32.,
                height: 33.,
            },
        );
        let original_layout = app.layout.clone();
        let original_source = app.source.clone();
        let output = app
            .ccd_profile
            .pair(&app.layout, app.art.as_ref().unwrap())
            .unwrap();
        app.tick_autosave(&ctx);
        wait_autosave(&mut app);
        assert!(app.is_dirty());
        assert!(app.autosave.store.as_ref().unwrap().path().is_file());
        assert_eq!(std::fs::read(&project_path).unwrap(), saved); // Never replace the user's project.
        #[cfg(windows)]
        {
            let mut other = crate::recovery::AutoSave::default();
            other.start(&root);
            assert!(other.offered.is_empty());
        }
        drop(app); // Abandoned session, as after a process crash.
        let mut restored = A4Studio::default();
        restored.autosave.start(&root);
        assert_eq!(restored.autosave.offered.len(), 1);
        restored.restore_recovery(&ctx).unwrap();
        assert!(restored.layout == original_layout);
        assert_eq!(restored.source, original_source);
        assert_eq!(restored.unit, Unit::Cm);
        assert_eq!(restored.project_path, Some(project_path.clone()));
        assert!(restored.is_dirty());
        assert_eq!(
            restored
                .ccd_profile
                .pair(&restored.layout, restored.art.as_ref().unwrap())
                .unwrap(),
            output
        );
        restored.save_project_to(project_path.clone()).unwrap();
        assert!(!restored.is_dirty());
        drop(restored);
        std::fs::remove_file(project_path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn intentional_dont_save_exit_removes_recovery() {
        let ctx = egui::Context::default();
        let root = project_test_path("discard-recovery").with_extension("recovery");
        let mut app = A4Studio::default();
        app.autosave.start(&root);
        app.layout.width = 31.;
        app.tick_autosave(&ctx);
        wait_autosave(&mut app);
        app.request_action(DocumentAction::Quit, &ctx);
        app.finish_unsaved(UnsavedChoice::Discard, &ctx);
        drop(app);
        let mut next = A4Studio::default();
        next.autosave.start(&root);
        assert!(next.autosave.offered.is_empty());
        drop(next);
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn recovery_preserves_overflow_drafts_failed_writes_and_legacy_defaults() {
        let mut app = A4Studio::default();
        app.layout.count = 999;
        let path = project_test_path("draft-recovery");
        let mut record = app.autosave_data().record();
        assert!(record.project.source().is_err());
        record.write(&path).unwrap();
        let read = crate::recovery::Record::read(&path).unwrap();
        assert_eq!(read.project.settings.layout.count, 999);
        let bytes = std::fs::read(&path).unwrap();
        record.project.settings.layout.page_width = -1.;
        assert!(record.write(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_file(path).unwrap();
        let mut prefs = serde_json::to_value(A4Studio::default().preferences()).unwrap();
        prefs.as_object_mut().unwrap().remove("autosave_enabled");
        prefs.as_object_mut().unwrap().remove("autosave_seconds");
        let prefs: Preferences = serde_json::from_value(prefs).unwrap();
        assert!(prefs.autosave_enabled);
        assert_eq!(prefs.autosave_seconds, 15);
    }
    #[test]
    fn unsaved_signature_tracks_edits_and_undo_but_not_navigation() {
        let mut app = A4Studio::default();
        assert!(!app.is_dirty());
        app.zoom = 3.;
        app.canvas_pan = egui::vec2(22., 15.);
        assert!(!app.is_dirty());
        app.select_target(0, false, false);
        app.nudge_selection(0.1, 0.);
        assert!(app.is_dirty());
        app.undo_object(&egui::Context::default());
        assert!(!app.is_dirty());
        app.unit = Unit::Cm;
        assert!(app.is_dirty());
        app.mark_saved();
        assert!(!app.is_dirty());
        app.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.refresh_source_digest();
        assert!(app.is_dirty());
    }
    #[test]
    fn unsaved_cancel_discard_and_save_continue_only_after_success() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::default();
        app.layout.width = 31.;
        app.request_action(DocumentAction::New, &ctx);
        assert_eq!(app.pending_action, Some(DocumentAction::New));
        assert!(!app.show_new);
        app.finish_unsaved(UnsavedChoice::Cancel, &ctx);
        assert!(!app.show_new);
        assert!(app.is_dirty());
        app.request_action(DocumentAction::New, &ctx);
        app.finish_unsaved(UnsavedChoice::Discard, &ctx);
        assert!(app.show_new);
        assert!(app.is_dirty());
        app.show_new = false;
        let path = project_test_path("unsaved-save");
        app.save_project_to(path.clone()).unwrap();
        app.layout.height = 32.;
        app.request_action(DocumentAction::New, &ctx);
        app.finish_unsaved(UnsavedChoice::Save, &ctx);
        assert!(app.show_new);
        assert!(!app.is_dirty());
        assert_eq!(
            crate::project::Project::read(&path)
                .unwrap()
                .settings
                .layout
                .height,
            32.
        );
        app.show_new = false;
        app.layout.height = 33.;
        app.project_path = Some(path.join("missing-parent.mc630"));
        app.request_action(DocumentAction::New, &ctx);
        app.finish_unsaved(UnsavedChoice::Save, &ctx);
        assert!(!app.show_new);
        assert!(app.is_dirty());
        assert_eq!(app.layout.height, 33.);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn close_request_is_vetoed_until_discard_or_successful_save() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::default();
        app.layout.width = 31.;
        let mut input = egui::RawInput::default();
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .events
            .push(egui::ViewportEvent::Close);
        let output = ctx.run(input, |ctx| app.intercept_close(ctx));
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::CancelClose))
        );
        assert_eq!(app.pending_action, Some(DocumentAction::Quit));
        app.finish_unsaved(UnsavedChoice::Cancel, &ctx);
        assert!(!app.close_approved);
        app.request_action(DocumentAction::Quit, &ctx);
        app.finish_unsaved(UnsavedChoice::Discard, &ctx);
        assert!(app.close_approved);
    }
    #[test]
    fn project_file_roundtrip_embeds_image_custom_layout_and_document_settings() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::default();
        app.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.name = "Original image.png".into();
        app.retrace(&ctx);
        app.unit = Unit::Cm;
        app.ccd_trial = true;
        app.layout.smoothing = 0.3;
        app.layout.contour_offset = 0.5;
        app.layout.custom.insert(
            0,
            sheet::Placement {
                x: 17.,
                y: 16.,
                width: 32.,
                height: 33.,
            },
        );
        app.zoom = 2.;
        app.canvas_pan = egui::vec2(12., -24.);
        app.nudge_fine = 0.254;
        app.ratio = 1.25;
        let original_source = app.source.clone();
        let original_layout = app.layout.clone();
        let before = app
            .ccd_profile
            .pair(&app.layout, app.art.as_ref().unwrap())
            .unwrap();
        let path = project_test_path("roundtrip");
        app.save_project_to(path.clone()).unwrap();
        // Save again to the same file verifies atomic replacement on Windows.
        app.unit = Unit::Inches;
        app.save_project_to(path.clone()).unwrap();
        let mut restored = A4Studio::default();
        restored.load_project_from(path.clone(), &ctx).unwrap();
        assert_eq!(restored.source, original_source);
        assert!(restored.layout == original_layout);
        assert_eq!(restored.name, "Original image.png");
        assert_eq!(restored.unit, Unit::Inches);
        assert_eq!(restored.zoom, 2.);
        assert_eq!(restored.canvas_pan, egui::vec2(12., -24.));
        assert_eq!(restored.ratio, 1.25);
        assert_eq!(restored.nudge_fine, 0.254);
        assert_eq!(
            restored
                .ccd_profile
                .pair(&restored.layout, restored.art.as_ref().unwrap())
                .unwrap(),
            before
        );
        assert_eq!(restored.project_path, Some(path.clone()));
        assert!(restored.undo.is_empty());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn failed_project_open_and_failed_save_preserve_current_data() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::default();
        let path = project_test_path("invalid");
        app.save_project_to(path.clone()).unwrap();
        let saved = std::fs::read(&path).unwrap();
        let mut invalid = app.project_snapshot();
        invalid.settings.layout.page_width = -1.;
        assert!(invalid.save(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        let mut json: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        json["version"] = 99.into();
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        app.layout.width = 31.;
        app.name = "Keep this document".into();
        assert!(app.load_project_from(path.clone(), &ctx).is_err());
        assert_eq!(app.layout.width, 31.);
        assert_eq!(app.name, "Keep this document");
        std::fs::write(&path, b"broken json").unwrap();
        assert!(app.load_project_from(path.clone(), &ctx).is_err());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn settings_roundtrip_and_unit_changes_preserve_physical_output() {
        let mut app = A4Studio::default();
        assert_eq!(app.unit, Unit::Inches);
        app.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.retrace(&egui::Context::default());
        let a = app.art.as_ref().unwrap();
        let print = app.layout.print_html(a).unwrap();
        let cut = app.layout.plt(a).unwrap();
        for unit in [Unit::Inches, Unit::Cm, Unit::Mm] {
            app.unit = unit;
            assert_eq!(
                app.layout.print_html(app.art.as_ref().unwrap()).unwrap(),
                print
            );
            assert_eq!(app.layout.plt(app.art.as_ref().unwrap()).unwrap(), cut);
        }
        app.nudge_fine = 0.254;
        app.nudge_coarse = 2.54;
        app.zoom = 2.;
        let json = serde_json::to_string(&app.preferences()).unwrap();
        let prefs: Preferences = serde_json::from_str(&json).unwrap();
        prefs.validate().unwrap();
        let mut next = A4Studio::default();
        next.apply_preferences(prefs);
        assert_eq!(next.unit, Unit::Mm);
        assert_eq!(next.nudge_fine, 0.254);
        assert!(next.layout == app.layout);
        assert_eq!(next.zoom, 2.);
        let mut invalid = next.preferences();
        invalid.nudge_fine = -1.;
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn arrow_nudge_moves_only_selected_preserves_group_and_undo() {
        let mut app = A4Studio::default();
        let original = app.layout.items().unwrap();
        app.select_target(0, false, false);
        app.select_target(1, true, false);
        app.nudge_selection(0.1, -1.);
        let moved = app.layout.items().unwrap();
        assert!((moved[0].x - original[0].x - 0.1).abs() < 1e-9);
        assert_eq!(moved[1].y, original[1].y - 1.);
        assert_eq!(&moved[2..], &original[2..]);
        assert_eq!(moved[1].x - moved[0].x, original[1].x - original[0].x);
        app.undo_object(&egui::Context::default());
        assert_eq!(app.layout.items().unwrap(), original);
        app.redo_object(&egui::Context::default());
        assert_eq!(app.layout.items().unwrap(), moved);
    }
    #[test]
    fn arrow_nudge_clamps_to_ccd_bounds_and_noop_does_not_add_undo() {
        let mut app = A4Studio::default();
        app.ccd_trial = true;
        app.select_target(0, false, false);
        app.select_target(1, true, false);
        let inset = app.edit_inset();
        app.nudge_selection(-1000., -1000.);
        let items = app.layout.items().unwrap();
        assert!((items[0].x - inset).abs() < 1e-9);
        assert!((items[0].y - inset).abs() < 1e-9);
        let history = app.undo.len();
        app.nudge_selection(-1., -1.);
        assert_eq!(app.undo.len(), history);
        app.clear_selection();
        app.nudge_selection(1., 1.);
        assert_eq!(app.undo.len(), history);
    }
    #[test]
    fn all_group_corners_scale_positions_sizes_and_keep_opposite_anchor() {
        let members = resize_members();
        let b = group_bounds(&members).unwrap();
        for corner in 0..4 {
            let left = corner == 0 || corner == 3;
            let top = corner == 0 || corner == 1;
            let delta = [if left { -12. } else { 12. }, if top { -6. } else { 6. }];
            let after = group_resize(&members, corner, delta, true, 0., [210., 297.]);
            let next = group_bounds(&after).unwrap();
            assert!((next.width / b.width - 1.2).abs() < 1e-9);
            let anchor = |p: sheet::Placement| {
                [
                    if left { p.x + p.width } else { p.x },
                    if top { p.y + p.height } else { p.y },
                ]
            };
            for (a, c) in anchor(b).iter().zip(anchor(next)) {
                assert!((a - c).abs() < 1e-9);
            }
            for ((_, old), (_, new)) in members.iter().zip(&after) {
                assert!((new.width / old.width - 1.2).abs() < 1e-9);
                assert!((new.height / old.height - 1.2).abs() < 1e-9);
                assert!((new.x - next.x - (old.x - b.x) * 1.2).abs() < 1e-9);
            }
        }
    }
    #[test]
    fn group_resize_respects_individual_minimum_page_and_unlocked_axes() {
        let members = resize_members();
        for corner in 0..4 {
            for delta in [[-10000., -10000.], [10000., 10000.]] {
                for lock in [true, false] {
                    let after = group_resize(&members, corner, delta, lock, 10., [210., 297.]);
                    for (_, p) in after {
                        assert!(p.width >= 1. - 1e-9 && p.height >= 1. - 1e-9);
                        assert!(p.x >= 10. - 1e-9 && p.y >= 10. - 1e-9);
                        assert!(p.x + p.width <= 200. + 1e-9 && p.y + p.height <= 287. + 1e-9);
                    }
                }
            }
        }
        let after = group_resize(&members, 2, [60., 0.], false, 0., [210., 297.]);
        assert_eq!(after[0].1.width, 40.);
        assert_eq!(after[0].1.height, 30.);
        assert!(group_resize(&[], 0, [0., 0.], true, 0., [210., 297.]).is_empty());
    }
    #[test]
    fn ctrl_click_toggles_and_shift_selects_from_fixed_anchor() {
        let mut app = A4Studio::default();
        app.select_target(2, false, false);
        app.select_target(5, true, false);
        assert_eq!(
            app.selection.iter().copied().collect::<Vec<_>>(),
            vec![2, 5]
        );
        assert_eq!(app.selected, None);
        app.select_target(5, true, false);
        assert_eq!(app.selected, Some(2));
        app.select_target(7, false, true);
        assert_eq!(
            app.selection.iter().copied().collect::<Vec<_>>(),
            vec![5, 6, 7]
        );
        app.select_target(3, false, true);
        assert_eq!(
            app.selection.iter().copied().collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        app.select_target(9, true, true);
        assert_eq!(
            app.selection.iter().copied().collect::<Vec<_>>(),
            vec![3, 4, 5, 6, 7, 8, 9]
        );
    }
    #[test]
    fn all_selection_clears_on_close_and_preserves_settings() {
        let mut app = A4Studio::default();
        app.retrace(&egui::Context::default());
        app.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.retrace(&egui::Context::default());
        app.layout.smoothing = 0.5;
        app.select_all();
        assert_eq!(app.selection.len(), 20);
        app.select_target(4, false, false);
        assert_eq!(app.selection.len(), 20); // press on selected item preserves group for dragging
        app.close_image();
        assert!(app.selection.is_empty());
        assert_eq!(app.selection_anchor, None);
        assert_eq!(app.layout.smoothing, 0.5);
    }
    #[test]
    fn group_move_clamps_as_one_and_undo_restores_entire_group() {
        let members = vec![
            (
                0,
                sheet::Placement {
                    x: 15.,
                    y: 15.,
                    width: 35.,
                    height: 35.,
                },
            ),
            (
                1,
                sheet::Placement {
                    x: 53.,
                    y: 15.,
                    width: 35.,
                    height: 35.,
                },
            ),
        ];
        let delta = group_move_delta(&members, 500., -500., 10., [210., 297.]);
        assert_eq!(delta, (112., -5.));
        let mut app = A4Studio::default();
        let before = app.layout.custom.clone();
        for (i, p) in &members {
            app.layout.custom.insert(
                *i,
                sheet::Placement {
                    x: p.x + delta.0,
                    y: p.y + delta.1,
                    ..*p
                },
            );
        }
        assert_eq!(app.layout.custom[&1].x - app.layout.custom[&0].x, 38.);
        let after = app.layout.custom.clone();
        app.record_object_edit(before.clone());
        assert_eq!(app.undo.len(), 1);
        app.undo_object(&egui::Context::default());
        assert_eq!(app.layout.custom, before);
        app.redo_object(&egui::Context::default());
        assert_eq!(app.layout.custom, after);
    }

    #[test]
    fn duplicate_repeat_and_delete_preserve_mixed_image_identity() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::default();
        let pixels = image::RgbaImage::from_fn(20, 10, |x, _| {
            image::Rgba([if x < 10 { 0 } else { 255 }, 0, 0, 255])
        });
        let mut png = std::io::Cursor::new(Vec::new());
        pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
        app.source = png.into_inner();
        app.layout.count = 2;
        app.layout.image_tiles = vec![[0, 0, 10, 10], [10, 0, 10, 10]];
        app.layout.custom = [20., 50.]
            .into_iter()
            .enumerate()
            .map(|(i, x)| {
                (
                    i,
                    sheet::Placement {
                        x,
                        y: 25.,
                        width: 15.,
                        height: 10.,
                    },
                )
            })
            .collect();
        app.retrace(&ctx);
        let original = app.layout.items().unwrap();
        let selected: Vec<_> = original.iter().copied().enumerate().collect();
        app.duplicate_images(&selected, (0., 20.), &ctx);
        app.repeat_duplicate(&ctx);
        assert_eq!(app.layout.count, 6);
        assert_eq!(&app.layout.items().unwrap()[..2], original.as_slice());
        assert_eq!(app.layout.custom[&5].y, 65.);
        assert_eq!(app.layout.image_tiles[5], [10, 0, 10, 10]);
        app.delete_selected_images(&ctx);
        assert_eq!(app.layout.count, 4);
        app.undo_object(&ctx);
        assert_eq!(app.layout.count, 6);
        app.select_all();
        app.delete_selected_images(&ctx);
        assert!(app.art.is_none());
        app.undo_object(&ctx);
        assert_eq!(app.layout.count, 6);
        assert_eq!(
            app.layout
                .page_cut_paths(app.art.as_ref().unwrap())
                .unwrap()
                .len(),
            6
        );
    }
}

include!("settings_ui.rs");

include!("project_ui.rs");

include!("document_lifecycle.rs");

include!("autosave_ui.rs");

impl A4Studio {
    pub fn apply_palette_state(&mut self, state: crate::palette_state::State) {
        self.section_palettes = state.sections;
        let legacy_section_open = self
            .section_palettes
            .iter()
            .any(|s| s.visible && s.geometry.is_some());
        for section in &mut self.section_palettes {
            section.geometry = None;
        }
        self.show_properties = state.visible || legacy_section_open;
    }
    pub fn palette_state(&self) -> crate::palette_state::State {
        crate::palette_state::State {
            geometry: self.palette_geometry,
            visible: self.show_properties,
            production: false,
            sections: self.section_palettes,
        }
    }
}

impl A4Studio {
    fn properties_sidebar(&mut self, ctx: &egui::Context) {
        if !self.show_properties {
            return;
        }
        egui::SidePanel::right("properties-sidebar")
            .default_width(330.)
            .width_range(300. ..=480.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Properties");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text("Hide sidebar").clicked() {
                            self.show_properties = false;
                        }
                    });
                });
                let active = self
                    .section_palettes
                    .iter()
                    .position(|section| section.visible)
                    .unwrap_or(0);
                ui.horizontal_wrapped(|ui| {
                    for (index, (icon, label)) in
                        [(9, "Artwork"), (1, "Size and Repeat"), (3, "Cut Outline")]
                            .into_iter()
                            .enumerate()
                    {
                        if Self::tool_icon(ui, icon, index == active, label) {
                            self.open_sidebar_section(index);
                        }
                    }
                });
                ui.separator();
                let properties_height = (ui.available_height() * 0.62).max(0.);
                egui::ScrollArea::vertical()
                    .id_salt(("properties-scroll", active))
                    .max_height(properties_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match self
                            .section_palettes
                            .iter()
                            .position(|section| section.visible)
                            .unwrap_or(0)
                        {
                            0 => self.artwork_dialog_content(ui, ctx),
                            1 => self.repeat_dialog_content(ui),
                            _ => self.outline_dialog_content(ui),
                        }
                    });
                ui.separator();
                self.layers_sidebar(ui);
            });
    }

    fn image_uv(&self, index: usize, art: &sheet::Artwork) -> egui::Rect {
        let Some(&[x, y, w, h]) = self.layout.image_tiles.get(index) else {
            return egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.));
        };
        let point = |x, y| {
            egui::pos2(
                x as f32 / art.pixels.width() as f32,
                y as f32 / art.pixels.height() as f32,
            )
        };
        egui::Rect::from_min_max(point(x, y), point(x + w, y + h))
    }

    fn layers_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Layers");
            if self.art.is_some() {
                ui.weak(format!("{}", self.layout.count));
            }
        });
        let (Some(art), Some(texture)) = (&self.art, &self.texture) else {
            ui.weak("Import an image to see it here.");
            return;
        };
        let Ok(items) = self.layout.items() else {
            ui.weak("Adjust the layout to show its images.");
            return;
        };
        let mut clicked = None;
        let mut visibility_clicked = None;
        egui::ScrollArea::vertical()
            .id_salt("image-layers")
            .auto_shrink([false, false])
            .show_rows(ui, 44., items.len(), |ui, rows| {
                for row in rows {
                    // The last image is drawn on top on the canvas.
                    let index = items.len() - 1 - row;
                    let item = items[index];
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 44.),
                        egui::Sense::hover(),
                    );
                    let eye = egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.left() + 32., rect.bottom()),
                    );
                    let row =
                        egui::Rect::from_min_max(egui::pos2(eye.right(), rect.top()), rect.max);
                    let response =
                        ui.interact(row, ui.id().with(("layer", index)), egui::Sense::click());
                    let hidden = self.layout.hidden_images.contains(&index);
                    let hint = if hidden { "Show image" } else { "Hide image" };
                    let eye_response = ui
                        .interact(
                            eye,
                            ui.id().with(("layer-eye", index)),
                            egui::Sense::click(),
                        )
                        .on_hover_text(hint);
                    eye_response.widget_info(|| {
                        egui::WidgetInfo::selected(egui::WidgetType::Button, true, !hidden, hint)
                    });
                    if eye_response.clicked() {
                        visibility_clicked = Some(index);
                    }
                    let selected = self.selection.contains(&index);
                    let visuals = ui.style().interact(&response);
                    let fill = if selected {
                        ui.visuals().selection.bg_fill
                    } else {
                        visuals.weak_bg_fill
                    };
                    let stroke = if selected {
                        ui.visuals().selection.stroke
                    } else {
                        visuals.bg_stroke
                    };
                    let painter = ui.painter();
                    painter.rect(
                        rect,
                        crate::ui_theme::BUTTON_RADIUS,
                        fill,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                    let center = eye.center();
                    let point = |x, y| center + egui::vec2(x, y);
                    let eye_ink = if hidden {
                        ui.visuals().weak_text_color()
                    } else {
                        ui.visuals().text_color()
                    };
                    let eye_stroke = egui::Stroke::new(1.2_f32, eye_ink);
                    painter.add(egui::Shape::closed_line(
                        vec![
                            point(-9., 0.),
                            point(-5., -4.),
                            point(0., -5.),
                            point(5., -4.),
                            point(9., 0.),
                            point(5., 4.),
                            point(0., 5.),
                            point(-5., 4.),
                        ],
                        eye_stroke,
                    ));
                    painter.circle_filled(center, 2.2, eye_ink);
                    if hidden {
                        painter.line_segment([point(-8., -7.), point(8., 7.)], eye_stroke);
                    }
                    let thumb = egui::Rect::from_min_size(
                        rect.min + egui::vec2(35., 4.),
                        egui::vec2(36., 36.),
                    );
                    painter.rect_filled(thumb, 3., egui::Color32::WHITE);
                    let scale = (36. / item.width.max(item.height)) as f32;
                    let image_rect = egui::Rect::from_center_size(
                        thumb.center(),
                        egui::vec2(item.width as f32, item.height as f32) * scale,
                    );
                    painter.image(
                        texture.id(),
                        image_rect,
                        self.image_uv(index, art),
                        egui::Color32::from_white_alpha(if hidden { 110 } else { 255 }),
                    );
                    painter.text(
                        rect.min + egui::vec2(79., 6.),
                        egui::Align2::LEFT_TOP,
                        format!("Image {}", index + 1),
                        egui::FontId::proportional(13.),
                        ui.visuals().text_color(),
                    );
                    painter.text(
                        rect.min + egui::vec2(79., 23.),
                        egui::Align2::LEFT_TOP,
                        self.unit.size(item.width, item.height),
                        egui::FontId::proportional(11.),
                        ui.visuals().weak_text_color(),
                    );
                    if response.clicked() {
                        clicked = Some(index);
                    }
                }
            });
        if let Some(index) = visibility_clicked {
            let before = self.layout.hidden_images.clone();
            if self.layout.hidden_images.remove(&index) {
                self.status = format!("Image {} shown.", index + 1);
            } else {
                self.layout.hidden_images.insert(index);
                self.status = format!("Image {} hidden from the canvas, print and cut.", index + 1);
            }
            self.record_layer_edit(ObjectEdit::Visibility(before));
        }
        if let Some(index) = clicked {
            self.cut_draft = None;
            self.pick_cut_shapes = false;
            self.selected_cut_shape = None;
            self.cut_shape_drag = None;
            let modifiers = ui.input(|input| input.modifiers);
            if !modifiers.command && !modifiers.shift {
                self.clear_selection();
            }
            self.select_target(index, modifiers.command, modifiers.shift);
        }
    }

    fn open_sidebar_section(&mut self, index: usize) {
        self.show_properties = true;
        for (i, section) in self.section_palettes.iter_mut().enumerate() {
            section.visible = i == index;
        }
    }

    fn print_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_print_dialog {
            return;
        }
        let mut close = false;
        let dialog = egui::Modal::new(egui::Id::new("print-dialog")).show(ctx, |ui| {
            ui.set_width(330.);
            ui.horizontal(|ui| {
                ui.heading("Print");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui
                        .small_button("×")
                        .on_hover_text("Close print dialog")
                        .clicked();
                });
            });
            ui.small(format!(
                "{} · {} · {}",
                self.layout.paper_label(),
                self.unit
                    .size(self.layout.page_width, self.layout.page_height),
                if self.ccd_trial {
                    "CCD marks"
                } else {
                    "Manual marks"
                },
            ));
            ui.separator();
            #[cfg(windows)]
            {
                let ccd = self.ccd_trial.then_some(&self.ccd_profile);
                self.print_panel
                    .ui(ui, self.art.as_ref(), &self.layout, ccd, &mut self.status);
            }
            #[cfg(not(windows))]
            ui.label("Direct printing is available on Windows.");
        });
        if close || dialog.should_close() {
            self.show_print_dialog = false;
        }
    }

    fn artwork_dialog_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(4.);

        if ui
            .add_sized(
                [ui.available_width(), 30.],
                egui::Button::new("Import image..."),
            )
            .clicked()
        {
            self.choose_image(ctx);
        }

        if ui
            .add_enabled(!self.source.is_empty(), egui::Button::new("Close image"))
            .clicked()
        {
            self.request_action(DocumentAction::CloseImage, ctx);
        }
        if self.art.is_none() {
            if ui.button("Load sample artwork").clicked() {
                self.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
                self.name = "Sample sticker".into();
                self.retrace(ctx);
                self.ratio = 1.;
                self.layout.height = self.layout.width;
            }
        }

        ui.add(egui::Label::new(egui::RichText::new(&self.name).small().weak()).truncate())
            .on_hover_text(&self.name);

        let previous = self.cut_shape;
        egui::ComboBox::from_id_salt("cut-shape")
            .selected_text(self.cut_shape.label())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for mode in [
                    sheet::CutShape::AlphaOutline,
                    sheet::CutShape::ImageFrame,
                    sheet::CutShape::PlainBackgroundObject,
                    sheet::CutShape::Ellipse,
                ] {
                    ui.selectable_value(&mut self.cut_shape, mode, mode.label());
                }
            });
        let mut changed = previous != self.cut_shape;
        if self.cut_shape == sheet::CutShape::PlainBackgroundObject {
            changed |= ui
                .add(
                    egui::Slider::new(&mut self.background_tolerance, 5..=100).text("BG tolerance"),
                )
                .changed();
            changed |= ui
                .checkbox(
                    &mut self.print_without_background,
                    "Print without background",
                )
                .changed();
        }
        if self.cut_shape != sheet::CutShape::ImageFrame {
            changed |= ui
                .checkbox(&mut self.remove_white, "Remove white background")
                .changed();
            if self.remove_white {
                changed |= ui
                    .add(egui::Slider::new(&mut self.threshold, 180..=254).text("Threshold"))
                    .changed();
            }
        }
        if changed && !self.source.is_empty() {
            self.retrace(ctx);
        }
    }
    fn repeat_dialog_content(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.);
        let selected = self.selected.and_then(|index| {
            let items = self.layout.items().ok()?;
            Some((index, *items.get(index)?))
        });
        ui.small(match selected {
            Some((index, _)) => format!("Image {} size and sheet spacing", index + 1),
            None => "Default size".into(),
        });
        if !self.layout.custom.is_empty()
            && self.layout.image_tiles.is_empty()
            && ui.button("Reflow copies").clicked()
        {
            self.reflow();
        }
        let mut size = selected.map(|(_, p)| p);
        let mut width_changed = false;
        let mut height_changed = false;
        egui::Grid::new("sheet-dimensions")
            .num_columns(2)
            .spacing([16., 8.])
            .show(ui, |ui| {
                ui.spacing_mut().interact_size.x = 94.;

                ui.label("Width");
                width_changed = ui
                    .add(
                        self.unit
                            .distance(
                                size.as_mut()
                                    .map_or(&mut self.layout.width, |p| &mut p.width),
                            )
                            .speed(0.1)
                            .range(1.0..=self.layout.page_width - 10.),
                    )
                    .changed();
                if width_changed && self.lock && selected.is_none() {
                    self.layout.height = self.layout.width / self.ratio;
                }
                ui.end_row();

                ui.label("Height");
                height_changed = ui
                    .add(
                        self.unit
                            .distance(
                                size.as_mut()
                                    .map_or(&mut self.layout.height, |p| &mut p.height),
                            )
                            .speed(0.1)
                            .range(1.0..=self.layout.page_height - 10.),
                    )
                    .changed();
                if height_changed && self.lock && selected.is_none() {
                    self.layout.width = self.layout.height * self.ratio;
                }
                ui.end_row();

                ui.label("Width / Height");
                if ui
                    .button(if self.lock { "Linked" } else { "Unlinked" })
                    .on_hover_text(if self.lock {
                        "Unlink width and height"
                    } else {
                        "Link width and height to keep the current proportions"
                    })
                    .clicked()
                {
                    self.lock = !self.lock;
                    if self.lock {
                        self.ratio = self.layout.width / self.layout.height;
                    }
                }
                ui.end_row();

                ui.label("Copies");
                ui.add_enabled(
                    self.layout.image_tiles.is_empty(),
                    egui::DragValue::new(&mut self.layout.count)
                        .update_while_editing(false)
                        .range(1..=1000),
                );
                ui.end_row();

                ui.label("Column spacing");
                ui.add(
                    self.unit
                        .distance(&mut self.layout.column_gap)
                        .speed(0.1)
                        .range(0.0..=100.),
                );
                ui.end_row();
                ui.label("Row spacing");
                ui.add(
                    self.unit
                        .distance(&mut self.layout.row_gap)
                        .speed(0.1)
                        .range(0.0..=100.),
                );
                ui.end_row();

                ui.label("Page margin");
                ui.add(
                    self.unit
                        .distance(&mut self.layout.margin)
                        .speed(0.1)
                        .range(10.0..=60.),
                );
                ui.end_row();
            });
        if let (Some((index, original)), Some(p)) = (selected, size) {
            if width_changed || height_changed {
                self.apply_selected_dimensions(index, original, p, width_changed, height_changed);
            }
        }
        ui.add_space(8.);

        if self.layout.image_tiles.is_empty()
            && let Ok(capacity) = self.cached_capacity()
        {
            if ui
                .add_enabled(
                    capacity > 0,
                    egui::Button::new(format!("Fill sheet ({capacity})")),
                )
                .clicked()
            {
                self.layout.count = capacity;
            }
        }
    }
    fn outline_dialog_content(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.);
        ui.strong("Manual cut tools");
        ui.horizontal(|ui| {
            if ui.button("Select shape").clicked() {
                self.activate_cut_picker();
            }
            if ui
                .add_enabled(
                    self.selected_cut_shape.is_some(),
                    egui::Button::new("Delete shape"),
                )
                .clicked()
            {
                self.remove_selected_cut_shape();
            }
        });
        let active = self.cut_draft.as_ref().map(|draft| draft.tool);
        ui.horizontal_wrapped(|ui| {
            for (tool, label) in [
                (Tool::Pen, "Pen"),
                (Tool::Ellipse, "Circle / Ellipse"),
                (Tool::Rectangle, "Rectangle"),
            ] {
                let button = egui::Button::new(label)
                    .selected(active == Some(tool))
                    .corner_radius(crate::ui_theme::BUTTON_RADIUS);
                if ui.add(button).clicked() {
                    self.activate_cut_tool(tool);
                }
            }
        });
        if let Some(draft) = &self.cut_draft {
            let tool = draft.tool;
            let count = draft.anchors.len();
            match tool {
                Tool::Pen => {
                    ui.small("Click for a corner; drag an anchor for a curve.");
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(count >= 2, egui::Button::new("Finish open"))
                            .clicked()
                        {
                            self.finish_manual_cut(false);
                        }
                        if ui
                            .add_enabled(count >= 3, egui::Button::new("Close path"))
                            .clicked()
                        {
                            self.finish_manual_cut(true);
                        }
                    });
                }
                Tool::Ellipse => {
                    ui.small("Drag to draw. Hold Shift for a circle.");
                }
                Tool::Rectangle => {
                    ui.horizontal(|ui| {
                        ui.label("Corner radius");
                        ui.add(
                            self.unit
                                .distance(&mut self.cut_corner_radius)
                                .speed(0.1)
                                .range(0. ..=100.),
                        );
                    });
                    ui.small("Drag to draw. Hold Shift for a square.");
                }
            }
            if ui.button("Cancel").clicked() {
                self.cut_draft = None;
            }
        } else if ui
            .add_enabled(
                !self.layout.manual_cut_paths.is_empty(),
                egui::Button::new("Remove last"),
            )
            .clicked()
        {
            self.record_manual_cut_edit();
            self.layout.manual_cut_paths.pop();
            self.layout.use_manual_cut_paths &= !self.layout.manual_cut_paths.is_empty();
        }
        if !self.layout.manual_cut_paths.is_empty() {
            let mut enabled = self.layout.use_manual_cut_paths;
            if ui
                .checkbox(
                    &mut enabled,
                    "Use manual lines instead of automatic outline",
                )
                .changed()
            {
                self.record_manual_cut_edit();
                self.layout.use_manual_cut_paths = enabled;
            }
        }
        ui.separator();
        ui.label("Outline smoothing");
        ui.checkbox(&mut self.layout.preserve_corners, "Preserve sharp corners");
        ui.add(
            self.unit
                .slider(&mut self.layout.smoothing, 0.0..=4.0)
                .fixed_decimals(2),
        );
        ui.horizontal_wrapped(|ui| {
            for (label, value) in [("Off", 0.), ("Light", 0.2), ("Medium", 0.5), ("Strong", 4.)] {
                if ui
                    .add_sized(
                        [60., 26.],
                        egui::Button::new(label)
                            .selected((self.layout.smoothing - value).abs() < 1e-6)
                            .corner_radius(crate::ui_theme::BUTTON_RADIUS),
                    )
                    .clicked()
                {
                    self.layout.smoothing = value;
                }
            }
        });
        ui.add_space(8.);

        ui.checkbox(&mut self.layout.cut_inner_holes, "Cut inner holes");
        ui.add_space(8.);
        ui.horizontal(|ui| {
            ui.label("Outline distance");
            ui.add(
                self.unit
                    .distance(&mut self.layout.contour_offset)
                    .speed(0.1)
                    .range(-5.0..=5.0)
                    .fixed_decimals(1),
            );
        });
        ui.horizontal_wrapped(|ui| {
            for (label, value) in [("Inside", -0.5), ("Exact", 0.), ("Outside", 1.)] {
                if ui
                    .add_sized(
                        [72., 26.],
                        egui::Button::new(label)
                            .selected((self.layout.contour_offset - value).abs() < 1e-6)
                            .corner_radius(crate::ui_theme::BUTTON_RADIUS),
                    )
                    .clicked()
                {
                    self.layout.contour_offset = value;
                }
            }
        });
        ui.add_space(12.);
        ui.separator();
        ui.add_space(8.);
        ui.strong("Registration");
        ui.add_space(8.);

        ui.checkbox(&mut self.ccd_trial, "CCD camera · 4 dots");

        if self.ccd_trial {
            egui::CollapsingHeader::new("Dot settings").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Diameter");
                    ui.add(
                        self.unit
                            .distance(&mut self.ccd_profile.diameter)
                            .speed(0.1)
                            .range(1.0..=10.),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Center gap");
                    ui.add(
                        self.unit
                            .distance(&mut self.ccd_profile.inset)
                            .speed(0.1)
                            .range(5.0..=30.),
                    );
                });
            });
        } else {
            egui::CollapsingHeader::new("Manual alignment").show(ui, |ui| {
                ui.checkbox(&mut self.preview_mark_origin, "Origin at lower-left cross");

                ui.horizontal(|ui| {
                    ui.label("X offset");
                    ui.add(
                        self.unit
                            .distance(&mut self.layout.offset_x)
                            .speed(0.1)
                            .range(-self.layout.page_width..=self.layout.page_width),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Y offset");
                    ui.add(
                        self.unit
                            .distance(&mut self.layout.offset_y)
                            .speed(0.1)
                            .range(-self.layout.page_height..=self.layout.page_height),
                    );
                });

                ui.checkbox(&mut self.layout.flip_x, "Reverse X");
                ui.checkbox(&mut self.layout.flip_y, "Reverse Y");
            });
        }

        egui::CollapsingHeader::new("Calibration").show(ui, |ui| {
            ui.add_enabled_ui(!self.ccd_trial, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Units / mm");
                    ui.add(
                        egui::DragValue::new(&mut self.layout.units)
                            .speed(0.1)
                            .range(1.0..=1000.),
                    );
                });
            });
        });
    }
}
