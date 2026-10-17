//! The Lip Sync window (docs/LIP_SYNC.md §7): for one audio clip, the mouth, the line's
//! words, the keyer's settings and the mouth rig. Edits stay in a draft until
//! Generate, which saves the track and makes its keys as one undo step.

use crate::lipsync::{self, KeyerSettings, LipSyncTrack, MouthRig, Smoothing, TrackSource, VisemeSet};
use crate::*;

fn loc(shared_ui: &crate::Ui, key: &str) -> String {
    shared_ui.loc(&format!("lipsync.{key}"))
}

/// What kind of mouth a host bone is, for its label.
fn host_label(shared_ui: &crate::Ui, armature: &Armature, id: i32) -> String {
    let Some(b) = armature.bones.iter().find(|b| b.id == id) else { return "?".into() };
    let kind = if crate::face_panel::rigs(armature).iter().any(|(r, _)| *r == id) {
        "kind_face"
    } else if !b.shapes.is_empty() {
        "kind_shapes"
    } else {
        "kind_drawn"
    };
    format!("{} ({})", b.name, loc(shared_ui, kind))
}

/// What a pose uses, briefly.
fn pose_summary(shared_ui: &crate::Ui, armature: &Armature, p: Option<&lipsync::MouthPose>) -> String {
    let Some(p) = p else { return loc(shared_ui, "uses_nothing") };
    let mut parts: Vec<String> = vec![];
    if !p.controls.is_empty() {
        parts.push(p.controls.iter().map(|c| c.control.clone()).collect::<Vec<_>>().join(", "));
    }
    for s in &p.shapes {
        let name = armature.bones.iter().find(|b| b.id == s.mesh).and_then(|b| b.shapes.iter().find(|x| x.id == s.shape)).map_or("?".into(), |x| x.name.clone());
        parts.push(format!("◆ {name}"));
    }
    if let Some(t) = &p.texture {
        parts.push(format!("▣ {}", t.tex));
    }
    if parts.is_empty() {
        // rest: the mouth as set up
        return loc(shared_ui, "uses_rest");
    }
    parts.join("; ")
}

