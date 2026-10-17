//! The Face Rig panel (docs/LIP_SYNC.md §6.5): FaceBot's mask, with the coloured
//! manipulators the artist drags to pose the face. The manipulators drive the rig's
//! control bones, which stay hidden plumbing: keyed, layered and exported like bones.
//!
//! The graphics come from `assets/face_rig/facebot.json`, made from FaceBot's FBX by
//! `tools/facebot_rig.py`: 2D triangles in the mask's units, x across (the
//! character's left is +x) and y up.

use crate::*;
use std::sync::OnceLock;

#[derive(serde::Deserialize)]
pub struct RigArt {
    pub name: String,
    pub mask: Vec<ArtPart>,
    pub controls: Vec<ArtControl>,
}

#[derive(serde::Deserialize)]
pub struct ArtPart {
    pub color: [u8; 3],
    pub tris: Vec<[[f32; 2]; 3]>,
}

#[derive(serde::Deserialize)]
pub struct ArtControl {
    pub name: String,
    pub color: [u8; 3],
    pub rest: [f32; 2],
    pub tris: Vec<[[f32; 2]; 3]>,
}

/// The built-in FaceBot panel.
pub fn art() -> &'static RigArt {
    static ART: OnceLock<RigArt> = OnceLock::new();
    ART.get_or_init(|| serde_json::from_str(include_str!("../assets/face_rig/facebot.json")).expect("face rig art"))
}

/// The rigs in the armature: each root bone with its controls (by control name).
pub fn rigs(armature: &Armature) -> Vec<(i32, Vec<(String, i32)>)> {
    let mut out: Vec<(i32, Vec<(String, i32)>)> = vec![];
    for b in armature.bones.iter().filter(|b| b.face_control.is_some()) {
        let mut root = b.parent_id;
        while let Some(p) = armature.bones.iter().find(|p| p.id == root && p.face_control.is_some()) {
            root = p.parent_id;
        }
        let name = b.face_control.as_ref().unwrap().name.clone();
        match out.iter_mut().find(|(r, _)| *r == root) {
            Some((_, list)) => list.push((name, b.id)),
            None => out.push((root, vec![(name, b.id)])),
        }
    }
    out
}

/// A control's offset on the panel (mask units) and twist, from its bone's pose,
/// plus its parent controls' offsets (the mouth carries the corners, lips and jaw).
pub fn placement(armature: &Armature, posed: &[Bone], bone_id: i32) -> (egui::Vec2, f32) {
    let mut off = egui::Vec2::ZERO;
    let mut twist = 0.;
    let mut id = bone_id;
    let mut first = true;
    while let Some(b) = posed.iter().find(|b| b.id == id) {
        let Some(fc) = armature.bones.iter().find(|x| x.id == id).and_then(|x| x.face_control.as_ref()) else { break };
        let s = (fc.range / face::RANGE).max(1e-4);
        let d = (b.pos - fc.rest) / s;
        off += egui::vec2(d.x, d.y);
        if first {
            twist = b.rot - fc.rest_rot;
            first = false;
        }
        id = b.parent_id;
    }
    (off, twist)
}

fn tri_contains(p: egui::Pos2, t: &[egui::Pos2; 3]) -> bool {
    let s = |a: egui::Pos2, b: egui::Pos2, c: egui::Pos2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let (d1, d2, d3) = (s(t[0], t[1], p), s(t[1], t[2], p), s(t[2], t[0], p));
    let neg = d1 < 0. || d2 < 0. || d3 < 0.;
    let pos = d1 > 0. || d2 > 0. || d3 > 0.;
    !(neg && pos)
}

