use crate::ui_theme::{Icon, icon_button, icon_toggle};
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
                    ..*p
                },
            )
        })
        .collect()
}
fn next_group_id(groups: &std::collections::BTreeMap<usize, usize>) -> usize {
    let used: std::collections::BTreeSet<_> = groups.values().copied().collect();
    (0..=groups.len()).find(|id| !used.contains(id)).unwrap()
}

fn remove_singleton_groups(groups: &mut std::collections::BTreeMap<usize, usize>) {
    let mut counts = std::collections::BTreeMap::new();
    for &id in groups.values() {
        *counts.entry(id).or_insert(0) += 1;
    }
    groups.retain(|_, id| counts[id] >= 2);
}

pub(crate) fn paint_oriented_image(
    painter: &egui::Painter,
    texture: egui::TextureId,
    rect: egui::Rect,
    uv: egui::Rect,
    orientation: sheet::Orientation,
    color: egui::Color32,
) {
    if orientation == sheet::Orientation::default() {
        painter.image(texture, rect, uv, color);
        return;
    }
    let mut mesh = egui::Mesh::with_texture(texture);
    let source_corners = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
    for (point, [x, y]) in source_corners.into_iter().zip(orientation.unit_corners()) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: rect.min + egui::vec2(x as f32 * rect.width(), y as f32 * rect.height()),
            uv: uv.min + egui::vec2(point[0] as f32 * uv.width(), point[1] as f32 * uv.height()),
            color,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
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
    Nudge(std::collections::BTreeMap<usize,sheet::Placement>,Vec<sheet::DesignShape>,Vec<Vec<mc630_plt_studio::Point>>),
    Shaping(Vec<sheet::DesignShape>,Vec<Vec<mc630_plt_studio::Point>>),
    Designs(Vec<sheet::DesignShape>),
    PageGeometry(Box<Layout>),
    Positions(std::collections::BTreeMap<usize, sheet::Placement>),
    Layers(Box<Layout>),
    LastImage(Box<Layout>, Vec<u8>, String),
    Visibility(std::collections::BTreeSet<usize>),
    Groups(std::collections::BTreeMap<usize, usize>),
    ManualCuts(Vec<Vec<mc630_plt_studio::Point>>, bool, std::collections::BTreeMap<usize, Vec<Vec<mc630_plt_studio::Point>>>),
}
struct GuideDrag {
    axis: usize,
    index: Option<usize>,
    value: f64,
    grab_offset: f64,
}
struct ShapeGroupDrag {
    designs:Vec<sheet::DesignShape>,
    cuts:Vec<Vec<mc630_plt_studio::Point>>,
    targets:Vec<crate::shape_ops::Target>,
    lengths:Vec<usize>,
    points:Vec<mc630_plt_studio::Point>,
    start:egui::Pos2,
    handle:Option<usize>,
    toggle_click:Option<crate::shape_ops::Target>,
}
struct CutShapeDrag {
    before: Vec<Vec<mc630_plt_studio::Point>>,
    duplicate: bool,
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

struct WebImageImport {
    back: bool,
    url: String,
    error: Option<String>,
    pending: Option<std::sync::mpsc::Receiver<Result<crate::image_import::ImportedImage, String>>>,
}

pub struct A4Studio {
    other_pages: Vec<crate::project::Project>,
    page_signatures: Vec<u64>,
    page_histories: Vec<PageHistory>,
    active_page: usize,
    pending_page: Option<usize>,
    delete_page_open: bool,
    delete_page_after_switch: bool,
    web_import: Option<WebImageImport>,
    vector_trace:Option<crate::vector_trace_ui::Dialog>,
    io_job: Option<crate::project_io::Job>,
    io_after_save: Option<DocumentAction>,
    io_workspace_result: Option<Result<(),String>>,
    io_workspace_waiting: bool,
    io_close_requested: bool,
    back: crate::duplex::BackEditor,
    back_paths_cache: Option<(Layout, u64, std::sync::Arc<Vec<Vec<[f64;2]>>>)>,
    show_back: bool,
    print_back: bool,
    print_range_mode:u8,
    print_range:String,
    pub workspace_enabled: bool,
    pub workspace_action: Option<WorkspaceAction>,
    pub tab_closed: bool,
    pub close_canceled: bool,
    reopen_last_project: bool,
    startup_pending: bool,
    startup_project: Option<PathBuf>,
    recent_gallery: crate::recent_gallery::Gallery,
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
    show_navigator: bool,
    navigator_view: Option<(egui::Rect, egui::Rect)>,
    palette_geometry: Option<crate::palette_state::Geometry>,
    section_palettes: [crate::palette_state::Section; 4],
    show_margin_guides: bool,
    nudge_fine: f64,
    nudge_coarse: f64,
    preview_cache: Option<PreviewCache>,
    outline_cache: sheet::OutlineCache,
    capacity_cache: Option<(Layout, Result<usize, String>)>,
    art: Option<std::sync::Arc<Artwork>>,

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
    show_manual_marks: bool,
    overflow_warning_seen: bool,
    overflow_warning_open: bool,
    crop_edit: Option<(usize, egui::Rect)>,
    cut_draft: Option<Draft>,
    cut_corner_radius: f64,
    drawing_mode: usize,
    design_fill: egui::Color32,
    design_fill_enabled: bool,
    design_stroke: egui::Color32,
    design_stroke_width: f64,
    selected_design: Option<usize>,
    shape_selection: Vec<crate::shape_ops::Target>,
    shape_group_drag:Option<ShapeGroupDrag>,
    shape_angle:f64,
    design_drag: Option<(usize,Vec<sheet::DesignShape>,egui::Pos2,Option<usize>,bool)>,
    design_preview: crate::design_preview::Preview,
    svg_previews: std::collections::BTreeMap<usize,crate::design_preview::Preview>,
    pick_cut_shapes: bool,
    selected_cut_shape: Option<usize>,
    cut_shape_drag: Option<CutShapeDrag>,
    show_print_dialog: bool,
    zoom: f32,
    canvas_pan: egui::Vec2,
    camera: crate::canvas_camera::CanvasCamera,
    rulers: bool,
    guide_drag: Option<GuideDrag>,
    selected_guide: Option<(usize, usize)>,
    snap_guides: bool,
    grid: bool,
    show_guide: bool,
    show_new: bool,
    page_size_editor: Option<PageSizeEditor>,
    new_preset: usize,
    new_width: f64,
    new_height: f64,
    new_margins: [f64; 4],
    new_margins_linked: bool,
    margins_linked: bool,
    new_landscape: bool,
    contour_color: egui::Color32,
    selected: Option<usize>,
    selection: std::collections::BTreeSet<usize>,
    marquee: Option<(egui::Pos2, std::collections::BTreeSet<usize>,Vec<crate::shape_ops::Target>)>,
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

struct PageSizeEditor {
    width: f64,
    height: f64,
    unit: Unit,
    error: Option<String>,
}

impl Default for A4Studio {
    fn default() -> Self {
        #[allow(unused_mut)]
        let mut app=Self{other_pages:Vec::new(),page_signatures:Vec::new(),page_histories:Vec::new(),active_page:0,pending_page:None,delete_page_open:false,delete_page_after_switch:false,vector_trace:None,web_import:None,io_job:None,io_after_save:None,io_workspace_result:None,io_workspace_waiting:false,io_close_requested:false,back_paths_cache:None,back:Default::default(),show_back:false,print_back:false,print_range_mode:0,print_range:String::new(),workspace_enabled:false,workspace_action:None,tab_closed:false,close_canceled:false,reopen_last_project:false,startup_pending:true,startup_project:None,recent_projects:Default::default(),recent_gallery:Default::default(),autosave:Default::default(),autosave_enabled:true,autosave_seconds:15,saved_signature:0,source_digest:0,pending_action:None,close_approved:false,unit:Unit::Inches,show_settings:false,show_properties:false,show_navigator:true,navigator_view:None,palette_geometry:None,section_palettes:Default::default(),show_margin_guides:true,nudge_fine:0.1,nudge_coarse:1.,preview_cache:None,outline_cache:Default::default(),capacity_cache:None,art:None,texture:None,source:vec![],name:"No image selected".into(),layout:Layout{offset_x:-7.,offset_y:-7.,..Layout::default()},remove_white:false,cut_shape:sheet::CutShape::AlphaOutline,background_tolerance:20,print_without_background:true,threshold:245,status:"Choose a transparent PNG. Repeat it on A4 and export matching PRINT.html + CUT.plt.".into(),show_cut:true,show_manual_marks:false,overflow_warning_seen:false,overflow_warning_open:false,crop_edit:None,cut_draft:None,cut_corner_radius:0.,drawing_mode:0,design_fill:egui::Color32::from_rgb(0,103,192),design_fill_enabled:true,design_stroke:egui::Color32::BLACK,design_stroke_width:0.3,selected_design:None,shape_selection:vec![],shape_group_drag:None,shape_angle:0.,design_drag:None,design_preview:Default::default(),svg_previews:Default::default(),pick_cut_shapes:false,selected_cut_shape:None,cut_shape_drag:None,show_print_dialog:false,zoom:1.,canvas_pan:egui::Vec2::ZERO,camera:Default::default(),rulers:true,guide_drag:None,selected_guide:None,snap_guides:true,grid:false,show_guide:false,show_new:false,page_size_editor:None,new_preset:0,new_width:210.,new_height:297.,new_margins:[15.;4],new_margins_linked:true,margins_linked:false,new_landscape:false,contour_color:egui::Color32::RED,selected:None,selection:Default::default(),marquee:None,selection_anchor:None,drag:None,duplicate_step:None,undo:vec![],redo:vec![],ratio:1.,lock:true,last_print:None,project_path:None,preview_mark_origin:true,ccd_trial:false,ccd_profile:sheet::CcdTrial::default(),#[cfg(windows)] print_panel:crate::native_print::PrintPanel::default()};
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
        if delta == (0., 0.) || !self.snap_guides {
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
            let lines = self.layout.margin_lines()[i];
            for (_, item) in members {
                let (start, end) = if i == 0 {
                    (item.x, item.x + item.width)
                } else {
                    (item.y, item.y + item.height)
                };
                for (edge, margin) in [start, end].into_iter().zip(lines) {
                    let shift = margin - (edge + *offset);
                    if shift.abs() <= threshold && best.is_none_or(|value| shift.abs() < value.abs()) {
                        best = Some(shift);
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
            if pointer.is_some_and(|p| view.contains(p) && page.contains(p))
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
            } else if pointer.is_some() && let Some(index) = index {
                if index < guides.len() {
                    guides.remove(index);
                    self.selected_guide = None;
                    self.status = "Guide deleted outside the page.".into();
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
        self.back_paths_cache=None;
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

                self.art = Some(a.into());
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
        self.layout.hidden_images.clear();self.layout.image_opacity.clear();
        self.layout.image_groups.clear();
        self.layout.image_cut_paths.clear();self.layout.svg_sources.clear();self.svg_previews.clear();
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
        match crate::image_import::read_file(&path) {
            Ok(image) => {
                self.install_single_image(image.bytes, image.name, ctx);
                if self.art.is_some() && let Some(svg)=image.svg {self.layout.svg_sources.insert(0,svg);}
            },
            Err(error) => self.status = error.to_string(),
        }
    }

    fn install_single_image(&mut self, bytes: Vec<u8>, name: String, ctx: &egui::Context) {
        if bytes.len() > 30_000_000 {
            self.status = "Image too large (30 MB maximum).".into();
            return;
        }
        if let Err(error) = sheet::decode_image(&bytes) {
            self.status = format!("Cannot open image: {error}");
            return;
        }
        self.close_image();
        self.layout.use_manual_cut_paths = true;
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
        if self.art.is_some() { self.layout.items()?; }
        match &self.art {
            None if self.ccd_trial && !self.layout.design_shapes.is_empty()=>{
                let (layout,art)=self.layout.design_only_artwork()?;
                self.ccd_profile.pair(&layout,&art).map(|(_,plt)|plt)
            }
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
        let standalone=if self.art.is_none() && self.layout.has_print_designs() {self.layout.design_only_artwork().ok()} else {None};
        let (layout,a)=if let Some(a)=&self.art {(&self.layout,a.as_ref())} else if let Some((layout,art))=&standalone {(layout,art)} else {
            self.export_cut_file(frame);return;
        };

        let output = if self.ccd_trial {
            self.ccd_profile.pair(layout, a)
        } else {
            (|| Ok::<_, String>((layout.print_html(a)?, layout.plt(a)?)))()
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
                                "A4 portrait 210 x 297 mm\nCopies: {}\nImage size: {} x {} mm\nMargins (left, right, top, bottom): {:?} mm, column gap: {} mm, row gap: {} mm\nUnits/mm: {}\nCut offsets: {}, {} mm\nFlip X: {}, Flip Y: {}\n\nPRINT.html: Open and print at 100% / Actual size, no margins, no headers/footers. Do not use Fit to page.\nCUT.plt: Copy to USB stick. Coordinates use the A4 lower-left corner as origin before flips/offsets. Manually align sheet and blade origin; use a pen test first.\nThe four crosses are visual alignment references, NOT a verified MC630 automatic registration format. Scale/rotation/skew compensation is not automatic. Re-export both files after changing layout.\nTrace: outer alpha silhouettes, alpha >=128; white removal: {}, white threshold: {}. Interior holes follow the Cut inner holes setting below. No image bleed is generated. See Cut border distance below.\n",
                                self.layout.count,
                                self.layout.width,
                                self.layout.height,
                                self.layout.margins(),
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
        if self.layout.use_manual_cut_paths { self.layout.has_manual_cuts() }
        else { self.art.is_some() || self.layout.has_manual_cuts() }
    }

    fn open_page_size(&mut self) {
        self.page_size_editor = Some(PageSizeEditor {
            width: self.layout.page_width,
            height: self.layout.page_height,
            unit: self.unit,
            error: None,
        });
    }

    fn apply_page_size(&mut self, width: f64, height: f64) -> Result<(), String> {
        if !(50. ..=630.).contains(&width) || !(50. ..=2000.).contains(&height) {
            return Err("Width: 50–630 mm. Height: 50–2000 mm.".into());
        }
        if self.layout.page_width == width && self.layout.page_height == height {
            return Ok(());
        }
        let mut next = self.layout.clone();
        next.page_width = width;
        next.page_height = height;
        if next.page_margins.is_some() {
            next.capacity()?;
        }
        if self.art.is_some() {
            // Page width must not reflow existing implicit grid copies.
            if let Ok(items) = self.layout.editor_items() {
                next.custom = items.into_iter().enumerate().collect();
            }
            next.items()
                .map_err(|e| format!("Images do not fit this page. {e}"))?;
        }
        self.back.data.validate([width,height])?;
        next.validate_manual_cut_paths()?;
        for (axis, guides) in next.guides.iter().enumerate() {
            if guides
                .iter()
                .any(|value| !(0. ..=[width, height][axis]).contains(value))
            {
                return Err("A guide is outside the new page. Move or delete it first.".into());
            }
        }
        if self.ccd_trial
            && let Some(art) = &self.art
        {
            self.ccd_profile.pair(&next, art)?;
        }
        let previous = std::mem::replace(&mut self.layout, next);
        self.record_layer_edit(ObjectEdit::PageGeometry(Box::new(previous)));
        self.preview_cache = None;
        self.capacity_cache = None;
        self.zoom = 1.;
        self.canvas_pan = egui::Vec2::ZERO;
        self.camera = Default::default();
        self.status = format!("Page size: {}", self.unit.size(width, height));
        Ok(())
    }

    fn page_size_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.page_size_editor.take() else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        let modal = crate::ui_theme::dialog_modal(ctx, "page-size-dialog").show(ctx, |ui| {
            ui.set_width(320.);
            cancel |= crate::ui_theme::dialog_header(ui,"Page size",true);
            let preset=sheet::PAPER_PRESETS.iter().find(|(_,w,h)|
                ((draft.width-*w).abs()<0.01 && (draft.height-*h).abs()<0.01)
                || ((draft.width-*h).abs()<0.01 && (draft.height-*w).abs()<0.01))
                .map_or("Custom size",|(name,_,_)|*name);
            egui::Grid::new("manual-page-size")
                .num_columns(3)
                .min_col_width(0.)
                .spacing([12.,8.])
                .show(ui,|ui| {
                    ui.allocate_ui_with_layout(egui::vec2(62.,28.),egui::Layout::left_to_right(egui::Align::Center),|ui| {ui.label("Paper");});
                    egui::ComboBox::from_id_salt("page-size-preset")
                        .selected_text(preset).width(184.)
                        .show_ui(ui,|ui| {
                            for &(name,width,height) in sheet::PAPER_PRESETS {
                                if ui.selectable_label(preset==name,format!("{name}   {}",draft.unit.size(width,height))).clicked() {
                                    draft.width=width;draft.height=height;draft.error=None;
                                }
                            }
                        });
                    ui.label("");ui.end_row();
                    ui.label("Units");
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x=4.;
                        for (label,unit) in [("mm",Unit::Mm),("cm",Unit::Cm),("in",Unit::Inches)] {
                            ui.selectable_value(&mut draft.unit,unit,label);
                        }
                    });
                    ui.label("");ui.end_row();
                    ui.label("Width");
                    if ui.add_sized([184.,28.],draft.unit.distance(&mut draft.width).range(50. ..=630.).speed(1.)).changed() {draft.error=None;}
                    if icon_button(ui,Icon::Swap,"Swap width and height") {
                        std::mem::swap(&mut draft.width,&mut draft.height);draft.error=None;
                    }
                    ui.end_row();
                    ui.label("Height");
                    if ui.add_sized([184.,28.],draft.unit.distance(&mut draft.height).range(50. ..=2000.).speed(1.)).changed() {draft.error=None;}
                    ui.label("");ui.end_row();
                });
            if let Some(error) = &draft.error {
                ui.add_space(8.);
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.add_space(6.);
            ui.separator();
            crate::ui_theme::dialog_actions(ui,|ui| {
                apply = crate::ui_theme::primary_button(ui,"Apply").clicked();
                cancel |= ui.add_sized([80.,28.],egui::Button::new("Cancel")).clicked();
            });
        });
        if apply {
            if let Err(error) = self.apply_page_size(draft.width, draft.height) {
                draft.error = Some(error);
                self.page_size_editor = Some(draft);
            }
        } else if !cancel && !modal.should_close() {
            self.page_size_editor = Some(draft);
        }
    }

    pub fn new_document_dialog_open(&self) -> bool { self.show_new }
    pub fn new_document_dialog(&mut self, ctx: &egui::Context) -> bool {
        if !self.show_new {
            return false;
        }
        let mut open = true;
        let mut create = false;
        egui::Window::new("New document")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(380.)
            .default_pos(ctx.screen_rect().center() - egui::vec2(205., 215.))
            .show(ctx, |ui| {
                let previous = self.new_preset;
                egui::Grid::new("new-document-form")
                    .num_columns(2)
                    .min_col_width(105.)
                    .spacing([18., 8.])
                    .show(ui, |ui| {
                        ui.label("Paper");
                        egui::ComboBox::from_id_salt("new-paper-preset")
                            .selected_text(sheet::PAPER_PRESETS.get(self.new_preset)
                                .map(|preset| preset.0).unwrap_or("Custom"))
                            .width(195.)
                            .show_ui(ui, |ui| {
                                for (i, (name, w, h)) in sheet::PAPER_PRESETS.iter().enumerate() {
                                    ui.selectable_value(&mut self.new_preset, i,
                                        format!("{name}   {}", self.unit.size(*w, *h)));
                                }
                                ui.selectable_value(&mut self.new_preset, sheet::PAPER_PRESETS.len(), "Custom size");
                            });
                        ui.end_row();
                        if previous != self.new_preset
                            && let Some(&(_, w, h)) = sheet::PAPER_PRESETS.get(self.new_preset)
                        {
                            (self.new_width, self.new_height) = if self.new_landscape { (h, w) } else { (w, h) };
                        }

                        ui.label("Orientation");
                        let orientation = self.new_landscape;
                        ui.horizontal(|ui| {
                            if icon_toggle(ui, Icon::Portrait, !self.new_landscape, "Portrait") {
                                self.new_landscape = false;
                            }
                            if icon_toggle(ui, Icon::Landscape, self.new_landscape, "Landscape") {
                                self.new_landscape = true;
                            }
                        });
                        ui.end_row();
                        if orientation != self.new_landscape {
                            std::mem::swap(&mut self.new_width, &mut self.new_height);
                        }

                        let mut size_changed = false;
                        for (label, value) in [("Page width", &mut self.new_width), ("Page height", &mut self.new_height)] {
                            ui.label(label);
                            size_changed |= Self::document_stepper(ui, self.unit, value, 50.);
                            ui.end_row();
                        }
                        if size_changed {
                            self.new_preset = sheet::PAPER_PRESETS.len();
                            self.new_landscape = self.new_width > self.new_height;
                        }
                    });

                ui.separator();
                ui.strong("Page margins");
                Self::margin_fields(ui, self.unit, &mut self.new_margins, &mut self.new_margins_linked);

                let margins_fit = Layout::margins_fit(self.new_margins, self.new_width, self.new_height);
                let valid_size = (50. ..=630.).contains(&self.new_width)
                    && (50. ..=2000.).contains(&self.new_height);
                if !valid_size {
                    ui.colored_label(egui::Color32::from_rgb(150, 60, 20),
                        "Width must be 50–630 mm and height 50–2000 mm.");
                } else if !margins_fit {
                    ui.colored_label(egui::Color32::from_rgb(150, 60, 20),
                        "Margins must leave at least 1 mm of usable width and height.");
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        create = ui.add_enabled(valid_size && margins_fit,
                            egui::Button::new(egui::RichText::new("Create document").color(egui::Color32::WHITE))
                                .fill(crate::ui_theme::ACCENT)
                                .min_size(egui::vec2(132., 30.)))
                            .clicked();
                        if ui.button("Cancel").clicked() {
                            self.show_new = false;
                        }
                    });
                });
            });
        if !open {
            self.show_new = false;
        }
        if create {
            self.other_pages.clear();self.page_signatures.clear();self.page_histories.clear();self.active_page=0;
            self.svg_previews.clear();
            self.back = Default::default();
            self.show_back = false;
            self.print_back = false;
            self.layout = Layout {
                page_width: self.new_width,
                page_height: self.new_height,
                page_margins: Some(self.new_margins),
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
            let [left, right, top, bottom] = self.layout.margins();
            let size = (self.new_width - left - right)
                .min(self.new_height - top - bottom)
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
            self.camera = Default::default();
            self.show_new = false;
            self.status = format!(
                "New document: {} / {:.1} x {:.1} mm. Import an image to begin.",
                self.layout.paper_label(),
                self.new_width,
                self.new_height
            );
            self.mark_saved();
        }
        create
    }
    fn clear_selection(&mut self) {
        self.marquee = None;
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
    fn movable_image(&self, index: usize) -> bool {
        !self.layout.hidden_images.contains(&index) || self.layout.image_groups.contains_key(&index)
    }

    fn group_members(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        let group = self.layout.image_groups.get(&index);
        std::iter::once(index).chain(group.into_iter().flat_map(move |group| {
            self.layout.image_groups.iter().filter_map(move |(&member, id)| {
                (member != index && group == id).then_some(member)
            })
        }))
    }

    fn select_target(&mut self, index: usize, ctrl: bool, shift: bool) {
        self.shape_selection.clear();
        self.selected_design=None;self.design_drag=None;
        self.selected_guide = None;
        let members: Vec<_> = self.group_members(index).collect();
        if shift {
            let anchor = self.selection_anchor.unwrap_or(index)
                .min(self.layout.count.saturating_sub(1));
            let range = anchor.min(index)..=anchor.max(index);
            let ids: std::collections::BTreeSet<_> = range.clone()
                .filter_map(|member| self.layout.image_groups.get(&member)).collect();
            if !ctrl {
                self.selection.clear();
            }
            self.selection.extend(range);
            self.selection.extend(self.layout.image_groups.iter()
                .filter_map(|(&member, id)| ids.contains(id).then_some(member)));
            self.selection_anchor = Some(anchor);
        } else if ctrl {
            if self.selection.contains(&index) {
                self.selection.retain(|member| !members.contains(member));
            } else {
                self.selection.extend(members);
            }
            self.selection_anchor = Some(index);
        } else {
            if !self.selection.contains(&index) {
                self.selection.clear();
            }
            self.selection.extend(members);
            self.selection_anchor = Some(index);
        }
        self.sync_selection();
    }

    fn group_selection(&mut self, ungroup: bool) {
        if self.art.is_none() || self.show_back || self.drag.is_some() || self.cut_draft.is_some() {
            return;
        }
        let before = self.layout.image_groups.clone();
        let ids: std::collections::BTreeSet<_> = self.selection.iter()
            .filter_map(|i| before.get(i).copied()).collect();
        if ungroup {
            self.layout.image_groups.retain(|_, id| !ids.contains(id));
        } else if self.selection.len() >= 2 {
            if ids.len() == 1 && self.selection.iter().all(|i| before.contains_key(i)) {
                return;
            }
            let id = next_group_id(&before);
            self.selection.extend(before.iter().filter_map(|(&i, group)| ids.contains(group).then_some(i)));
            self.layout.image_groups.extend(self.selection.iter().map(|&i| (i, id)));
        }
        if self.layout.image_groups != before {
            self.record_layer_edit(ObjectEdit::Groups(before));
            self.sync_selection();
            self.status = if ungroup {
                "Images ungrouped."
            } else if self.layout.use_manual_cut_paths {
                "Images grouped. Switch to automatic outlines to cut the group as one piece."
            } else {
                "Images grouped with one outer cut line."
            }.into();
        }
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
        self.drag = None;self.design_drag=None;self.shape_group_drag=None;self.shape_selection.clear();
        match edit {
            ObjectEdit::Nudge(positions,designs,cuts)=>{
                self.preview_cache=None;self.back_paths_cache=None;
                ObjectEdit::Nudge(std::mem::replace(&mut self.layout.custom,positions),
                    std::mem::replace(&mut self.layout.design_shapes,designs),
                    std::mem::replace(&mut self.layout.manual_cut_paths,cuts))
            }
            ObjectEdit::Shaping(shapes,paths)=> {
                self.selected_design=None;self.selected_cut_shape=None;self.cut_shape_drag=None;self.preview_cache=None;
                ObjectEdit::Shaping(std::mem::replace(&mut self.layout.design_shapes,shapes),std::mem::replace(&mut self.layout.manual_cut_paths,paths))
            }
            ObjectEdit::Designs(shapes)=>{
                self.selected_design=None;
                ObjectEdit::Designs(std::mem::replace(&mut self.layout.design_shapes,shapes))
            }
            ObjectEdit::PageGeometry(mut layout) => {
                std::mem::swap(&mut self.layout.page_width, &mut layout.page_width);
                std::mem::swap(&mut self.layout.page_height, &mut layout.page_height);
                std::mem::swap(&mut self.layout.page_margins, &mut layout.page_margins);
                std::mem::swap(&mut self.layout.column_gap, &mut layout.column_gap);
                std::mem::swap(&mut self.layout.row_gap, &mut layout.row_gap);
                std::mem::swap(&mut self.layout.custom, &mut layout.custom);
                self.preview_cache = None;
                self.capacity_cache = None;
                ObjectEdit::PageGeometry(layout)
            }
            ObjectEdit::ManualCuts(paths, enabled, linked) => {
                self.cut_draft = None;
                self.selected_cut_shape = None;
                self.cut_shape_drag = None;
                ObjectEdit::ManualCuts(
                    std::mem::replace(&mut self.layout.manual_cut_paths, paths),
                    std::mem::replace(&mut self.layout.use_manual_cut_paths, enabled),
                    std::mem::replace(&mut self.layout.image_cut_paths, linked),
                )
            }
            ObjectEdit::Groups(groups) => {
                let previous = std::mem::replace(&mut self.layout.image_groups, groups);
                self.clear_selection();
                ObjectEdit::Groups(previous)
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
                std::mem::swap(&mut self.layout.image_groups, &mut layout.image_groups);
                std::mem::swap(&mut self.layout.image_cut_paths, &mut layout.image_cut_paths);
                std::mem::swap(&mut self.layout.svg_sources,&mut layout.svg_sources);std::mem::swap(&mut self.layout.image_opacity,&mut layout.image_opacity);
                self.svg_previews.clear();self.clear_selection();
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
            self.layout.image_cut_paths.clone(),
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
        self.crop_edit = None;
        self.selected_design=None;self.design_drag=None;
        if self
            .cut_draft
            .as_ref()
            .is_some_and(|draft| draft.tool == tool)
        {
            return;
        }
        self.selected_guide = None;
        self.pick_cut_shapes = false;
        self.selected_cut_shape = None;
        self.cut_shape_drag = None;
        self.cut_draft = Some(Draft::new(tool));
    }

    fn activate_cut_picker(&mut self) {
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
            if crate::design_transform::handles([x0,y0,x1,y1])
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
        if self.drawing_mode!=0 {
            if self.layout.design_shapes.len()>=100 {self.status="Design shape limit reached.".into();return;}
            self.record_layer_edit(ObjectEdit::Designs(self.layout.design_shapes.clone()));
            let fill=if self.design_fill_enabled && path.first()==path.last(){Some(self.design_fill.to_array())}else{None};
            self.layout.design_shapes.push(sheet::DesignShape{path,fill,
                stroke:self.design_stroke.to_array(),stroke_width:self.design_stroke_width,
                print:true,cut:self.drawing_mode==2});
            self.selected_design=Some(self.layout.design_shapes.len()-1);
            self.cut_draft=None;self.clear_selection();self.pick_cut_shapes=false;
            self.status="Design shape added.".into();return;
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
        let owner = self.selected.filter(|_|self.selection.len()==1)
            .or_else(|| (self.art.is_some() && self.layout.count==1).then_some(0));
        self.cut_draft = None;
        self.record_manual_cut_edit();
        if let Some(owner)=owner {
            if let Err(error)=self.layout.link_cut_paths(owner,vec![path]) {self.status=error;return;}
            self.selected_cut_shape=None;
            self.pick_cut_shapes=false;
        } else {
            self.layout.manual_cut_paths.push(path);
            self.pick_cut_shapes=true;
            self.selected_cut_shape=Some(self.layout.manual_cut_paths.len()-1);
        }
        self.layout.use_manual_cut_paths = true;
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
        if self.cut_draft.is_some() && !response.ctx.wants_keyboard_input()
            && response.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {self.cut_draft=None;return;}
        if self.cut_draft.as_ref().is_some_and(|draft| draft.tool == Tool::Pen)
            && !response.ctx.wants_keyboard_input()
        {
            if response.ctx.input(|i| i.key_pressed(egui::Key::Escape)) { self.cut_draft = None; return; }
            if response.ctx.input(|i| i.key_pressed(egui::Key::Enter)) { self.finish_manual_cut(false); return; }
        }
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
            .filter(|p| !panning && canvas.contains(*p) && (page.contains(*p) || draft.tool==Tool::Line && draft.drag_start.is_some()));
        if draft.tool == Tool::Pen {
            if response.ctx.input(|i| i.key_pressed(egui::Key::Backspace)) && !response.ctx.wants_keyboard_input() {
                draft.anchors.pop();
            }
            if let Some(pointer) = pointer {
                let closing = draft.anchors.len() >= 3 && screen(draft.anchors[0].point).distance(pointer) <= 8.;
                if crate::pen_cursor::show(&response.ctx, canvas.intersect(page), closing) {
                    response.ctx.set_cursor_icon(egui::CursorIcon::Default);
                } else {
                    response.ctx.set_cursor_icon(egui::CursorIcon::None);
                    crate::ui_theme::paint_pen_cursor(&response.ctx, pointer, closing);
                }
            }
        } else if pointer.is_some() {
            response.ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let shift = response.ctx.input(|i| i.modifiers.shift);
        let last_point = draft.anchors.last().map(|last| last.point);
        let constrain = |point| {
            if shift && let Some(last) = last_point {
                cut_tools::constrain_pen(last, point)
            } else { point }
        };
        let primary = egui::PointerButton::Primary;
        if !panning && (response.drag_started_by(primary) || draft.tool!=Tool::Pen && response.is_pointer_button_down_on() && response.ctx.input(|i|i.pointer.primary_pressed())) {
            draft.drag_start = response
                .ctx
                .input(|i| i.pointer.press_origin())
                .filter(|p| canvas.contains(*p) && page.contains(*p))
                .map(|p| constrain(to_page(p)));
        }
        let stop = response.drag_stopped_by(primary);
        let stroke = if self.drawing_mode==0 {egui::Stroke::new(1.5_f32,self.contour_color)} else {egui::Stroke::new((self.design_stroke_width as f32*scale).max(1.),self.design_stroke)};
        if draft.tool != Tool::Pen {
            if let (Some(start), Some(pointer)) = (draft.drag_start, pointer) {
                let equal = response.ctx.input(|i| i.modifiers.shift);
                let mut path = cut_tools::shape_path(
                    draft.tool,
                    start,
                    to_page(pointer),
                    equal,
                    self.cut_corner_radius,
                );
                if draft.tool==Tool::Line && path.len()==2 {
                    path[1]=cut_tools::clip_line_end(path[0],path[1],[self.layout.page_width,self.layout.page_height]);
                    if (path[1][0]-path[0][0]).hypot(path[1][1]-path[0][1])<0.2 {path.clear();}
                }
                if stop && !path.is_empty() {
                    self.commit_manual_cut(path);
                    return;
                }
                let mut points:Vec<_>=path.iter().copied().map(screen).collect();
                let closed=path.len()>2 && path.first()==path.last();
                if closed && self.drawing_mode!=0 && self.design_fill_enabled {
                    points.pop();
                    painter.add(egui::Shape::convex_polygon(points,self.design_fill,stroke));
                }else {painter.add(egui::Shape::line(points,stroke));}
                response.ctx.request_repaint();
            }
            if stop {
                draft.drag_start = None;
            }
            return;
        }
        let mut editing = draft.edit_drag.is_some();
        if response.drag_started_by(primary)
            && let Some(start) = response.ctx.input(|i| i.pointer.press_origin())
            && canvas.contains(start) && page.contains(start)
        {
            let anchor = draft.anchors.iter().position(|a| screen(a.point).distance(start) <= 9.);
            draft.edit_drag = anchor.or_else(|| {
                draft.segment_at(to_page(start), 6. / f64::from(scale))
                    .and_then(|(index,t)| draft.insert_anchor(index,t))
            });
            if draft.edit_drag.is_some() { draft.drag_start = None; editing = true; }
        }
        if let (Some(index), Some(pointer)) = (draft.edit_drag, pointer) {
            let point = to_page(pointer);
            if let Some(anchor) = draft.anchors.get_mut(index) {
                if shift {
                    anchor.tangent = [point[0]-anchor.point[0],point[1]-anchor.point[1]];
                    anchor.incoming = None;
                    anchor.break_outgoing = false;
                } else { anchor.point = point; }
            }
        }
        if stop { draft.edit_drag = None; }
        let pending_anchor = if editing { None } else { draft.drag_start.zip(pointer).map(|(point, pointer)| {
            let end = if shift { cut_tools::constrain_pen(point, to_page(pointer)) } else { to_page(pointer) };
            Anchor {
                point,
                tangent: [end[0] - point[0], end[1] - point[1]],
                break_outgoing: false, incoming: None,
            }
        }) };
        if stop {
            draft.drag_start = None;
            if let Some(anchor) = pending_anchor
                && draft.anchors.len() < 256
            {
                draft.anchors.push(anchor);
            }
        } else if !editing && response.clicked()
            && let Some(p) = pointer
        {
            if response.ctx.input(|i| i.modifiers.alt)
                && let Some(last) = draft.anchors.last_mut()
                && screen(last.point).distance(p) <= 9.
            {
                last.break_outgoing = true;
                self.status = "Outgoing Pen handle removed. Continue with a straight segment.".into();
                return;
            }
            if draft.anchors.len() >= 3 && screen(draft.anchors[0].point).distance(p) <= 7. {
                self.finish_manual_cut(true);
                return;
            }
            if draft.anchors.iter().any(|a| screen(a.point).distance(p) <= 9.) {
                return;
            }
            if let Some((index,t)) = draft.segment_at(to_page(p), 6. / f64::from(scale)) {
                draft.insert_anchor(index,t);
            } else if draft.anchors.len() < 256
                && draft
                    .anchors
                    .last()
                    .is_none_or(|last| screen(last.point).distance(p) >= 2.)
            {
                draft.anchors.push(Anchor {
                    point: constrain(to_page(p)),
                    tangent: [0.; 2],
                    break_outgoing: false, incoming: None,
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
        // Flatten settled curves only when anchors/zoom change. Live mouse motion
        // tessellates just the last segment at subpixel preview precision.
        let tolerance = (0.5 / f64::from(scale)).clamp(0.002, 0.5);
        let points: Vec<_> = draft.preview_path(tolerance).iter().copied().map(screen).collect();
        if points.len() >= 2 { painter.add(egui::Shape::line(points, stroke)); }
        if !stop && let (Some(last), Some(anchor)) = (draft.anchors.last(), pending_anchor) {
            let points = cut_tools::pen_path_with_tolerance(&[*last, anchor], false, tolerance)
                .into_iter().map(screen).collect();
            painter.add(egui::Shape::line(points, stroke));
        }
        for anchor in &anchors {
            let center = screen(anchor.point);
            let incoming = anchor.incoming.unwrap_or(anchor.tangent);
            if anchor.tangent != [0.; 2] || incoming != [0.;2] {
                let a = screen([anchor.point[0] - incoming[0], anchor.point[1] - incoming[1]]);
                let b = if anchor.break_outgoing { center } else {
                    screen([anchor.point[0] + anchor.tangent[0], anchor.point[1] + anchor.tangent[1]])
                };
                painter.line_segment([a,b], egui::Stroke::new(1_f32, crate::ui_theme::ACCENT));
                painter.circle_filled(a, 3., crate::ui_theme::ACCENT);
                if !anchor.break_outgoing { painter.circle_filled(b, 3., crate::ui_theme::ACCENT); }
            }
            painter.rect(egui::Rect::from_center_size(center,egui::vec2(7.,7.)),0.,egui::Color32::WHITE,
                egui::Stroke::new(1_f32,crate::ui_theme::ACCENT),egui::StrokeKind::Inside);
        }
        if !editing && pending_anchor.is_none()
            && let (Some(last), Some(pointer)) = (anchors.last(), pointer)
        {
            let end = if shift { screen(cut_tools::constrain_pen(last.point, to_page(pointer))) } else { pointer };
            let points = cut_tools::pen_path_with_tolerance(&[*last, Anchor { point: to_page(end), tangent: [0.; 2], break_outgoing: false, incoming: None }], false, tolerance)
                .into_iter().map(screen).collect();
            painter.add(egui::Shape::line(points, stroke));
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
            crate::design_transform::handles([x0,y0,x1,y1]).map(screen)
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
            if let Some(index)=hit {
                self.clear_selection();
                self.select_shape(crate::shape_ops::Target::Cut(index),false);
            }
            self.cut_shape_drag = hit.map(|index| {
                let before=self.layout.manual_cut_paths.clone();
                let duplicate=response.ctx.input(|i|i.modifiers.ctrl) && handle.is_none() && before.len()<100;
                let path=before[index].clone();
                let index=if duplicate {self.layout.manual_cut_paths.push(path.clone());self.layout.manual_cut_paths.len()-1}else{index};
                self.selected_cut_shape=Some(index);
                CutShapeDrag {before,duplicate,index,start:physical(start),path,corner:handle.map(|(_,corner)|corner)}
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
            if let Some(index)=self.selected_cut_shape {
                self.clear_selection();
                self.select_shape(crate::shape_ops::Target::Cut(index),response.ctx.input(|i|i.modifiers.shift));
            }
        }
        if let Some(drag) = &self.cut_shape_drag
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let [x,y]=physical(pointer);
            let modifiers=response.ctx.input(|i|i.modifiers);
            self.layout.manual_cut_paths[drag.index]=crate::design_transform::transform(&drag.path,[x-drag.start[0],y-drag.start[1]],drag.corner,modifiers.shift,modifiers.alt,[self.layout.page_width,self.layout.page_height]);
            response.ctx.set_cursor_icon(drag.corner.map(crate::design_transform::cursor).unwrap_or(if drag.duplicate {egui::CursorIcon::Copy}else{egui::CursorIcon::Grabbing}));
        }else if self.pick_cut_shapes && !panning && response.hovered() && let Some(pointer)=response.hover_pos() {
            let handle=self.selected_cut_shape.and_then(|i|self.layout.manual_cut_paths.get(i)).and_then(|path|corners(path).iter().position(|p|p.distance(pointer)<=8.));
            if let Some(corner)=handle {response.ctx.set_cursor_icon(crate::design_transform::cursor(corner));}
            else if self.layout.manual_cut_paths.iter().any(|path|cut_tools::hit(path,physical(pointer),6./f64::from(scale))) {
                response.ctx.set_cursor_icon(if response.ctx.input(|i|i.modifiers.ctrl) {egui::CursorIcon::Copy}else{egui::CursorIcon::Grab});
            }
        }
        if response.drag_stopped_by(primary) && let Some(drag)=self.cut_shape_drag.take() {
            if self.layout.manual_cut_paths!=drag.before {
                let current=std::mem::replace(&mut self.layout.manual_cut_paths,drag.before);
                self.record_manual_cut_edit();
                self.layout.manual_cut_paths=current;
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
            painter.add(egui::Shape::closed_line(corners[..4].to_vec(), stroke));
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
        let Ok(items) = self.layout.editor_items() else {
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
        let mut new_groups = std::collections::BTreeMap::new();
        for (new, (old, _)) in (first..).zip(members) {
            if let Some(paths)=before.image_cut_paths_for(*old) {self.layout.image_cut_paths.insert(new,paths.clone());}
            self.layout.image_opacity.insert(new,before.opacity_for(*old));
            if let Some(svg)=before.svg_source_for(*old) {self.layout.svg_sources.insert(new,svg.clone());}
            if let Some(id) = before.image_groups.get(old) {
                let duplicate_group = *new_groups.entry(*id)
                    .or_insert_with(|| next_group_id(&self.layout.image_groups));
                self.layout.image_groups.insert(new, duplicate_group);
            }
        }
        remove_singleton_groups(&mut self.layout.image_groups);
        for (new, (old, _)) in (first..).zip(members) {
            if before.hidden_images.contains(old) {
                self.layout.hidden_images.insert(new);
            }
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
        let Ok(items) = self.layout.editor_items() else {
            return;
        };
        let members: Vec<_> = self
            .selection
            .iter()
            .filter(|i| self.movable_image(**i))
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
        let Ok(items) = self.layout.editor_items() else {
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
            self.layout.hidden_images.clear();self.layout.image_opacity.clear();
            self.layout.image_groups.clear();
            self.layout.image_cut_paths.clear();self.layout.svg_sources.clear();self.svg_previews.clear();
            self.layout.count = 1;
            self.art = None;
            self.texture = None;
            self.preview_cache = None;
            self.refresh_source_digest();
            self.record_layer_edit(ObjectEdit::LastImage(Box::new(before), source, name));
        } else {
            self.layout.count = keep.len();
            self.layout.custom = keep.iter().map(|(_, p)| *p).enumerate().collect();
            self.layout.image_opacity=keep.iter().enumerate().map(|(new,(old,_))|(new,before.opacity_for(*old))).collect();
            self.layout.svg_sources=keep.iter().enumerate().filter_map(|(new,(old,_))|before.svg_source_for(*old).cloned().map(|svg|(new,svg))).collect();
            self.svg_previews.clear();
            self.layout.image_cut_paths=keep.iter().enumerate().filter_map(|(new,(old,_))|
                before.image_cut_paths_for(*old).cloned().map(|paths|(new,paths))).collect();
            self.layout.hidden_images = keep
                .iter()
                .enumerate()
                .filter_map(|(new, (old, _))| before.hidden_images.contains(old).then_some(new))
                .collect();
            self.layout.image_groups = keep.iter().enumerate().filter_map(|(new, (old, _))| {
                before.image_groups.get(old).map(|&id| (new, id))
            }).collect();
            remove_singleton_groups(&mut self.layout.image_groups);
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
        let Ok(items) = self.layout.editor_items() else {
            return;
        };
        let members: Vec<_> = self
            .selection
            .iter()
            .filter(|i| self.movable_image(**i))
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
    fn nudge_step(&self,mods:egui::Modifiers)->f64 {
        let step=if mods.shift {self.nudge_coarse}else{self.nudge_fine};
        step*if mods.ctrl{10.}else{1.}*if mods.alt{0.1}else{1.}
    }
    fn nudge_canvas_selection(&mut self,dx:f64,dy:f64) {
        use crate::shape_ops::Target;
        if self.drag.is_some() || self.design_drag.is_some() || self.cut_shape_drag.is_some()
            || self.shape_group_drag.is_some() || self.cut_draft.is_some() || self.crop_edit.is_some()
            || self.guide_drag.is_some() || !dx.is_finite() || !dy.is_finite(){return;}
        let mut targets=self.shape_selection.clone();
        if let Some(active)=self.selected_design.map(Target::Design).or_else(||self.selected_cut_shape.map(Target::Cut))
            && !targets.contains(&active) {targets=vec![active];}
        targets.sort_by_key(|t|match t {Target::Design(i)=>(0,*i),Target::Cut(i)=>(1,*i)});
        targets.dedup();
        let mut lengths=vec![];let mut points=vec![];
        targets.retain(|target|{
            let path=match target {Target::Design(i)=>self.layout.design_shapes.get(*i).map(|s|&s.path),Target::Cut(i)=>self.layout.manual_cut_paths.get(*i)};
            if let Some(path)=path.filter(|p|!p.is_empty()) {lengths.push(path.len());points.extend_from_slice(path);true}else{false}
        });
        if targets.is_empty(){self.nudge_selection(dx,dy);return;}
        let items=self.layout.editor_items().unwrap_or_default();
        let members:Vec<_>=self.selection.iter().filter(|i|self.movable_image(**i))
            .filter_map(|i|items.get(*i).map(|p|(*i,*p))).collect();
        let [x0,y0,x1,y1]=cut_tools::bounds(&points);
        let mut bounds=members.clone();
        bounds.push((usize::MAX,sheet::Placement{x:x0,y:y0,width:(x1-x0).max(0.),height:(y1-y0).max(0.),orientation:Default::default()}));
        let (dx,dy)=group_move_delta(&bounds,dx,dy,if members.is_empty(){0.}else{self.edit_inset()},[self.layout.page_width,self.layout.page_height]);
        if dx.abs()<1e-10 && dy.abs()<1e-10{return;}
        let before=if members.is_empty(){ObjectEdit::Shaping(self.layout.design_shapes.clone(),self.layout.manual_cut_paths.clone())}
            else {ObjectEdit::Nudge(self.layout.custom.clone(),self.layout.design_shapes.clone(),self.layout.manual_cut_paths.clone())};
        if !members.is_empty(){
            for (i,p) in items.into_iter().enumerate(){self.layout.custom.insert(i,p);}
            for (i,p) in members {self.layout.custom.insert(i,sheet::Placement{x:p.x+dx,y:p.y+dy,..p});}
        }
        for point in &mut points {point[0]+=dx;point[1]+=dy;}
        self.set_shape_group_points(&targets,&lengths,&points);
        self.record_layer_edit(before);
    }
    fn document_stepper(ui: &mut egui::Ui, unit: Unit, value: &mut f64, min: f64) -> bool {
        let before = *value;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.;
            ui.add_sized([110., 28.], unit.distance(value)
                .speed(0.1).range(min..=2000.).update_while_editing(false));
            if ui.add_sized([28., 28.], egui::Button::new("−")).on_hover_text("Decrease by 1 mm").clicked() {
                *value = (*value - 1.).max(min);
            }
            if ui.add_sized([28., 28.], egui::Button::new("+")).on_hover_text("Increase by 1 mm").clicked() {
                *value = (*value + 1.).min(2000.);
            }
        });
        *value != before
    }

    fn margin_fields(ui: &mut egui::Ui, unit: Unit, margins: &mut [f64; 4], linked: &mut bool) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.;
            egui::Grid::new(ui.id().with("margin-rows"))
                .num_columns(2).spacing([10., 8.]).show(ui, |ui| {
                for (index, (icon, hint)) in [(Icon::MarginLeft, "Left margin"),
                    (Icon::MarginRight, "Right margin"), (Icon::MarginTop, "Top margin"),
                    (Icon::MarginBottom, "Bottom margin")].into_iter().enumerate() {
                    crate::ui_theme::margin_indicator(ui, icon, hint);
                    let before = margins[index];
                    Self::document_stepper(ui, unit, &mut margins[index], 0.);
                    if margins[index] != before && *linked { *margins = [margins[index]; 4]; }
                    ui.end_row();
                }
            });
            let kind = if *linked { Icon::Link } else { Icon::Unlink };
            if icon_toggle(ui, kind, *linked, if *linked { "Unlink margins" } else { "Link all margins" }) {
                *linked = !*linked;
                if *linked { *margins = [margins[0]; 4]; }
            }
        });
    }

    fn edit_margin_fields(&mut self, ui: &mut egui::Ui) {
        let previous = self.layout.margins();
        let mut margins = previous;
        Self::margin_fields(ui, self.unit, &mut margins, &mut self.margins_linked);
        if margins == previous {
            return;
        }
        if !Layout::margins_fit(margins, self.layout.page_width, self.layout.page_height) {
            self.status = "Margins must leave at least 1 mm of usable width and height.".into();
            return;
        }
        let previous_layout = self.layout.clone();
        if self.art.is_some() && let Ok(items) = self.layout.editor_items() {
            self.layout.custom = items.into_iter().enumerate().collect();
        }
        self.layout.page_margins = Some(margins);
        self.record_layer_edit(ObjectEdit::PageGeometry(Box::new(previous_layout)));
    }

    fn edit_inset(&self) -> f64 {
        if self.ccd_trial {
            self.ccd_profile.inset + self.ccd_profile.diameter / 2. + 3.
        } else {
            0.
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
        let items=if self.art.is_some() {
            let Ok(items)=self.layout.editor_items() else{return};items
        }else{vec![]};
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
                    .filter(|i| self.movable_image(**i))
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
                        self.group_members(index).map(|i| (i, items[i])).collect()
                    } else {
                        self.selection
                            .iter()
                            .filter(|i| self.movable_image(**i))
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
                } else {
                    let previous = if modifiers.ctrl || modifiers.shift { self.selection.clone() } else { Default::default() };
                    let mut previous_shapes=if modifiers.ctrl || modifiers.shift {self.shape_selection.clone()}else{vec![]};
                    if modifiers.ctrl || modifiers.shift {
                        for target in self.selected_design.map(crate::shape_ops::Target::Design).into_iter().chain(self.selected_cut_shape.map(crate::shape_ops::Target::Cut)) {
                            if !previous_shapes.contains(&target) {previous_shapes.push(target);}
                        }
                    }
                    self.marquee = Some((start, previous,previous_shapes));
                    self.pick_cut_shapes=false;
                    self.selected_design = None;
                    self.selected_cut_shape = None;
                    self.selected_guide = None;
                }
            }
        }
        if let Some((start, previous,previous_shapes)) = self.marquee.clone() {
            if let Some(pointer) = pointer {
                let bounds = egui::Rect::from_two_pos(start, pointer);
                self.selection = previous;self.shape_selection=previous_shapes;
                if bounds.width() > 3. || bounds.height() > 3. {
                    for (index, placement) in items.iter().enumerate() {
                        if !self.layout.hidden_images.contains(&index) && bounds.intersects(screen(*placement)) {
                            let members: Vec<_> = self.group_members(index).collect();
                            self.selection.extend(members);
                        }
                    }
                    let a=(bounds.min-page.min)/scale;let b=(bounds.max-page.min)/scale;
                    let rect=[a.x as f64,a.y as f64,b.x as f64,b.y as f64];
                    for (index,shape) in self.layout.design_shapes.iter().enumerate() {
                        let target=crate::shape_ops::Target::Design(index);
                        if (shape.print || self.show_cut && shape.cut) && cut_tools::intersects_box(&shape.path,rect) && !self.shape_selection.contains(&target) {self.shape_selection.push(target);}
                    }
                    if self.show_cut {
                        for (index,path) in self.layout.manual_cut_paths.iter().enumerate() {
                            let target=crate::shape_ops::Target::Cut(index);
                            if cut_tools::intersects_box(path,rect) && !self.shape_selection.contains(&target) {self.shape_selection.push(target);}
                        }
                    }
                    ui.painter().rect(bounds, 0., crate::ui_theme::ACCENT.linear_multiply(0.12),
                        egui::Stroke::new(1_f32, crate::ui_theme::ACCENT), egui::StrokeKind::Inside);
                }
                self.selection_anchor = self.selection.first().copied();
                self.sync_selection();
            }
            if ui.input(|i| i.pointer.primary_released() || !i.pointer.primary_down()) {
                self.marquee = None;
                self.selected_design=if self.shape_selection.len()==1 {self.shape_selection.iter().find_map(|t|if let crate::shape_ops::Target::Design(i)=t {Some(*i)}else{None})}else{None};
                self.selected_cut_shape=if self.shape_selection.len()==1 {self.shape_selection.iter().find_map(|t|if let crate::shape_ops::Target::Cut(i)=t {Some(*i)}else{None})}else{None};
                self.pick_cut_shapes=self.selected_cut_shape.is_some();
            }
            return;
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
                        if self.layout.hidden_images.contains(i) { continue; }
                        let rect = screen(sheet::Placement {
                            x: p.x + dx,
                            y: p.y + dy,
                            ..*p
                        });
                        paint_oriented_image(
                            ui.painter(), texture.id(), rect, self.image_uv(*i, art),
                            p.orientation, egui::Color32::from_white_alpha(150),
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
                        if self.layout.hidden_images.contains(i) { continue; }
                        let rect = screen(*original);
                        paint_oriented_image(
                            ui.painter(), texture.id(), rect, self.image_uv(*i, art),
                            original.orientation, egui::Color32::from_white_alpha(80),
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
                    .filter(|i| self.movable_image(**i))
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
        let Ok(items) = self.layout.editor_items() else {
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
                .filter(|i| self.movable_image(**i))
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
    fn handle_dropped_artwork(&mut self, ctx: &egui::Context) {
        if self.web_import.is_some() {
            return;
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.first().cloned());
        let Some(dropped) = dropped else { return };
        if let Some(path) = dropped.path {
            if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("mc630")) {
                self.request_action(DocumentAction::OpenProject(path), ctx);
            } else if self.show_back {
                match crate::image_import::read_file(&path) {
                    Ok(image) => self.add_imported_image(image, true, ctx),
                    Err(error) => self.back.message = error,
                }
            } else if self.source.is_empty() {
                self.request_action(DocumentAction::OpenImage(path), ctx);
            } else {
                self.add_image(&path, ctx);
            }
        } else if let Some(bytes) = dropped.bytes {
            match crate::image_import::prepare(bytes.to_vec(), dropped.name) {
                Ok(image) => self.add_imported_image(image, self.show_back, ctx),
                Err(error) if self.show_back => self.back.message = error,
                Err(error) => self.status = error,
            }
        }
    }

    fn add_imported_image(
        &mut self,
        image: crate::image_import::ImportedImage,
        back: bool,
        ctx: &egui::Context,
    ) {
        if back {
            let paths = self.back_cut_paths();
            self.back.message = match self.back.add_image_bytes(
                image.bytes, image.name, [self.layout.page_width, self.layout.page_height], &paths,
            ) {
                Ok(()) => "Image imported.".into(),
                Err(error) => format!("Cannot import image: {error}"),
            };
        } else if self.source.is_empty() {
            self.install_image_fitted(image.bytes, image.name, ctx);
            if self.art.is_some() && let Some(svg)=image.svg {self.layout.svg_sources.insert(0,svg);}
        } else {
            let result = crate::photo_size::append_image_bytes(
                &self.layout, &self.source, &image.bytes, self.edit_inset(),
            ).map(|(source,mut layout)| {
                if let Some(mut svg)=image.svg {svg.raster_rect=layout.image_tiles[layout.count-1];layout.svg_sources.insert(layout.count-1,svg);}
                (source,layout)
            });
            self.finish_add_image(result, ctx);
        }
    }

    pub(crate) fn web_image_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.web_import else { return };
        if let Some(pending) = &dialog.pending {
            use std::sync::mpsc::TryRecvError;
            match pending.try_recv() {
                Ok(Ok(image)) => {
                    let back = dialog.back;
                    self.web_import = None;
                    self.add_imported_image(image, back, ctx);
                    ctx.request_repaint();
                    return;
                }
                Ok(Err(error)) => {
                    dialog.pending = None;
                    dialog.error = Some(error);
                }
                Err(TryRecvError::Disconnected) => {
                    dialog.pending = None;
                    dialog.error = Some("Image download stopped unexpectedly.".into());
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut cancel = false;
        let modal = crate::ui_theme::dialog_modal(ctx, "web-image-import").show(ctx, |ui| {
            ui.set_width(360.);
            cancel |= crate::ui_theme::dialog_header(ui,"Import image from URL",true);
            let pending = dialog.pending.is_some();
            let field = ui.add_enabled(!pending, egui::TextEdit::singleline(&mut dialog.url)
                .hint_text("https://…").desired_width(360.));
            if !pending && dialog.url.is_empty() {
                field.request_focus();
            }
            if let Some(error) = &dialog.error {
                ui.label(error);
            }
            ui.separator();
            crate::ui_theme::dialog_actions(ui,|ui| {
                if pending {
                    ui.spinner();
                    ui.label("Downloading image…");
                } else if crate::ui_theme::primary_button(ui,"Import").clicked()
                    || (field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                {
                    match crate::image_import::web_url(&dialog.url) {
                        Ok(url) => {
                            let (sender, receiver) = std::sync::mpsc::channel();
                            let ctx = ctx.clone();
                            std::thread::spawn(move || {
                                let _ = sender.send(crate::image_import::download(url));
                                ctx.request_repaint();
                            });
                            dialog.pending = Some(receiver);
                            dialog.error = None;
                        }
                        Err(error) => dialog.error = Some(error),
                    }
                }
                cancel |= ui.add_sized([80.,28.],egui::Button::new("Cancel")).clicked();
            });
        });
        if cancel || modal.should_close() {
            self.web_import = None;
        }
    }

    fn choose_image(&mut self, ctx: &egui::Context) {
        if !self.source.is_empty() {
            self.choose_add_image(ctx);
            return;
        }
        if let Some(mut paths) = rfd::FileDialog::new()
            .add_filter("Images", crate::image_import::EXTENSIONS)
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
            .add_filter("Images", crate::image_import::EXTENSIONS)
            .pick_file()
        {
            self.add_image(&path, ctx);
        }
    }

    fn add_image(&mut self, path: &std::path::Path, ctx: &egui::Context) {
        let inset = self.edit_inset();
        let result = crate::photo_size::append_image(&self.layout, &self.source, path, inset);
        self.finish_add_image(result, ctx);
    }

    fn clipboard_image_bytes() -> Result<Vec<u8>, String> {
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
    }

    fn copy_selected_images(&mut self) {
        if !self.show_back && self.selection.is_empty() && let Some(copy)=self.selected_shape_copy() {
            self.status=match crate::studio_clipboard::store_shapes(&copy) {Ok(())=>"Shapes copied with styles and cutline settings.".into(),Err(error)=>format!("Cannot copy shapes: {error}")};return;
        }
        if self.show_back {self.back.copy_selected();return;}
        if self.show_back || self.selection.is_empty() || self.source.is_empty() { return; }
        let copy=crate::studio_clipboard::ArtworkCopy {source:self.source.clone(),layout:self.layout.clone(),indices:self.selection.iter().copied().collect()};
        self.status=match crate::studio_clipboard::store(copy) {
            Ok(())=>"Images and linked outlines copied.".into(),Err(error)=>format!("Cannot copy images: {error}"),
        };
    }
    fn paste_image(&mut self, ctx: &egui::Context) {
        if !self.show_back {
            match crate::studio_clipboard::read_shapes() {
                Ok(Some(copy))=>{self.paste_shapes(&copy);return;}
                Err(error)=>{self.status=format!("Cannot paste shapes: {error}");return;}
                Ok(None)=>{}
            }
        }
        if !self.show_back && let Some(copy)=crate::studio_clipboard::read() {
            let result=copy.compose(&self.layout,&self.source,self.edit_inset());
            self.finish_add_image(result,ctx);
            return;
        }
        if self.show_back && self.back.paste_copied([self.layout.page_width,self.layout.page_height]) { return; }
        let result = Self::clipboard_image_bytes();
        if self.show_back {
            let paths = self.back_cut_paths();
            let result = result.and_then(|bytes| self.back.add_image_bytes(bytes, "Clipboard image".into(), [self.layout.page_width,self.layout.page_height], &paths));
            self.back.message = match result { Ok(()) => "Image pasted.".into(), Err(e) => format!("Cannot paste image: {e}") };
            return;
        }
        match result {
            Ok(bytes) if self.source.is_empty() => {
                self.install_image_fitted(bytes, "Clipboard image".into(), ctx);
            }
            Ok(bytes) => {
                let inset = self.edit_inset();
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

    fn install_image_fitted(&mut self, bytes: Vec<u8>, name: String, ctx: &egui::Context) {
        self.install_single_image(bytes, name, ctx);
        if self.art.is_some() {
            let inset = self.edit_inset();
            let [left, right, top, bottom] = self.layout.margins().map(|margin| margin.max(inset));
            let max_width = self.layout.page_width - left - right;
            let max_height = self.layout.page_height - top - bottom;
            let fit = (max_width / self.layout.width)
                .min(max_height / self.layout.height)
                .min(1.);
            if fit > 0. && fit < 1. {
                self.layout.width *= fit;
                self.layout.height *= fit;
                self.status
                    .push_str(" Image was scaled to fit the sheet.");
            }
            let [margin_left, margin_right, margin_top, margin_bottom] = self.layout.margins();
            if [margin_left, margin_right, margin_top, margin_bottom].iter().any(|&margin| margin < inset)
                && self.layout.width <= max_width
                && self.layout.height <= max_height
            {
                self.layout.custom.insert(0, sheet::Placement {
                    x: left,
                    y: top,
                    width: self.layout.width,
                    height: self.layout.height,
                    orientation: Default::default(),
                });
            }
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
        let inset = self.edit_inset();
        let [left, right, top, bottom] = self.layout.margins().map(|margin| margin.max(inset));
        let result = crate::photo_size::image_group(
            paths,
            [self.layout.page_width - left - right, self.layout.page_height - top - bottom],
        );
        match result {
            Ok(group) => {
                let [width, height] = group.size;
                let mut layout = self.layout.clone();
                layout.use_manual_cut_paths = true;
                layout.count = group.tiles.len();
                layout.image_tiles = group.tiles;
                layout.svg_sources=group.svg_sources;self.svg_previews.clear();
                layout.custom = group
                    .placements
                    .into_iter()
                    .enumerate()
                    .map(|(i, mut p)| {
                        p.x += left;
                        p.y += top;
                        (i, p)
                    })
                    .collect();
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
                    if ui.add(egui::Button::new("Open project...").shortcut_text("Ctrl+Shift+O")).clicked() {
                        ui.close_menu();
                        self.choose_project(ctx);
                    }
                    self.recent_projects_menu(ui, ctx);
                    if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
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
                    if ui.add(egui::Button::new("Close project").shortcut_text("Ctrl+W")).clicked() {
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
                    if ui.add(egui::Button::new("Save As...").shortcut_text("Ctrl+Shift+S")).clicked() {
                        ui.close_menu();
                        self.save_project(true);
                    }
                    ui.separator();
                    if ui.button("Settings...").clicked() {
                        self.show_settings = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.add(egui::Button::new("New document...").shortcut_text("Ctrl+N")).clicked() {
                        self.request_action(DocumentAction::New, ctx);
                        ui.close_menu();
                    }
                    ui.separator();

                    if ui.add(egui::Button::new("Import image...").shortcut_text("Ctrl+O")).clicked() {
                        ui.close_menu();
                        if self.show_back { self.import_back_image(); } else { self.choose_image(ctx); }
                    }
                    if ui.add_enabled(self.art.is_some() && !self.show_back,egui::Button::new("Vector Trace…")).clicked() {
                        ui.close_menu();self.open_vector_trace(ctx);
                    }
                    if ui.button("Import image from URL…").clicked() {
                        ui.close_menu();
                        self.web_import = Some(WebImageImport {
                            back: self.show_back,
                            url: String::new(),
                            error: None,
                            pending: None,
                        });
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
                            self.art.is_some() || self.layout.has_print_designs(),
                            egui::Button::new("Export print & cut job..."),
                        )
                        .clicked()
                    {
                        ui.close_menu();
                        self.export_pair(frame);
                    }
                    if ui.add(egui::Button::new("Print...").shortcut_text("Ctrl+P")).clicked() {
                        self.show_print_dialog = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.add_enabled((!self.show_back && (!self.selection.is_empty() || self.selected_shape_copy().is_some())) || self.show_back,egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
                        self.copy_selected_images();ui.close_menu();
                    }
                    if ui.add(egui::Button::new("Paste").shortcut_text("Ctrl+V")).clicked() {
                        ui.close_menu();
                        self.paste_image(ctx);
                    }
                    if ui
                        .add_enabled(
                            self.art.is_some(),
                            egui::Button::new("Select all images").shortcut_text("Ctrl+A"),
                        )
                        .clicked()
                    {
                        self.select_all();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            !self.selection.is_empty(),
                            egui::Button::new("Repeat duplicate").shortcut_text("Ctrl+R"),
                        )
                        .clicked()
                    {
                        self.repeat_duplicate(ctx);
                        ui.close_menu();
                    }
                    for (label, shortcut, ungroup, enabled) in [
                        ("Group images", "Ctrl+G", false, self.selection.len() >= 2),
                        ("Ungroup images", "Ctrl+Shift+G", true,
                            self.selection.iter().any(|i| self.layout.image_groups.contains_key(i))),
                    ] {
                        if ui.add_enabled(!self.show_back && enabled, egui::Button::new(label).shortcut_text(shortcut)).clicked() {
                            self.group_selection(ungroup);
                            ui.close_menu();
                        }
                    }
                    if ui
                        .add_enabled(
                            !self.selection.is_empty(),
                            egui::Button::new("Delete selected images").shortcut_text("Del"),
                        )
                        .clicked()
                    {
                        self.delete_selected_images(ctx);
                        ui.close_menu();
                    }

                    if ui
                        .add_enabled(
                            !self.undo.is_empty(),
                            egui::Button::new("Undo object edit").shortcut_text("Ctrl+Z"),
                        )
                        .clicked()
                    {
                        self.undo_object(ctx);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            !self.redo.is_empty(),
                            egui::Button::new("Redo object edit").shortcut_text("Ctrl+Y"),
                        )
                        .clicked()
                    {
                        self.redo_object(ctx);
                        ui.close_menu();
                    }
                    if ui.add(egui::Button::new("Deselect").shortcut_text("Esc")).clicked() {
                        self.clear_selection();
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.rulers, "Rulers");
                    ui.checkbox(&mut self.snap_guides, "Snap to guides and margins");
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
                    ui.checkbox(&mut self.show_properties, "Right dock");
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
                    ui.checkbox(&mut self.show_manual_marks, "Manual alignment crosses")
                        .on_hover_text("Show corner crosses on the canvas. Printed registration marks are unchanged.");
                    if ui.button("Fit page").clicked() {
                        self.camera.fit(ctx);
                        ui.close_menu();
                    }
                });
                ui.menu_button("Layout", |ui| {
                    if ui.button("Back-side artwork…").clicked() { self.show_back = true; ui.close_menu(); }

                    if ui.button("Page size...").clicked() {
                        self.open_page_size();
                        ui.close_menu();
                    }
                    ui.separator();
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
                        self.open_sidebar_section(1);
                        ui.close_menu();
                    }
                });
                ui.menu_button("Cut", |ui| {
                    ui.checkbox(&mut self.ccd_trial, "CCD four-dot registration");
                    ui.checkbox(&mut self.layout.cut_inner_holes, "Cut inner holes");
                    if ui.button("Contour properties").clicked() {
                        self.open_sidebar_section(2);
                        ui.close_menu();
                    }
                });
                updates.menu(ui, ctx);
            });
        });
    }
    pub fn standard_toolbar(&mut self, ctx: &egui::Context, frame: &eframe::Frame, document_open: bool) {
        egui::TopBottomPanel::top("standard-toolbar")
            .frame(crate::ui_theme::compact_bar(ctx))
            .show(ctx, |ui| {
            crate::ui_theme::compact_spacing(ui);
            ui.horizontal(|ui| {
                if icon_toggle(ui, Icon::New, false, "New project (Ctrl+N)") {
                    self.request_action(DocumentAction::New, ctx);
                }
                if icon_toggle(ui, Icon::Open, false, "Open project (Ctrl+Shift+O)") {
                    self.choose_project(ctx);
                }
                if ui.add_enabled_ui(document_open, |ui| {
                    icon_toggle(ui, Icon::Save, false, "Save project (Ctrl+S)")
                }).inner {
                    self.save_project(false);
                }
                ui.separator();
                if icon_toggle(ui, Icon::Import, false, "Import image (Ctrl+O)") {
                    if self.show_back { self.import_back_image(); } else { self.choose_image(ctx); }
                }
                ui.add_enabled_ui(document_open, |ui| {
                if ui
                    .add_enabled_ui(self.show_back || self.art.is_some(), |ui| {
                        icon_toggle(ui, Icon::AddImage, false, "Add image")
                    })
                    .inner
                {
                    if self.show_back { self.import_back_image(); } else { self.choose_add_image(ctx); }
                }
                if icon_toggle(ui, Icon::Print, self.show_print_dialog, "Print (Ctrl+P)") {
                    self.print_back = self.show_back;
                    self.show_print_dialog = true;
                }
                if ui
                    .add_enabled_ui(self.can_export_cut(), |ui| {
                        icon_toggle(ui, Icon::Export, false, "Export PLT")
                    })
                    .inner
                {
                    self.export_cut_file(frame);
                }
                ui.separator();
                for (index, (icon, label)) in
                    [(Icon::Artwork, "Artwork"), (Icon::Settings, "Size and Repeat"), (Icon::Outline, "Cut Outline"), (Icon::Registration, "Registration")]
                        .into_iter()
                        .enumerate()
                {
                    let visible = self.show_properties
                        && !self.show_navigator
                        && !self.show_print_dialog
                        && self
                            .section_palettes
                            .iter()
                            .position(|section| section.visible)
                            .unwrap_or(0)
                            == index;
                    if icon_toggle(ui, icon, visible, label) {
                        if visible { self.show_properties = false; }
                        else { self.open_sidebar_section(index); }
                    }
                }

                ui.separator();
                if icon_toggle(ui, Icon::Visibility, self.show_cut, "Show / hide cut outlines") {
                    self.show_cut = !self.show_cut;
                }
                if icon_toggle(ui, Icon::Grid, self.grid, "Show / hide page grid") {
                    self.grid = !self.grid;
                }
                if icon_button(ui, Icon::Fit, "Fit page (Ctrl + wheel to zoom)") {
                    self.camera.fit(ctx);
                }
                });
            });
        });
    }
    fn transform_selected_images(&mut self, transform: impl Fn(sheet::Placement) -> sheet::Placement) {
        let Ok(items) = self.layout.editor_items() else {
            return;
        };
        let page = [self.layout.page_width, self.layout.page_height];
        let mut updates = Vec::new();
        for &index in &self.selection {
            let Some(original) = items.get(index).copied()
                .filter(|_| self.movable_image(index))
            else {
                continue;
            };
            let mut item = transform(original);
            if item == original {
                continue;
            }
            if item.width > page[0] + 1e-8 || item.height > page[1] + 1e-8 {
                self.status = "The rotated image would be larger than the page.".into();
                return;
            }
            item = item.moved_on(0., 0., 0., page);
            updates.push((index, item));
        }
        if !updates.is_empty() {
            let before = self.layout.custom.clone();
            self.layout.custom = items.into_iter().enumerate().collect();
            self.layout.custom.extend(updates);
            self.record_object_edit(before);
        }
    }

    pub(crate) fn tool_options_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tool-options")
            .frame(crate::ui_theme::compact_bar(ctx))
            .show(ctx, |ui| {
            crate::ui_theme::compact_spacing(ui);
            ui.horizontal_wrapped(|ui| {
                if self.crop_edit.is_some() {
                    crate::ui_theme::tool_indicator(ui, Icon::Crop, "Crop");
                    if icon_button(ui, Icon::Check, "Apply crop (Enter)") { self.apply_crop(ctx); }
                    if icon_button(ui, Icon::Refresh, "Reset crop") {
                        if let Some((_, rect)) = &mut self.crop_edit { *rect = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)); }
                    }
                    if icon_button(ui, Icon::Close, "Cancel crop (Esc)") { self.crop_edit = None; }
                    return;
                }
                let draft = self.cut_draft.as_ref().filter(|_| !self.show_back);
                let tool = draft.map(|draft| draft.tool);
                let anchors = draft.map_or(0, |draft| draft.anchors.len());
                let (icon, label) = match tool {
                    Some(Tool::Pen) => (Icon::Pen, "Pen"),
                    Some(Tool::Line) => (Icon::Line, "Line"),
                    Some(Tool::Ellipse) => (Icon::Ellipse, "Ellipse"),
                    Some(Tool::Rectangle) => (Icon::Rectangle, "Rectangle"),
                    None => (Icon::Select, "Select"),
                };
                crate::ui_theme::tool_indicator(ui, icon, label);
                ui.separator();
                if !self.show_back && tool.is_none() {
                    self.shape_selection.retain(|target|match target {crate::shape_ops::Target::Design(i)=>*i<self.layout.design_shapes.len(),crate::shape_ops::Target::Cut(i)=>*i<self.layout.manual_cut_paths.len()});
                    for operation in [crate::shape_ops::Operation::Weld,crate::shape_ops::Operation::Trim,crate::shape_ops::Operation::Intersect] {
                        let clicked=ui.add_enabled_ui(self.shape_selection.len()>=2,|ui|crate::shape_ops::button(ui,operation)).inner;
                        if clicked {self.apply_shape_operation(operation);}
                    }
                    if self.shape_selection.len()>1 {
                        for (icon,angle) in [(Icon::RotateLeft,-90.),(Icon::RotateRight,90.)] {if icon_button(ui,icon,"Rotate selected objects 90 degrees") {self.transform_shape_group(angle,false,false);}}
                        if icon_button(ui,Icon::FlipHorizontal,"Flip selected objects horizontally") {self.transform_shape_group(0.,true,false);}
                        if icon_button(ui,Icon::FlipVertical,"Flip selected objects vertically") {self.transform_shape_group(0.,false,true);}
                        ui.label("Rotate by");ui.add(egui::DragValue::new(&mut self.shape_angle).range(-360. ..=360.).suffix("°").update_while_editing(false));
                        if icon_button(ui,Icon::Check,"Apply rotation to selected objects") {self.transform_shape_group(self.shape_angle,false,false);self.shape_angle=0.;}
                    }
                    ui.separator();
                }
                if tool.is_some() || self.selected_design.is_some() {
                    let before=self.selected_design.and_then(|i|self.layout.design_shapes.get(i).cloned());
                    let mut changed=false;
                    for (mode,label) in [(1,"Design"),(0,"Cutline"),(2,"Print + Cut")] {
                        changed|=ui.selectable_value(&mut self.drawing_mode,mode,label).changed();
                    }
                    if self.drawing_mode!=0 {
                        ui.separator();
                        if tool!=Some(Tool::Line) && before.as_ref().is_none_or(|shape|shape.path.first()==shape.path.last()) {
                            changed|=ui.checkbox(&mut self.design_fill_enabled,"Fill").changed();
                            if self.design_fill_enabled {changed|=crate::color_dialog::button(ui, "Fill color", &mut self.design_fill).changed();}
                        }
                        ui.label("Stroke");changed|=crate::color_dialog::button(ui, "Stroke color", &mut self.design_stroke).changed();
                        changed|=ui.add(self.unit.distance(&mut self.design_stroke_width).range(0. ..=20.).speed(0.05)).changed();
                    }
                    if changed && let Some(index)=self.selected_design && let Some(mut shape)=before {
                        self.record_layer_edit(ObjectEdit::Designs(self.layout.design_shapes.clone()));
                        shape.fill=if self.design_fill_enabled && shape.path.first()==shape.path.last(){Some(self.design_fill.to_array())}else{None};
                        shape.stroke=self.design_stroke.to_array();shape.stroke_width=self.design_stroke_width;
                        shape.print=self.drawing_mode!=0;shape.cut=self.drawing_mode!=1;
                        self.layout.design_shapes[index]=shape;
                    }
                    if let Some(index)=self.selected_design {
                        if icon_button(ui,Icon::Duplicate,"Duplicate design shape") && self.layout.design_shapes.len()<100 {
                            self.record_layer_edit(ObjectEdit::Designs(self.layout.design_shapes.clone()));
                            let mut copy=self.layout.design_shapes[index].clone();
                            let [_,_,right,bottom]=cut_tools::bounds(&copy.path);
                            let dx=5_f64.min((self.layout.page_width-right).max(0.));
                            let dy=5_f64.min((self.layout.page_height-bottom).max(0.));
                            for p in &mut copy.path {p[0]+=dx;p[1]+=dy;}
                            self.layout.design_shapes.push(copy);self.selected_design=Some(self.layout.design_shapes.len()-1);
                        }
                        for (icon,clockwise) in [(Icon::RotateLeft,false),(Icon::RotateRight,true)] {
                            if icon_button(ui,icon,"Rotate design 90 degrees") {
                                let original=self.layout.design_shapes[index].path.clone();
                                let [x0,y0,x1,y1]=cut_tools::bounds(&original);let center=[(x0+x1)/2.,(y0+y1)/2.];
                                let rotated:Vec<_>=original.iter().map(|p| {
                                    let dx=p[0]-center[0];let dy=p[1]-center[1];
                                    if clockwise {[center[0]-dy,center[1]+dx]} else {[center[0]+dy,center[1]-dx]}
                                }).collect();
                                if rotated.iter().all(|p|(0. ..=self.layout.page_width).contains(&p[0]) && (0. ..=self.layout.page_height).contains(&p[1])) {
                                    self.record_layer_edit(ObjectEdit::Designs(self.layout.design_shapes.clone()));
                                    self.layout.design_shapes[index].path=rotated;
                                } else {self.status="Rotated design would be outside the page.".into();}
                            }
                        }
                    }
                    if self.selected_design.is_some() && icon_button(ui,Icon::Delete,"Delete design shape") {
                        self.record_layer_edit(ObjectEdit::Designs(self.layout.design_shapes.clone()));
                        self.layout.design_shapes.remove(self.selected_design.take().unwrap());
                    }
                }
                if tool.is_none() {
                    ui.checkbox(&mut self.snap_guides, "Snap")
                        .on_hover_text("Snap to guides and margins");
                }
                if self.show_back {
                    return;
                }
                match tool {
                    Some(Tool::Pen) => {
                        if ui.add_enabled_ui(anchors >= 2, |ui| {
                            icon_button(ui, Icon::OpenPath, "Finish open path")
                        }).inner {
                            self.finish_manual_cut(false);
                        }
                        if ui.add_enabled_ui(anchors >= 3, |ui| {
                            icon_button(ui, Icon::ClosedPath, "Close path")
                        }).inner {
                            self.finish_manual_cut(true);
                        }
                    }
                    Some(Tool::Line) => {ui.small("Drag to draw · Shift: 45° steps · Esc: cancel");}
                    Some(Tool::Ellipse) => {
                        ui.small("Shift for circle");
                    }
                    Some(Tool::Rectangle) => {
                        ui.label("Corner radius");
                        let mut radius = self.cut_corner_radius / self.unit.factor();
                        if crate::ui_theme::spin_box(ui, &mut radius, 0.1 / self.unit.factor(),
                            0. ..=100. / self.unit.factor(), self.unit.label()).changed() {
                            self.cut_corner_radius = radius * self.unit.factor();
                        }
                    }
                    None => {
                        ui.checkbox(&mut self.lock, "Lock ratio")
                            .on_hover_text("Keep proportions when resizing");
                        if !self.selection.is_empty() {
                            ui.separator();
                            if let Some(index) = self.selected
                                && let Ok(items) = self.layout.editor_items()
                                && let Some(item) = items.get(index)
                            {
                                let mut angle = item.orientation.degrees();
                                if ui.add(egui::DragValue::new(&mut angle)
                                    .speed(0.5)
                                    .max_decimals(3)
                                    .update_while_editing(false)
                                    .suffix("°"))
                                    .on_hover_text("Rotation angle. Click to enter degrees.")
                                    .changed()
                                {
                                    self.transform_selected_images(|item| item.with_rotation(angle));
                                }
                            }
                            for (icon, label, clockwise) in [
                                (Icon::RotateLeft, "Rotate 90° left", false),
                                (Icon::RotateRight, "Rotate 90° right", true),
                            ] {
                                if icon_button(ui, icon, label) {
                                    self.transform_selected_images(|item| item.turned_quarter(clockwise));
                                }
                            }
                            for (icon, label, horizontal) in [
                                (Icon::FlipHorizontal, "Flip horizontally", true),
                                (Icon::FlipVertical, "Flip vertically", false),
                            ] {
                                if icon_button(ui, icon, label) {
                                    self.transform_selected_images(|mut item| {
                                        item.orientation.flip(horizontal);
                                        item
                                    });
                                }
                            }
                        }
                        if ui.add_enabled_ui(self.selected_cut_shape.is_some(), |ui| {
                            icon_button(ui, Icon::Delete, "Delete selected cut shape")
                        }).inner {
                            self.remove_selected_cut_shape();
                        }
                    }
                }
                if tool.is_some() && icon_button(ui, Icon::Close, "Cancel drawing") {
                    self.cut_draft = None;
                }
            });
        });
    }
    fn back_cut_paths(&mut self) -> std::sync::Arc<Vec<Vec<[f64;2]>>> {
        if let Some((layout,digest,paths))=&self.back_paths_cache {
            if layout==&self.layout && *digest==self.source_digest {return paths.clone();}
        }
        let paths=if self.layout.use_manual_cut_paths {self.layout.manual_page_cut_paths().unwrap_or_default()}
            else {self.art.as_ref().and_then(|a|self.layout.page_cut_paths_cached(a,&mut self.outline_cache).ok()).unwrap_or_default()};
        let paths=std::sync::Arc::new(paths);
        self.back_paths_cache=Some((self.layout.clone(),self.source_digest,paths.clone()));
        paths
    }
    fn import_back_image(&mut self) {
        let paths = self.back_cut_paths();
        self.back.import([self.layout.page_width,self.layout.page_height], &paths);
    }
    pub(crate) fn side_tabs(&mut self, ui: &mut egui::Ui) {
        let previous_side = self.show_back;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(184., 28.), egui::Sense::hover());
        ui.painter().rect(rect, 14., egui::Color32::from_rgb(230, 234, 239),
            egui::Stroke::new(1_f32, egui::Color32::from_rgb(194, 202, 212)), egui::StrokeKind::Inside);
        for (back, label, hint) in [(false,"Main Page","Front - Print & Cut"),(true,"Back Page","Back - Artwork")] {
            let half = egui::Rect::from_min_size(rect.min + egui::vec2(if back {92.} else {0.}, 0.), egui::vec2(92.,28.));
            let hit = ui.interact(half, response.id.with(back), egui::Sense::click())
                .on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand);
            let active = self.show_back == back;
            if active || hit.hovered() {
                ui.painter().rect_filled(half.shrink(2.),12.,if active {crate::ui_theme::ACCENT} else {egui::Color32::from_rgb(215,228,241)});
            }
            ui.painter().text(half.center(),egui::Align2::CENTER_CENTER,label,egui::FontId::proportional(12.),
                if active {egui::Color32::WHITE} else {ui.visuals().text_color()});
            hit.widget_info(||egui::WidgetInfo::selected(egui::WidgetType::RadioButton,ui.is_enabled(),active,label));
            if hit.clicked() {self.show_back = back;}
        }
        if previous_side != self.show_back {
            self.print_back = self.show_back;
            ui.ctx().request_repaint();
        }
    }
    pub fn ui(&mut self, ctx: &egui::Context) {
        // Bottom panels reserve space from the outside in.
        self.document_status_bar(ctx);
        self.pages_bar(ctx);
        if self.io_job.is_some() {self.project_io_progress(ctx);return;}
        #[cfg(windows)]
        self.print_panel.poll_job(ctx, &mut self.status);
        if self.show_back {
            if !self.show_print_dialog && self.pending_action.is_none() && !self.show_settings
                && !ctx.wants_keyboard_input() && ctx.input(|input| input.events.iter().any(|event| {
                    matches!(event, egui::Event::Key { key: egui::Key::V, pressed: false, modifiers, .. }
                        if modifiers.ctrl && !modifiers.alt && !modifiers.shift)
                })) { self.paste_image(ctx); }
            if !self.show_print_dialog && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL,egui::Key::S)) { self.save_project(false); }
            if !self.show_print_dialog && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL,egui::Key::O)) { self.import_back_image(); }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL,egui::Key::P)) { self.print_back=true; self.show_print_dialog=true; }
            self.handle_dropped_artwork(ctx);
            let paths=self.back_cut_paths();
            self.back.ui(ctx, &self.layout, &paths, self.unit, self.show_margin_guides, self.snap_guides);
            if self.back.paste_requested { self.back.paste_requested=false; self.paste_image(ctx); }
            if self.back.print_requested {self.back.print_requested=false;self.print_back=true;self.show_print_dialog=true;}
            self.settings_dialog(ctx);
            self.print_dialog(ctx);
            return;
        }

        #[cfg(windows)]
        self.print_panel.poll_job(ctx, &mut self.status);
        if !self.show_print_dialog
            && self.page_size_editor.is_none()
            && self.pending_action.is_none()
            && self.autosave.offered.is_empty()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::P))
        {
            self.show_print_dialog = true;
        }
        if !self.show_print_dialog
            && self.page_size_editor.is_none()
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
        let group_count = self.layout.image_groups.len();
        self.layout.image_groups.retain(|i, _| *i < self.layout.count);
        if self.layout.image_groups.len() != group_count {
            remove_singleton_groups(&mut self.layout.image_groups);
        }
        self.sync_selection();
        if !self.show_print_dialog
            && self.page_size_editor.is_none()
            && !ctx.wants_keyboard_input()
            && self.pending_action.is_none()
        {
            if !self.show_new && !self.show_settings && self.drag.is_none() {
                if ctx.input(|i|i.events.iter().any(|e|matches!(e,egui::Event::Copy)))
                    || ctx.input_mut(|i|i.consume_key(egui::Modifiers::CTRL,egui::Key::C)) {
                    self.copy_selected_images();
                }
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
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::G)) {
                    self.group_selection(true);
                } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::G)) {
                    self.group_selection(false);
                }
                if self.guide_drag.is_none()
                    && self.cut_draft.is_none()
                    && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
                    && !self.delete_selected_guide()
                    && !self.remove_selected_cut_shape()
                {
                    if let Some(index)=self.selected_design.take() {
                        self.record_layer_edit(ObjectEdit::Designs(self.layout.design_shapes.clone()));
                        self.layout.design_shapes.remove(index);
                    } else {self.delete_selected_images(ctx);}
                }
            }
            if !self.show_new
                && !self.show_settings
                && (!self.selection.is_empty() || !self.shape_selection.is_empty() || self.selected_design.is_some() || self.selected_cut_shape.is_some())
                && !ctx.memory(|m|m.any_popup_open() || m.top_modal_layer().is_some())
                && self.drag.is_none()
                && !self.camera.is_panning()
            {
                let delta = ctx.input_mut(|i| {
                    let mods = i.modifiers;
                    let keymods = mods;
                    let step = self.nudge_step(mods);
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
                    self.nudge_canvas_selection(delta[0], delta[1]);
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
                    self.layout.manual_cut_paths = drag.before;
                }
                if let Some(drag) = self.drag.take() {
                    self.layout.custom = drag.before;
                }
                self.clear_selection();
            }
        }

        self.handle_dropped_artwork(ctx);

        if self.show_guide {
            egui::Window::new("Print & Cut workflow").open(&mut self.show_guide).collapsible(false).resizable(false).show(ctx,|ui|{
                ui.label("1. Import an image and choose frame / object / transparent outline.");
                ui.label("2. Set dimensions, copies, spacing and cut border in Properties.");
                ui.label("3. Enable CCD and use the top-left dot as origin.");
                ui.label("4. Open Print / Export; match the document paper size at Actual size (100%).");
                ui.label("5. Export the matching PLT and copy it directly to your USB stick.");
                ui.small("Click an image to select; drag to move; drag a corner to resize. Keep proportions controls aspect ratio. Ctrl+Z / Ctrl+Y undo / redo. Zoom, rulers, grid and palette colours affect the screen only. Force / speed are set on the cutter.");
            });
        }
        egui::SidePanel::left("toolbox")
            .exact_width(48.)
            .resizable(false)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if icon_toggle(ui, Icon::Select, self.pick_cut_shapes, "Select images and cut shapes") {
                        self.crop_edit = None;
                        self.activate_cut_picker();
                        ctx.request_repaint();
                    }
                    let active = self.cut_draft.as_ref().map(|draft| draft.tool);
                    for (tool, icon, name) in [
                        (Tool::Pen, Icon::Pen, "Pen cut tool"),
                        (Tool::Line, Icon::Line, "Line tool — Design / Cutline"),
                        (Tool::Ellipse, Icon::Ellipse, "Circle / ellipse cut tool"),
                        (Tool::Rectangle, Icon::Rectangle, "Rectangle cut tool"),
                    ] {
                        if icon_toggle(ui, icon, active == Some(tool), name) {
                            self.activate_cut_tool(tool);
                            ctx.request_repaint();
                        }
                    }
                    if ui.add_enabled_ui(self.selection.len() == 1 && (!self.layout.use_manual_cut_paths || self.layout.manual_cut_paths.is_empty()), |ui| {
                        icon_button(ui, Icon::Crop, "Crop selected image")
                    }).inner {
                        if let Some(index) = self.selected {
                            self.cut_draft = None;
                            self.pick_cut_shapes = false;
                            self.crop_edit = Some((index, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.))));
                        }
                    }
                });
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
                let origin_offset = egui::vec2(14., 12.);
                let pan_gesture = self.camera.update(
                    ui,
                    viewport_rect,
                    origin_offset,
                    &mut self.zoom,
                    &mut self.canvas_pan,
                    self.drag.is_none() && self.guide_drag.is_none() && self.cut_shape_drag.is_none() && self.shape_group_drag.is_none()
                        && !self.cut_draft.as_ref().is_some_and(|draft|draft.drag_start.is_some()),
                );
                let canvas = ui.scope(|ui| {
                        let (area, canvas_response) =
                            ui.allocate_exact_size(viewport, egui::Sense::click_and_drag());
                        let painter = ui.painter_at(area);

                        crate::ui_theme::paint_workspace(&painter, area);

                        let scale = ((viewport.x - 90.) / self.layout.page_width as f32)
                            .min((viewport.y - 80.) / self.layout.page_height as f32)
                            .max(0.01)
                            * self.zoom;
                        let page = egui::Rect::from_center_size(
                            crate::canvas_camera::CanvasCamera::origin(area, self.zoom, origin_offset) + self.canvas_pan,
                            egui::vec2(
                                self.layout.page_width as f32 * scale,
                                self.layout.page_height as f32 * scale,
                            ),
                        );
                        self.navigator_view = Some((page, viewport_rect));
                        painter.rect_filled(
                            page.translate(egui::vec2(2., 3.)),
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
                            let [[left, right], [top, bottom]] = self.layout.margin_lines();
                            let guide = egui::Rect::from_min_max(
                                page.min + egui::vec2(left as f32, top as f32) * scale,
                                page.min + egui::vec2(right as f32, bottom as f32) * scale,
                            );
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
                        let pos = |x: f64, y: f64| {
                            page.min + egui::vec2(x as f32 * scale, y as f32 * scale)
                        };

                        let view = viewport_rect;
                        let design_gesture=self.shape_group_canvas(ui,&canvas_response,page,scale,pan_gesture)
                            || self.design_canvas(ui,&canvas_response,page,scale,pan_gesture);
                        if self.cut_draft.is_none() && self.crop_edit.is_none() && !design_gesture {
                            let guide_gesture =
                                self.update_ruler_guides(ui, view, page, scale, pan_gesture);
                            if !pan_gesture
                                && !guide_gesture
                                && (self.marquee.is_some() || !self.cut_picker_owns_gesture(ui.ctx(), page, scale))
                            {
                                self.canvas_edit(ui, &canvas_response, page, scale);
                            }
                        }
                        match self.layout.editor_items() {
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
                                        let needs_vector=r.intersects(painter.clip_rect()) && (r.width()*ui.ctx().pixels_per_point()>uv.width()*a.pixels.width() as f32 || r.height()*ui.ctx().pixels_per_point()>uv.height()*a.pixels.height() as f32);
                                        if let Some(asset)=needs_vector.then(||self.layout.svg_source_for(i).cloned()).flatten() {
                                            let mut svg_layout=self.layout.clone();svg_layout.svg_sources.clear();svg_layout.svg_sources.insert(i,asset);svg_layout.design_shapes.clear();
                                            svg_layout.hidden_images=(0..svg_layout.count).filter(|index|*index!=i).collect();
                                            let preview=self.svg_previews.entry(i).or_default();
                                            if !preview.has_base() {paint_oriented_image(&painter,texture.id(),r,uv,p.orientation,egui::Color32::from_white_alpha(self.layout.opacity_for(i)));}
                                            if let Err(error)=preview.paint(ui.ctx(),&painter,page,&svg_layout) {self.status=error;}
                                        }else{paint_oriented_image(&painter, texture.id(), r, uv, p.orientation, egui::Color32::from_white_alpha(self.layout.opacity_for(i)));}
                                    }

                                    if self.show_cut && self.layout.items().is_ok() && (!self.layout.use_manual_cut_paths || self.layout.has_manual_cuts()) {
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

                        self.paint_designs(ui.ctx(),&painter,page);
                        self.paint_design_selection(&painter,page,scale);
                        if self.ccd_trial && self.layout.has_visible_images() {
                            let standalone=if self.art.is_none(){self.layout.design_only_artwork().ok()}else{None};
                            let mark_layout=standalone.as_ref().map_or(&self.layout,|(layout,_)|layout);
                            for p in self.ccd_profile.marks(mark_layout) {
                                painter.circle_filled(
                                    pos(p[0], p[1]),
                                    self.ccd_profile.diameter as f32 / 2. * scale,
                                    egui::Color32::BLACK,
                                );
                            }
                        } else if !self.ccd_trial && self.show_manual_marks {
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

                        if self.crop_edit.is_some() {
                            self.crop_canvas(ui.ctx(), &painter, page, scale, pan_gesture);
                        } else {
                        self.cut_shape_canvas(
                            &canvas_response,
                            &painter,
                            page,
                            view,
                            scale,
                            pan_gesture || design_gesture,
                        );
                        if self.crop_edit.is_none() { self.draw_selection(&painter, page, scale); }
                        self.manual_cut_canvas(
                            &canvas_response,
                            &painter,
                            page,
                            view,
                            scale,
                            pan_gesture,
                        );
                        }
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
        self.vector_trace_dialog(ctx);
        self.layout_overflow_notice(ctx);
        self.print_dialog(ctx);
        if self.pending_action.is_none() {
            self.page_size_dialog(ctx);
        }
    }
}

#[cfg(test)]
mod selection_tests {
    #[test]
    fn asymmetric_margins_snap_image_edges_without_ruler_guides() {
        let mut app = super::A4Studio::default();
        app.layout.page_margins = Some([12., 23., 28., 33.]);
        let members = [(0, sheet::Placement { x: 15., y: 31., width: 20., height: 30., orientation: Default::default() })];
        assert_eq!(app.snap_move(&members, (-2., -2.), 6., None), (-3., -3.));
        assert_eq!(app.snap_move(&members, (151., 202.), 6., None), (152., 203.));
        assert_eq!(app.snap_move(&members, (-2., 0.), 6., Some(0)), (-3., 0.));
        app.snap_guides = false;
        assert_eq!(app.snap_move(&members, (-2., -2.), 6., None), (-2., -2.));
    }

    #[test]
    fn close_requested_while_opening_is_handled_after_loading() {
        let ctx=egui::Context::default();let path=project_test_path("async-close-open");
        let mut fixture=A4Studio::fresh_tab();fixture.save_project_to(path.clone()).unwrap();
        let mut app=A4Studio::fresh_tab();assert!(app.open_tab_file(path.clone(),&ctx));
        app.request_tab_close(&ctx);assert!(!app.tab_closed);assert!(app.io_close_requested);
        app.wait_project_io(&ctx);assert!(app.tab_closed);assert!(!app.io_close_requested);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn background_open_commits_prepared_images_and_failure_preserves_document() {
        let ctx=egui::Context::default();let path=project_test_path("async-open");
        let mut saved=A4Studio::fresh_tab();saved.source=include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();saved.retrace(&ctx);
        saved.back.data.images.push(crate::duplex::tests::image_item());saved.save_project_to(path.clone()).unwrap();
        let mut app=A4Studio::fresh_tab();app.layout.width=31.;
        assert!(app.open_tab_file(path.clone(),&ctx));assert!(app.project_io_busy());assert_eq!(app.layout.width,31.);
        let _=ctx.run(Default::default(),|ctx|app.project_io_progress(ctx));
        app.wait_project_io(&ctx);assert!(app.art.is_some());assert!(app.texture.is_some());assert_eq!(app.back.data.images.len(),1);assert!(!app.is_dirty());
        let before=app.document_signature();
        let bad=project_test_path("async-bad");std::fs::write(&bad,b"invalid project").unwrap();
        assert!(app.open_tab_file(bad.clone(),&ctx));app.wait_project_io(&ctx);
        assert_eq!(before,app.document_signature());assert_eq!(app.project_path,Some(path.clone()));assert!(app.status.contains("Cannot open"));
        std::fs::remove_file(path).unwrap();std::fs::remove_file(bad).unwrap();
    }
    #[test]
    fn background_save_waits_before_close_and_failed_save_keeps_dirty_work() {
        let ctx=egui::Context::default();let path=project_test_path("async-save");
        let mut app=A4Studio::fresh_tab();app.save_project_to(path.clone()).unwrap();app.layout.width=31.;
        app.request_action(DocumentAction::CloseProject,&ctx);
        app.finish_unsaved(UnsavedChoice::Save,&ctx);
        assert!(app.project_io_busy());assert!(!app.tab_closed);assert!(app.is_dirty());
        app.wait_project_io(&ctx);assert!(app.tab_closed);assert_eq!(crate::project::Project::read(&path).unwrap().settings.layout.width,31.);
        let mut app=A4Studio::fresh_tab();app.layout.width=32.;
        app.project_path=Some(project_test_path("missing-parent").join("failure.mc630"));
        app.request_action(DocumentAction::CloseProject,&ctx);app.finish_unsaved(UnsavedChoice::Save,&ctx);
        app.wait_project_io(&ctx);assert!(!app.tab_closed);assert!(app.is_dirty());assert!(app.close_canceled);assert!(app.status.contains("Cannot save"));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn back_contours_are_cached_and_recovery_includes_back_artwork() {
        let ctx=egui::Context::default();let mut app=super::A4Studio::fresh_tab();
        app.source=include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();app.retrace(&ctx);
        let first=app.back_cut_paths();let second=app.back_cut_paths();assert!(std::sync::Arc::ptr_eq(&first,&second));
        app.back.data.images.push(crate::duplex::tests::image_item());
        app.back.data.images[0].rect[0]+=1.;let third=app.back_cut_paths();assert!(std::sync::Arc::ptr_eq(&first,&third));
        app.layout.contour_offset+=0.1;let fourth=app.back_cut_paths();assert!(!std::sync::Arc::ptr_eq(&first,&fourth));
        app.retrace(&ctx);let fifth=app.back_cut_paths();assert!(!std::sync::Arc::ptr_eq(&fourth,&fifth));
        let record=app.autosave_data().record();assert_eq!(record.project.back.images.len(),1);assert_eq!(record.project.back.images[0].rect[0],31.);
    }
    #[test]
    fn duplex_artwork_survives_project_roundtrip_without_changing_front_cut_data() {
        let ctx=egui::Context::default();
        let mut app=super::A4Studio::fresh_tab();
        app.source=include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.retrace(&ctx);
        app.mark_saved();
        let before=app.layout.page_cut_paths(app.art.as_ref().unwrap()).unwrap();
        app.back.data.images.push(crate::duplex::tests::image_item());
        assert!(app.is_dirty());
        let snapshot=app.project_snapshot();
        let json=serde_json::to_value(&snapshot).unwrap();
        let project:crate::project::Project=serde_json::from_value(json.clone()).unwrap();
        let source=project.source().unwrap();
        let mut restored=super::A4Studio::fresh_tab();
        restored.install_project(project,source,&ctx);
        assert_eq!(restored.back.data.images.len(),1);
        assert_eq!(restored.back.data.images[0].rect,[30.,40.,20.,10.]);
        assert_eq!(restored.layout.page_cut_paths(restored.art.as_ref().unwrap()).unwrap(),before);
        let mut legacy=json; legacy.as_object_mut().unwrap().remove("back");
        let legacy:crate::project::Project=serde_json::from_value(legacy).unwrap();
        assert!(legacy.back.images.is_empty());
        assert!(legacy.source().is_ok());
    }

    #[test]
    fn custom_page_size_preserves_layout_exports_and_project_roundtrip() {
        let ctx = egui::Context::default();
        let mut app = super::A4Studio::default();
        app.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.retrace(&ctx);
        let before = app.layout.clone();
        let items = before.items().unwrap();
        let paths = before.page_cut_paths(app.art.as_ref().unwrap()).unwrap();
        app.apply_page_size(500., 400.).unwrap();
        assert_eq!(app.layout.items().unwrap(), items);
        assert_eq!(
            app.layout
                .page_cut_paths(app.art.as_ref().unwrap())
                .unwrap(),
            paths
        );
        let art = app.art.as_ref().unwrap();
        assert!(
            app.layout
                .print_html(art)
                .unwrap()
                .contains("size:500mm 400mm")
        );
        assert!(
            app.cut_file_data()
                .unwrap()
                .starts_with("TB25,20000,16000;")
        );
        let project = app.project_snapshot();
        let source = project.source().unwrap();
        let mut restored = super::A4Studio::default();
        restored.install_project(project, source, &ctx);
        assert_eq!(restored.layout.page_width, 500.);
        assert_eq!(restored.layout.page_height, 400.);
        assert_eq!(restored.layout.items().unwrap(), items);
        app.undo_object(&ctx);
        assert!(app.layout == before);
        app.redo_object(&ctx);
        assert_eq!(app.layout.page_width, 500.);
    }

    #[test]
    fn page_resize_rejects_clipping_without_changing_the_document() {
        let ctx = egui::Context::default();
        let mut app = super::A4Studio::default();
        app.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.retrace(&ctx);
        let before = app.layout.clone();
        assert!(app.apply_page_size(100., 100.).is_err());
        assert!(app.layout == before);
        assert!(app.apply_page_size(f64::NAN, 300.).is_err());
        app.art = None;
        app.layout.guides = [vec![150.], vec![]];
        assert!(app.apply_page_size(100., 200.).is_err());
        app.layout.guides = Default::default();
        app.layout.manual_cut_paths = vec![vec![[20., 20.], [150., 20.]]];
        assert!(app.apply_page_size(100., 200.).is_err());
        app.layout.manual_cut_paths.clear();
        app.apply_page_size(50., 50.).unwrap();
        assert_eq!(app.layout.page_width, 50.);
        assert!(app.apply_page_size(631., 200.).is_err());
    }

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
        app.layout.image_cut_paths.clear();
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
            before: app.layout.manual_cut_paths.clone(),
            duplicate: false,
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
                orientation: Default::default(),
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
                    orientation: Default::default(),
                },
            ),
            (
                1,
                sheet::Placement {
                    x: 50.,
                    y: 40.,
                    width: 30.,
                    height: 20.,
                    orientation: Default::default(),
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
        app.wait_project_io(&ctx);
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
        app.wait_project_io(&ctx);
        assert!(app.startup_pending);
        assert!(app.project_path.is_none());
        app.restore_recovery(&ctx).unwrap();
        app.reopen_on_startup(&ctx);
        app.wait_project_io(&ctx);
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
        app.wait_project_io(&ctx);
        assert!(app.project_path.is_none());
        app.startup_pending = true;
        app.reopen_last_project = true;
        assert!(!app.is_dirty());
        app.reopen_on_startup(&ctx);
        app.wait_project_io(&ctx);
        assert_eq!(
            std::fs::canonicalize(app.project_path.as_ref().unwrap()).unwrap(),
            std::fs::canonicalize(&path).unwrap()
        );
        assert_eq!(app.layout.width, 31.);
        assert!(app.reopen_last_project);
        assert!(!app.is_dirty());
        app.layout.width = 32.;
        app.reopen_on_startup(&ctx);
        app.wait_project_io(&ctx);
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
        app.wait_project_io(&ctx);
        assert!(app.project_path.is_none());
        assert_eq!(app.autosave.offered.len(), 1);
        app.restore_recovery(&ctx).unwrap();
        app.reopen_on_startup(&ctx);
        app.wait_project_io(&ctx);
        assert_eq!(app.layout.width, 31.);
        app.autosave.clear();
        drop(app);
        std::fs::remove_dir(root).unwrap();
        std::fs::remove_file(&path).unwrap();
        let mut missing = A4Studio::default();
        missing.reopen_last_project = true;
        missing.recent_projects.remember(&path).unwrap();
        missing.reopen_on_startup(&ctx);
        missing.wait_project_io(&ctx);
        assert!(missing.project_path.is_none());
        assert!(!missing.is_dirty());
        assert!(missing.status.contains("Cannot open project"));
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
                orientation: Default::default(),
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
        app.wait_project_io(&ctx);
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
        app.wait_project_io(&ctx);
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
                orientation: Default::default(),
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
        assert_eq!(restored.zoom, 1.);
        assert_eq!(restored.canvas_pan, egui::Vec2::ZERO);
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
                    orientation: Default::default(),
                },
            ),
            (
                1,
                sheet::Placement {
                    x: 53.,
                    y: 15.,
                    width: 35.,
                    height: 35.,
                    orientation: Default::default(),
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
                        orientation: Default::default(),
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

include!("workspace_ui.rs");
include!("settings_ui.rs");

include!("project_ui.rs");
include!("pages_ui.rs");
#[cfg(test)]
#[path="nudge_tests.rs"]
mod nudge_tests;

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
            .default_width(300.)
            .width_range(265. ..=450.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.show_navigator, true, "Navigator");
                    ui.selectable_value(&mut self.show_navigator, false, "Properties");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::ui_theme::close_icon(ui).on_hover_text("Hide right dock").clicked() {
                            self.show_properties = false;
                        }
                    });
                });
                ui.separator();
                if self.show_navigator {
                    self.navigator(ui);
                    ui.separator();
                    ui.strong("Cut line color");
                    ui.horizontal_wrapped(|ui| {
                        for (name, color) in crate::ui_theme::CUT_COLORS {
                            let selected = self.contour_color == color;
                            let (rect, response) = ui.allocate_exact_size(egui::vec2(21., 21.), egui::Sense::click());
                            response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name));
                            ui.painter().rect(
                                rect,
                                3.,
                                color,
                                egui::Stroke::new(1_f32, egui::Color32::from_gray(160)),
                                egui::StrokeKind::Inside,
                            );
                            if selected || response.has_focus() {
                                ui.painter().rect_stroke(rect.shrink(2.), 2., egui::Stroke::new(1.5_f32, egui::Color32::WHITE), egui::StrokeKind::Inside);
                            }
                            if response.on_hover_text(format!("{name} cut line")).clicked() {
                                self.contour_color = color;
                            }
                        }
                    });
                } else {
                    let active = self.section_palettes.iter().position(|section| section.visible).unwrap_or(0);
                    ui.horizontal(|ui| {
                        for (index, (icon, label)) in [
                            (Icon::Artwork, "Artwork"),
                            (Icon::Settings, "Size and Repeat"),
                            (Icon::Outline, "Cut Outline"),
                            (Icon::Registration, "Registration"),
                        ].into_iter().enumerate() {
                            if icon_toggle(ui, icon, index == active, label) {
                                self.open_sidebar_section(index);
                            }
                        }
                    });
                    let height = (ui.available_height() * 0.64).max(0.);
                    egui::ScrollArea::vertical()
                        .id_salt(("properties-scroll", active))
                        .max_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| match active {
                            0 => self.artwork_dialog_content(ui, ctx),
                            1 => self.repeat_dialog_content(ui),
                            2 => self.outline_dialog_content(ui),
                            _ => self.registration_dialog_content(ui),
                        });
                }
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
        let mut cut_clicked=None;
        for index in (0..self.layout.manual_cut_paths.len()).rev() {
            if ui.selectable_label(self.selected_cut_shape==Some(index),format!("Manual cutline {}",index+1)).on_hover_text("Select this cutline, then click Link on its image layer").clicked() {cut_clicked=Some(index);}
        }
        if let Some(index)=cut_clicked {
            self.selected_cut_shape=Some(index);self.selected_design=None;self.clear_selection();self.cut_draft=None;self.pick_cut_shapes=true;
            self.select_shape(crate::shape_ops::Target::Cut(index),ui.input(|i|i.modifiers.shift));
        }
        let mut design_clicked=None;
        for index in (0..self.layout.design_shapes.len()).rev() {
            let shape=&self.layout.design_shapes[index];
            let mode=if shape.print && shape.cut {"Print + Cut"}else if shape.print {"Design"}else{"Cutline"};
            if ui.selectable_label(self.selected_design==Some(index),format!("Shape {} · {}",index+1,mode)).clicked() {design_clicked=Some(index);}
        }
        if let Some(index)=design_clicked {
            self.select_shape(crate::shape_ops::Target::Design(index),ui.input(|i|i.modifiers.shift));
            let shape=self.layout.design_shapes[index].clone();
            self.selected_design=Some(index);self.clear_selection();self.cut_draft=None;
            self.drawing_mode=if shape.print && shape.cut {2}else if shape.print {1}else{0};
            self.design_fill_enabled=shape.fill.is_some();
            if let Some(c)=shape.fill {self.design_fill=egui::Color32::from_rgba_unmultiplied(c[0],c[1],c[2],c[3]);}
            let c=shape.stroke;self.design_stroke=egui::Color32::from_rgba_unmultiplied(c[0],c[1],c[2],c[3]);self.design_stroke_width=shape.stroke_width;
        }
        if let Some(index)=self.selection.iter().next().copied() {
            let mut percent=self.layout.opacity_for(index) as f32/255.*100.;
            let before_id=ui.id().with("image-opacity-before");
            let response=ui.add(crate::ui_theme::small_slider(egui::Slider::new(&mut percent,0. ..=100.).text("Opacity").suffix("%")));
            if response.changed() {
                let before=self.layout.clone();
                if response.dragged() && ui.data(|data|data.get_temp::<Layout>(before_id)).is_none() {
                    ui.data_mut(|data|data.insert_temp(before_id,before.clone()));
                }
                for selected in &self.selection {self.layout.image_opacity.insert(*selected,(percent*2.55).round() as u8);}
                if !response.dragged() {self.record_layer_edit(ObjectEdit::Layers(Box::new(before)));}
            }
            if response.drag_stopped() {
                if let Some(before)=ui.data_mut(|data|data.remove_temp::<Layout>(before_id)) {
                    self.record_layer_edit(ObjectEdit::Layers(Box::new(before)));
                }
            }
        }
        let (Some(art), Some(texture)) = (&self.art, &self.texture) else {
            if self.layout.design_shapes.is_empty() {ui.weak("Import an image or draw a shape.");}
            return;
        };
        let Ok(items) = self.layout.editor_items() else {
            ui.weak("Adjust the layout to show its images.");
            return;
        };
        let mut clicked = None;
        let mut visibility_clicked = None;
        let mut link_clicked=None;let mut image_cut_clicked=None;
        egui::ScrollArea::vertical()
            .id_salt("image-layers")
            .auto_shrink([false, false])
            .show_rows(ui, 66., items.len(), |ui, rows| {
                for row in rows {
                    // The last image is drawn on top on the canvas.
                    let index = items.len() - 1 - row;
                    let item = items[index];
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 66.),
                        egui::Sense::hover(),
                    );
                    let cut_rect=egui::Rect::from_min_max(rect.min+egui::vec2(34.,44.),rect.max);
                    if !self.layout.use_manual_cut_paths || self.layout.image_cut_paths_for(index).is_some_and(|paths|!paths.is_empty()) {
                        let cut=ui.interact(cut_rect,ui.id().with(("image-cutline",index)),egui::Sense::click());
                        ui.painter().text(cut_rect.left_center(),egui::Align2::LEFT_CENTER,if self.layout.use_manual_cut_paths {"Manual cutline"}else{"Auto cutline"},egui::FontId::proportional(12.),crate::ui_theme::ACCENT);
                        if cut.clicked() {image_cut_clicked=Some(index);}
                    }
                    let rect=egui::Rect::from_min_max(rect.min,rect.max-egui::vec2(0.,22.));
                    let eye = egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.left() + 32., rect.bottom()),
                    );
                    let row =
                        egui::Rect::from_min_max(egui::pos2(eye.right(), rect.top()), rect.max-egui::vec2(30.,0.));
                    let response =
                        ui.interact(row, ui.id().with(("layer", index)), egui::Sense::click());
                    let linked=self.layout.image_cut_paths_for(index).is_some_and(|paths|!paths.is_empty());
                    let link_rect=egui::Rect::from_min_max(egui::pos2(rect.right()-29.,rect.top()+8.),egui::pos2(rect.right()-3.,rect.bottom()-8.));
                    let link_response=ui.interact(link_rect,ui.id().with(("layer-cutline-link",index)),egui::Sense::click()).on_hover_text(if linked {"Unlink manual cutline (keep it on the page)"}else{"Link selected manual cutline to this image"});
                    if link_response.clicked() {link_clicked=Some((index,linked));}
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
                    paint_oriented_image(
                        &painter,
                        texture.id(),
                        image_rect,
                        self.image_uv(index, art),
                        item.orientation,
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
                    crate::ui_theme::paint_icon(ui,link_rect,if linked {Icon::Link}else{Icon::Unlink});
                    if response.clicked() {
                        clicked = Some(index);
                    }
                }
            });
        if let Some(index)=image_cut_clicked {self.clear_selection();self.select_target(index,false,false);self.selected_design=None;self.selected_cut_shape=None;self.open_sidebar_section(2);}
        if let Some((index,linked))=link_clicked {self.toggle_layer_cutline_link(index,linked);}
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
        self.show_navigator = false;
        for (i, section) in self.section_palettes.iter_mut().enumerate() {
            section.visible = i == index;
        }
    }