pub fn window(egui_ctx: &egui::Context, shared_ui: &mut crate::Ui, armature: &Armature, selections: &SelectionState, events: &mut EventState) {
    let Some(clip_id) = shared_ui.lipsync_clip else { return };
    let Some(anim) = armature.animations.get(selections.anim) else { return };
    let Some(clip) = anim.clips.iter().find(|c| c.id == clip_id) else {
        shared_ui.lipsync_clip = None;
        shared_ui.lipsync_draft = None;
        return;
    };
    let existing = anim.lipsync.iter().find(|t| t.clip == clip_id);
    let hosts = lipsync::mouth_hosts(armature);

    // the draft: the track as saved, or a new one for this clip
    let mut draft = match shared_ui.lipsync_draft.take() {
        Some(d) if d.clip == clip_id => d,
        _ => existing.cloned().unwrap_or_else(|| LipSyncTrack {
            id: crate::audio::next_id(anim.lipsync.iter().map(|t| t.id)),
            clip: clip_id,
            mouth: hosts.first().copied().unwrap_or(-1),
            ..Default::default()
        }),
    };
    if !hosts.contains(&draft.mouth) {
        draft.mouth = hosts.first().copied().unwrap_or(-1);
    }

    let mut open = true;
    let title = format!("{}: {}", loc(shared_ui, "title"), clip.name);
    egui::Window::new(title)
        .id(egui::Id::new("lipsync_window"))
        .open(&mut open)
        .resizable(true)
        .default_width(380.)
        .show(egui_ctx, |ui| {
            if hosts.is_empty() {
                ui.label(loc(shared_ui, "no_mouth"));
                return;
            }

            egui::Grid::new("lipsync_grid").num_columns(2).spacing([10., 6.]).show(ui, |ui| {
                ui.label(loc(shared_ui, "mouth")).on_hover_text(loc(shared_ui, "mouth_desc"));
                egui::ComboBox::from_id_salt("lipsync_mouth").selected_text(host_label(shared_ui, armature, draft.mouth)).show_ui(ui, |ui| {
                    for h in &hosts {
                        ui.selectable_value(&mut draft.mouth, *h, host_label(shared_ui, armature, *h));
                    }
                });
                ui.end_row();

                ui.label(loc(shared_ui, "source"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut draft.source, TrackSource::Text, loc(shared_ui, "source_text")).on_hover_text(loc(shared_ui, "source_text_desc"));
                    ui.radio_value(&mut draft.source, TrackSource::Loudness, loc(shared_ui, "source_loudness")).on_hover_text(loc(shared_ui, "source_loudness_desc"));
                });
                ui.end_row();
            });

            if draft.source == TrackSource::Text {
                ui.add_space(4.);
                ui.label(loc(shared_ui, "words"));
                ui.add(egui::TextEdit::multiline(&mut draft.text).desired_rows(3).desired_width(f32::INFINITY).hint_text(loc(shared_ui, "words_hint")));
            }

            ui.add_space(4.);
            ui.collapsing(loc(shared_ui, "settings"), |ui| settings(ui, shared_ui, &mut draft.settings));
            ui.collapsing(loc(shared_ui, "rig"), |ui| rig_section(ui, shared_ui, armature, draft.mouth, events));

            if let Some(t) = existing.filter(|t| !t.words.is_empty()) {
                ui.collapsing(loc(shared_ui, "timing"), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for w in &t.words {
                            ui.label(egui::RichText::new(format!("{} {:.2}s", w.text, w.start)).small()).on_hover_text(format!("{:.2}–{:.2} s", w.start, w.end));
                        }
                    });
                });
            }

            ui.separator();
            ui.horizontal(|ui| {
                let ready = draft.source == TrackSource::Loudness || !draft.text.trim().is_empty();
                let label = loc(shared_ui, if existing.is_some() { "regenerate" } else { "generate" });
                let resp = ui.add_enabled(ready, egui::Button::new(label)).on_hover_text(loc(shared_ui, "generate_desc"));
                if resp.clicked() {
                    events.generate_lipsync(&draft);
                }
                if let Some(t) = existing {
                    if ui.button(loc(shared_ui, "bake")).on_hover_text(loc(shared_ui, "bake_desc")).clicked() {
                        events.bake_lipsync(t.id);
                    }
                    if ui.button(loc(shared_ui, "remove")).on_hover_text(loc(shared_ui, "remove_desc")).clicked() {
                        events.delete_lipsync(t.id);
                    }
                }
            });
            if existing.is_some_and(|t| *t != draft && (t.text != draft.text || t.mouth != draft.mouth || t.source != draft.source || t.settings != draft.settings)) {
                ui.label(egui::RichText::new(loc(shared_ui, "changed")).small().italics());
            }
        });

    if open {
        shared_ui.lipsync_draft = Some(draft);
    } else {
        shared_ui.lipsync_clip = None;
        shared_ui.lipsync_draft = None;
    }
}

fn settings(ui: &mut egui::Ui, shared_ui: &crate::Ui, s: &mut KeyerSettings) {
    egui::Grid::new("lipsync_settings").num_columns(2).spacing([10., 4.]).show(ui, |ui| {
        ui.label(loc(shared_ui, "lead")).on_hover_text(loc(shared_ui, "lead_desc"));
        ui.add(egui::DragValue::new(&mut s.lead).range(0..=6).speed(0.05).suffix(" fr"));
        ui.end_row();
        ui.label(loc(shared_ui, "hold")).on_hover_text(loc(shared_ui, "hold_desc"));
        ui.add(egui::DragValue::new(&mut s.min_hold).range(1..=8).speed(0.05).suffix(" fr"));
        ui.end_row();
        ui.label(loc(shared_ui, "smoothing")).on_hover_text(loc(shared_ui, "smoothing_desc"));
        let name = |m: Smoothing| {
            loc(shared_ui, match m {
                Smoothing::Off => "smooth_off",
                Smoothing::Low => "smooth_low",
                Smoothing::Medium => "smooth_medium",
                Smoothing::High => "smooth_high",
            })
        };
        egui::ComboBox::from_id_salt("lipsync_smoothing").selected_text(name(s.smoothing)).show_ui(ui, |ui| {
            for m in [Smoothing::Off, Smoothing::Low, Smoothing::Medium, Smoothing::High] {
                ui.selectable_value(&mut s.smoothing, m, name(m));
            }
        });
        ui.end_row();
        ui.label(loc(shared_ui, "intensity")).on_hover_text(loc(shared_ui, "intensity_desc"));
        let mut pct = s.intensity * 100.;
        if ui.add(egui::Slider::new(&mut pct, 0.0..=150.0).suffix("%").max_decimals(0)).changed() {
            s.intensity = pct / 100.;
        }
        ui.end_row();
        ui.label(loc(shared_ui, "rest")).on_hover_text(loc(shared_ui, "rest_desc"));
        ui.add(egui::Slider::new(&mut s.rest_gap, 0.05..=1.0).suffix(" s").max_decimals(2));
        ui.end_row();
    });
}

