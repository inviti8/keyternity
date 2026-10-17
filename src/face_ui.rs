//! Face rig UI (docs/LIP_SYNC.md §6.5): the mask and control tracks over the canvas,
//! and the Face Control section of the bone panel.

use crate::camera_ui::View;
use crate::ui::EguiUi;
use crate::*;

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

    if ui.skf_button(loc(shared_ui, "open_panel")).on_hover_text(loc(shared_ui, "open_panel_desc")).clicked() {
        events.open_face_panel();
    }

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
    if rigged {
        if ui.skf_button(loc(shared_ui, "open_panel")).on_hover_text(loc(shared_ui, "open_panel_desc")).clicked() {
            events.open_face_panel();
        }
    } else if ui.skf_button(loc(shared_ui, "set_up")).on_hover_text(loc(shared_ui, "set_up_desc")).clicked() {
        events.set_up_face(bone.id, &loc(shared_ui, "root_name"));
        events.open_face_panel();
    }
}

/// The Face Landmarks section (docs/LIP_SYNC.md §6.7): the view, the landmarks in
/// mapping order with Pick, Derive Other Side, the checks, and Generate Face.
pub fn landmarks_section(
    ui: &mut egui::Ui,
    bone: &Bone,
    selections: &SelectionState,
    shared_ui: &mut crate::Ui,
    config: &Config,
    edit_mode: &EditMode,
    events: &mut EventState,
) {
    let loc = |shared_ui: &crate::Ui, key: &str| shared_ui.loc(&format!("bone_panel.landmarks.{key}"));
    let text: egui::Color32 = config.colors.text.into();
    let dim = text.gamma_multiply(0.6);
    let green = egui::Color32::from_rgb(110, 240, 120);
    let amber = egui::Color32::from_rgb(240, 190, 80);
    let empty = FaceMap::default();
    let map = bone.face_map.as_ref().unwrap_or(&empty);

    let frame = egui::Frame::new().fill(config.colors.dark_accent.into()).inner_margin(egui::Margin::same(5));
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            let mapped = map.points.iter().filter(|p| !p.derived).count();
            ui.label(loc(shared_ui, "heading").replace("$n", &mapped.to_string())).on_hover_text(loc(shared_ui, "desc"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let icon = if shared_ui.landmarks_open { "⏷" } else { "⏴" };
                if ui.label(icon).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    shared_ui.landmarks_open = !shared_ui.landmarks_open;
                }
            });
        });
    });
    ui.add_space(2.5);
    if !shared_ui.landmarks_open {
        shared_ui.picking_landmark = None;
        return;
    }

    // picking: the next single vertex selected maps the landmark, then move on
    if let Some(name) = shared_ui.picking_landmark.clone() {
        if selections.vert_ids.len() == 1 && selections.vert_ids != shared_ui.picking_from {
            events.face_landmarks(bone.id, &format!("map|{}|{}", name, selections.vert_ids[0]));
            shared_ui.picking_from = selections.vert_ids.clone();
            let next = face_gen::MAP_ORDER
                .iter()
                .skip_while(|n| **n != name)
                .skip(1)
                .find(|n| !map.points.iter().any(|p| p.name == **n && !p.derived));
            shared_ui.picking_landmark = next.map(|n| n.to_string());
        }
    }

    // the view
    ui.horizontal(|ui| {
        ui.label(loc(shared_ui, "view")).on_hover_text(loc(shared_ui, "view_desc"));
        for (v, key) in [(FaceView::Front, "front"), (FaceView::ThreeQuarter, "three_quarter"), (FaceView::Profile, "profile")] {
            if ui.selectable_label(map.view == v, loc(shared_ui, key)).clicked() && map.view != v {
                events.face_landmarks(bone.id, &format!("view|{v:?}"));
            }
        }
    });
    if map.view == FaceView::Profile {
        ui.label(egui::RichText::new(loc(shared_ui, "profile_note")).color(dim).size(11.));
    }
    if let Some(name) = &shared_ui.picking_landmark {
        ui.label(egui::RichText::new(loc(shared_ui, "picking").replace("$name", name)).color(amber));
    }

    // the landmarks, in mapping order, then derived ones
    let status = |name: &str| map.points.iter().find(|p| p.name == name).map(|p| p.derived);
    let mut rows: Vec<&str> = face_gen::MAP_ORDER.to_vec();
    for p in &map.points {
        if !rows.contains(&p.name.as_str()) {
            rows.push(&p.name);
        }
    }
    egui::ScrollArea::vertical().id_salt(("landmarks", bone.id)).max_height(220.).show(ui, |ui| {
        for name in rows {
            ui.horizontal(|ui| {
                let (mark, col) = match status(name) {
                    Some(false) => ("●", green),
                    Some(true) => ("○", amber),
                    None => ("·", dim),
                };
                ui.label(egui::RichText::new(mark).color(col));
                let picking = shared_ui.picking_landmark.as_deref() == Some(name);
                ui.label(egui::RichText::new(name).color(if picking { amber } else { text }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if status(name).is_some() && ui.small_button("✕").on_hover_text(loc(shared_ui, "clear")).clicked() {
                        events.face_landmarks(bone.id, &format!("clear|{name}"));
                    }
                    if status(name) == Some(true) && ui.small_button("✓").on_hover_text(loc(shared_ui, "confirm")).clicked() {
                        if let Some(p) = map.points.iter().find(|p| p.name == name) {
                            events.face_landmarks(bone.id, &format!("map|{}|{}", name, p.vert));
                        }
                    }
                    let label = if picking { loc(shared_ui, "picking_short") } else { loc(shared_ui, "pick") };
                    if ui.small_button(label).on_hover_text(loc(shared_ui, "pick_desc")).clicked() {
                        if picking {
                            shared_ui.picking_landmark = None;
                        } else {
                            shared_ui.picking_landmark = Some(name.to_string());
                            shared_ui.picking_from = selections.vert_ids.clone();
                            if !edit_mode.editing_mesh {
                                events.toggle_editing_mesh();
                            }
                        }
                    }
                });
            });
        }
    });

    ui.horizontal(|ui| {
        let enabled = map.view != FaceView::Profile;
        if ui.add_enabled(enabled, egui::Button::new(loc(shared_ui, "derive"))).on_hover_text(loc(shared_ui, "derive_desc")).clicked() {
            events.face_landmarks(bone.id, "derive");
        }
        if ui.skf_button(loc(shared_ui, "clear_all")).clicked() {
            events.face_landmarks(bone.id, "clear_all");
            shared_ui.picking_landmark = None;
        }
    });

    // what generating needs
    for issue in face_gen::checks(bone, map) {
        ui.label(egui::RichText::new(loc(shared_ui, &format!("check_{issue}"))).color(amber).size(11.5));
    }

    // style and Generate Face
    ui.horizontal(|ui| {
        ui.label(loc(shared_ui, "style")).on_hover_text(loc(shared_ui, "style_desc"));
        let strength = if shared_ui.face_strength <= 0. { 1. } else { shared_ui.face_strength };
        for (s, key) in [(0.6, "subtle"), (1., "standard"), (1.5, "cartoon")] {
            if ui.selectable_label((strength - s).abs() < 1e-3, loc(shared_ui, key)).clicked() {
                shared_ui.face_strength = s;
            }
        }
    });
    let visemes_label = loc(shared_ui, "with_visemes");
    let visemes_tip = loc(shared_ui, "with_visemes_desc");
    ui.checkbox(&mut shared_ui.face_visemes, visemes_label).on_hover_text(visemes_tip);
    let can = face_gen::units(&face_gen::positions(bone, map)).is_some();
    let tip = loc(shared_ui, if can { "generate_desc" } else { "check_units" });
    let button = ui.add_enabled(can, egui::Button::new(loc(shared_ui, "generate"))).on_hover_text(&tip).on_disabled_hover_text(&tip);
    if button.clicked() {
        let strength = if shared_ui.face_strength <= 0. { 1. } else { shared_ui.face_strength };
        events.face_landmarks(bone.id, &format!("generate|{}|{}", if shared_ui.face_visemes { 1 } else { 0 }, strength));
    }
}