    fn crop_canvas(&mut self, ctx: &egui::Context, painter: &egui::Painter, page: egui::Rect, scale: f32, panning: bool) {
        let Some((index, mut crop)) = self.crop_edit else { return };
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) { self.crop_edit = None; return; }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) && !ctx.wants_keyboard_input() { self.apply_crop(ctx); return; }
        let Ok(items) = self.layout.editor_items() else { return };
        let Some(item) = items.get(index) else { self.crop_edit = None; return };
        let corners = item.orientation.unit_corners();
        let origin = egui::pos2(corners[0][0] as f32, corners[0][1] as f32);
        let ax = egui::vec2((corners[1][0]-corners[0][0]) as f32, (corners[1][1]-corners[0][1]) as f32);
        let ay = egui::vec2((corners[3][0]-corners[0][0]) as f32, (corners[3][1]-corners[0][1]) as f32);
        let size = egui::vec2(item.width as f32, item.height as f32) * scale;
        let base = page.min + egui::vec2(item.x as f32, item.y as f32) * scale;
        let screen = |p: egui::Pos2| base + (origin.to_vec2() + ax * p.x + ay * p.y) * size;
        let source = |p: egui::Pos2| {
            let d = (p-base)/size-origin.to_vec2();
            let det = ax.x*ay.y-ax.y*ay.x;
            egui::pos2((d.x*ay.y-d.y*ay.x)/det, (ax.x*d.y-ax.y*d.x)/det)
        };
        let handles = [crop.left_top(), crop.center_top(), crop.right_top(), crop.right_center(), crop.right_bottom(), crop.center_bottom(), crop.left_bottom(), crop.left_center()];
        let id = egui::Id::new("crop-canvas-drag");
        if !panning && let Some(pointer) = ctx.pointer_interact_pos() {
            let p = source(pointer);
            let handle = handles.iter().position(|h| screen(*h).distance(pointer) < 9.);
            if painter.clip_rect().contains(pointer) && (handle.is_some() || (0. ..=1.).contains(&p.x) && (0. ..=1.).contains(&p.y)) {
                let resize_cursor = |mode: usize| {
                    let direction = match mode {
                        1 | 5 => (ax * size).rot90(),
                        3 | 7 => (ay * size).rot90(),
                        0 | 4 => ax * size + ay * size,
                        _ => ax * size - ay * size,
                    };
                    crop_resize_cursor(direction)
                };
                ctx.set_cursor_icon(if let Some(mode) = handle { resize_cursor(mode) }
                    else if crop.contains(p) { egui::CursorIcon::Move }
                    else { egui::CursorIcon::Crosshair });
                if ctx.input(|i| i.pointer.primary_pressed()) {
                    let mode = handle.map(|n| n as i32).unwrap_or(if crop.contains(p) {8} else {9});
                    ctx.data_mut(|d| d.insert_temp(id, (mode, crop, p)));
                }
            }
            if ctx.input(|i| i.pointer.primary_down()) && let Some((mode, old, start)) = ctx.data(|d| d.get_temp::<(i32, egui::Rect, egui::Pos2)>(id)) {
                // Keep the active drag cursor even after leaving its handle or image.
                ctx.set_cursor_icon(match mode {
                    1 | 5 => crop_resize_cursor((ax * size).rot90()),
                    3 | 7 => crop_resize_cursor((ay * size).rot90()),
                    0 | 4 => crop_resize_cursor(ax * size + ay * size),
                    2 | 6 => crop_resize_cursor(ax * size - ay * size),
                    8 => egui::CursorIcon::Move,
                    _ => egui::CursorIcon::Crosshair,
                });
                let p = egui::pos2(p.x.clamp(0.,1.),p.y.clamp(0.,1.));
                crop = old;
                if mode == 8 {
                    let delta = p-start;
                    crop = old.translate(egui::vec2(delta.x.clamp(-old.left(),1.-old.right()), delta.y.clamp(-old.top(),1.-old.bottom())));
                } else if mode == 9 { crop = egui::Rect::from_two_pos(start,p); }
                else {
                    if [0,6,7].contains(&mode) { crop.min.x = p.x.min(old.right()-0.001); }
                    if [2,3,4].contains(&mode) { crop.max.x = p.x.max(old.left()+0.001); }
                    if [0,1,2].contains(&mode) { crop.min.y = p.y.min(old.bottom()-0.001); }
                    if [4,5,6].contains(&mode) { crop.max.y = p.y.max(old.top()+0.001); }
                }
            }
        }
        if ctx.input(|i| i.pointer.primary_released()) { ctx.data_mut(|d| d.remove::<(i32, egui::Rect, egui::Pos2)>(id)); }
        let polygon = |rect: egui::Rect| vec![screen(rect.left_top()),screen(rect.right_top()),screen(rect.right_bottom()),screen(rect.left_bottom())];
        for rect in [egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,crop.top())),
            egui::Rect::from_min_max(egui::pos2(0.,crop.bottom()),egui::pos2(1.,1.)),
            egui::Rect::from_min_max(egui::pos2(0.,crop.top()),crop.left_bottom()),
            egui::Rect::from_min_max(crop.right_top(),egui::pos2(1.,crop.bottom()))] {
            painter.add(egui::Shape::convex_polygon(polygon(rect),egui::Color32::from_black_alpha(140),egui::Stroke::NONE));
        }
        // Two-tone strokes remain visible over both white margins and dark artwork.
        painter.add(egui::Shape::closed_line(polygon(crop),
            egui::Stroke::new(4_f32, egui::Color32::from_gray(25))));
        painter.add(egui::Shape::closed_line(polygon(crop),
            egui::Stroke::new(2_f32, egui::Color32::from_rgb(64, 170, 255))));
        for fraction in [1./3.,2./3.] {
            let x = crop.left()+crop.width()*fraction;
            let y = crop.top()+crop.height()*fraction;
            for segment in [
                [screen(egui::pos2(x,crop.top())),screen(egui::pos2(x,crop.bottom()))],
                [screen(egui::pos2(crop.left(),y)),screen(egui::pos2(crop.right(),y))],
            ] {
                painter.line_segment(segment, egui::Stroke::new(2_f32, egui::Color32::from_black_alpha(160)));
                painter.line_segment(segment, egui::Stroke::new(1_f32, egui::Color32::from_white_alpha(210)));
            }
        }
        for point in [crop.left_top(),crop.center_top(),crop.right_top(),crop.right_center(),crop.right_bottom(),crop.center_bottom(),crop.left_bottom(),crop.left_center()] {
            let handle = egui::Rect::from_center_size(screen(point), egui::vec2(9.,9.));
            painter.rect(handle, 1., egui::Color32::WHITE,
                egui::Stroke::new(2_f32, crate::ui_theme::ACCENT), egui::StrokeKind::Inside);
        }
        self.crop_edit = Some((index,crop));
    }

    fn apply_crop(&mut self, ctx: &egui::Context) {
        let Some((index, crop)) = self.crop_edit else { return };
        let Some(art) = self.art.as_ref() else { return };
        let Ok(items) = self.layout.editor_items() else { return };
        let Some(item) = items.get(index).copied() else { return };
        let tile = self.layout.image_tiles.get(index).copied().unwrap_or([0, 0, art.pixels.width(), art.pixels.height()]);
        if crop.width() * (tile[2] as f32) < 1. || crop.height() * (tile[3] as f32) < 1. { return; }
        {
            let x = (crop.left() * tile[2] as f32).floor() as u32;
            let y = (crop.top() * tile[3] as f32).floor() as u32;
            let w = ((crop.right() * tile[2] as f32).ceil() as u32).min(tile[2]) - x;
            let h = ((crop.bottom() * tile[3] as f32).ceil() as u32).min(tile[3]) - y;
            let cropped = item.cropped([x as f64 / tile[2] as f64, y as f64 / tile[3] as f64,
                w as f64 / tile[2] as f64, h as f64 / tile[3] as f64]);
            if cropped.width < 1. || cropped.height < 1. { self.status = "Crop must be at least 1 mm.".into(); return; }
            let before = self.layout.clone();
            self.layout.image_cut_paths=(0..self.layout.count).filter_map(|i|
                self.layout.image_cut_paths_for(i).cloned().map(|paths|(i,paths))).collect();
            if self.layout.image_tiles.is_empty() {
                self.layout.image_tiles = vec![tile; self.layout.count];
            }
            self.layout.image_tiles[index] = [tile[0] + x, tile[1] + y, w, h];
            self.layout.custom = items.into_iter().enumerate().collect();
            self.layout.custom.insert(index, cropped);
            if let Some(paths)=self.layout.image_cut_paths.get_mut(&index) {
                for point in paths.iter_mut().flatten() {
                    point[0]=(point[0]-x as f64/tile[2] as f64)/(w as f64/tile[2] as f64);
                    point[1]=(point[1]-y as f64/tile[3] as f64)/(h as f64/tile[3] as f64);
                }
            }
            self.record_layer_edit(ObjectEdit::Layers(Box::new(before)));
            self.retrace(ctx);
            self.crop_edit = None;
            self.status = "Image cropped. Ctrl+Z to undo.".into();
        }
    }

    fn layout_overflow_notice(&mut self,ctx:&egui::Context) {
        let overflow=self.art.is_some() && self.layout.items().is_err() && self.layout.editor_items().is_ok();
        if !overflow {self.overflow_warning_seen=false;self.overflow_warning_open=false;return;}
        if self.overflow_warning_seen {return;}
        if !self.overflow_warning_open && (self.drag.is_some() || self.show_print_dialog || self.show_new || self.show_settings
            || self.pending_action.is_some() || self.page_size_editor.is_some() || self.crop_edit.is_some() || self.cut_draft.is_some() || ctx.input(|i|i.pointer.any_down())) {return;}
        self.overflow_warning_open=true;
        let mut acknowledged=false;
        let dialog=crate::ui_theme::dialog_modal(ctx, "layout-overflow-notice").show(ctx,|ui| {
            ui.set_width(340.);
            acknowledged |= crate::ui_theme::dialog_header(ui,"Images do not fit the page",true);
            ui.label("Your images remain visible. Move or resize them, adjust spacing, or change the page size before printing or cutting.");
            ui.separator();
            crate::ui_theme::dialog_actions(ui,|ui| {acknowledged |= crate::ui_theme::primary_button(ui,"Close").clicked();});
        });
        if acknowledged || dialog.should_close() {self.overflow_warning_seen=true;self.overflow_warning_open=false;}
    }

    fn selected_shape_points(&self)->(Vec<crate::shape_ops::Target>,Vec<usize>,Vec<mc630_plt_studio::Point>) {
        let mut targets=vec![];let mut lengths=vec![];let mut points=vec![];
        for target in &self.shape_selection {
            let path=match target {crate::shape_ops::Target::Design(i)=>self.layout.design_shapes.get(*i).map(|s|&s.path),crate::shape_ops::Target::Cut(i)=>self.layout.manual_cut_paths.get(*i)};
            if let Some(path)=path.filter(|p|!p.is_empty()) {targets.push(*target);lengths.push(path.len());points.extend_from_slice(path);}
        }
        (targets,lengths,points)
    }
    fn set_shape_group_points(&mut self,targets:&[crate::shape_ops::Target],lengths:&[usize],points:&[mc630_plt_studio::Point]) {
        let mut offset=0;
        for (target,&length) in targets.iter().zip(lengths) {
            let path=match target {crate::shape_ops::Target::Design(i)=>self.layout.design_shapes.get_mut(*i).map(|s|&mut s.path),crate::shape_ops::Target::Cut(i)=>self.layout.manual_cut_paths.get_mut(*i)};
            if let Some(path)=path {*path=points[offset..offset+length].to_vec();}offset+=length;
        }
        self.preview_cache=None;self.back_paths_cache=None;self.last_print=None;
    }
    fn shape_group_canvas(&mut self,ui:&egui::Ui,response:&egui::Response,page:egui::Rect,scale:f32,panning:bool)->bool {
        if self.cut_draft.is_some() || self.crop_edit.is_some() || self.marquee.is_some() || panning {return false;}
        let (targets,lengths,points)=self.selected_shape_points();
        if targets.len()<2 && self.shape_group_drag.is_none() {return false;}
        self.selected_design=None;self.selected_cut_shape=None;self.pick_cut_shapes=false;
        let pointer=ui.input(|i|i.pointer.interact_pos());
        let bounds=cut_tools::bounds(&points);
        let screen=|p:mc630_plt_studio::Point|page.min+egui::vec2(p[0] as f32,p[1] as f32)*scale;
        let handle=pointer.and_then(|p|crate::design_transform::handles(bounds).iter().position(|q|screen(*q).distance(p)<=8.));
        let rect=egui::Rect::from_min_max(screen([bounds[0],bounds[1]]),screen([bounds[2],bounds[3]]));
        let mut over=pointer.is_some_and(|p|response.rect.contains(p) && ui.ctx().layer_id_at(p)==Some(response.layer_id) && (handle.is_some() || rect.contains(p)));
        let press=ui.input(|i|i.events.iter().find_map(|event|match event {egui::Event::PointerButton {pos,button:egui::PointerButton::Primary,pressed:true,..}=>Some(*pos),_=>None}));
        if self.shape_group_drag.is_none() && let Some(start)=press.filter(|p|response.rect.contains(*p) && ui.ctx().layer_id_at(*p)==Some(response.layer_id)) {
            let handle=crate::design_transform::handles(bounds).iter().position(|q|screen(*q).distance(start)<=8.);
            let additive=ui.input(|i|i.modifiers.shift || i.modifiers.ctrl);
            let physical=[(start.x-page.left()) as f64/scale as f64,(start.y-page.top()) as f64/scale as f64];
            let hit=targets.iter().rev().find(|target| {
                let path=match target {crate::shape_ops::Target::Design(i)=>self.layout.design_shapes.get(*i).map(|s|&s.path),crate::shape_ops::Target::Cut(i)=>self.layout.manual_cut_paths.get(*i)};
                path.is_some_and(|path|cut_tools::hit(path,physical,6./scale as f64))
            }).copied();
            if handle.is_some() || rect.contains(start) && (!additive || hit.is_some()) {
                self.design_drag=None;self.cut_shape_drag=None;
                self.shape_group_drag=Some(ShapeGroupDrag {designs:self.layout.design_shapes.clone(),cuts:self.layout.manual_cut_paths.clone(),targets,lengths,points,start,handle,toggle_click:if additive && handle.is_none(){hit}else{None}});
            }else if additive {over=false;}
        }
        if let Some(drag)=self.shape_group_drag.take() {
            if ui.input(|i|i.key_pressed(egui::Key::Escape)) {
                self.layout.design_shapes=drag.designs;self.layout.manual_cut_paths=drag.cuts;self.preview_cache=None;return true;
            }
            if let Some(pointer)=pointer {
                let delta=(pointer-drag.start)/scale;let modifiers=ui.input(|i|i.modifiers);
                let transformed=if pointer.distance(drag.start)<=2. {drag.points.clone()}else{crate::design_transform::transform(&drag.points,[delta.x as f64,delta.y as f64],drag.handle,modifiers.shift,modifiers.alt,[self.layout.page_width,self.layout.page_height])};
                self.set_shape_group_points(&drag.targets,&drag.lengths,&transformed);
            }
            ui.ctx().set_cursor_icon(drag.handle.map(crate::design_transform::cursor).unwrap_or(egui::CursorIcon::Grabbing));
            if ui.input(|i|i.pointer.primary_released() || !i.pointer.primary_down()) {
                if self.layout.design_shapes!=drag.designs || self.layout.manual_cut_paths!=drag.cuts {self.record_layer_edit(ObjectEdit::Shaping(drag.designs,drag.cuts));}
                if pointer.is_some_and(|p|p.distance(drag.start)<=2.) && let Some(target)=drag.toggle_click {
                    self.shape_selection.retain(|selected|*selected!=target);
                    self.selected_design=if self.shape_selection.len()==1 {self.shape_selection.iter().find_map(|t|if let crate::shape_ops::Target::Design(i)=t{Some(*i)}else{None})}else{None};
                    self.selected_cut_shape=if self.shape_selection.len()==1 {self.shape_selection.iter().find_map(|t|if let crate::shape_ops::Target::Cut(i)=t{Some(*i)}else{None})}else{None};
                    self.pick_cut_shapes=self.selected_cut_shape.is_some();
                }
            }else {self.shape_group_drag=Some(drag);}
            return true;
        }
        if over {ui.ctx().set_cursor_icon(handle.map(crate::design_transform::cursor).unwrap_or(egui::CursorIcon::Grab));}
        over
    }
    fn transform_shape_group(&mut self,degrees:f64,flip_x:bool,flip_y:bool) {
        if degrees.rem_euclid(360.)==0. && !flip_x && !flip_y {return;}
        let (targets,lengths,points)=self.selected_shape_points();if targets.len()<2 {return;}
        let [x0,y0,x1,y1]=cut_tools::bounds(&points);let center=[(x0+x1)/2.,(y0+y1)/2.];
        let (sin,cos)=degrees.to_radians().sin_cos();
        let mut next:Vec<_>=points.iter().map(|p| {
            let x=(p[0]-center[0])*if flip_x {-1.}else{1.};let y=(p[1]-center[1])*if flip_y {-1.}else{1.};
            [center[0]+cos*x-sin*y,center[1]+sin*x+cos*y]
        }).collect();
        if next.iter().any(|p|!(-1e-7..=self.layout.page_width+1e-7).contains(&p[0]) || !(-1e-7..=self.layout.page_height+1e-7).contains(&p[1])) {self.status="Selection would be outside the page.".into();return;}
        for p in &mut next {p[0]=p[0].clamp(0.,self.layout.page_width);p[1]=p[1].clamp(0.,self.layout.page_height);}
        if next!=points {self.record_layer_edit(ObjectEdit::Shaping(self.layout.design_shapes.clone(),self.layout.manual_cut_paths.clone()));self.set_shape_group_points(&targets,&lengths,&next);}
    }
    fn design_canvas(&mut self,ui:&egui::Ui,response:&egui::Response,page:egui::Rect,scale:f32,panning:bool)->bool {
        if self.cut_draft.is_some() || self.crop_edit.is_some() || self.marquee.is_some() || panning {return false;}
        let pointer=ui.input(|i|i.pointer.interact_pos());
        let physical=|p:egui::Pos2|[(p.x-page.left()) as f64/scale as f64,(p.y-page.top()) as f64/scale as f64];
        let screen=|p:mc630_plt_studio::Point|page.min+egui::vec2(p[0] as f32,p[1] as f32)*scale;
        let mut owned=self.design_drag.is_some();
        if ui.input(|i|i.pointer.primary_pressed()) && let Some(pointer)=pointer.filter(|p|response.rect.contains(*p) && ui.ctx().layer_id_at(*p)==Some(response.layer_id)) {
            let corner=self.selected_design.and_then(|index|self.layout.design_shapes.get(index).and_then(|s| {
                let [x0,y0,x1,y1]=cut_tools::bounds(&s.path);
                crate::design_transform::handles([x0,y0,x1,y1]).iter().position(|p|screen(*p).distance(pointer)<9.).map(|corner|(index,corner))
            }));
            let hit=corner.map(|(i,_)|i).or_else(||self.layout.design_shapes.iter().rposition(|s|cut_tools::hit(&s.path,physical(pointer),6./scale as f64)));
            if hit.is_some() {self.selected_design=hit;}
            self.pick_cut_shapes=hit.is_none();
            if let Some(index)=hit {
                self.select_shape(crate::shape_ops::Target::Design(index),ui.input(|i|i.modifiers.shift));
                if ui.input(|i|i.modifiers.shift) && corner.is_none() {self.design_drag=None;return true;}

                self.clear_selection();self.pick_cut_shapes=false;self.selected_cut_shape=None;
                let shape=&self.layout.design_shapes[index];
                self.drawing_mode=if shape.print && shape.cut {2}else if shape.print {1}else{0};
                self.design_fill_enabled=shape.fill.is_some();
                if let Some(color)=shape.fill {self.design_fill=egui::Color32::from_rgba_unmultiplied(color[0],color[1],color[2],color[3]);}
                let c=shape.stroke;self.design_stroke=egui::Color32::from_rgba_unmultiplied(c[0],c[1],c[2],c[3]);
                self.design_stroke_width=shape.stroke_width;
                self.design_drag=Some((index,self.layout.design_shapes.clone(),pointer,corner.map(|(_,c)|c),ui.input(|i|i.modifiers.ctrl) && corner.is_none()));owned=true;
            }
        }
        if let (Some((index,before,start,corner,duplicate)),Some(pointer))=(&self.design_drag,pointer) {
            if (ui.input(|i|i.pointer.primary_down() || i.pointer.primary_released())) && pointer.distance(*start)>2. {
                let old=&before[*index];
                let delta=(pointer-*start)/scale;
                let modifiers=ui.input(|i|i.modifiers);
                let path=crate::design_transform::transform(&old.path,[delta.x as f64,delta.y as f64],*corner,modifiers.shift,modifiers.alt,[self.layout.page_width,self.layout.page_height]);
                let target=if *duplicate && before.len()<100 {
                    if self.layout.design_shapes.len()==before.len() {self.layout.design_shapes.push(old.clone());}
                    before.len()
                }else{*index};
                self.layout.design_shapes[target].path=path;
                self.selected_design=Some(target);
            }
            ui.ctx().set_cursor_icon(corner.map(crate::design_transform::cursor).unwrap_or(if *duplicate {egui::CursorIcon::Copy}else{egui::CursorIcon::Grabbing}));
        }else if let Some(pointer)=pointer.filter(|p| response.rect.contains(*p) && ui.ctx().layer_id_at(*p)==Some(response.layer_id)) {
            if let Some(shape)=self.selected_design.and_then(|i|self.layout.design_shapes.get(i)) {
                let [x0,y0,x1,y1]=cut_tools::bounds(&shape.path);
                if let Some(corner)=crate::design_transform::handles([x0,y0,x1,y1]).iter().position(|p|screen(*p).distance(pointer)<9.) {
                    ui.ctx().set_cursor_icon(crate::design_transform::cursor(corner));owned=true;
                }
            }
            if !owned && self.layout.design_shapes.iter().any(|s|cut_tools::hit(&s.path,physical(pointer),6./scale as f64)) {
                ui.ctx().set_cursor_icon(if ui.input(|i|i.modifiers.ctrl) {egui::CursorIcon::Copy}else{egui::CursorIcon::Grab});owned=true;
            }
        }
        if ui.input(|i|i.key_pressed(egui::Key::Escape)) && let Some((index,before,_,_,_))=self.design_drag.take() {
            self.layout.design_shapes=before;self.selected_design=Some(index);
        }
        if ui.input(|i|i.pointer.primary_released()) && let Some((_,before,_,_,_))=self.design_drag.take() {
            if self.layout.design_shapes!=before {self.record_layer_edit(ObjectEdit::Designs(before));}
        }
        owned
    }
    fn paint_design_selection(&self,painter:&egui::Painter,page:egui::Rect,scale:f32) {
        let (targets,_,points)=self.selected_shape_points();
        if targets.len()>1 {
            let bounds=cut_tools::bounds(&points);
            let screen=|p:mc630_plt_studio::Point|page.min+egui::vec2(p[0] as f32,p[1] as f32)*scale;
            let stroke=egui::Stroke::new(1_f32,crate::ui_theme::ACCENT);
            painter.rect_stroke(egui::Rect::from_min_max(screen([bounds[0],bounds[1]]),screen([bounds[2],bounds[3]])),0.,stroke,egui::StrokeKind::Inside);
            for handle in crate::design_transform::handles(bounds) {painter.rect(egui::Rect::from_center_size(screen(handle),egui::vec2(7.,7.)),0.,egui::Color32::WHITE,stroke,egui::StrokeKind::Inside);}
            return;
        }
        for target in &self.shape_selection {
            let path=match target {crate::shape_ops::Target::Design(i)=>self.layout.design_shapes.get(*i).map(|s|&s.path),crate::shape_ops::Target::Cut(i)=>self.layout.manual_cut_paths.get(*i)};
            if let Some(path)=path {
                let [x0,y0,x1,y1]=cut_tools::bounds(path);
                painter.rect_stroke(egui::Rect::from_min_max(page.min+egui::vec2(x0 as f32,y0 as f32)*scale,page.min+egui::vec2(x1 as f32,y1 as f32)*scale),0.,egui::Stroke::new(1_f32,crate::ui_theme::ACCENT),egui::StrokeKind::Inside);
            }
        }
        let Some(shape)=self.selected_design.and_then(|i|self.layout.design_shapes.get(i)) else{return};
        let [x0,y0,x1,y1]=cut_tools::bounds(&shape.path);
        let rect=egui::Rect::from_min_max(page.min+egui::vec2(x0 as f32,y0 as f32)*scale,page.min+egui::vec2(x1 as f32,y1 as f32)*scale);
        painter.rect_stroke(rect,0.,egui::Stroke::new(1_f32,crate::ui_theme::ACCENT),egui::StrokeKind::Inside);
        for corner in crate::design_transform::handles([rect.left() as f64,rect.top() as f64,rect.right() as f64,rect.bottom() as f64]).map(|p|egui::pos2(p[0] as f32,p[1] as f32)) {
            painter.rect(egui::Rect::from_center_size(corner,egui::vec2(7.,7.)),0.,egui::Color32::WHITE,egui::Stroke::new(1_f32,crate::ui_theme::ACCENT),egui::StrokeKind::Inside);
        }
    }
    fn paint_designs(&mut self,ctx:&egui::Context,painter:&egui::Painter,page:egui::Rect) {
        if self.layout.has_print_designs() && !self.design_preview.paint_simple(painter,page,&self.layout) {
            let mut design_layout=self.layout.clone();design_layout.svg_sources.clear();
            if let Err(error)=self.design_preview.paint(ctx,painter,page,&design_layout) {self.status=error;}
        }
        if self.show_cut {
            for shape in self.layout.design_shapes.iter().filter(|s|s.cut) {
                let points=shape.path.iter().map(|p|page.min+egui::vec2((p[0]/self.layout.page_width) as f32*page.width(),(p[1]/self.layout.page_height) as f32*page.height())).collect();
                painter.add(egui::Shape::line(points,egui::Stroke::new(1_f32,self.contour_color)));
            }
        }
    }

    fn print_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_print_dialog {
            return;
        }
        let mut open = true;
        egui::Window::new("Print")
            .id(egui::Id::new("print-dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(380.)
            .default_pos(ctx.screen_rect().center() - egui::vec2(205., 150.))
            .show(ctx, |ui| {
            egui::Grid::new("print-document-form")
                .num_columns(2).min_col_width(85.).spacing([18., 8.])
                .show(ui, |ui| {
                    ui.label("Page");
                    ui.label(format!("{} of {}",self.active_page+1,self.other_pages.len()+1));
                    ui.end_row();
                    ui.label("Print pages");
                    ui.horizontal(|ui|{
                        ui.selectable_value(&mut self.print_range_mode,0,"Current");
                        ui.selectable_value(&mut self.print_range_mode,1,"All");
                        ui.selectable_value(&mut self.print_range_mode,2,"Range");
                    });
                    ui.end_row();
                    if self.print_range_mode==2 {
                        ui.label("Page range");
                        ui.add(egui::TextEdit::singleline(&mut self.print_range).desired_width(210.).hint_text("1, 3, 5-8"));
                        ui.end_row();
                    }
                    ui.label("Side");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.print_back, false, "Main Page");
                        ui.selectable_value(&mut self.print_back, true, "Back Page");
                    });
                    ui.end_row();
                    ui.label("Paper");
                    ui.label(format!("{}  {}", self.layout.paper_label(),
                        self.unit.size(self.layout.page_width, self.layout.page_height)));
                    ui.end_row();
                    ui.label("Marks");
                    ui.label(if self.print_back { "None" } else if self.ccd_trial { "CCD · 4 dots" } else { "Manual crosses" });
                    ui.end_row();
                });
            ui.separator();
            let selected_pages=self.selected_print_pages();
            if let Err(error)=&selected_pages {ui.colored_label(egui::Color32::from_rgb(180,45,30),error);}
            #[cfg(windows)]
            {
                let ready=selected_pages.is_ok();
                // Build requests only when Print is clicked. Rasterizing and
                // decoding inactive pages remains on the print worker.
                if self.print_panel.ui(ui,ready) {
                    let pages=selected_pages.and_then(|pages|self.prepare_print_pages(&pages));
                    self.print_panel.submit(pages,&mut self.status,ctx);
                }
            }
            #[cfg(not(windows))]
            ui.label("Direct printing is available on Windows.");
        });
        if !open {
            self.show_print_dialog = false;
        }
    }

    fn open_vector_trace(&mut self,ctx:&egui::Context) {
        let Some(art)=&self.art else{return};
        let index=self.selected.or_else(||self.selection.iter().next().copied()).unwrap_or(0);
        let Ok(items)=self.layout.editor_items() else {self.status="Adjust image placement before tracing.".into();return};
        let Some(item)=items.get(index) else{return};
        let pixels=art.tiles.get(index).map(|a|&a.pixels).unwrap_or(&art.pixels);
        let tile=self.layout.image_tiles.get(index).copied().unwrap_or([0,0,pixels.width(),pixels.height()]);
        self.vector_trace=Some(crate::vector_trace_ui::Dialog::new(ctx,index,self.source_digest,tile,{let (w,h)=item.orientation.source_size(item.width,item.height);[w,h]},pixels));
    }
    fn apply_vector_trace(&mut self,index:usize,asset:mc630_plt_studio::svg_artwork::SvgArtwork) {
        let before=self.layout.clone();self.layout.svg_sources.insert(index,asset);
        self.record_layer_edit(ObjectEdit::Layers(Box::new(before)));
        self.svg_previews.clear();self.outline_cache=Default::default();self.preview_cache=None;
    }
    fn vector_trace_dialog(&mut self,ctx:&egui::Context) {
        let action=self.vector_trace.as_mut().and_then(|dialog|dialog.show(ctx));
        match action {
            Some(crate::vector_trace_ui::Action::Close)=>{self.vector_trace=None;},
            Some(crate::vector_trace_ui::Action::Apply(asset))=>{
                let dialog=self.vector_trace.as_ref().unwrap();
                if dialog.source_digest!=self.source_digest || dialog.index>=self.layout.count || self.art.is_none() {
                    self.vector_trace=None;self.status="Image changed. Open Vector Trace again.".into();return;
                }
                let index=dialog.index;
                self.apply_vector_trace(index,asset);
                self.vector_trace=None;self.status="Vector trace applied. Original pixels retained. Ctrl+Z to undo.".into();
            },
            None=>{}
        }
    }
    fn artwork_dialog_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if icon_button(ui, Icon::Import, "Import image (Ctrl+O)") {
                self.choose_image(ctx);
            }
            if ui.add_enabled_ui(!self.source.is_empty(), |ui| {
                icon_button(ui, Icon::Close, "Close image")
            }).inner {
                self.request_action(DocumentAction::CloseImage, ctx);
            }
            if self.art.is_none() && icon_button(ui, Icon::Artwork, "Load sample artwork") {
                self.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
                self.name = "Sample sticker".into();
                self.retrace(ctx);
                self.ratio = 1.;
                self.layout.height = self.layout.width;
            }
        });

        ui.add(egui::Label::new(egui::RichText::new(&self.name).small().weak()).truncate())
            .on_hover_text(&self.name);

        if ui.add_enabled(self.art.is_some(),egui::Button::new("Vector Trace…")).clicked() {self.open_vector_trace(ctx);}
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
                .add(crate::ui_theme::small_slider(
                    egui::Slider::new(&mut self.background_tolerance, 5..=100).text("BG tolerance")))
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
                    .add(crate::ui_theme::small_slider(egui::Slider::new(&mut self.threshold, 180..=254).text("Threshold")))
                    .changed();
            }
        }
        if changed && !self.source.is_empty() {
            self.retrace(ctx);
        }
    }
    fn apply_copy_count(&mut self,count:usize,ctx:&egui::Context) {
        if count==self.layout.count {return;}
        match self.layout.with_copy_count(count) {
            Ok(layout)=>{
                let before=std::mem::replace(&mut self.layout,layout);
                self.record_layer_edit(ObjectEdit::Layers(Box::new(before)));
                self.clear_selection();
                if !self.layout.image_tiles.is_empty() {self.retrace(ctx);}
            }
            Err(error)=>self.status=error,
        }
    }
    fn apply_layout_spacing(&mut self,column_gap:f64,row_gap:f64) {
        if !column_gap.is_finite() || !row_gap.is_finite() || column_gap<0. || row_gap<0. {return;}
        let before=self.layout.clone();
        if self.art.is_some() && let Ok(items)=before.editor_items() {
            // Preserve row/column membership. Increasing a column gap expands right,
            // rather than repacking columns onto a new row when capacity changes.
            let mut rows:Vec<f64>=items.iter().map(|p|p.y).collect();
            rows.sort_by(f64::total_cmp);rows.dedup_by(|a,b|(*a-*b).abs()<0.01);
            self.layout.custom=items.iter().enumerate().map(|(i,item)| {
                let row=rows.iter().position(|y|(*y-item.y).abs()<0.01).unwrap_or(0);
                let mut xs:Vec<f64>=items.iter().filter(|p|(p.y-item.y).abs()<0.01).map(|p|p.x).collect();
                xs.sort_by(f64::total_cmp);xs.dedup_by(|a,b|(*a-*b).abs()<0.01);
                let col=xs.iter().position(|x|(*x-item.x).abs()<0.01).unwrap_or(0);
                (i,sheet::Placement {x:item.x+col as f64*(column_gap-before.column_gap),
                    y:item.y+row as f64*(row_gap-before.row_gap),..*item})
            }).collect();
        }
        self.layout.column_gap=column_gap;self.layout.row_gap=row_gap;
        self.record_layer_edit(ObjectEdit::PageGeometry(Box::new(before)));
    }

    fn repeat_dialog_content(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.);
        let selected = self.selected.and_then(|index| {
            let items = self.layout.editor_items().ok()?;
            Some((index, *items.get(index)?))
        });
        ui.small(match selected {
            Some((index, _)) => format!("Image {} size and sheet spacing", index + 1),
            None => "Default size".into(),
        });
        if !self.layout.custom.is_empty()
            && self.layout.image_tiles.is_empty()
            && icon_button(ui, Icon::Grid, "Reflow copies")
        {
            self.reflow();
        }
        let mut size = selected.map(|(_, p)| p);
        let mut column_gap=self.layout.column_gap;
        let mut row_gap=self.layout.row_gap;
        let mut width_changed = false;
        let mut height_changed = false;
        ui.horizontal(|ui| {
        egui::Grid::new("sheet-dimensions")
            .num_columns(2)
            .min_col_width(90.)
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

            });
                if icon_toggle(ui, if self.lock { Icon::Link } else { Icon::Unlink }, self.lock,
                    if self.lock { "Unlink width and height" } else { "Link width and height" }) {
                    self.lock = !self.lock;
                    if self.lock {
                        self.ratio = size.as_ref().map_or(self.layout.width / self.layout.height, |p|p.width/p.height);
                    }
                }
        });
        egui::Grid::new("sheet-repeat-controls")
            .num_columns(2).min_col_width(90.).spacing([16.,8.])
            .show(ui,|ui| {
                ui.spacing_mut().interact_size.x=94.;
                ui.label("Copies");
                ui.horizontal(|ui| {
                    let repeatable=self.layout.image_tiles.is_empty() || self.layout.can_repeat_tiled_image();
                    let mut count=self.layout.count;
                    let limit=if self.layout.image_tiles.is_empty(){1000}else{24};
                    if ui.add_enabled(repeatable,egui::DragValue::new(&mut count).update_while_editing(false).range(1..=limit)).changed() {
                        self.apply_copy_count(count,ui.ctx());
                    }
                    let capacity=self.cached_capacity().unwrap_or(0).min(limit);
                    if ui.add_enabled_ui(repeatable && capacity>0,|ui| {
                        icon_button(ui,Icon::Duplicate,&format!("Fill sheet ({capacity} copies)"))
                    }).inner {self.apply_copy_count(capacity,ui.ctx());}
                });
                ui.end_row();

                ui.label("Column spacing");
                ui.scope(|ui| {
                    ui.spacing_mut().slider_width=80.;
                    ui.spacing_mut().interact_size=egui::vec2(60.,20.);
                    ui.add(crate::ui_theme::small_slider(self.unit.slider(&mut column_gap,0. ..=100.).fixed_decimals(3)));
                });
                ui.end_row();
                ui.label("Row spacing");
                ui.scope(|ui| {
                    ui.spacing_mut().slider_width=80.;
                    ui.spacing_mut().interact_size=egui::vec2(60.,20.);
                    ui.add(crate::ui_theme::small_slider(self.unit.slider(&mut row_gap,0. ..=100.).fixed_decimals(3)));
                });
                ui.end_row();

            });
        if column_gap!=self.layout.column_gap || row_gap!=self.layout.row_gap {
            self.apply_layout_spacing(column_gap,row_gap);
        }
        ui.separator();
        self.edit_margin_fields(ui);
        if let (Some((index, original)), Some(p)) = (selected, size) {
            if width_changed || height_changed {
                self.apply_selected_dimensions(index, original, p, width_changed, height_changed);
            }
        }
        ui.add_space(8.);

    }
    fn outline_dialog_content(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.);
        let before = self.layout.use_manual_cut_paths;
        let mut manual = before;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut manual, true, "Manual");
            ui.selectable_value(&mut manual, false, "Auto");
        });
        if manual != before {
            self.record_manual_cut_edit();
            self.layout.use_manual_cut_paths = manual;
            self.preview_cache = None;
            self.back_paths_cache = None;
            if !manual && !self.source.is_empty() { self.retrace(ui.ctx()); }
        }
        ui.separator();
        if manual {
            if ui.add_enabled_ui(self.selected.is_some() && self.selection.len()==1 && !self.layout.manual_cut_paths.is_empty(), |ui| {
                icon_button(ui,Icon::Link,"Link manual cutlines to selected image")
            }).inner {
                self.toggle_layer_cutline_link(self.selected.unwrap(),false);
            }
            ui.label("Outline distance");
            ui.scope(|ui| {
                ui.spacing_mut().interact_size.y=20.;
                ui.add(crate::ui_theme::small_slider(self.unit.slider(&mut self.layout.contour_offset,-5. ..=5.).fixed_decimals(3)));
            });
            ui.separator();
            ui.label("Outline smoothing");
            ui.checkbox(&mut self.layout.preserve_corners, "Preserve sharp corners");
            ui.scope(|ui| {
                ui.spacing_mut().interact_size.y=20.;
                ui.add(crate::ui_theme::small_slider(self.unit.slider(&mut self.layout.smoothing,0. ..=sheet::MAX_SMOOTHING_MM).fixed_decimals(2)));
            });
            crate::ui_theme::preset_buttons(ui, &mut self.layout.smoothing,
                &[("Off",0.),("Light",0.2),("Medium",0.5),("Strong",4.)]);
            return;
        }
        let mut straight = self.cut_shape == sheet::CutShape::ImageFrame;
        if ui.checkbox(&mut straight, "Straight rectangle cut")
            .on_hover_text("Cut four straight sides around each image frame. Crop unwanted margins first.")
            .changed()
        {
            self.cut_shape = if straight { sheet::CutShape::ImageFrame } else { sheet::CutShape::AlphaOutline };
            if straight {
                self.layout.preserve_corners = true;
                self.layout.smoothing = 0.;
                if self.layout.use_manual_cut_paths {
                    self.record_manual_cut_edit();
                    self.layout.use_manual_cut_paths = false;
                }
            }
            if !self.source.is_empty() { self.retrace(ui.ctx()); }
        }
        ui.separator();
        let native_svg=self.cut_shape==sheet::CutShape::AlphaOutline && self.layout.editor_items().is_ok_and(|items| {
            let visible:Vec<_>=(0..items.len()).filter(|i|!self.layout.hidden_images.contains(i)).collect();
            !visible.is_empty() && visible.iter().all(|i|self.layout.svg_source_for(*i).is_some())
        });
        if native_svg {ui.label("SVG vector cutline");ui.small("Original paths · optional smoothing");}
        ui.label("Outline smoothing");
        ui.checkbox(&mut self.layout.preserve_corners, "Preserve sharp corners");
        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = 20.;
            ui.add(crate::ui_theme::small_slider(self.unit.slider(&mut self.layout.smoothing, 0.0..=sheet::MAX_SMOOTHING_MM).fixed_decimals(2)));
        });
        crate::ui_theme::preset_buttons(ui, &mut self.layout.smoothing,
            &[("Off",0.),("Light",0.2),("Medium",0.5),("Strong",4.)]);
        ui.add_space(8.);

        ui.checkbox(&mut self.layout.cut_inner_holes, "Cut inner holes");
        ui.add_space(8.);
        ui.label("Outline distance");
        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = 20.;
            ui.add(crate::ui_theme::small_slider(self.unit.slider(&mut self.layout.contour_offset, -5.0..=5.0)
                .fixed_decimals(3)));
        });
        crate::ui_theme::preset_buttons(ui, &mut self.layout.contour_offset,
            &[("Inside",-0.5),("Exact",0.),("Outside",1.)]);
    }
    fn dot_settings_fields(&mut self,ui:&mut egui::Ui) {
        ui.horizontal(|ui| {
            egui::Grid::new(ui.id().with("dot-settings-fields")).num_columns(2).spacing([12.,8.]).show(ui,|ui| {
                ui.label("Diameter");
                ui.add(self.unit.distance(&mut self.ccd_profile.diameter).speed(0.1).range(1. ..=10.));ui.end_row();
                ui.label("Center gap");
                ui.add(self.unit.distance(&mut self.ccd_profile.inset).speed(0.1).range(5. ..=30.));ui.end_row();
            });
            if icon_button(ui,Icon::Refresh,"Reset dots: diameter 5.5 mm, center gap 7 mm") {
                self.ccd_profile=sheet::CcdTrial::default();
                self.preview_cache=None;
                self.status="Dot settings reset: diameter 5.5 mm, center gap 7 mm.".into();
            }
        });
    }
    fn registration_dialog_content(&mut self, ui: &mut egui::Ui) {
        ui.strong("Registration");
        ui.add_space(8.);

        ui.checkbox(&mut self.ccd_trial, "CCD camera · 4 dots");

        if self.ccd_trial {
            egui::CollapsingHeader::new("Dot settings").show(ui, |ui| {
                self.dot_settings_fields(ui);
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

fn crop_resize_cursor(direction: egui::Vec2) -> egui::CursorIcon {
    let sector = (direction.y.atan2(direction.x) / std::f32::consts::FRAC_PI_4).round() as i32;
    match sector.rem_euclid(4) {
        0 => egui::CursorIcon::ResizeHorizontal,
        1 => egui::CursorIcon::ResizeNwSe,
        2 => egui::CursorIcon::ResizeVertical,
        _ => egui::CursorIcon::ResizeNeSw,
    }
}

#[cfg(test)]
mod crop_cursor_tests {
    use super::*;
    #[test]
    fn resize_cursor_matches_screen_direction() {
        for (direction, expected) in [
            (egui::vec2(1.,0.), egui::CursorIcon::ResizeHorizontal),
            (egui::vec2(0.,1.), egui::CursorIcon::ResizeVertical),
            (egui::vec2(1.,1.), egui::CursorIcon::ResizeNwSe),
            (egui::vec2(-1.,1.), egui::CursorIcon::ResizeNeSw),
        ] {
            assert_eq!(crop_resize_cursor(direction), expected);
            assert_eq!(crop_resize_cursor(-direction), expected);
        }
    }
}

#[cfg(test)]
mod import_cut_mode_tests {
    use super::*;
    #[test]
    fn import_starts_manual_and_saved_choice_survives_roundtrip() {
        let ctx=egui::Context::default();
        let mut app=A4Studio::fresh_tab();
        app.install_single_image(include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec(),"sample.png".into(),&ctx);
        assert!(app.art.is_some());
        assert!(app.layout.use_manual_cut_paths);
        assert!(app.layout.manual_cut_paths.is_empty());
        assert!(!app.can_export_cut());
        app.layout.width=40.;app.layout.height=40.;
        let project=app.project_snapshot();
        let restored:crate::project::Project=serde_json::from_slice(&serde_json::to_vec(&project).unwrap()).unwrap();
        assert!(restored.settings.layout.use_manual_cut_paths);
        app.layout.use_manual_cut_paths=false;
        assert!(app.can_export_cut());
        assert!(!app.layout.page_cut_paths(app.art.as_ref().unwrap()).unwrap().is_empty());
        app.layout.use_manual_cut_paths=true;
        app.commit_manual_cut(vec![[20.,20.],[40.,20.],[40.,40.],[20.,20.]]);
        assert!(app.can_export_cut());
        app.undo_object(&ctx);
        assert!(app.layout.use_manual_cut_paths);
        assert!(!app.can_export_cut());
    }
}

#[cfg(test)]
mod linked_image_cut_tests {
    use super::*;
    #[test]
    fn drawn_cutline_follows_duplicate_delete_undo_and_save() {
        let ctx=egui::Context::default(); let mut app=A4Studio::fresh_tab();
        app.install_single_image(include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec(),"test.png".into(),&ctx);
        app.layout.width=40.;app.layout.height=40.;
        app.layout.custom.insert(0,sheet::Placement{x:20.,y:20.,width:40.,height:40.,orientation:Default::default()});
        app.commit_manual_cut(vec![[22.,22.],[58.,22.],[58.,58.],[22.,58.],[22.,22.]]);
        assert!(app.layout.manual_cut_paths.is_empty());
        assert!(app.layout.image_cut_paths.contains_key(&0));
        let original=app.layout.manual_page_cut_paths().unwrap();
        let items=app.layout.editor_items().unwrap();
        app.duplicate_images(&[(0,items[0])],(50.,0.),&ctx);
        assert!(app.layout.image_cut_paths.contains_key(&1));
        let paths=app.layout.manual_page_cut_paths().unwrap();
        assert_eq!(paths.len(),2);
        assert!((paths[1][0][0]-original[0][0][0]-50.).abs()<1e-8);
        let snapshot=app.project_snapshot();
        let restored:crate::project::Project=serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
        assert_eq!(restored.settings.layout.manual_page_cut_paths().unwrap(),paths);
        app.clear_selection();app.select_target(0,false,false);app.delete_selected_images(&ctx);
        assert_eq!(app.layout.manual_page_cut_paths().unwrap().len(),1);
        app.undo_object(&ctx);
        assert_eq!(app.layout.manual_page_cut_paths().unwrap(),paths);
        app.undo_object(&ctx);
        assert_eq!(app.layout.manual_page_cut_paths().unwrap(),original);
    }
}

#[cfg(test)]
mod open_view_tests {
    use super::*;
    #[test]
    fn reopening_saved_zoomed_panned_project_centers_view_without_moving_artwork() {
        let ctx=egui::Context::default();
        let mut old=A4Studio::fresh_tab();
        old.source=include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        old.layout.count=1;
        old.layout.custom.insert(0,sheet::Placement{x:25.,y:35.,width:40.,height:50.,orientation:Default::default()});
        old.zoom=3.58;old.canvas_pan=egui::vec2(-1700.,-900.);
        let project=old.project_snapshot();
        let expected=project.settings.layout.clone();
        let source=old.source.clone();
        let mut opened=A4Studio::fresh_tab();
        opened.install_project(project,source,&ctx);
        assert_eq!(opened.zoom,1.);
        assert_eq!(opened.canvas_pan,egui::Vec2::ZERO);
        assert!(opened.layout==expected);
        let viewport=egui::Rect::from_min_size(egui::pos2(60.,180.),egui::vec2(980.,650.));
        assert_eq!(crate::canvas_camera::CanvasCamera::origin(viewport,opened.zoom,egui::Vec2::ZERO)+opened.canvas_pan,viewport.center());
    }
}

#[cfg(test)]
mod overflow_interaction_tests {
    use super::*;
    #[test]
    fn column_spacing_keeps_rows_and_moves_overflow_right_with_undo() {
        let ctx=egui::Context::default();let mut app=A4Studio::fresh_tab();
        app.source=include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();app.retrace(&ctx);
        app.layout.count=6;app.layout.width=50.;app.layout.height=60.;
        let original=app.layout.editor_items().unwrap();
        app.apply_layout_spacing(60.,app.layout.row_gap);
        let changed=app.layout.editor_items().unwrap();
        assert_eq!(changed.len(),6);
        for (i,(old,new)) in original.iter().zip(&changed).enumerate() {
            assert_eq!(old.y,new.y);
            assert!((new.x-old.x-(i%3) as f64*57.).abs()<1e-8);
        }
        assert!(changed.iter().any(|p|p.x+p.width>app.layout.page_width));
        app.undo_object(&ctx);
        assert_eq!(app.layout.editor_items().unwrap(),original);
        assert_eq!(app.layout.column_gap,3.);
    }
    #[test]
    fn overflow_close_handles_press_release_and_stays_dismissed() {
        let ctx=egui::Context::default();let mut app=A4Studio::fresh_tab();
        app.source=include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();app.retrace(&ctx);
        app.layout.count=6;app.layout.width=80.;app.layout.height=130.;
        let mut target=None;
        for _ in 0..3 {
            let output=ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(800.,600.))),..Default::default()},|ctx|app.layout_overflow_notice(ctx));
            for shape in output.shapes {
                if let egui::Shape::Text(text)=shape.shape {
                    if text.galley.text()=="Close" {target=Some(text.pos+text.galley.size()/2.);}
                }
            }
        }
        let target=target.expect("Close button must render");
        assert!(app.overflow_warning_open);
        for pressed in [true,false] {
            let _=ctx.run(egui::RawInput{events:vec![egui::Event::PointerMoved(target),egui::Event::PointerButton{pos:target,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}],..Default::default()},|ctx|app.layout_overflow_notice(ctx));
        }
        assert!(app.overflow_warning_seen);
        assert!(!app.overflow_warning_open);
        let output=ctx.run(Default::default(),|ctx|app.layout_overflow_notice(ctx));
        assert!(!output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="Images do not fit the page")));
    }
}

