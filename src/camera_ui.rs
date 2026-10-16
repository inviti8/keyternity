//! Render cameras in the editor (docs/FRAME_SEQUENCES.md §4.2–4.4): drawing them on the
//! canvas, dragging their edges, handles and pivot, their properties panel, and their
//! events. The geometry is in `render_camera.rs`.

use crate::render_camera::{self, CameraLock, Handle, RenderCamera};
use crate::ui::EguiUi;
use crate::*;

/// Room left around fitted content, in output pixels.
const FIT_PADDING: f32 = 8.;
/// How close (points) the pointer must be to an edge to grab it.
const EDGE_GRAB: f32 = 6.;
/// Half the size (points) of a resize handle.
const HANDLE: f32 = 4.5;

/// A camera being dragged on the canvas.
#[derive(Clone, Debug)]
pub struct CameraDrag {
    pub id: i32,
    pub handle: Handle,
    pub start: RenderCamera,
    /// the pointer's world position when the drag started
    pub start_world: Vec2,
    /// an undo step has been made for this drag
    pub moved: bool,
}

/// Apply a camera event (`Events::AddCamera` etc).
pub fn camera_event(
    event: &Events,
    value: f32,
    str_value: &str,
    armature: &mut Armature,
    selections: &mut SelectionState,
    edit_mode: &mut EditMode,
    ui: &mut crate::Ui,
) {
    let id = value as i32;
    match event {
        Events::AddCamera => {
            let mut cam = RenderCamera {
                id: render_camera::next_id(&armature.cameras),
                name: render_camera::unique_name(&armature.cameras, "Camera"),
                ..Default::default()
            };
            if let Some((min, max)) = render_camera::pose_bounds(armature, &armature.bones) {
                cam = render_camera::fit(&cam, min, max, FIT_PADDING);
            }
            let new_id = cam.id;
            armature.cameras.push(cam);
            edit_mode.show_cameras = true;
            select(new_id, selections, edit_mode, ui);
        }
        Events::SelectCamera => {
            if id < 0 {
                selections.camera = None;
                if ui.last_selected == "camera" {
                    ui.last_selected = "".to_string();
                }
            } else {
                select(id, selections, edit_mode, ui);
            }
        }
        Events::SetCamera => {
            let Ok(cam) = serde_json::from_str::<RenderCamera>(str_value) else { return };
            if let Some(c) = armature.cameras.iter_mut().find(|c| c.id == cam.id) {
                *c = cam;
            }
        }
        Events::DeleteCamera => {
            armature.cameras.retain(|c| c.id != id);
            if selections.camera == Some(id) {
                selections.camera = None;
                ui.last_selected = "".to_string();
            }
        }
        Events::DuplicateCamera => {
            let Some(src) = armature.cameras.iter().find(|c| c.id == id) else { return };
            let mut cam = src.clone();
            cam.id = render_camera::next_id(&armature.cameras);
            cam.name = render_camera::unique_name(&armature.cameras, &src.name);
            // offset, so the copy is visible
            cam.pos += Vec2::new(1., -1.) * (cam.world_size().y * 0.05);
            let new_id = cam.id;
            armature.cameras.push(cam);
            select(new_id, selections, edit_mode, ui);
        }
        Events::FitCamera => {
            let bounds = if str_value == "anims" {
                let all: Vec<usize> = (0..armature.animations.len()).collect();
                render_camera::animation_bounds(armature, &all)
            } else {
                render_camera::pose_bounds(armature, &armature.bones)
            };
            let Some((min, max)) = bounds else { return };
            if let Some(c) = armature.cameras.iter_mut().find(|c| c.id == id) {
                *c = render_camera::fit(c, min, max, FIT_PADDING);
            }
        }
        _ => {}
    }
}

/// Select the camera `id`, deselecting bones.
fn select(id: i32, selections: &mut SelectionState, edit_mode: &mut EditMode, ui: &mut crate::Ui) {
    selections.bone_idx = usize::MAX;
    selections.bone_ids = vec![];
    selections.vert_ids = vec![];
    selections.bind = -1;
    edit_mode.showing_mesh = false;
    edit_mode.editing_mesh = false;
    edit_mode.setting_ik_target = false;
    edit_mode.setting_bind_bone = false;
    // cameras are dragged with the Transform tool: leave Pan, Zoom and the mesh tools
    if edit_mode.tool.edits_mesh() || edit_mode.tool.navigates() {
        edit_mode.tool = Tool::Transform;
    }
    selections.camera = Some(id);
    ui.last_selected = "camera".to_string();
}

