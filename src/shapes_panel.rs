//! The Shapes section of the bone panel (docs/LIP_SYNC.md §4.4): a mesh's shapes,
//! their weights (keyed while animating), sculpting, and the shape menu.

use crate::ui::{EguiUi, TextInputOptions};
use crate::*;

pub fn draw(
    ui: &mut egui::Ui,
    bone: &Bone,
    selections: &SelectionState,
    shared_ui: &mut crate::Ui,
    config: &Config,
    armature: &Armature,
    events: &mut EventState,
    edit_mode: &EditMode,
) {
    let loc = |shared_ui: &crate::Ui, key: &str| shared_ui.loc(&format!("bone_panel.shapes.{key}"));
    let frame = egui::Frame::new().fill(config.colors.dark_accent.into()).inner_margin(egui::Margin::same(5));
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            let heading = loc(shared_ui, "heading").replace("$n", &bone.shapes.len().to_string());
            ui.label(heading).on_hover_text(loc(shared_ui, "desc"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let fold_icon = if shared_ui.shapes_folded { "⏴" } else { "⏷" };
                if ui.label(fold_icon).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    shared_ui.shapes_folded = !shared_ui.shapes_folded;
                }
            });
        });
    });
    ui.add_space(2.5);
    if shared_ui.shapes_folded {
        return;
    }

    let animating = shared_ui.is_animating(edit_mode, selections);
    let frame = selections.anim_frame;
    let anim = if animating { selections.anim } else { usize::MAX };
    let keys_frame = animating && frame >= 0 && !bone.locked;
    let posed = armature.animated_bones.iter().find(|b| b.id == bone.id);
    let text: egui::Color32 = config.colors.text.into();
    let dim = text.gamma_multiply(0.6);

    // sculpting one of these: say so, with a way out
    if let Some((_, sid)) = edit_mode.sculpt_shape.filter(|(b, _)| *b == bone.id) {
        let name = bone.shapes.iter().find(|s| s.id == sid).map_or(String::new(), |s| s.name.clone());
        egui::Frame::new().fill(egui::Color32::from_rgb(110, 80, 40)).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(loc(shared_ui, "sculpting").replace("$name", &name)).color(text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.skf_button(loc(shared_ui, "done")).clicked() {
                        events.toggle_sculpt_shape(bone.id, sid);
                    }
                });
            });
        });
        ui.add_space(4.);
    }

    for shape in &bone.shapes {
        let weight = posed.and_then(|b| b.shapes.iter().find(|s| s.id == shape.id)).map_or(shape.weight, |s| s.weight);
        let keyed_here = keys_frame
            && armature.animations.get(selections.anim).is_some_and(|a| {
                a.keyframes.iter().any(|k| k.bone_id == bone.id && k.element == AnimElement::ShapeWeight && k.shape == shape.id && k.frame == frame)
            });
        let sculpting = edit_mode.sculpt_shape == Some((bone.id, shape.id));
        ui.horizontal(|ui| {
            // name: double-click (or the menu) to rename
            let context_id = format!("shape_{}_{}", bone.id, shape.id);
            if shared_ui.rename_id == context_id {
                let options = Some(TextInputOptions { focus: true, size: Vec2::new(110., 20.), ..Default::default() });
                let (edited, value, _) = ui.text_input(context_id, shared_ui, shape.name.clone(), options);
                if edited && !value.trim().is_empty() {
                    events.set_shape(bone.id, &MeshShape { name: value.trim().to_string(), ..shape.clone() }, true);
                }
            } else {
                let name = egui::RichText::new(&shape.name).color(if sculpting { egui::Color32::from_rgb(240, 190, 80) } else { text });
                let label = ui.add_sized([110., 18.], egui::Label::new(name).truncate().sense(egui::Sense::click()));
                if label.double_clicked() {
                    shared_ui.rename_id = context_id.clone();
                }
                if label.secondary_clicked() {
                    shared_ui.context_menu.show(&context_id);
                }
                let tip = loc(shared_ui, "name_desc").replace("$n", &shape.deltas.len().to_string());
                label.on_hover_text(tip);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // sculpt
                let edit = egui::RichText::new(loc(shared_ui, "edit")).color(if sculpting { egui::Color32::from_rgb(240, 190, 80) } else { text });
                if ui.skf_button(edit).on_hover_text(loc(shared_ui, "edit_desc")).clicked() {
                    events.toggle_sculpt_shape(bone.id, shape.id);
                }
                // key diamond
                if keys_frame {
                    let (icon, col) = if keyed_here { ("◆", egui::Color32::from_rgb(110, 240, 120)) } else { ("◇", dim) };
                    let key = ui.add(egui::Label::new(egui::RichText::new(icon).color(col)).sense(egui::Sense::click()));
                    let tip = loc(shared_ui, if keyed_here { "keyed" } else { "key" }).replace("$frame", &frame.to_string());
                    if key.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked() && !keyed_here {
                        events.edit_shape_weight(bone.id, shape.id, weight, anim, frame, true);
                    }
                }
                // weight
                let mut w = weight;
                let slider = ui.add(egui::Slider::new(&mut w, 0.0..=1.0).max_decimals(2).show_value(true));
                if slider.changed() {
                    let step = slider.drag_started() || !slider.dragged();
                    events.edit_shape_weight(bone.id, shape.id, w, anim, frame, step);
                } else if slider.drag_started() {
                    events.edit_shape_weight(bone.id, shape.id, w, anim, frame, true);
                }
            });
        });
    }
    if bone.shapes.is_empty() {
        ui.label(egui::RichText::new(loc(shared_ui, "empty")).color(dim));
    }

    ui.horizontal(|ui| {
        if ui.skf_button(loc(shared_ui, "add")).on_hover_text(loc(shared_ui, "add_desc")).clicked() {
            events.add_shape(bone.id, &loc(shared_ui, "new_name"));
        }
        if animating && frame < 0 {
            ui.label(egui::RichText::new(loc(shared_ui, "no_frame")).color(dim).size(11.));
        }
    });
}