#[cfg(test)]
mod design_tool_tests {
    use super::*;
    #[test]
    fn design_tool_creates_printable_vectors_and_print_cut_exports_without_image() {
        let ctx=egui::Context::default();let mut app=A4Studio::fresh_tab();
        app.drawing_mode=1;
        app.commit_manual_cut(vec![[20.,20.],[50.,20.],[50.,50.],[20.,20.]]);
        assert_eq!(app.layout.design_shapes.len(),1);
        assert!(app.layout.design_shapes[0].print && !app.layout.design_shapes[0].cut);
        assert!(!app.can_export_cut());
        app.drawing_mode=2;
        app.commit_manual_cut(vec![[70.,20.],[90.,20.],[90.,40.],[70.,20.]]);
        assert!(app.can_export_cut());assert!(app.cut_file_data().unwrap().contains("PD"));
        let project=app.project_snapshot();
        let restored:crate::project::Project=serde_json::from_slice(&serde_json::to_vec(&project).unwrap()).unwrap();
        assert_eq!(restored.settings.layout.design_shapes.len(),2);
        app.undo_object(&ctx);assert_eq!(app.layout.design_shapes.len(),1);
        app.redo_object(&ctx);assert_eq!(app.layout.design_shapes.len(),2);
    }
}

#[cfg(test)]
mod marquee_tests {
    use super::*;
    #[test]
    fn blank_drag_selects_design_cutline_and_line_without_an_image() {
        use crate::shape_ops::Target;
        let ctx=egui::Context::default();let mut app=A4Studio::fresh_tab();app.show_cut=true;
        app.layout.design_shapes.push(sheet::DesignShape {path:vec![[20.,20.],[40.,20.],[40.,40.],[20.,40.],[20.,20.]],fill:Some([0,103,192,255]),stroke:[0,0,0,255],stroke_width:0.3,print:true,cut:false});
        app.layout.manual_cut_paths=vec![vec![[50.,20.],[70.,20.],[70.,40.],[50.,40.],[50.,20.]],vec![[90.,20.],[120.,40.]]];
        let page=egui::Rect::from_min_size(egui::pos2(20.,20.),egui::vec2(420.,594.));
        let frame=|app:&mut A4Studio,events,shift| {
            let _=ctx.run(egui::RawInput {events,modifiers:egui::Modifiers {shift,..Default::default()},..Default::default()},|ctx| {
                egui::CentralPanel::default().show(ctx,|ui| {
                    let response=ui.allocate_rect(page,egui::Sense::click_and_drag());
                    if !app.design_canvas(ui,&response,page,2.,false) {app.canvas_edit(ui,&response,page,2.);}
                });
            });
        };
        let event=|pos,pressed|egui::Event::PointerButton {pos,pressed,button:egui::PointerButton::Primary,modifiers:Default::default()};
        frame(&mut app,vec![],false);
        let start=egui::pos2(30.,30.);let end=egui::pos2(175.,110.);
        frame(&mut app,vec![egui::Event::PointerMoved(start),event(start,true)],false);
        frame(&mut app,vec![egui::Event::PointerMoved(end)],false);
        assert!(app.shape_selection.contains(&Target::Design(0)));assert!(app.shape_selection.contains(&Target::Cut(0)));
        frame(&mut app,vec![event(end,false)],false);
        assert_eq!(app.shape_selection.len(),2);assert!(app.marquee.is_none());
        let start=egui::pos2(190.,50.);let end=egui::pos2(270.,110.);
        frame(&mut app,vec![egui::Event::PointerMoved(start),event(start,true)],true);
        frame(&mut app,vec![egui::Event::PointerMoved(end),event(end,false)],true);
        assert_eq!(app.shape_selection.len(),3);assert!(app.shape_selection.contains(&Target::Cut(1)));
        app.shape_selection.clear();app.selected_design=Some(0);app.selected_cut_shape=None;
        frame(&mut app,vec![egui::Event::PointerMoved(start),event(start,true)],true);
        frame(&mut app,vec![egui::Event::PointerMoved(end),event(end,false)],true);
        assert_eq!(app.shape_selection.len(),2);assert!(app.shape_selection.contains(&Target::Design(0)));assert!(app.shape_selection.contains(&Target::Cut(1)));
    }
    #[test]
    fn blank_drag_selects_intersecting_images_and_preserves_additive_selection() {
        let ctx = egui::Context::default();
        let mut app = A4Studio::default();
        app.source = include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();
        app.layout.count = 3;
        app.retrace(&ctx);
        for index in 0..3 {
            app.layout.custom.insert(index, sheet::Placement { x: 20. + index as f64 * 40., y: 20., width: 20., height: 20., orientation: Default::default() });
        }
        let page = egui::Rect::from_min_size(egui::pos2(20.,20.),egui::vec2(420.,594.));
        let frame = |app: &mut A4Studio, events| {
            let _ = ctx.run(egui::RawInput {events,..Default::default()}, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = ui.allocate_rect(page, egui::Sense::click_and_drag());
                    app.canvas_edit(ui,&response,page,2.);
                });
            });
        };
        frame(&mut app, vec![]);
        let start = egui::pos2(30.,30.);
        frame(&mut app, vec![egui::Event::PointerMoved(start),egui::Event::PointerButton {pos:start,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}]);
        assert!(app.marquee.is_some());
        let end=egui::pos2(185.,110.);
        frame(&mut app,vec![egui::Event::PointerMoved(end),egui::Event::PointerButton {pos:end,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]);
        assert_eq!(app.selection, [0,1].into_iter().collect());
        assert!(app.marquee.is_none());
        assert!(app.drag.is_none());
        app.marquee=Some((egui::pos2(215.,55.),app.selection.clone(),vec![]));
        let end=egui::pos2(265.,105.);
        frame(&mut app,vec![egui::Event::PointerMoved(end)]);
        assert_eq!(app.selection,[0,1,2].into_iter().collect());
    }
}