/// Draw the panel window, while it's open.
pub fn draw(
    ctx: &egui::Context,
    shared_ui: &mut crate::Ui,
    config: &Config,
    armature: &Armature,
    selections: &SelectionState,
    edit_mode: &EditMode,
    events: &mut EventState,
) {
    if !shared_ui.face_panel_open {
        return;
    }
    let all = rigs(armature);
    if all.is_empty() {
        shared_ui.face_panel_open = false;
        return;
    }
    // the rig: the chosen one, else the selection's, else the first
    let root = shared_ui
        .face_panel_root
        .filter(|r| all.iter().any(|(x, _)| x == r))
        .or_else(|| {
            all.iter()
                .find(|(_, cs)| cs.iter().any(|(_, id)| selections.bone_ids.contains(id)))
                .map(|(r, _)| *r)
        })
        .unwrap_or(all[0].0);
    let controls = all.iter().find(|(r, _)| *r == root).map(|(_, c)| c.clone()).unwrap_or_default();
    let loc = |shared_ui: &crate::Ui, k: &str| shared_ui.loc(&format!("face_panel.{k}"));
    let mut open = true;
    let title = loc(shared_ui, "title");
    egui::Window::new(title)
        .id(egui::Id::new("face_rig_panel"))
        .open(&mut open)
        .default_size([340., 420.])
        .resizable(true)
        .collapsible(true)
        .show(ctx, |ui| {
            if all.len() > 1 {
                let name = |r: i32| armature.bones.iter().find(|b| b.id == r).map_or("?".to_string(), |b| b.name.clone());
                egui::ComboBox::from_id_salt("face_panel_rig").selected_text(name(root)).show_ui(ui, |ui| {
                    for (r, _) in &all {
                        if ui.selectable_label(*r == root, name(*r)).clicked() {
                            shared_ui.face_panel_root = Some(*r);
                        }
                    }
                });
            }
            board(ui, shared_ui, config, armature, selections, edit_mode, events, &controls);
            ui.label(egui::RichText::new(loc(shared_ui, "hint")).size(11.).color(egui::Color32::from_gray(150)));
        });
    if !open {
        shared_ui.face_panel_open = false;
    }
}