/// Landmark dots over the mesh while its Face Landmarks section is open: mapped
/// (filled), derived (hollow), and their names.
pub fn landmark_dots(ctx: &egui::Context, shared_ui: &crate::Ui, config: &Config, camera: &Camera, armature: &Armature, selections: &SelectionState) {
    if !shared_ui.landmarks_open {
        return;
    }
    let Some(bone) = armature.sel_bone(selections).filter(|b| b.face_map.is_some()) else { return };
    let map = bone.face_map.as_ref().unwrap();
    let posed = if armature.animated_bones.len() == armature.bones.len() { &armature.animated_bones } else { &armature.bones };
    let mut built = vec![];
    renderer::construction(&mut built, posed);
    let Some(world) = built.iter().find(|b| b.id == bone.id) else { return };
    let canvas = ctx.available_rect();
    let view = View::new(ctx, camera, config);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("face_landmarks"))).with_clip_rect(canvas);
    let green = egui::Color32::from_rgb(110, 240, 120);
    let amber = egui::Color32::from_rgb(240, 190, 80);
    for p in &map.points {
        let Some(v) = world.vertices.iter().find(|v| v.id == p.vert) else { continue };
        let at = view.to_pt(v.pos);
        if p.derived {
            painter.circle_stroke(at, 5., egui::Stroke::new(1.5, amber));
        } else {
            painter.circle_filled(at, 4., green);
        }
        let short = p.name.replace("Lip_", "").replace("Mouth", "");
        painter.text(at + egui::vec2(6., -6.), egui::Align2::LEFT_BOTTOM, short, egui::FontId::proportional(10.), if p.derived { amber } else { green });
    }
}