#[cfg(test)]
mod outline_transform_tests {
    use super::*;
    #[test]
    fn outline_ctrl_drag_duplicates_and_undo_restores_original() {
        let ctx=egui::Context::default();let mut app=A4Studio::default();
        app.rulers=false;app.pick_cut_shapes=true;
        app.layout.manual_cut_paths=vec![vec![[20.,20.],[40.,20.],[40.,40.],[20.,40.],[20.,20.]]];
        let original=app.layout.manual_cut_paths.clone();
        let page=egui::Rect::from_min_size(egui::pos2(20.,20.),egui::vec2(420.,594.));
        let modifiers=egui::Modifiers {ctrl:true,..Default::default()};
        let frame=|app:&mut A4Studio,events| {
            let _=ctx.run(egui::RawInput {events,modifiers,..Default::default()},|ctx| {
                egui::CentralPanel::default().show(ctx,|ui| {
                    let response=ui.allocate_rect(page,egui::Sense::click_and_drag());
                    app.cut_shape_canvas(&response,ui.painter(),page,page,2.,false);
                });
            });
        };
        frame(&mut app,vec![]);
        let start=egui::pos2(80.,80.);let end=egui::pos2(110.,90.);
        frame(&mut app,vec![egui::Event::PointerMoved(start),egui::Event::PointerButton {pos:start,button:egui::PointerButton::Primary,pressed:true,modifiers}]);
        frame(&mut app,vec![egui::Event::PointerMoved(end)]);
        frame(&mut app,vec![egui::Event::PointerButton {pos:end,button:egui::PointerButton::Primary,pressed:false,modifiers}]);
        assert_eq!(app.layout.manual_cut_paths.len(),2);
        assert_eq!(app.layout.manual_cut_paths[0],original[0]);
        assert_eq!(cut_tools::bounds(&app.layout.manual_cut_paths[1]),[35.,25.,55.,45.]);
        app.undo_object(&ctx);assert_eq!(app.layout.manual_cut_paths,original);
        app.redo_object(&ctx);assert_eq!(app.layout.manual_cut_paths.len(),2);
    }
}