fn board(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    config: &Config,
    armature: &Armature,
    selections: &SelectionState,
    edit_mode: &EditMode,
    events: &mut EventState,
    controls: &[(String, i32)],
) {
    let art = art();
    // fit the mask in the space left
    let (mut lo, mut hi) = (egui::pos2(f32::MAX, f32::MAX), egui::pos2(f32::MIN, f32::MIN));
    for t in art.mask.iter().flat_map(|m| &m.tris).chain(art.controls.iter().flat_map(|c| &c.tris)) {
        for p in t {
            lo = egui::pos2(lo.x.min(p[0]), lo.y.min(p[1]));
            hi = egui::pos2(hi.x.max(p[0]), hi.y.max(p[1]));
        }
    }
    let lo = egui::pos2(lo.x - 1.5, lo.y - 4.);
    let hi = egui::pos2(hi.x + 1.5, hi.y + 1.);
    let avail = ui.available_size() - egui::vec2(0., 22.);
    let scale = (avail.x / (hi.x - lo.x)).min(avail.y / (hi.y - lo.y)).max(1.);
    let size = egui::vec2((hi.x - lo.x) * scale, (hi.y - lo.y) * scale);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let to_screen = |x: f32, y: f32| egui::pos2(rect.left() + (x - lo.x) * scale, rect.top() + (hi.y - y) * scale);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::CornerRadius::same(4), egui::Color32::from_rgb(28, 28, 32));

    let mesh_of = |tris: &[[[f32; 2]; 3]], color: egui::Color32, map: &dyn Fn(f32, f32) -> egui::Pos2| {
        let mut m = egui::Mesh::default();
        for t in tris {
            let base = m.vertices.len() as u32;
            for p in t {
                m.colored_vertex(map(p[0], p[1]), color);
            }
            m.add_triangle(base, base + 1, base + 2);
        }
        m
    };
    for part in &art.mask {
        let c = egui::Color32::from_rgb(part.color[0], part.color[1], part.color[2]);
        painter.add(egui::Shape::mesh(mesh_of(&part.tris, c, &|x, y| to_screen(x, y))));
    }

    let posed: &[Bone] = if armature.animated_bones.len() == armature.bones.len() { &armature.animated_bones } else { &armature.bones };
    let animating = shared_ui.is_animating(edit_mode, selections);
    let frame = selections.anim_frame;
    let anim = armature.animations.get(selections.anim);
    let pointer = ui.ctx().pointer_hover_pos().filter(|p| rect.contains(*p));

    // each control's screen triangles, in draw order
    struct Placed<'a> {
        art: &'a ArtControl,
        bone: i32,
        tris: Vec<[egui::Pos2; 3]>,
        rest: egui::Pos2,
    }
    let mut placed: Vec<Placed> = vec![];
    for a in &art.controls {
        let Some((_, bone)) = controls.iter().find(|(n, _)| *n == a.name) else { continue };
        let (off, twist) = placement(armature, posed, *bone);
        let (c, s) = (twist.cos(), twist.sin());
        let at = |x: f32, y: f32| {
            let (rx, ry) = (x * c - y * s, x * s + y * c);
            to_screen(a.rest[0] + off.x + rx, a.rest[1] + off.y + ry)
        };
        let tris = a.tris.iter().map(|t| [at(t[0][0], t[0][1]), at(t[1][0], t[1][1]), at(t[2][0], t[2][1])]).collect();
        placed.push(Placed { art: a, bone: *bone, tris, rest: to_screen(a.rest[0], a.rest[1]) });
    }
    // the topmost control under the pointer
    let hovered = pointer.and_then(|p| placed.iter().rev().find(|pl| pl.tris.iter().any(|t| tri_contains(p, t))).map(|pl| pl.bone));
    let active = shared_ui.face_panel_drag.or(hovered);

    for pl in &placed {
        let base = egui::Color32::from_rgb(pl.art.color[0], pl.art.color[1], pl.art.color[2]);
        let lit = active == Some(pl.bone);
        let color = if lit { base.lerp_to_gamma(egui::Color32::WHITE, 0.35) } else { base };
        let mut m = egui::Mesh::default();
        for t in &pl.tris {
            let i = m.vertices.len() as u32;
            for p in t {
                m.colored_vertex(*p, color);
            }
            m.add_triangle(i, i + 1, i + 2);
        }
        painter.add(egui::Shape::mesh(m));
        // keyed on this frame: a diamond at its rest
        let keyed = animating && anim.is_some_and(|a| a.keyframes.iter().any(|k| k.bone_id == pl.bone && k.frame == frame));
        if keyed {
            let r = 4.;
            let c = pl.rest;
            let pts = vec![c + egui::vec2(0., -r), c + egui::vec2(r, 0.), c + egui::vec2(0., r), c + egui::vec2(-r, 0.)];
            painter.add(egui::Shape::convex_polygon(pts, egui::Color32::WHITE, egui::Stroke::new(1., egui::Color32::BLACK)));
        }
    }

    // the active control's travel box, and its name
    if let Some(pl) = placed.iter().find(|pl| Some(pl.bone) == active) {
        if let Some(fc) = armature.bones.iter().find(|b| b.id == pl.bone).and_then(|b| b.face_control.as_ref()) {
            let has = |a: ControlAxis| fc.slots.iter().any(|s| s.axis == a && !s.targets.is_empty());
            let r = face::RANGE * scale;
            let x0 = if has(ControlAxis::Left) { -r } else { 0. };
            let x1 = if has(ControlAxis::Right) { r } else { 0. };
            let y0 = if has(ControlAxis::Up) { -r } else { 0. };
            let y1 = if has(ControlAxis::Down) { r } else { 0. };
            // the box moves with the control's parents
            let (parent_off, _) = armature.bones.iter().find(|b| b.id == pl.bone).map_or((egui::Vec2::ZERO, 0.), |b| placement(armature, posed, b.parent_id));
            let o = pl.rest + egui::vec2(parent_off.x, -parent_off.y) * scale;
            let boxr = egui::Rect::from_min_max(o + egui::vec2(x0, y0), o + egui::vec2(x1, y1)).expand(1.);
            painter.rect_stroke(boxr, egui::CornerRadius::same(2), egui::Stroke::new(1., egui::Color32::from_white_alpha(140)), egui::StrokeKind::Middle);
            painter.text(rect.left_top() + egui::vec2(6., 4.), egui::Align2::LEFT_TOP, &fc.name, egui::FontId::proportional(12.), egui::Color32::from_gray(220));
        }
        shared_ui.cursor_icon = if shared_ui.face_panel_drag.is_some() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab };
    }

    // dragging: move (or with Shift, twist) the control, within its travel
    let bone_of = |id: i32| armature.bones.iter().find(|b| b.id == id);
    let anim_idx = if animating { selections.anim } else { usize::MAX };
    // without an animation frame, edits change the rest pose
    let frame = if animating { frame } else { -1 };
    if resp.drag_started() {
        shared_ui.face_panel_drag = hovered;
        if let Some(idx) = hovered.and_then(|id| armature.bones.iter().position(|b| b.id == id)) {
            events.save_edited_bone(idx);
        }
    }
    if let Some(id) = shared_ui.face_panel_drag {
        if resp.dragged() {
            let (Some(bone), Some(pose)) = (bone_of(id), posed.iter().find(|b| b.id == id)) else { return };
            let fc = bone.face_control.as_ref().unwrap();
            let s = fc.range / face::RANGE;
            let d = resp.drag_delta();
            let twisting = ui.input(|i| i.modifiers.shift);
            let has = |a: ControlAxis| fc.slots.iter().any(|sl| sl.axis == a && !sl.targets.is_empty());
            if twisting {
                let lo = if has(ControlAxis::TwistCw) { -1.5 } else { 0. } * std::f32::consts::FRAC_PI_4;
                let hi = if has(ControlAxis::TwistCcw) { 1.5 } else { 0. } * std::f32::consts::FRAC_PI_4;
                let rot = (pose.rot - fc.rest_rot - d.x * 0.01).clamp(lo, hi) + fc.rest_rot;
                events.edit_bone(id, &AnimElement::Rotation, rot, "", anim_idx, frame);
            } else {
                let lim = |neg: ControlAxis, pos: ControlAxis| {
                    (if has(neg) { -1.5 * fc.range } else { 0. }, if has(pos) { 1.5 * fc.range } else { 0. })
                };
                let (xl, xh) = lim(ControlAxis::Left, ControlAxis::Right);
                let (yl, yh) = lim(ControlAxis::Down, ControlAxis::Up);
                let off = pose.pos - fc.rest + Vec2::new(d.x / scale * s, -d.y / scale * s);
                let x = fc.rest.x + off.x.clamp(xl, xh);
                let y = fc.rest.y + off.y.clamp(yl, yh);
                events.edit_bone(id, &AnimElement::PositionX, x, "", anim_idx, frame);
                events.edit_bone(id, &AnimElement::PositionY, y, "", anim_idx, frame);
            }
        }
        if resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            shared_ui.face_panel_drag = None;
        }
    }

    // double-click: back to rest; right-click: the control's menu
    if resp.double_clicked() {
        if let Some(id) = hovered {
            reset(armature, id, anim_idx, frame, events);
        }
    }
    if resp.secondary_clicked() {
        shared_ui.face_panel_menu = hovered;
    }
    if let Some(id) = shared_ui.face_panel_menu {
        let mut close = false;
        resp.context_menu(|ui| {
            if ui.button(shared_ui.loc("face_panel.reset")).clicked() {
                reset(armature, id, anim_idx, frame, events);
                close = true;
            }
            if animating && frame >= 0 && ui.button(shared_ui.loc("face_panel.key")).clicked() {
                if let (Some(idx), Some(p)) = (armature.bones.iter().position(|b| b.id == id), posed.iter().find(|b| b.id == id)) {
                    events.save_edited_bone(idx);
                    events.edit_bone(id, &AnimElement::PositionX, p.pos.x, "", anim_idx, frame);
                    events.edit_bone(id, &AnimElement::PositionY, p.pos.y, "", anim_idx, frame);
                    events.edit_bone(id, &AnimElement::Rotation, p.rot, "", anim_idx, frame);
                }
                close = true;
            }
            if ui.button(shared_ui.loc("face_panel.select")).clicked() {
                if let Some(idx) = armature.bones.iter().position(|b| b.id == id) {
                    events.select_bone(idx, false);
                }
                close = true;
            }
        });
        if close {
            shared_ui.face_panel_menu = None;
        }
    }
    let _ = config;
}

/// Put a control back at rest (keyed, while animating).
fn reset(armature: &Armature, id: i32, anim: usize, frame: i32, events: &mut EventState) {
    let Some((idx, b)) = armature.bones.iter().enumerate().find(|(_, b)| b.id == id) else { return };
    let Some(fc) = &b.face_control else { return };
    events.save_edited_bone(idx);
    events.edit_bone(id, &AnimElement::PositionX, fc.rest.x, "", anim, frame);
    events.edit_bone(id, &AnimElement::PositionY, fc.rest.y, "", anim, frame);
    events.edit_bone(id, &AnimElement::Rotation, fc.rest_rot, "", anim, frame);
}
