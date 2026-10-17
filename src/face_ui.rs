//! Face rig UI (docs/LIP_SYNC.md §6.5): the mask and control tracks over the canvas,
//! and the Face Control section of the bone panel.

use crate::camera_ui::View;
use crate::ui::EguiUi;
use crate::*;

/// World transforms of the posed bones (position, rotation and scale only).
fn world(bones: &[Bone]) -> Vec<Bone> {
    let mut light: Vec<Bone> = bones
        .iter()
        .map(|b| Bone { id: b.id, parent_id: b.parent_id, pos: b.pos, rot: b.rot, scale: b.scale, ..Default::default() })
        .collect();
    renderer::inheritance(&mut light, std::collections::HashMap::new(), &vec![]);
    light
}

/// The face rigs to show: a rig's root (a bone whose children are controls) when the
/// root, one of its controls, or a mesh they drive is selected.
fn shown_roots(armature: &Armature, selections: &SelectionState) -> Vec<i32> {
    let selected: Vec<i32> = selections.bone_ids.clone();
    let mut roots = vec![];
    for b in armature.bones.iter().filter(|b| b.face_control.is_some()) {
        // a control's rig root: up its parents to the first non-control
        let mut root = b.parent_id;
        while let Some(p) = armature.bones.iter().find(|p| p.id == root && p.face_control.is_some()) {
            root = p.parent_id;
        }
        let drives_selected = b.face_control.as_ref().unwrap().slots.iter().any(|s| s.targets.iter().any(|t| selected.contains(&t.mesh)));
        if (selected.contains(&b.id) || selected.contains(&root) || drives_selected) && !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

/// Draw each shown rig's mask, the controls' travel boxes, and a line from each
/// control's rest to where it is.
pub fn overlay(ctx: &egui::Context, config: &Config, camera: &Camera, armature: &Armature, selections: &SelectionState, edit_mode: &EditMode) {
    if !edit_mode.show_face_rig {
        return;
    }
    let roots = shown_roots(armature, selections);
    if roots.is_empty() {
        return;
    }
    let canvas = ctx.available_rect();
    let view = View::new(ctx, camera, config);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("face_rig"))).with_clip_rect(canvas);
    let posed = if armature.animated_bones.len() == armature.bones.len() { &armature.animated_bones } else { &armature.bones };
    let w = world(posed);
    let find = |id: i32| w.iter().find(|b| b.id == id);
    let amber = egui::Color32::from_rgb(240, 190, 80);
    let text: egui::Color32 = config.colors.text.into();

    for root in roots {
        let Some(r) = find(root) else { continue };
        let controls: Vec<&Bone> = armature
            .bones
            .iter()
            .filter(|b| b.face_control.is_some() && {
                let mut p = b.parent_id;
                while let Some(pb) = armature.bones.iter().find(|x| x.id == p && x.face_control.is_some()) {
                    p = pb.parent_id;
                }
                p == root
            })
            .collect();
        // the mask: an oval around the controls' rest positions
        let s = controls.first().and_then(|c| c.face_control.as_ref()).map_or(1., |fc| fc.range / face::RANGE);
        let centre = r.pos + utils::rotate(&(Vec2::new(0., (face::MASK_TOP + face::MASK_BOTTOM) / 2.) * s * r.scale), r.rot);
        let radii = Vec2::new(face::MASK_HALF_WIDTH * 1.25, (face::MASK_TOP - face::MASK_BOTTOM) / 2. * 1.1) * s * r.scale;
        let pts: Vec<egui::Pos2> = (0..48)
            .map(|i| {
                let a = i as f32 / 48. * std::f32::consts::TAU;
                view.to_pt(centre + utils::rotate(&Vec2::new(a.cos() * radii.x, a.sin() * radii.y), r.rot))
            })
            .collect();
        painter.add(egui::Shape::closed_line(pts, egui::Stroke::new(1.5, text.gamma_multiply(0.35))));

        for c in controls {
            let fc = c.face_control.as_ref().unwrap();
            let (Some(parent), Some(now)) = (find(c.parent_id), find(c.id)) else { continue };
            let to_world = |local: Vec2| parent.pos + utils::rotate(&(local * parent.scale), parent.rot);
            let rest = to_world(fc.rest);
            // the box the control travels in for full weight, along the axes it uses
            let has = |a: ControlAxis| fc.slots.iter().any(|s| s.axis == a && !s.targets.is_empty());
            let (l, rt, up, dn) = (has(ControlAxis::Left), has(ControlAxis::Right), has(ControlAxis::Up), has(ControlAxis::Down));
            let x0 = if l { -fc.range } else { 0. };
            let x1 = if rt { fc.range } else { 0. };
            let y0 = if dn { -fc.range } else { 0. };
            let y1 = if up { fc.range } else { 0. };
            let corners = [Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)];
            let quad: Vec<egui::Pos2> = corners.iter().map(|k| view.to_pt(to_world(fc.rest + *k))).collect();
            let selected = selections.bone_ids.contains(&c.id);
            let col = if selected { amber } else { text.gamma_multiply(0.45) };
            painter.add(egui::Shape::closed_line(quad, egui::Stroke::new(1., col)));
            let (a, b) = (view.to_pt(rest), view.to_pt(now.pos));
            painter.line_segment([a, b], egui::Stroke::new(1.5, amber.gamma_multiply(0.8)));
            painter.circle_stroke(a, 2.5, egui::Stroke::new(1., col));
            if selected {
                painter.text(b + egui::vec2(8., -8.), egui::Align2::LEFT_BOTTOM, &fc.name, egui::FontId::proportional(11.), amber);
            }
        }
    }
}