impl A4Studio {
    fn select_shape(&mut self,target:crate::shape_ops::Target,additive:bool) {
        if !additive {self.shape_selection.clear();}
        if let Some(index)=self.shape_selection.iter().position(|t|*t==target) {
            if additive {self.shape_selection.remove(index);}
        }else{self.shape_selection.push(target);}
    }
    fn apply_shape_operation(&mut self,operation:crate::shape_ops::Operation) {
        use crate::shape_ops::Target;
        let paths:Vec<_>=self.shape_selection.iter().filter_map(|target|match target {
            Target::Design(i)=>self.layout.design_shapes.get(*i).map(|shape|shape.path.clone()),
            Target::Cut(i)=>self.layout.manual_cut_paths.get(*i).cloned(),
        }).collect();
        let result=match crate::shape_ops::combine(&paths,operation) {Ok(result)=>result,Err(error)=>{self.status=error;return;}};
        let style=self.shape_selection.iter().find_map(|target|match target {Target::Design(i)=>self.layout.design_shapes.get(*i).cloned(),_=>None});
        if style.as_ref().is_some_and(|s|s.print) && result.iter().any(|(_,hole)|*hole) {
            self.status="This result contains an inner hole. Use Cutline shapes for this operation; the original design shapes were kept.".into();return;
        }
        let before=self.layout.clone();let mut next=before.clone();
        let targets=self.shape_selection.clone();
        next.design_shapes=next.design_shapes.into_iter().enumerate().filter_map(|(i,s)|(!targets.contains(&Target::Design(i))).then_some(s)).collect();
        next.manual_cut_paths=next.manual_cut_paths.into_iter().enumerate().filter_map(|(i,s)|(!targets.contains(&Target::Cut(i))).then_some(s)).collect();
        let mut selected=Vec::new();
        for (path,_) in result {
            if let Some(mut shape)=style.clone() {
                shape.path=path;selected.push(Target::Design(next.design_shapes.len()));next.design_shapes.push(shape);
            }else{
                selected.push(Target::Cut(next.manual_cut_paths.len()));next.manual_cut_paths.push(path);
            }
        }
        if next.design_shapes.len()>100 {self.status="Maximum 100 design shapes per page.".into();return;}
        self.record_layer_edit(ObjectEdit::Shaping(before.design_shapes,before.manual_cut_paths));
        self.layout=next;self.shape_selection=selected;
        self.selected_design=self.shape_selection.iter().find_map(|t|if let Target::Design(i)=t{Some(*i)}else{None});
        self.selected_cut_shape=self.shape_selection.iter().find_map(|t|if let Target::Cut(i)=t{Some(*i)}else{None});
        self.design_drag=None;self.cut_shape_drag=None;self.preview_cache=None;
        self.status=format!("{} completed. Undo: Ctrl+Z.",operation.label());
    }
}