/// World and screen (egui points) conversions for the canvas this frame.
struct View {
    cam: Camera,
    aspect: f32,
    window: Vec2,
    ppp: f32,
}

impl View {
    fn new(ctx: &egui::Context, camera: &Camera, config: &Config) -> View {
        View {
            cam: renderer::world_camera(camera, config),
            aspect: camera.aspect_ratio(),
            window: camera.window,
            ppp: ctx.pixels_per_point(),
        }
    }

    fn to_pt(&self, world: Vec2) -> egui::Pos2 {
        let clip = Vec2::new(
            (world.x - self.cam.pos.x) / self.cam.zoom * self.aspect,
            (world.y - self.cam.pos.y) / self.cam.zoom,
        );
        let px = mesh_tools::to_screen(clip, self.window);
        egui::pos2(px.x / self.ppp, px.y / self.ppp)
    }

    fn to_world(&self, pt: egui::Pos2) -> Vec2 {
        navigation::screen_to_world(&self.cam, Vec2::new(pt.x * self.ppp, pt.y * self.ppp))
    }

    fn rect(&self, cam: &RenderCamera) -> egui::Rect {
        egui::Rect::from_two_pos(self.to_pt(cam.min()), self.to_pt(cam.max()))
    }
}

/// A camera's label: its name and output size.
fn label_text(cam: &RenderCamera) -> String {
    let (w, h) = cam.px_size();
    if (cam.scale - 1.).abs() < 1e-4 {
        format!("{}  {w}×{h}", cam.name)
    } else {
        format!("{}  {w}×{h} @ {}", cam.name, (cam.scale * 1000.).round() / 1000.)
    }
}

fn label_font() -> egui::FontId {
    egui::FontId::proportional(12.)
}

/// The label's rectangle: above the camera's top-left corner.
fn label_rect(ctx: &egui::Context, view: &View, cam: &RenderCamera) -> egui::Rect {
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label_text(cam), label_font(), egui::Color32::WHITE));
    let r = view.rect(cam);
    let size = galley.size() + egui::vec2(10., 4.);
    egui::Rect::from_min_size(egui::pos2(r.left(), r.top() - size.y - 2.), size)
}

/// The part of a camera under `pt`, if any. The selected camera's handles and pivot
/// come first, then labels, then edges.
fn hit(ctx: &egui::Context, view: &View, cameras: &[RenderCamera], selected: Option<i32>, pt: egui::Pos2) -> Option<(i32, Handle)> {
    if let Some(cam) = cameras.iter().find(|c| Some(c.id) == selected) {
        if view.to_pt(cam.pivot).distance(pt) <= HANDLE * 2. {
            return Some((cam.id, Handle::Pivot));
        }
        for (handle, at) in cam.handles() {
            let r = egui::Rect::from_center_size(view.to_pt(at), egui::vec2(HANDLE * 2. + 4., HANDLE * 2. + 4.));
            if r.contains(pt) {
                return Some((cam.id, handle));
            }
        }
    }
    // the selected camera first, so it wins where cameras overlap
    let mut order: Vec<&RenderCamera> = cameras.iter().collect();
    order.sort_by_key(|c| Some(c.id) != selected);
    for cam in &order {
        if label_rect(ctx, view, cam).contains(pt) {
            return Some((cam.id, Handle::Move));
        }
    }
    for cam in &order {
        let r = view.rect(cam);
        let near_x = pt.x >= r.left() - EDGE_GRAB && pt.x <= r.right() + EDGE_GRAB;
        let near_y = pt.y >= r.top() - EDGE_GRAB && pt.y <= r.bottom() + EDGE_GRAB;
        let on_v = near_y && ((pt.x - r.left()).abs() <= EDGE_GRAB || (pt.x - r.right()).abs() <= EDGE_GRAB);
        let on_h = near_x && ((pt.y - r.top()).abs() <= EDGE_GRAB || (pt.y - r.bottom()).abs() <= EDGE_GRAB);
        if on_v || on_h {
            return Some((cam.id, Handle::Move));
        }
    }
    None
}

fn cursor(handle: Handle) -> egui::CursorIcon {
    use egui::CursorIcon as C;
    match handle {
        Handle::Corner(0) | Handle::Corner(2) => C::ResizeNwSe,
        Handle::Corner(_) => C::ResizeNeSw,
        Handle::Side(0) | Handle::Side(2) => C::ResizeVertical,
        Handle::Side(_) => C::ResizeHorizontal,
        Handle::Move => C::Move,
        Handle::Pivot => C::Crosshair,
    }
}