/// The bone panel's Face Control section: what the selected control drives, editable.
pub fn panel(ui: &mut egui::Ui, bone: &Bone, shared_ui: &mut crate::Ui, config: &Config, armature: &Armature, events: &mut EventState) {
    let Some(fc) = &bone.face_control else { return };
    let loc = |shared_ui: &crate::Ui, key: &str| shared_ui.loc(&format!("bone_panel.face.{key}"));
    let frame = egui::Frame::new().fill(config.colors.dark_accent.into()).inner_margin(egui::Margin::same(5));
    frame.show(ui, |ui| {
        ui.label(loc(shared_ui, "control").replace("$name", &fc.name)).on_hover_text(loc(shared_ui, "control_desc"));
    });
    ui.add_space(2.5);
    let text: egui::Color32 = config.colors.text.into();
    let dim = text.gamma_multiply(0.6);

    let mut new = fc.clone();
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(loc(shared_ui, "range")).on_hover_text(loc(shared_ui, "range_desc"));
        let r = ui.add(egui::DragValue::new(&mut new.range).range(0.01..=100_000.0).speed(0.2).max_decimals(1));
        changed |= r.changed();
        if ui.skf_button(loc(shared_ui, "set_rest")).on_hover_text(loc(shared_ui, "set_rest_desc")).clicked() {
            new.rest = bone.pos;
            new.rest_rot = bone.rot;
            changed = true;
        }
    });

    let meshes: Vec<&Bone> = armature.bones.iter().filter(|b| !b.shapes.is_empty()).collect();
    let shape_name = |mesh: i32, shape: i32| {
        armature.bones.iter().find(|b| b.id == mesh).and_then(|b| b.shapes.iter().find(|s| s.id == shape)).map_or("?".to_string(), |s| s.name.clone())
    };
    for (si, slot) in fc.slots.iter().enumerate() {
        ui.horizontal(|ui| {
            let axis = loc(shared_ui, &format!("axis_{:?}", slot.axis).to_lowercase());
            ui.add_sized([62., 18.], egui::Label::new(egui::RichText::new(axis).color(dim)));
            if slot.targets.is_empty() {
                ui.label(egui::RichText::new(loc(shared_ui, "unmapped")).color(dim).italics());
            }
            for (ti, t) in slot.targets.iter().enumerate() {
                let label = format!("{} ×{:.2}", shape_name(t.mesh, t.shape), t.weight);
                let resp = ui.add(egui::Label::new(label).sense(egui::Sense::click())).on_hover_text(loc(shared_ui, "target_desc"));
                if resp.secondary_clicked() || resp.double_clicked() {
                    let targets = &mut new.slots[si].targets;
                    if ti < targets.len() {
                        targets.remove(ti);
                    }
                    changed = true;
                }
            }
            // add a shape to this direction
            egui::ComboBox::from_id_salt(("face_slot_add", bone.id, si)).selected_text("+").width(28.).show_ui(ui, |ui| {
                for m in &meshes {
                    for s in &m.shapes {
                        let name = if meshes.len() > 1 { format!("{}: {}", m.name, s.name) } else { s.name.clone() };
                        if ui.selectable_label(false, name).clicked() {
                            new.slots[si].targets.push(SlotTarget { mesh: m.id, shape: s.id, weight: 1. });
                            changed = true;
                        }
                    }
                }
            });
        });
    }
    if changed {
        events.set_face_control(bone.id, &new);
    }
}

/// The Shapes section's face buttons: Add Standard Shapes, Set Up Face.
pub fn face_buttons(ui: &mut egui::Ui, bone: &Bone, shared_ui: &mut crate::Ui, armature: &Armature, events: &mut EventState) {
    let loc = |shared_ui: &crate::Ui, key: &str| shared_ui.loc(&format!("bone_panel.face.{key}"));
    let rigged = armature
        .bones
        .iter()
        .any(|b| b.face_control.as_ref().is_some_and(|fc| fc.slots.iter().any(|s| s.targets.iter().any(|t| t.mesh == bone.id))));
    ui.horizontal(|ui| {
        if ui.skf_button(loc(shared_ui, "standard")).on_hover_text(loc(shared_ui, "standard_desc")).clicked() {
            events.add_standard_shapes(bone.id, false);
        }
        if ui.skf_button(loc(shared_ui, "visemes")).on_hover_text(loc(shared_ui, "visemes_desc")).clicked() {
            events.add_standard_shapes(bone.id, true);
        }
    });
    ui.add_enabled_ui(!rigged, |ui| {
        let tip = loc(shared_ui, if rigged { "rigged" } else { "set_up_desc" });
        if ui.skf_button(loc(shared_ui, "set_up")).on_hover_text(&tip).on_disabled_hover_text(&tip).clicked() {
            events.set_up_face(bone.id, &loc(shared_ui, "root_name"));
        }
    });
}