#[cfg(test)]
mod shaping_tests {
    use super::*;
    use crate::shape_ops::{Target,Operation};
    fn square(x:f64)->Vec<[f64;2]>{vec![[x,20.],[x+20.,20.],[x+20.,40.],[x,40.],[x,20.]]}
    #[test]
    fn weld_cutlines_and_undo_redo_preserve_original_geometry() {
        let mut app=A4Studio::default();let ctx=egui::Context::default();
        app.layout.manual_cut_paths=vec![square(20.),square(30.)];
        let original=app.layout.manual_cut_paths.clone();
        app.select_shape(Target::Cut(0),false);app.select_shape(Target::Cut(1),true);
        app.apply_shape_operation(Operation::Weld);
        assert_eq!(app.layout.manual_cut_paths.len(),1);
        assert_eq!(cut_tools::bounds(&app.layout.manual_cut_paths[0]),[20.,20.,50.,40.]);
        app.undo_object(&ctx);assert_eq!(app.layout.manual_cut_paths,original);
        app.redo_object(&ctx);assert_eq!(app.layout.manual_cut_paths.len(),1);
    }
    #[test]
    fn intersect_designs_preserves_style_and_export_flags_and_empty_results_keep_originals() {
        let mut app=A4Studio::default();
        let shape=sheet::DesignShape {path:square(20.),fill:Some([0,103,192,255]),stroke:[0,0,0,255],stroke_width:0.3,print:true,cut:true};
        app.layout.design_shapes=vec![shape.clone(),sheet::DesignShape {path:square(30.),..shape.clone()}];
        app.select_shape(Target::Design(0),false);app.select_shape(Target::Design(1),true);
        app.apply_shape_operation(Operation::Intersect);
        assert_eq!(app.layout.design_shapes.len(),1);
        assert_eq!(app.layout.design_shapes[0].fill,shape.fill);
        assert!(app.layout.design_shapes[0].print && app.layout.design_shapes[0].cut);
        assert_eq!(cut_tools::bounds(&app.layout.design_shapes[0].path),[30.,20.,40.,40.]);
        app.layout.design_shapes.push(sheet::DesignShape {path:square(90.),..shape});
        let before=app.layout.design_shapes.clone();
        app.select_shape(Target::Design(0),false);app.select_shape(Target::Design(1),true);
        app.apply_shape_operation(Operation::Intersect);
        assert!(app.layout.design_shapes==before);
    }
}