/// Draw the render cameras on the canvas and handle dragging them. Returns whether the
/// pointer is over (or dragging) a camera, so the canvas leaves the click alone.
pub fn overlay(
    ctx: &egui::Context,
    shared_ui: &mut crate::Ui,
    config: &Config,
    camera: &Camera,
    armature: &Armature,
    selections: &SelectionState,
    edit_mode: &EditMode,
    events: &mut EventState,
) -> bool {
    if !edit_mode.show_cameras || armature.cameras.is_empty() {
        shared_ui.camera_drag = None;
        return false;
    }
    let canvas = ctx.available_rect();
    let view = View::new(ctx, camera, config);
    let selected = selections.camera.filter(|id| armature.cameras.iter().any(|c| c.id == *id));
    let painter = ctx
        .layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("render_cameras")))
        .with_clip_rect(canvas);

    let accent: egui::Color32 = config.colors.light_accent.into();
    let text: egui::Color32 = config.colors.text.into();
    let dim = text.gamma_multiply(0.55);

    // outside the selected camera is dimmed, so its frame reads as the render
    if let Some(cam) = armature.cameras.iter().find(|c| Some(c.id) == selected) {
        let r = view.rect(cam).intersect(canvas);
        let shade = egui::Color32::from_black_alpha(110);
        let c = canvas;
        for part in [
            egui::Rect::from_min_max(c.min, egui::pos2(c.max.x, r.top())),
            egui::Rect::from_min_max(egui::pos2(c.min.x, r.bottom()), c.max),
            egui::Rect::from_min_max(egui::pos2(c.min.x, r.top()), egui::pos2(r.left(), r.bottom())),
            egui::Rect::from_min_max(egui::pos2(r.right(), r.top()), egui::pos2(c.max.x, r.bottom())),
        ] {
            if part.is_positive() {
                painter.rect_filled(part, 0., shade);
            }
        }
    }

    for cam in &armature.cameras {
        let is_sel = Some(cam.id) == selected;
        let r = view.rect(cam);
        let color = if is_sel { accent } else { dim };
        let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        let stroke = egui::Stroke::new(if is_sel { 1.5 } else { 1. }, color);
        painter.extend(egui::Shape::dashed_line(&corners, stroke, 6., 4.));

        // label
        let lr = label_rect(ctx, &view, cam);
        let fill: egui::Color32 = if is_sel { config.colors.light_accent } else { config.colors.dark_accent }.into();
        painter.rect_filled(lr, 3., fill.gamma_multiply(0.9));
        painter.text(lr.center(), egui::Align2::CENTER_CENTER, label_text(cam), label_font(), text);

        if is_sel {
            for (_, at) in cam.handles() {
                let hr = egui::Rect::from_center_size(view.to_pt(at), egui::vec2(HANDLE * 2., HANDLE * 2.));
                painter.rect_filled(hr, 1., accent);
                painter.rect_stroke(hr, 1., egui::Stroke::new(1., egui::Color32::BLACK), egui::StrokeKind::Outside);
            }
            // pivot: a crosshair in a circle
            let p = view.to_pt(cam.pivot);
            let s = egui::Stroke::new(1.5, accent);
            painter.circle_stroke(p, HANDLE * 1.6, s);
            painter.line_segment([p - egui::vec2(HANDLE * 2.6, 0.), p + egui::vec2(HANDLE * 2.6, 0.)], s);
            painter.line_segment([p - egui::vec2(0., HANDLE * 2.6), p + egui::vec2(0., HANDLE * 2.6)], s);
        }
    }

    // dragging: only with the Transform tool, so Pan, Zoom and the mesh tools are unaffected
    let (pointer, pressed, down) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.primary_pressed(), i.pointer.primary_down()));
    if !down {
        shared_ui.camera_drag = None;
    }
    if let (Some(drag), Some(pt)) = (shared_ui.camera_drag.as_mut(), pointer) {
        let delta = view.to_world(pt) - drag.start_world;
        let moved = render_camera::drag(&drag.start, drag.handle, delta);
        if armature.cameras.iter().find(|c| c.id == drag.id) != Some(&moved) {
            events.set_camera(&moved, !drag.moved);
            drag.moved = true;
        }
        ctx.set_cursor_icon(cursor(drag.handle));
        return true;
    }

    let usable = edit_mode.tool == Tool::Transform && !ctx.is_pointer_over_area() && ctx.dragged_id().is_none();
    let Some(pt) = pointer.filter(|pt| usable && canvas.contains(*pt)) else { return false };
    let Some((id, handle)) = hit(ctx, &view, &armature.cameras, selected, pt) else { return false };
    ctx.set_cursor_icon(cursor(handle));
    if pressed {
        if selected != Some(id) {
            events.select_camera(id);
        }
        let start = armature.cameras.iter().find(|c| c.id == id).unwrap().clone();
        shared_ui.camera_drag = Some(CameraDrag {
            id,
            handle,
            start,
            start_world: view.to_world(pt),
            moved: false,
        });
    }
    true
}