/// The mouth rig: its viseme set and each viseme's pose. Changes save on the host bone
/// (undoable); poses can be taken from the face rig as posed in the Face Rig panel.
fn rig_section(ui: &mut egui::Ui, shared_ui: &crate::Ui, armature: &Armature, host: i32, events: &mut EventState) {
    let Some(bone) = armature.bones.iter().find(|b| b.id == host) else { return };
    let rig = lipsync::rig_for(armature, host, None);
    let stored = bone.mouth_rig.is_some();

    ui.horizontal(|ui| {
        ui.label(loc(shared_ui, "set")).on_hover_text(loc(shared_ui, "set_desc"));
        let mut set = rig.set;
        egui::ComboBox::from_id_salt("lipsync_set").selected_text(set.label()).show_ui(ui, |ui| {
            for s in VisemeSet::ALL {
                ui.selectable_value(&mut set, s, s.label());
            }
        });
        if set != rig.set {
            events.set_mouth_rig(host, Some(&lipsync::auto_fill(armature, host, set)));
        }
        ui.label(egui::RichText::new(loc(shared_ui, if stored { "rig_custom" } else { "rig_auto" })).small().weak());
    });

    let posed: &[Bone] = if armature.animated_bones.len() == armature.bones.len() { &armature.animated_bones } else { &armature.bones };
    let auto = lipsync::auto_fill(armature, host, rig.set);
    egui::Grid::new("lipsync_rig").num_columns(3).striped(true).spacing([8., 3.]).show(ui, |ui| {
        for v in rig.set.visemes() {
            let pose = rig.pose(v);
            ui.label(egui::RichText::new(*v).strong());
            ui.label(egui::RichText::new(pose_summary(shared_ui, armature, pose)).small());
            ui.horizontal(|ui| {
                if let Some(face) = rig.face {
                    if ui.small_button(loc(shared_ui, "use_pose")).on_hover_text(loc(shared_ui, "use_pose_desc")).clicked() {
                        let mut new = rig.clone();
                        let controls = lipsync::capture_pose(armature, posed, face);
                        match new.poses.iter_mut().find(|p| p.viseme == *v) {
                            Some(p) => {
                                p.controls = controls;
                                p.shapes.clear();
                                p.texture = None;
                            }
                            None => new.poses.push(lipsync::MouthPose { viseme: v.to_string(), controls, ..Default::default() }),
                        }
                        events.set_mouth_rig(host, Some(&new));
                    }
                }
                let custom = pose != auto.pose(v);
                if custom && ui.small_button(loc(shared_ui, "auto")).on_hover_text(loc(shared_ui, "auto_desc")).clicked() {
                    let mut new: MouthRig = rig.clone();
                    new.poses.retain(|p| p.viseme != *v);
                    if let Some(p) = auto.pose(v) {
                        new.poses.push(p.clone());
                    }
                    events.set_mouth_rig(host, Some(&new));
                }
            });
            ui.end_row();
        }
    });
    if stored && ui.small_button(loc(shared_ui, "reset_rig")).on_hover_text(loc(shared_ui, "reset_rig_desc")).clicked() {
        events.set_mouth_rig(host, None);
    }
}