impl A4Studio {
    fn selected_shape_copy(&self)->Option<crate::studio_clipboard::ShapeCopy> {
        use crate::shape_ops::Target;
        let mut targets=self.shape_selection.clone();
        let active=self.selected_design.map(Target::Design).or_else(||self.selected_cut_shape.map(Target::Cut));
        if let Some(active)=active && !targets.contains(&active) {targets=vec![active];}
        if targets.is_empty() {
            if let Some(index)=self.selected_design {targets.push(Target::Design(index));}
            else if let Some(index)=self.selected_cut_shape {targets.push(Target::Cut(index));}
        }
        let designs=targets.iter().filter_map(|t|if let Target::Design(i)=t {self.layout.design_shapes.get(*i).cloned()}else{None}).collect::<Vec<_>>();
        let paths=targets.iter().filter_map(|t|if let Target::Cut(i)=t {self.layout.manual_cut_paths.get(*i).cloned()}else{None}).collect::<Vec<_>>();
        (!designs.is_empty() || !paths.is_empty()).then_some(crate::studio_clipboard::ShapeCopy {designs,paths})
    }
    fn paste_shapes(&mut self,copy:&crate::studio_clipboard::ShapeCopy) {
        use crate::shape_ops::Target;
        let mut next=match copy.compose(&self.layout) {Ok(layout)=>layout,Err(error)=>{self.status=format!("Cannot paste shapes: {error}");return;}};
        let first_design=self.layout.design_shapes.len();let first_cut=self.layout.manual_cut_paths.len();
        self.record_layer_edit(ObjectEdit::Shaping(self.layout.design_shapes.clone(),self.layout.manual_cut_paths.clone()));
        // Keep the current outline mode: pasting a vector must not change existing image cuts.
        std::mem::swap(&mut self.layout.design_shapes,&mut next.design_shapes);
        std::mem::swap(&mut self.layout.manual_cut_paths,&mut next.manual_cut_paths);
        self.clear_selection();self.design_drag=None;self.cut_shape_drag=None;
        self.shape_selection=(first_design..self.layout.design_shapes.len()).map(Target::Design).chain((first_cut..self.layout.manual_cut_paths.len()).map(Target::Cut)).collect();
        self.selected_design=(first_design<self.layout.design_shapes.len()).then_some(first_design);
        self.selected_cut_shape=(first_cut<self.layout.manual_cut_paths.len()).then_some(first_cut);
        self.pick_cut_shapes=self.selected_design.is_none();self.preview_cache=None;
        self.status=format!("Pasted {} shape(s).",copy.designs.len()+copy.paths.len());
    }
}