/// The render cameras in the hierarchy, under the bones: click to select, double-click
/// to rename, + to add one.
pub fn hierarchy(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    config: &Config,
    armature: &Armature,
    selections: &SelectionState,
    events: &mut EventState,
) {
    let mut muted = config.colors.text;
    muted -= Color::new(60, 60, 60, 0);
    ui.add_space(8.);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(shared_ui.loc("armature_panel.cameras")).color(muted).size(12.));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let add = ui.add(egui::Button::new("+").small().frame(false));
            if add.on_hover_text(shared_ui.loc("top_bar.view.add_camera")).clicked() {
                events.add_camera();
            }
        });
    });

    let width = ui.available_width();
    let icon = shared_ui.toolbar_icons.get(crate::ui::CAMERA_ICON).cloned();
    for cam in &armature.cameras {
        let rename_id = format!("camera_{}", cam.id);
        if shared_ui.rename_id == rename_id {
            let options = Some(crate::ui::TextInputOptions {
                size: Vec2::new(width, 21.),
                focus: true,
                default: cam.name.clone(),
                ..Default::default()
            });
            let (done, name, _) = ui.text_input(rename_id, shared_ui, cam.name.clone(), options);
            if done && !name.trim().is_empty() && name.trim() != cam.name {
                let mut renamed = cam.clone();
                renamed.name = name.trim().to_string();
                events.set_camera(&renamed, true);
            }
            continue;
        }

        let selected = selections.camera == Some(cam.id);
        let mut fill = config.colors.dark_accent;
        if selected {
            fill += Color::new(20, 20, 20, 0);
        }
        let row = egui::Frame::new().fill(fill.into()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_width(width);
                ui.set_height(21.);
                ui.add_space(5.);
                if let Some(tex) = &icon {
                    let tint: egui::Color32 = if selected { config.colors.light_accent } else { config.colors.text }.into();
                    ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(16., 16.)).tint(tint));
                }
                ui.label(egui::RichText::new(&cam.name).color(config.colors.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(4.);
                    let (w, h) = cam.px_size();
                    ui.label(egui::RichText::new(format!("{w}×{h}")).color(muted).size(11.));
                });
            });
        });
        let response = ui
            .interact(row.response.rect, egui::Id::new(("camera_row", cam.id)), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.double_clicked() {
            shared_ui.rename_id = rename_id;
        } else if response.clicked() {
            events.select_camera(cam.id);
        }
    }
}