#[cfg(test)]
mod shape_paste_tests {
    use super::*;
    #[test]
    fn pasted_cutlines_are_selected_and_undoable_without_changing_outline_mode() {
        let ctx=egui::Context::default();let mut app=A4Studio::default();
        app.layout.manual_cut_paths=vec![vec![[20.,20.],[40.,40.]]];
        app.selected_cut_shape=Some(0);
        let copied=app.selected_shape_copy().unwrap();
        let mode=app.layout.use_manual_cut_paths;
        app.paste_shapes(&copied);
        assert_eq!(app.layout.manual_cut_paths.len(),2);assert_eq!(app.selected_cut_shape,Some(1));
        assert_eq!(app.layout.manual_cut_paths[1][0],[25.,25.]);
        assert_eq!(app.layout.use_manual_cut_paths,mode);
        app.undo_object(&ctx);assert_eq!(app.layout.manual_cut_paths.len(),1);
        app.redo_object(&ctx);assert_eq!(app.layout.manual_cut_paths.len(),2);
    }
}

impl A4Studio {
    fn toggle_layer_cutline_link(&mut self,index:usize,unlink:bool) {
        let mut next=self.layout.clone();
        let result=if unlink {
            next.unlink_image_cut_paths(index)
        }else{
            let mut selected:Vec<usize>=self.shape_selection.iter().filter_map(|target|if let crate::shape_ops::Target::Cut(i)=target {(*i<next.manual_cut_paths.len()).then_some(*i)}else{None}).collect();
            if selected.is_empty() && let Some(i)=self.selected_cut_shape.filter(|i|*i<next.manual_cut_paths.len()) {selected.push(i);}
            if selected.is_empty() {
                self.status="Select a manual cutline in Layers or on the page, then click Link on the image layer.".into();return;
            }
            selected.sort_unstable();selected.dedup();
            let paths=selected.iter().map(|i|next.manual_cut_paths[*i].clone()).collect();
            match next.link_cut_paths(index,paths) {
                Ok(())=>{
                    for i in selected.into_iter().rev() {next.manual_cut_paths.remove(i);}
                    next.use_manual_cut_paths=true;
                    next.validate_manual_cut_paths()
                }
                Err(error)=>Err(error),
            }
        };
        if let Err(error)=result {self.status=format!("Cannot {} cutline: {error}",if unlink {"unlink"}else{"link"});return;}
        self.record_manual_cut_edit();self.layout=next;
        self.selected_cut_shape=None;self.shape_selection.clear();self.cut_shape_drag=None;
        self.preview_cache=None;self.back_paths_cache=None;
        self.status=if unlink {"Manual cutline unlinked; it stays on the page and can be edited separately.".into()}else{format!("Manual cutline linked to Image {}. Move, resize, rotate and copy follow the image.",index+1)};
    }
}

#[cfg(test)]
mod layer_link_tests {
    use super::*;
    #[test]
    fn layer_link_moves_only_selected_cutline_and_undoes_link_and_unlink() {
        let ctx=egui::Context::default();let mut app=A4Studio::default();
        app.layout.count=1;app.layout.width=40.;app.layout.height=40.;
        app.layout.custom.insert(0,sheet::Placement {x:20.,y:20.,width:40.,height:40.,orientation:Default::default()});
        let first=vec![[25.,25.],[50.,25.],[50.,50.],[25.,25.]];
        let second=vec![[70.,70.],[80.,80.]];
        app.layout.manual_cut_paths=vec![first.clone(),second.clone()];app.selected_cut_shape=Some(0);
        app.toggle_layer_cutline_link(0,false);
        assert_eq!(app.layout.manual_cut_paths,vec![second.clone()]);
        assert!(app.layout.image_cut_paths.contains_key(&0));
        app.undo_object(&ctx);
        assert_eq!(app.layout.manual_cut_paths,vec![first,second]);
        assert!(app.layout.image_cut_paths.is_empty());
        app.redo_object(&ctx);
        app.toggle_layer_cutline_link(0,true);
        assert_eq!(app.layout.manual_cut_paths.len(),2);
        assert!(app.layout.image_cut_paths_for(0).unwrap().is_empty());
        app.undo_object(&ctx);assert!(!app.layout.image_cut_paths_for(0).unwrap().is_empty());
    }
}

#[cfg(test)]
mod retained_svg_tests {
    use super::*;
    #[test]
    fn imported_svg_survives_project_copy_crop_and_duplicate_metadata() {
        let ctx=egui::Context::default();let mut app=A4Studio::default();
        let imported=crate::image_import::prepare(br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><circle cx="20" cy="20" r="18" fill="red"/></svg>"#.to_vec(),"circle.svg".into()).unwrap();
        app.add_imported_image(imported,false,&ctx);
        assert!(app.layout.svg_source_for(0).is_some());
        let project=app.autosave_data().record().project;
        let restored:crate::project::Project=serde_json::from_str(&serde_json::to_string(&project).unwrap()).unwrap();
        let source=restored.source().unwrap();
        let mut loaded=A4Studio::default();loaded.install_project(restored,source,&ctx);
        assert!(loaded.layout.svg_source_for(0).is_some());
        assert!(loaded.preferences().layout.svg_sources.is_empty());
        let item=loaded.layout.editor_items().unwrap()[0];
        loaded.duplicate_images(&[(0,item)],(15.,0.),&ctx);
        assert_eq!(loaded.layout.svg_sources.len(),2);
        let copied=crate::studio_clipboard::ArtworkCopy {source:loaded.source.clone(),layout:loaded.layout.clone(),indices:vec![1]};
        let (_,pasted)=copied.compose(&Layout::default(),&[],0.).unwrap();
        assert!(pasted.svg_source_for(0).is_some());
        let asset=pasted.svg_source_for(0).unwrap();
        assert!(asset.raster(asset.raster_rect,600,600,Default::default()).is_ok());
    }
}

#[cfg(test)]
mod vector_trace_integration_tests {
    use super::*;
    #[test]
    fn applying_vector_trace_preserves_original_and_undo_redo_and_project_metadata() {
        let ctx=egui::Context::default();let mut app=A4Studio::fresh_tab();
        app.source=include_bytes!("../examples/A4-DEMO/sample-sticker.png").to_vec();app.retrace(&ctx);
        let original=app.source.clone();let pixels=&app.art.as_ref().unwrap().pixels;
        let asset=mc630_plt_studio::svg_artwork::SvgArtwork::from_bytes(br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><path fill="#0067c0" d="M0 0H40V40H0Z"/></svg>"##,[pixels.width(),pixels.height()]).unwrap();
        app.apply_vector_trace(0,asset.clone());
        assert_eq!(app.source,original);assert_eq!(app.layout.svg_sources.get(&0),Some(&asset));
        let loaded:Layout=serde_json::from_str(&serde_json::to_string(&app.layout).unwrap()).unwrap();
        assert_eq!(loaded.svg_sources.get(&0),Some(&asset));
        app.undo_object(&ctx);assert!(app.layout.svg_sources.is_empty());assert_eq!(app.source,original);
        app.redo_object(&ctx);assert_eq!(app.layout.svg_sources.get(&0),Some(&asset));
    }
}

#[cfg(test)]
mod line_tool_tests {
    use super::*;
    #[test]
    fn line_commits_print_cut_stroke_and_undo_as_an_open_path() {
        let ctx=egui::Context::default();
        let line=cut_tools::shape_path(Tool::Line,[20.,30.],[80.,30.],false,0.);
        for mode in 0..=2 {
            let mut app=A4Studio::fresh_tab();app.drawing_mode=mode;app.design_stroke_width=0.7;
            app.activate_cut_tool(Tool::Line);app.commit_manual_cut(line.clone());
            if mode==0 {assert_eq!(app.layout.manual_cut_paths,vec![line.clone()]);}
            else {
                let shape=&app.layout.design_shapes[0];assert_eq!(shape.path,line);assert!(shape.fill.is_none());
                assert!(shape.print);assert_eq!(shape.cut,mode==2);assert_eq!(shape.stroke_width,0.7);
                let svg=app.layout.design_svg_paths();assert!(svg.contains("fill=\"none\""));assert!(!svg.contains(" Z"));
                let restored:Layout=serde_json::from_str(&serde_json::to_string(&app.layout).unwrap()).unwrap();
                assert_eq!(restored.design_shapes[0].path,line);
            }
            if mode!=1 {assert!(app.layout.manual_design_plt().unwrap().contains("PD"));}
            app.undo_object(&ctx);assert!(app.layout.manual_cut_paths.is_empty());assert!(app.layout.design_shapes.is_empty());
        }
        let vertical=vec![[20.,30.],[20.,70.]];
        let resized=crate::design_transform::transform(&vertical,[0.,20.],Some(2),true,false,[210.,297.]);
        assert_eq!(cut_tools::bounds(&resized),[20.,30.,20.,90.]);
    }
}

#[cfg(test)]
mod shape_drag_profile {
    use super::*;
    #[test]
    #[ignore = "Manual CPU frame benchmark; MC630_SHAPE_PROFILE selects output"]
    fn drawing_cpu_frames() {
        let mut report=Vec::new();
        for (name,tool) in [("ellipse",Tool::Ellipse),("rectangle",Tool::Rectangle)] {
            let ctx=egui::Context::default();let mut app=A4Studio::fresh_tab();
            app.drawing_mode=1;app.cut_corner_radius=3.;
            for i in 0..40 {
                app.layout.design_shapes.push(sheet::DesignShape {
                    path:cut_tools::shape_path(Tool::Ellipse,[20.+(i%5) as f64*30.,20.+(i/5) as f64*25.],[40.+(i%5) as f64*30.,38.+(i/5) as f64*25.],false,0.),
                    fill:Some([0,103,192,255]),stroke:[0,0,0,255],stroke_width:0.3,print:true,cut:false});
            }
            let screen=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1360.,930.));
            let _=ctx.run(egui::RawInput {screen_rect:Some(screen),..Default::default()},|ctx|app.ui(ctx));
            app.activate_cut_tool(tool);
            let page=app.navigator_view.unwrap().0;
            let start=page.min+egui::vec2(35.,45.);
            let mut times=Vec::new();let mut counts=Vec::new();
            for frame in 0..100 {
                let pos=start+egui::vec2(frame as f32*1.2,frame as f32*1.4);
                let mut events=vec![egui::Event::PointerMoved(pos)];
                if frame==0 {events.push(egui::Event::PointerButton {pos,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()});}
                let now=std::time::Instant::now();
                let output=ctx.run(egui::RawInput {screen_rect:Some(screen),time:Some(frame as f64/60.),events,..Default::default()},|ctx|{app.ui(ctx);app.tick_autosave(ctx);});
                let count=output.shapes.len();
                let _=ctx.tessellate(output.shapes,output.pixels_per_point);
                if frame>=20 {times.push(now.elapsed().as_secs_f64()*1000.);counts.push(count);}
            }
            times.sort_by(f64::total_cmp);
            report.push(serde_json::json!({"tool":name,"median_cpu_ms":times[times.len()/2],"p95_cpu_ms":times[times.len()*95/100],"mean_paint_shapes":counts.iter().sum::<usize>() as f64/counts.len() as f64}));
        }
        let output=std::env::var("MC630_SHAPE_PROFILE").unwrap_or("target/shape-drag-profile.json".into());
        std::fs::write(output,serde_json::to_string_pretty(&report).unwrap()).unwrap();
        println!("{}",serde_json::to_string_pretty(&report).unwrap());
    }
}

#[cfg(test)]
mod shape_drag_autosave_tests {
    use super::*;
    #[test]
    fn shape_drag_defers_snapshot_signature_until_pointer_release() {
        let ctx=egui::Context::default();let mut app=A4Studio::fresh_tab();
        app.activate_cut_tool(Tool::Ellipse);app.cut_draft.as_mut().unwrap().drag_start=Some([20.,20.]);
        app.autosave.observed=None;
        let event=|pressed|egui::Event::PointerButton {pos:egui::pos2(100.,100.),button:egui::PointerButton::Primary,pressed,modifiers:Default::default()};
        let _=ctx.run(egui::RawInput {events:vec![event(true)],..Default::default()},|ctx|app.tick_autosave(ctx));
        assert!(app.autosave.observed.is_none());
        let _=ctx.run(egui::RawInput {events:vec![event(false)],..Default::default()},|ctx|app.tick_autosave(ctx));
        assert_eq!(app.autosave.observed,Some(app.document_signature()));
    }
}

#[cfg(test)]
mod shape_group_transform_tests {
    use super::*;
    use crate::shape_ops::Target;
    fn fixture()->A4Studio {
        let mut app=A4Studio::fresh_tab();app.rulers=false;
        app.layout.manual_cut_paths=vec![vec![[20.,20.],[40.,20.],[40.,40.],[20.,40.],[20.,20.]],vec![[60.,20.],[80.,20.],[80.,40.],[60.,40.],[60.,20.]]];
        app.shape_selection=vec![Target::Cut(0),Target::Cut(1)];app
    }
    fn frame(ctx:&egui::Context,app:&mut A4Studio,events:Vec<egui::Event>,modifiers:egui::Modifiers) {
        let page=egui::Rect::from_min_size(egui::pos2(20.,20.),egui::vec2(420.,594.));
        let _=ctx.run(egui::RawInput {events,modifiers,..Default::default()},|ctx| {
            egui::CentralPanel::default().show(ctx,|ui| {
                let response=ui.allocate_rect(page,egui::Sense::click_and_drag());
                let owned=app.shape_group_canvas(ui,&response,page,2.,false) || app.design_canvas(ui,&response,page,2.,false);
                if !owned {app.canvas_edit(ui,&response,page,2.);}
                app.cut_shape_canvas(&response,ui.painter(),page,page,2.,owned);
            });
        });
    }
    fn button(pos:egui::Pos2,pressed:bool)->egui::Event {egui::Event::PointerButton {pos,pressed,button:egui::PointerButton::Primary,modifiers:Default::default()}}
    #[test]
    fn selected_cutlines_move_together_and_undo_as_one_edit() {
        let ctx=egui::Context::default();let mut app=fixture();let original=app.layout.manual_cut_paths.clone();
        frame(&ctx,&mut app,vec![],Default::default());
        let start=egui::pos2(120.,80.);let end=egui::pos2(140.,110.);
        frame(&ctx,&mut app,vec![egui::Event::PointerMoved(start),button(start,true)],Default::default());
        frame(&ctx,&mut app,vec![egui::Event::PointerMoved(end)],Default::default());
        frame(&ctx,&mut app,vec![button(end,false)],Default::default());
        assert_eq!(cut_tools::bounds(&app.layout.manual_cut_paths[0]),[30.,35.,50.,55.]);
        assert_eq!(cut_tools::bounds(&app.layout.manual_cut_paths[1]),[70.,35.,90.,55.]);
        assert_eq!(app.shape_selection.len(),2);assert_eq!(app.undo.len(),1);
        let moved=app.layout.manual_cut_paths.clone();app.undo_object(&ctx);assert_eq!(app.layout.manual_cut_paths,original);
        app.redo_object(&ctx);assert_eq!(app.layout.manual_cut_paths,moved);
    }
    #[test]
    fn clicking_group_does_not_edit_and_shift_click_can_deselect_a_member() {
        let ctx=egui::Context::default();let mut app=fixture();let original=app.layout.manual_cut_paths.clone();
        frame(&ctx,&mut app,vec![],Default::default());
        let point=egui::pos2(80.,80.);
        frame(&ctx,&mut app,vec![egui::Event::PointerMoved(point),button(point,true)],Default::default());
        frame(&ctx,&mut app,vec![button(point,false)],Default::default());
        assert_eq!(app.shape_selection.len(),2);assert_eq!(app.layout.manual_cut_paths,original);assert!(app.undo.is_empty());
        let modifiers=egui::Modifiers {shift:true,..Default::default()};
        frame(&ctx,&mut app,vec![button(point,true)],modifiers);
        frame(&ctx,&mut app,vec![button(point,false)],modifiers);
        assert_eq!(app.shape_selection.len(),1);assert!(app.shape_selection.contains(&Target::Cut(1)));assert!(app.undo.is_empty());
        app.shape_selection=vec![Target::Cut(0),Target::Cut(1)];app.transform_shape_group(0.,false,false);
        assert!(app.undo.is_empty());
    }
    #[test]
    fn side_handle_shift_alt_resizes_about_the_shared_center() {
        let ctx=egui::Context::default();let mut app=fixture();
        frame(&ctx,&mut app,vec![],Default::default());
        let modifiers=egui::Modifiers {shift:true,alt:true,..Default::default()};
        let start=egui::pos2(180.,80.);let end=egui::pos2(200.,80.);
        frame(&ctx,&mut app,vec![egui::Event::PointerMoved(start),button(start,true)],modifiers);
        frame(&ctx,&mut app,vec![egui::Event::PointerMoved(end),button(end,false)],modifiers);
        let (_,_,points)=app.selected_shape_points();let [x0,y0,x1,y1]=cut_tools::bounds(&points);
        assert!(((x0+x1)/2.-50.).abs()<1e-8);assert!(((y0+y1)/2.-30.).abs()<1e-8);
        assert!((x1-x0-80.).abs()<1e-8);assert!(((x1-x0)/(y1-y0)-3.).abs()<1e-8);
        assert_eq!(app.undo.len(),1);
    }
    #[test]
    fn mixed_selection_resizes_with_shared_anchor_and_escape_restores_it() {
        let ctx=egui::Context::default();let mut app=fixture();
        app.layout.design_shapes.push(sheet::DesignShape {path:app.layout.manual_cut_paths.remove(0),fill:Some([10,20,200,128]),stroke:[0,0,0,255],stroke_width:0.4,print:true,cut:true});
        app.shape_selection=vec![Target::Design(0),Target::Cut(0)];
        let before_design=app.layout.design_shapes.clone();let before_cut=app.layout.manual_cut_paths.clone();
        frame(&ctx,&mut app,vec![],Default::default());
        let start=egui::pos2(180.,100.);let end=egui::pos2(220.,120.);
        frame(&ctx,&mut app,vec![egui::Event::PointerMoved(start),button(start,true),egui::Event::PointerMoved(end),button(end,false)],Default::default());
        let (_,_,points)=app.selected_shape_points();assert_eq!(cut_tools::bounds(&points),[20.,20.,100.,50.]);
        assert_eq!(app.layout.design_shapes[0].fill,before_design[0].fill);assert_eq!(app.layout.design_shapes[0].stroke_width,0.4);
        assert_eq!(app.undo.len(),1);
        let resized=app.layout.manual_cut_paths.clone();
        let start=egui::pos2(150.,90.);let end=egui::pos2(170.,120.);
        frame(&ctx,&mut app,vec![egui::Event::PointerMoved(start),button(start,true)],Default::default());
        frame(&ctx,&mut app,vec![egui::Event::PointerMoved(end)],Default::default());
        frame(&ctx,&mut app,vec![egui::Event::Key {key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}],Default::default());
        assert_eq!(app.layout.manual_cut_paths,resized);assert_eq!(app.undo.len(),1);
        app.undo_object(&ctx);assert!(app.layout.design_shapes==before_design);assert_eq!(app.layout.manual_cut_paths,before_cut);
    }
    #[test]
    fn group_rotation_and_flip_use_shared_center_and_keep_styles() {
        let ctx=egui::Context::default();let mut app=fixture();let original=app.layout.manual_cut_paths.clone();
        app.transform_shape_group(0.,true,false);
        assert_eq!(cut_tools::bounds(&app.layout.manual_cut_paths[0]),[60.,20.,80.,40.]);
        assert_eq!(cut_tools::bounds(&app.layout.manual_cut_paths[1]),[20.,20.,40.,40.]);
        app.undo_object(&ctx);assert_eq!(app.layout.manual_cut_paths,original);
        app.shape_selection=vec![Target::Cut(0),Target::Cut(1)];app.transform_shape_group(90.,false,false);
        let (_,_,points)=app.selected_shape_points();let bounds=cut_tools::bounds(&points);
        for (actual,expected) in bounds.into_iter().zip([40.,0.,60.,60.]) {assert!((actual-expected).abs()<1e-8);}
    }
}