/// The selected camera's properties (docs/FRAME_SEQUENCES.md §4.3).
pub fn panel(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    config: &Config,
    armature: &Armature,
    selections: &SelectionState,
    events: &mut EventState,
) {
    let Some(cam) = selections.camera.and_then(|id| armature.cameras.iter().find(|c| c.id == id)) else {
        ui.disable();
        return;
    };
    let loc = |shared_ui: &crate::Ui, key: &str| shared_ui.loc(&format!("camera_panel.{key}"));
    let mut edited = cam.clone();
    let input_widths = 120.;
    let opts = |drag: f32| {
        Some(crate::ui::TextInputOptions {
            size: Vec2::new(40., 20.),
            drag_modifier: drag,
            ..Default::default()
        })
    };

    ui.heading(loc(shared_ui, "heading"));
    ui.add_space(5.);

    ui.horizontal(|ui| {
        ui.label(loc(shared_ui, "name"));
        let (done, name, _) = ui.text_input("camera_name".into(), shared_ui, cam.name.clone(), None);
        if done && !name.trim().is_empty() {
            edited.name = name.trim().to_string();
        }
    });

    macro_rules! pair {
        ($label:expr, $id:expr, $a:expr, $b:expr, $la:expr, $lb:expr, $drag:expr) => {
            ui.horizontal(|ui| {
                ui.label($label);
                ui.add_space(ui.available_width() - input_widths);
                ui.label($la);
                let (e, v, _) = ui.float_input(format!("{}_a", $id), shared_ui, $a, 1., opts($drag));
                if e {
                    $a = v;
                }
                ui.label($lb);
                let (e, v, _) = ui.float_input(format!("{}_b", $id), shared_ui, $b, 1., opts($drag));
                if e {
                    $b = v;
                }
            });
        };
    }
    pair!(loc(shared_ui, "position"), "camera_pos", edited.pos.x, edited.pos.y, "X", "Y", 1.);
    pair!(loc(shared_ui, "resolution"), "camera_res", edited.resolution.x, edited.resolution.y, "W", "H", 1.);
    ui.horizontal(|ui| {
        ui.label(loc(shared_ui, "scale")).on_hover_text(loc(shared_ui, "scale_desc"));
        ui.add_space(ui.available_width() - 40.);
        let (e, v, _) = ui.float_input("camera_scale".into(), shared_ui, edited.scale, 1., opts(0.01));
        if e && v > 0. {
            edited.scale = v;
        }
    });
    pair!(loc(shared_ui, "pivot"), "camera_pivot", edited.pivot.x, edited.pivot.y, "X", "Y", 1.);

    ui.horizontal(|ui| {
        ui.label(loc(shared_ui, "lock")).on_hover_text(loc(shared_ui, "lock_desc"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let name = |l: CameraLock| match l {
                CameraLock::Scale => loc(shared_ui, "lock_scale"),
                CameraLock::Resolution => loc(shared_ui, "lock_resolution"),
            };
            egui::ComboBox::new("camera_lock", "")
                .selected_text(name(edited.lock))
                .width(90.)
                .show_ui(ui, |ui| {
                    for l in [CameraLock::Scale, CameraLock::Resolution] {
                        ui.selectable_value(&mut edited.lock, l, name(l));
                    }
                });
        });
    });

    ui.horizontal(|ui| {
        ui.label(loc(shared_ui, "round_to")).on_hover_text(loc(shared_ui, "round_to_desc"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::new("camera_round", "")
                .selected_text(edited.round_to.to_string())
                .width(50.)
                .show_ui(ui, |ui| {
                    for n in [1, 2, 4, 8, 16] {
                        ui.selectable_value(&mut edited.round_to, n, n.to_string());
                    }
                });
        });
    });

    ui.horizontal(|ui| {
        ui.label(loc(shared_ui, "background"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut transparent = edited.background.is_none();
            if let Some(bg) = &mut edited.background {
                let mut col = [bg.r, bg.g, bg.b];
                ui.color_edit_button_srgb(&mut col);
                *bg = Color::new(col[0], col[1], col[2], 255);
            }
            if ui.checkbox(&mut transparent, loc(shared_ui, "transparent")).changed() {
                edited.background = if transparent { None } else { Some(config.colors.background) };
            }
        });
    });

    // what the numbers mean, in output pixels
    let world = cam.world_size();
    let pivot = cam.pivot_px();
    let info = loc(shared_ui, "info")
        .replace("$w", &format!("{:.0}", world.x))
        .replace("$h", &format!("{:.0}", world.y))
        .replace("$px", &format!("{:.0}", pivot.x))
        .replace("$py", &format!("{:.0}", pivot.y));
    ui.add_space(4.);
    let text: egui::Color32 = config.colors.text.into();
    ui.label(egui::RichText::new(info).color(text.gamma_multiply(0.7)));
    ui.add_space(8.);

    ui.horizontal_wrapped(|ui| {
        if ui.skf_button(loc(shared_ui, "fit_rest")).on_hover_text(loc(shared_ui, "fit_rest_desc")).clicked() {
            events.fit_camera(cam.id, false);
        }
        let has_anims = !armature.animations.is_empty();
        let fit_anims = ui.add_enabled_ui(has_anims, |ui| ui.skf_button(loc(shared_ui, "fit_anims")));
        if fit_anims.inner.on_hover_text(loc(shared_ui, "fit_anims_desc")).clicked() {
            events.fit_camera(cam.id, true);
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui.skf_button(loc(shared_ui, "duplicate")).clicked() {
            events.duplicate_camera(cam.id);
        }
        if ui.skf_button(loc(shared_ui, "delete")).clicked() {
            events.delete_camera(cam.id);
        }
    });

    // resolution edits obey rounding; scale edits keep the world size under Resolution lock
    // dragging a field edits every frame: only its first edit makes an undo step
    let continuing = shared_ui.edited_dragging && shared_ui.camera_field_dragging;
    if edited != *cam {
        if edited.resolution != cam.resolution || edited.round_to != cam.round_to {
            edited.round();
        }
        events.set_camera(&edited, !continuing);
        shared_ui.camera_field_dragging = shared_ui.edited_dragging;
    }
    if !shared_ui.started_edit_dragging {
        shared_ui.camera_field_dragging = false;
    }
}
