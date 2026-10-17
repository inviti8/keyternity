use crate::*;
use armature_window::get_all_children;
use spade::Triangulation;
use std::collections::HashMap;
use std::str::FromStr;

const MIN_ZOOM: f32 = 1.;

pub fn iterate_events(
    input: &InputStates,
    config: &mut Config,
    events: &mut EventState,
    camera: &mut Camera,
    edit_mode: &mut EditMode,
    selections: &mut SelectionState,
    undo_states: &mut UndoStates,
    armature: &mut Armature,
    psd_armature: &mut Armature,
    copy_buffer: &mut CopyBuffer,
    ui: &mut crate::Ui,
    renderer: &mut crate::Renderer,
) {
    let mut last_event = Events::None;
    let event = events.events[0].clone();

    // for every new event, create a new undo state
    // note: `edit_bone` is not included, as its undo is conditional (see `save_edited_bone`)
    if last_event != event {
        last_event = event.clone();

        type E = Events;
        #[rustfmt::skip]
        match last_event {
            E::NewBone | E::DragBone | E::DeleteBone | E::PasteBone | E::RaiseGlobalZindex | E::SetBindPose | E::SetBindPoseAll | E::ClearBindPose | E::AddIk | E::RemoveIk => undo_states.new_undo_bones(&armature.bones),
            E::SetIkTargetSpace => undo_states.new_undo_bones(&armature.bones),
            E::SetIkChain | E::ToggleIkEnabled | E::DeleteIkTarget => undo_states.new_undo_bone(&armature.bones[selections.bone_idx]),
            E::BindToSkeleton | E::ClearAllBinds | E::AddBonesInReach => undo_states.new_undo_bones(&armature.bones),
            E::ToggleEnvelopeWeights | E::SetMaxInfluences | E::UnlockSelectedVerts | E::UnlockAllVerts => undo_states.new_undo_bone(&armature.bones[selections.bone_idx]),
            E::ClickVertex if selections.bind != -1 => undo_states.new_undo_bone(&armature.bones[selections.bone_idx]),
            E::NewAnimation | E::DeleteAnim => undo_states.new_undo_anims(&armature.animations),
            E::DeleteSelectedTextures       => undo_states.new_undo_style(&armature.sel_style(&selections).unwrap()),
            E::DeleteStyle | E::NewStyle    => undo_states.new_undo_styles(&armature.styles),
            E::AddCamera | E::DeleteCamera | E::DuplicateCamera | E::FitCamera => undo_states.new_undo_cameras(&armature.cameras),
            E::SetCamera if events.values[0] == 1. => undo_states.new_undo_cameras(&armature.cameras),
            E::SetLayer if events.values[0] == 1. => undo_states.new_undo_anim(armature.sel_anim(&selections).unwrap()),
            E::AddShape | E::DeleteShape | E::DuplicateShape => {
                if let Some(b) = armature.bones.iter().find(|b| b.id == events.values[0] as i32) {
                    undo_states.new_undo_bone(b);
                    undo_states.new_undo_anims(&armature.animations);
                    undo_states.undo_actions.last_mut().unwrap().continued = true;
                }
            }
            E::SetShape if events.str_values[0].starts_with('1') => {
                if let Some(b) = armature.bones.iter().find(|b| b.id == events.values[0] as i32) {
                    undo_states.new_undo_bone(b);
                }
            }
            E::EditShapeWeight if events.values[5] == 1. => {
                if events.values[3] >= 0. && events.values[4] >= 0. {
                    if let Some(a) = armature.animations.get(events.values[3] as usize) {
                        undo_states.new_undo_anim(a);
                    }
                } else if let Some(b) = armature.bones.iter().find(|b| b.id == events.values[0] as i32) {
                    undo_states.new_undo_bone(b);
                }
            }
            E::RenameStyle => if !ui.just_made_style { undo_states.new_undo_style(&armature.sel_style(&selections).unwrap()); ui.just_made_style = false }
            E::DeleteSelectedKeyframes | E::DeleteKeyframeLine | E::PasteKeyframesOnFrame | E::InsertKeyframe | E::InsertKeyframeAll | E::DeleteBoneKeyframes | E::SetAnimEnd | E::DeleteKeyframeGroup | E::ScaleSelectedKeyframes
            | E::AddLayer | E::DeleteLayer | E::DuplicateLayer | E::MergeLayerDown | E::FlattenLayers
            | E::CloseLoop | E::BakeGenerator | E::MoveLayer => {
                undo_states.new_undo_anim(armature.sel_anim(&selections).unwrap())
            }
            E::ResetVertices | E::CenterBoneVerts | E::DeleteVertex | E::TraceBoneVerts | E::NewVertex | E::DeleteTriangle | E::RetriangulateVerts | E::PenApply => {
                undo_states.new_undo_bone(&armature.bones[selections.bone_idx])
            }
            _ => {}
        };
    }

    if event == Events::MoveAnimation {
        let drag = events.values[0] as usize;
        let mut hov = events.values[1] as usize;
        let anim = armature.animations[drag].clone();
        armature.animations.remove(drag);

        // anim will end up below selected if hov isn't adjusted
        if hov > drag {
            hov -= 1;
        }

        armature.animations.insert(hov, anim);

        selections.anim = hov;

        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::ToggleSelectedTexture {
        let tex_id = events.values[0] as i32;
        let select = events.values[1] == 1.;
        if select {
            if !selections.tex_ids.contains(&tex_id) {
                selections.tex_ids.push(tex_id);
            }
        } else {
            selections.tex_ids.retain(|id| *id != tex_id);
        }
        selections.tex_ids.sort();
        ui.last_selected = "texture".to_string();

        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::TrimTexture {
        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::SetExportTexPadding {
        edit_mode.export_tex_padding = Vec2::new(events.values[0], events.values[1]);
        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::EditVertexUV {
        let sel = &selections;
        let bone = armature.sel_bone_mut(sel).unwrap();
        let vert_id = events.values[0] as u32;
        let vert = bone.vertices.iter_mut().find(|v| v.id == vert_id).unwrap();
        vert.uv = Vec2::new(events.values[1], events.values[2]);

        events.events.remove(0);
        events.values.drain(0..=2);
    } else if event == Events::EditVertexPos {
        let sel = &selections;
        let bone = armature.sel_bone_mut(sel).unwrap();
        let vert_id = events.values[0] as u32;
        let vert = bone.vertices.iter_mut().find(|v| v.id == vert_id).unwrap();
        vert.pos = Vec2::new(events.values[1], events.values[2]);

        events.events.remove(0);
        events.values.drain(0..=2);
    } else if event == Events::SelectVertex {
        let vert_id = events.values[0] as i32;
        if vert_id == -1 {
            selections.vert_ids = vec![];
            renderer.clicked_vert_id = -1;
        } else {
            let force_append = events.values[1] == 1.;
            if input.holding_mod || input.holding_shift || force_append {
                selections.vert_ids.push(vert_id as usize);
            } else {
                selections.vert_ids = vec![vert_id as usize];
            }
        }

        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::UpdateKeyframeTransition {
        let frame = events.values[0] as usize;
        let is_in = events.values[1] == 1.;
        let handle = Vec2::new(events.values[2], events.values[3]);
        let preset = events.values[4] as i32;

        for kf in &mut armature.sel_anim_mut(selections).unwrap().keyframes {
            if kf.frame != frame as i32 {
                continue;
            }
            if is_in {
                kf.start_handle = handle;
            } else {
                kf.end_handle = handle;
            }
            kf.handle_preset = if preset == -1 {
                HandlePreset::Custom
            } else {
                HandlePreset::from_repr(preset as usize).unwrap()
            };
        }

        events.events.remove(0);
        events.values.drain(0..=4);
    } else if event == Events::SetExportClearColor {
        edit_mode.export_clear_color = Color::new(
            (events.values[0] * 255.).round() as u8,
            (events.values[1] * 255.).round() as u8,
            (events.values[2] * 255.).round() as u8,
            0,
        );

        events.events.remove(0);
        events.values.drain(0..=2);
    } else if event == Events::EditShapeWeight {
        let v = &events.values;
        let anim = if v[3] < 0. || !edit_mode.anim_open { usize::MAX } else { v[3] as usize };
        let frame = if anim == usize::MAX { -1 } else { v[4] as i32 };
        shapes::edit_weight(armature, v[0] as i32, v[1] as i32, v[2], anim, frame);
        events.events.remove(0);
        events.values.drain(0..=5);
    } else if event == Events::AddShape
        || event == Events::DeleteShape
        || event == Events::SetShape
        || event == Events::ToggleSculptShape
        || event == Events::DuplicateShape
    {
        let bone_id = events.values[0] as i32;
        let s = events.str_values[0].clone();
        match event {
            Events::AddShape => {
                if let Some(bone) = armature.bones.iter_mut().find(|b| b.id == bone_id) {
                    let id = shapes::next_id(bone);
                    let name = shapes::unique_name(bone, if s.is_empty() { "Shape" } else { &s });
                    bone.shapes.push(MeshShape { id, name, ..Default::default() });
                }
            }
            Events::DeleteShape => {
                if let Ok(id) = s.parse::<i32>() {
                    shapes::delete(armature, bone_id, id);
                    if edit_mode.sculpt_shape == Some((bone_id, id)) {
                        edit_mode.sculpt_shape = None;
                    }
                }
            }
            Events::ToggleSculptShape => {
                if let Ok(id) = s.parse::<i32>() {
                    if edit_mode.sculpt_shape == Some((bone_id, id)) {
                        edit_mode.sculpt_shape = None;
                    } else {
                        edit_mode.sculpt_shape = Some((bone_id, id));
                        // sculpting drags vertices: show them
                        edit_mode.editing_mesh = true;
                        edit_mode.showing_mesh = true;
                    }
                }
            }
            Events::DuplicateShape => {
                let mut parts = s.split('|');
                let id = parts.next().and_then(|p| p.parse::<i32>().ok());
                let mirror = parts.next() == Some("1");
                if let (Some(id), Some(bone)) = (id, armature.bones.iter_mut().find(|b| b.id == bone_id)) {
                    if let Some(src) = bone.shapes.iter().find(|s| s.id == id).cloned() {
                        let mut copy = src.clone();
                        copy.id = shapes::next_id(bone);
                        if mirror {
                            copy.deltas = shapes::mirrored(bone, &src, shapes::mirror_tolerance(bone)).0;
                            copy.name = shapes::unique_name(bone, &shapes::mirror_name(&src.name));
                        } else {
                            copy.name = shapes::unique_name(bone, &src.name);
                        }
                        bone.shapes.push(copy);
                    }
                }
            }
            _ => {
                let json = s.splitn(2, '|').nth(1).unwrap_or("");
                if let (Ok(shape), Some(bone)) = (serde_json::from_str::<MeshShape>(json), armature.bones.iter_mut().find(|b| b.id == bone_id)) {
                    if let Some(old) = bone.shapes.iter_mut().find(|o| o.id == shape.id) {
                        *old = shape;
                    }
                }
            }
        }
        events.events.remove(0);
        events.values.remove(0);
        events.str_values.remove(0);
    } else if event == Events::ScaleSelectedKeyframes {
        let (pivot, factor) = (events.values[0] as i32, events.values[1]);
        let anim = armature.sel_anim_mut(&selections).unwrap();
        let (keys, moved, _) = timeline_rows::retime(&anim.keyframes, &ui.selected_keyframes, |f| {
            timeline_rows::scale_frame(f, pivot, factor)
        });
        anim.keyframes = keys;
        ui.selected_keyframes = moved;
        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::DeleteKeyframeGroup {
        let bone_id = events.values[0] as i32;
        if let Some(group) = timeline_rows::Group::from_index(events.values[1] as usize) {
            let keys = &mut armature.sel_anim_mut(&selections).unwrap().keyframes;
            keys.retain(|kf| !(kf.bone_id == bone_id && timeline_rows::Group::of(&kf.element) == group));
        }
        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::DeleteKeyframeLine {
        armature
            .sel_anim_mut(&selections)
            .unwrap()
            .keyframes
            .retain(|kf| {
                !(kf.bone_id == events.values[0] as i32
                    && kf.element == AnimElement::from_repr(events.values[1] as usize).unwrap())
            });
        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::SelectAnimFrame {
        let selected_anim = selections.anim;
        let selected_bone_idx = selections.bone_idx;
        let selected_bone_ids = selections.bone_ids.clone();
        unselect_all(selections, edit_mode, ui);
        ui.last_selected = "keyframe".to_string();
        selections.anim = selected_anim;
        selections.anim_frame = events.values[0] as i32;

        // select all keyframes in this frame if requested
        if events.values[1] == 1. {
            if !input.holding_mod && !input.holding_shift && events.values[2] == 0. {
                ui.selected_keyframes = vec![];
            }
            if !input.holding_shift {
                // select just this diamond's keyframes
                for kf in &armature.sel_anim(&selections).unwrap().keyframes {
                    if kf.frame == selections.anim_frame && !ui.selected_keyframes.contains(&kf) {
                        ui.selected_keyframes.push(kf.clone());
                    }
                }
            } else {
                // select all diamonds' keyframes between this and last selected one
                let left = ui.last_selected_frame.min(selections.anim_frame);
                let right = ui.last_selected_frame.max(selections.anim_frame);
                for kf in &armature.sel_anim(&selections).unwrap().keyframes {
                    if kf.frame >= left && kf.frame <= right && !ui.selected_keyframes.contains(&kf)
                    {
                        ui.selected_keyframes.push(kf.clone());
                    }
                }
            }
        }

        selections.bone_idx = selected_bone_idx;
        selections.bone_ids = selected_bone_ids;
        ui.last_selected_frame = selections.anim_frame;
        events.events.remove(0);
        events.values.drain(0..=2);
    } else if event == Events::EditEnvelope {
        if let Some(field) = EnvelopeField::from_repr(events.values[0] as usize) {
            if let Some(id) = armature.sel_bone(selections).map(|b| b.id) {
                skin::set_envelope_field(armature, id, field, events.values[1]);
            }
        }
        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::SetBindWeight {
        let sel_bind = events.values[0] as usize;
        let vert = events.values[1] as usize;
        let weight = events.values[2];
        let mesh = armature.sel_bone_mut(&selections).unwrap();
        mesh.binds[sel_bind].verts[vert].weight = weight;
        let vert_id = mesh.binds[sel_bind].verts[vert].id as u32;
        skin::lock_vertex(mesh, vert_id);

        events.events.remove(0);
        events.values.drain(0..=2);
    } else if event == Events::SetVertShare {
        let (vert, bone, share) = (events.values[0] as u32, events.values[1] as i32, events.values[2]);
        let keep = events.values[3] == 1.;
        if let Some(mesh_id) = armature.sel_bone(&selections).map(|b| b.id) {
            skin::edit_shares(armature, mesh_id, bone, &[(vert, share)], keep);
        }
        events.events.remove(0);
        events.values.drain(0..=3);
    } else if event == Events::ToggleBindPathing {
        let sel_bind = events.values[0] as usize;
        let is_pathing = events.values[1] == 1.;
        armature.sel_bone_mut(&selections).unwrap().binds[sel_bind].is_path = is_pathing;

        // adjust vertices, so they stay in place
        let bind = &armature.sel_bone_mut(&selections).unwrap().binds[sel_bind].clone();
        let sel_bone = &mut armature.sel_bone_mut(&selections).unwrap();
        let temp_bones = &renderer.temp_bones;
        let temp_bone = temp_bones.iter().find(|b| b.id == sel_bone.id).unwrap();
        for vert in &bind.verts {
            let id = vert.id as u32;
            let vert = sel_bone.vertices.iter_mut().find(|v| v.id == id).unwrap();
            let mut rot = renderer::get_path_normal_angle(temp_bones, temp_bone, sel_bind);
            if is_pathing {
                rot = -rot;
            }
            vert.pos = utils::rotate(&vert.pos, rot);
        }

        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::EditCamera {
        camera.pos = Vec2::new(events.values[0], events.values[1]);
        camera.zoom = MIN_ZOOM.max(events.values[2]);

        events.events.remove(0);
        events.values.drain(0..=2);
    } else if event == Events::EditBone {
        let anim_el = AnimElement::from_repr(events.values[1] as usize).unwrap();
        let mut anim_id = events.values[3] as usize;
        let anim_frame = events.values[4] as i32;

        // don't record in anims if in Armature mode
        if !edit_mode.anim_open {
            anim_id = usize::MAX;
        }

        #[rustfmt::skip]
        edit_bone(armature, config, events.values[0] as i32, anim_el, events.values[2], events.str_values[0].clone(), anim_id, anim_frame);

        events.events.remove(0);
        events.values.drain(0..=4);
        events.str_values.remove(0);
    } else if event == Events::ToggleBoneFolded {
        let idx = events.values[0] as usize;
        armature.bones[idx].folded = events.values[1] == 1.;
        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::ToggleBoneAnimFolded {
        let idx = events.values[0] as usize;
        armature.bones[idx].anim_folded = events.values[1] == 1.;
        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::MoveTexture {
        let new_idx = events.values[0] as usize;
        let sel = &selections;
        let textures = &mut armature.sel_style_mut(sel).unwrap().textures;
        let tex = textures[events.values[1] as usize].clone();
        textures.remove(events.values[1] as usize);
        textures.insert(new_idx, tex);

        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::MigrateTexture {
        let style = &mut armature.sel_style_mut(selections).unwrap();
        let tex = style.textures[events.values[0] as usize].clone();
        style.textures.remove(events.values[0] as usize);
        armature.styles[events.values[1] as usize]
            .textures
            .push(tex);
        ui.dragging_tex = false;

        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::MoveStyle {
        armature
            .styles
            .swap(events.values[0] as usize, events.values[1] as usize);
        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::ToggleStyleActive {
        armature.styles[events.values[0] as usize].active =
            !armature.styles[events.values[0] as usize].active;
        for b in 0..armature.bones.len() {
            let bone = &armature.bones[b];
            if bone.tex != "" {
                armature.set_bone_tex(bone.id, bone.tex.clone(), usize::MAX, -1);
            }
        }
        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::ToggleAnimPlaying {
        let anim = &mut armature.animations[events.values[0] as usize];
        let playing = events.values[1] == 1.;
        anim.elapsed = if playing { Some(Instant::now()) } else { None };
        events.events.remove(0);
        events.values.drain(0..=1);
    } else if event == Events::DragBone {
        // dropping dragged bone and moving it (or setting it as child)
        let is_above = events.values[0] == 1.;
        let pointing_id = events.values[1] as i32;
        let dragging_id = events.values[2] as i32;
        if selections.bone_ids.len() < 2 {
            selections.bone_ids = vec![dragging_id];
            let tex = armature.tex_of(dragging_id);
            if tex == None {
                edit_mode.showing_mesh = false;
            }
        } else {
            // only move root bones (in context of selected bones)
            selections.bone_ids = selections.only_root_bones(&armature.bones)
        }
        let new_bone_idx = drag_bone(armature, pointing_id, &selections.bone_ids, is_above);
        if new_bone_idx != usize::MAX {
            selections.bone_idx = new_bone_idx;
        }
        events.events.remove(0);
        events.values.drain(0..=2);
    } else {
        // normal events: 1 event ID, 1 set of value(s)

        let event = &events.events[0].clone();
        let value = events.values[0];
        let str_value = events.str_values[0].clone().to_string();
        #[rustfmt::skip]
        editor::simple_event(
            event, value, str_value, camera, &input, edit_mode, selections,
            undo_states, armature, psd_armature, copy_buffer, ui, renderer, config
        );

        events.events.remove(0);
        events.values.remove(0);
        events.str_values.remove(0);
    }
}

// process events that only have one numerical, and one string value
pub fn simple_event(
    event: &crate::Events,
    value: f32,
    str_value: String,
    camera: &mut Camera,
    input: &InputStates,
    edit_mode: &mut EditMode,
    selections: &mut SelectionState,
    undo_states: &mut UndoStates,
    armature: &mut Armature,
    psd_armature: &mut Armature,
    copy_buffer: &mut CopyBuffer,
    ui: &mut crate::Ui,
    renderer: &mut crate::Renderer,
    config: &mut crate::Config,
) {
    match event {
        Events::CamZoomIn => camera.zoom = MIN_ZOOM.max(camera.zoom / navigation::ZOOM_STEP),
        Events::CamZoomOut => camera.zoom *= navigation::ZOOM_STEP,
        Events::FitView => {
            // the selected bone, or every bone if none is selected
            let ids = armature.sel_bone(selections).map(|bone| vec![bone.id]);
            let bones = &renderer.temp_bones;
            let bounds = navigation::bones_bounds(bones, armature, ids.as_deref());
            if let (Some((min, max)), Some(canvas)) = (bounds, ui.canvas_rect) {
                let canvas_min = Vec2::new(canvas.min.x, canvas.min.y) * ui.scale;
                let canvas_max = Vec2::new(canvas.max.x, canvas.max.y) * ui.scale;
                let offset = renderer::world_camera(camera, config).pos - camera.pos;
                *camera = navigation::fit(camera, min, max, canvas_min, canvas_max, offset);
            }
        }
        Events::EditModeMove => set_transform_tool(edit_mode, EditModes::Move),
        Events::EditModeRotate => set_transform_tool(edit_mode, EditModes::Rotate),
        Events::EditModeScale => set_transform_tool(edit_mode, EditModes::Scale),
        Events::SetTool => {
            edit_mode.tool = Tool::from_repr(value as usize).unwrap_or_default();
            // pending cuts are discarded on leaving the Pen (D7)
            if edit_mode.tool != Tool::Pen {
                clear_pen(renderer);
            }
            if edit_mode.tool.edits_mesh() {
                // a selection would be dragged by the stroke
                edit_mode.editing_mesh = true;
                selections.vert_ids = vec![];
            }
            // painting needs a bone to paint: the first weight bind, if none is picked
            if edit_mode.tool == Tool::PaintWeights {
                edit_mode.editing_mesh = false;
                let mesh = armature.sel_bone(&selections);
                let picked = mesh.map_or(false, |m| skin::bind_shares(&armature.bones, m, selections.bind.max(0) as usize).is_some() && selections.bind >= 0);
                if !picked {
                    if let Some(m) = mesh {
                        let first = (0..m.binds.len()).find(|k| skin::bind_shares(&armature.bones, m, *k).is_some());
                        selections.bind = first.map_or(-1, |k| k as i32);
                    }
                }
            }
        }
        Events::SetEraserDelete => edit_mode.eraser_delete = value == 1.,
        Events::SetPaintMode => edit_mode.paint_mode = weights::PaintMode::from_repr(value as usize).unwrap_or_default(),
        Events::SetPaintRadius => edit_mode.paint_radius = value.clamp(2., 1000.),
        Events::SetPaintStrength => edit_mode.paint_strength = value.clamp(0., 1.),
        Events::TogglePaintLasso => edit_mode.paint_lasso = !edit_mode.paint_lasso,
        // a closed lasso: Add gives the vertices fully to the bone, Subtract takes them
        // off it, Smooth smooths its share (undo is saved when the lasso starts)
        Events::PaintLasso => {
            let Some((ids, mode)) = renderer.lasso_hit.take() else { return };
            let Some(mesh) = armature.sel_bone(&selections).cloned() else { return };
            if selections.bind < 0 || ids.is_empty() {
                return;
            }
            let Some((bone, _)) = skin::bind_shares(&armature.bones, &mesh, selections.bind as usize) else { return };
            match mode {
                weights::PaintMode::Add | weights::PaintMode::Subtract => {
                    let value = if mode == weights::PaintMode::Add { 1. } else { 0. };
                    let edits: Vec<(u32, f32)> = ids.iter().map(|v| (*v, value)).collect();
                    skin::edit_shares(armature, mesh.id, bone, &edits, false);
                }
                weights::PaintMode::Smooth => {
                    skin::smooth_vertices(armature, mesh.id, &ids, Some(bone), 3);
                }
            }
        }
        // a dab of the brush (undo is saved when the stroke starts)
        Events::PaintWeights => {
            let Some((dab, mode)) = renderer.paint_dab.take() else { return };
            let Some(mesh) = armature.sel_bone(&selections).cloned() else { return };
            if selections.bind < 0 {
                return;
            }
            let Some((bone, _)) = skin::bind_shares(&armature.bones, &mesh, selections.bind as usize) else { return };
            skin::paint(armature, mesh.id, bone, &dab, mode, edit_mode.paint_strength);
        }
        Events::ToggleShowAllEnvelopes => edit_mode.show_all_envelopes = !edit_mode.show_all_envelopes,
        Events::ToggleShowCameras => edit_mode.show_cameras = !edit_mode.show_cameras,
        Events::AddLayer | Events::SelectLayer | Events::SetLayer | Events::DeleteLayer
        | Events::DuplicateLayer | Events::MergeLayerDown | Events::FlattenLayers
        | Events::CloseLoop | Events::BakeGenerator | Events::MoveLayer => {
            let bones = armature.bones.clone();
            let anims = armature.animations.clone();
            if let Some(anim) = armature.animations.get_mut(selections.anim) {
                let ctx = layers::Ctx { anims: &anims, clock: None };
                layers::layer_event(event, value, &str_value, anim, &ctx, &bones);
            }
            // keys selected on the old layer aren't on the new one
            ui.selected_keyframes = vec![];
        }
        Events::AddCamera | Events::SelectCamera | Events::SetCamera | Events::DeleteCamera
        | Events::DuplicateCamera | Events::FitCamera => {
            camera_ui::camera_event(event, value, &str_value, armature, selections, edit_mode, ui);
        }
        Events::TogglePenAngleLock => edit_mode.pen_angle_lock = !edit_mode.pen_angle_lock,
        Events::TogglePenSnap => edit_mode.pen_no_snap = !edit_mode.pen_no_snap,
        Events::PenApply => {
            if renderer.pen_cuts.is_empty() {
                return;
            }
            let bone = armature.sel_bone(&selections).unwrap();
            let tex_size = armature.tex_of(bone.id).map(|t| t.size).unwrap_or_default();
            let bone = armature.sel_bone_mut(&selections).unwrap();
            match pen::apply_cuts(bone, &renderer.pen_cuts, tex_size) {
                Ok(_) => clear_pen(renderer),
                Err(topology::TopoError::Degenerate) => {
                    open_modal(ui, false, ui.loc("pen_degenerate"));
                }
                Err(_) => open_modal(ui, false, ui.loc("pen_failed")),
            }
            let bone = armature.sel_bone(&selections).unwrap();
            let ids: Vec<usize> = bone.vertices.iter().map(|v| v.id as usize).collect();
            selections.vert_ids.retain(|id| ids.contains(id));
        }
        Events::PenCancel => {
            if renderer.pen_cuts.is_empty() {
                unselect_all(selections, edit_mode, ui);
            } else {
                clear_pen(renderer);
            }
        }
        Events::PenBackspace => {
            if let Some(cut) = renderer.pen_cuts.last_mut() {
                if cut.closed {
                    cut.closed = false;
                } else {
                    cut.points.pop();
                }
                if cut.points.is_empty() {
                    renderer.pen_cuts.pop();
                }
                // carry on drawing the cut that's now last
                renderer.pen_open = !renderer.pen_cuts.is_empty();
                renderer.pen_rev += 1;
            }
        }
        Events::EraseStart => {
            undo_states.new_undo_bone(&armature.bones[selections.bone_idx]);
        }
        Events::EraseMesh => {
            let Some((pick, delete)) = renderer.erase_pick.take() else {
                return;
            };
            let bone = armature.sel_bone_mut(&selections).unwrap();
            match mesh_tools::erase(bone, pick, delete) {
                Ok(()) => bone.verts_edited = true,
                Err(topology::TopoError::LastTriangle) => {
                    // end the stroke, so the modal doesn't reopen as it continues
                    renderer.erasing = false;
                    let msg = match pick {
                        mesh_tools::Pick::Vertex(_) => "vert_limit",
                        mesh_tools::Pick::Edge(..) => "indices_limit",
                    };
                    open_modal(ui, false, ui.loc(msg));
                }
                // a concave quad can't flip its diagonal; leave the edge as it is
                Err(_) => {}
            }
            let bone = armature.sel_bone(&selections).unwrap();
            let ids: Vec<usize> = bone.vertices.iter().map(|v| v.id as usize).collect();
            selections.vert_ids.retain(|id| ids.contains(id));
        }
        Events::UnselectAll => unselect_all(selections, edit_mode, ui),
        Events::Undo => {
            undo_redo(true, undo_states, armature, selections);
            ui.changed_window_name = false;
        }
        Events::Redo => {
            undo_redo(false, undo_states, armature, selections);
            ui.changed_window_name = false;
        }
        Events::ResetConfig => {
            if let Ok(data) = serde_json::from_str(&utils::config_str()) {
                *config = data;
            }
            if let Ok(data) = serde_json::from_str(&utils::color_str()) {
                config.colors = data;
            }
        }
        Events::RenameBone => armature.bones[value as usize].name = str_value,
        Events::RenameAnim => {
            if !ui.just_made_anim {
                undo_states.new_undo_anim(&armature.animations[value as usize]);
            }
            armature.animations[value as usize].name = str_value;
            ui.just_made_anim = false;
        }
        Events::PointerOnUi => camera.on_ui = value == 1.,
        Events::ToggleShowingMesh => {
            edit_mode.showing_mesh = value == 1.;
            if value != 1. {
                ui.tracing = false;
                selections.hovering_vert_id = -1;
                selections.vert_ids = vec![];
            }
        }
        Events::ToggleEditingMesh => {
            edit_mode.editing_mesh = !edit_mode.editing_mesh;
            if !edit_mode.editing_mesh && edit_mode.tool.edits_mesh() {
                edit_mode.tool = Tool::Transform;
            }

            // unselect all verts when switching modes
            selections.vert_ids = vec![]
        }
        Events::ToggleSettingIkTarget => {
            edit_mode.setting_ik_target = value == 1.;
            if edit_mode.setting_ik_target {
                ui.flash_armature_timer = Some(Instant::now());
            }
        }
        Events::ToggleSettingBindBone => {
            edit_mode.setting_bind_bone = value == 1.;
            if edit_mode.setting_bind_bone {
                ui.flash_armature_timer = Some(Instant::now());
            }
        }
        Events::ToggleOnionLayers => edit_mode.onion_layers = value == 1.,
        Events::DeleteIkTarget => {
            if let Some(ik) = &mut armature.sel_bone_mut(selections).unwrap().ik {
                ik.target_id = -1;
            }
        }
        Events::AddIk => add_ik(armature, selections),
        Events::SetIkTargetSpace => {
            if let Some(tip) = armature.sel_bone(selections).map(|b| b.id) {
                set_ik_target_space(armature, tip, value as i32);
            }
        }
        Events::BindToSkeleton => {
            let mut errors = vec![];
            let mut outside = 0;
            for id in selections.bone_ids.clone() {
                let name = armature.bones.iter().find(|b| b.id == id).map_or(String::new(), |b| b.name.clone());
                match skin::bind_to_skeleton(armature, id) {
                    Ok(verts) => outside += verts.len(),
                    Err(err) => errors.push(format!("{name}: {err}")),
                }
            }
            if !errors.is_empty() {
                ui.custom_error = errors.join("\n");
                let headline = ui.loc("bone_panel.skin.bind_failed");
                open_modal(ui, false, headline);
            } else if outside > 0 {
                ui.custom_error = outside.to_string();
                let headline = ui.loc("bone_panel.skin.outside_report");
                open_modal(ui, false, headline);
            }
        }
        Events::ClearAllBinds => {
            for id in selections.bone_ids.clone() {
                skin::clear_all_binds(armature, id);
            }
            selections.bind = -1;
        }
        Events::ToggleEnvelopeWeights => {
            let mesh = armature.sel_bone_mut(selections).unwrap();
            mesh.envelope_weights = value == 1.;
            mesh.skin_sig = 0;
        }
        Events::SetMaxInfluences => {
            armature.sel_bone_mut(selections).unwrap().max_influences = value as u32;
        }
        Events::AddBonesInReach => {
            let id = armature.sel_bone(selections).unwrap().id;
            skin::add_bones_in_reach(armature, id);
        }
        Events::UnlockSelectedVerts => {
            let sel_verts: Vec<u32> = selections.vert_ids.iter().map(|v| *v as u32).collect();
            let mesh = armature.sel_bone_mut(selections).unwrap();
            mesh.locked_verts.retain(|v| !sel_verts.contains(v));
        }
        Events::UnlockAllVerts => armature.sel_bone_mut(selections).unwrap().locked_verts.clear(),
        Events::SelectOutsideVerts => {
            let mesh = armature.sel_bone(selections).unwrap();
            selections.vert_ids = mesh.skin_outside.iter().map(|v| *v as usize).collect();
            edit_mode.editing_mesh = true;
        }
        Events::ToggleEnvelopeOpen => {
            armature.sel_bone_mut(selections).unwrap().envelope_open = value == 1.;
        }
        Events::RemoveIk => {
            armature.sel_bone_mut(selections).unwrap().ik = None;
        }
        Events::SetIkChain => {
            let id = armature.sel_bone(selections).unwrap().id;
            let max = ik::depth(&armature.bones, id).max(1);
            if let Some(ik) = &mut armature.sel_bone_mut(selections).unwrap().ik {
                ik.chain = (value as u32).clamp(1, max);
            }
        }
        Events::ToggleIkFolded => {
            armature.sel_bone_mut(&selections).unwrap().ik_folded = value == 1.
        }
        Events::TogglePhysFolded => {
            armature.sel_bone_mut(&selections).unwrap().phys_folded = value == 1.
        }
        Events::ToggleIkEnabled => {
            if let Some(ik) = &mut armature.sel_bone_mut(&selections).unwrap().ik {
                ik.enabled = value == 1.;
            }
        }
        Events::ToggleMeshdefFolded => {
            armature.sel_bone_mut(&selections).unwrap().meshdef_folded = value == 1.
        }
        Events::ToggleEffectsFolded => {
            armature.sel_bone_mut(&selections).unwrap().effects_folded = value == 1.
        }
        Events::CamZoomScroll => {
            *camera = navigation::zoom_scroll(camera, input.mouse, input.scroll_delta);
        }
        Events::ToggleAnimPanelOpen => {
            edit_mode.anim_open = value == 1.;
            if !edit_mode.anim_open {
                selections.anim_frame = -1;
            }
            for anim in &mut armature.animations {
                anim.elapsed = None;
            }
        }
        Events::CancelPendingTexture => {
            _ = armature.sel_style_mut(&selections).unwrap().textures.pop()
        }
        Events::DeleteAnim => {
            _ = armature.animations.remove(value as usize);
            selections.anim = usize::MAX
        }
        Events::RenameStyle => {
            armature.sel_style_mut(&selections).unwrap().name = str_value;
            ui.just_made_style = false
        }
        Events::NewArmature => {
            unselect_all(selections, edit_mode, ui);
            edit_mode.anim_open = false;
            camera.pos = Vec2::new(0., 0.);
            camera.zoom = 2000.;
            ui.save_path = None;
            ui.changed_window_name = false;
            *armature = Armature::default();
        }
        Events::NewStyle => {
            let ids = armature.styles.iter().map(|set| set.id).collect();
            armature.styles.push(crate::Style {
                id: generate_id(ids),
                name: "".to_string(),
                textures: vec![],
                active: true,
            });
            ui.rename_id = "style_".to_string() + &(armature.styles.len() - 1).to_string();
            ui.just_made_style = true;
        }
        Events::DeleteSelectedKeyframes => {
            for skf in &ui.selected_keyframes {
                let keyframes = &mut armature.sel_anim_mut(&selections).unwrap().keyframes;
                keyframes.retain(|kf| !timeline_rows::same_key(kf, skf));
            }
        }
        Events::SelectBone => {
            let render = str_value == "t";
            let val = value as usize;
            if edit_mode.setting_ik_target {
                if let Some(bone) = armature.sel_bone(selections) {
                    undo_states.new_undo_bone(bone);
                }
            }
            select_bone(selections, ui, armature, edit_mode, input, val, render);
        }
        Events::SelectAnim => {
            let val = value as usize;
            selections.anim = if value == f32::MAX { usize::MAX } else { val };
            selections.anim_frame = 0;
        }
        Events::SelectStyle => {
            selections.style_id = value as i32;
            ui.last_selected = if value as i32 == -1 { "" } else { "style" }.to_string();
            selections.tex_ids = vec![];
        }
        Events::OpenModal => {
            open_modal(ui, value == 1., ui.loc(&str_value));
        }
        Events::OpenPolarModal => {
            ui.polar_id = PolarId::from_repr(value as usize).unwrap();
            ui.polar_modal = true;
            ui.headline = str_value.to_string();
        }
        Events::DeleteBone => {
            let mut ids_to_delete = vec![armature.bones[value as usize].id];

            // delete all selected bones, if the one being deleted is also selected
            let bone_id = &armature.bones[value as usize].id;
            if selections.bone_ids.contains(bone_id) {
                for id in &selections.bone_ids {
                    ids_to_delete.push(*id);
                }
            }

            undo_states.new_undo_anims(&armature.animations);
            undo_states.undo_actions.last_mut().unwrap().continued = true;

            // the selected bone, by id: deleting bones above it shifts its index
            let selected_id = armature.bones.get(selections.bone_idx).map(|b| b.id);

            for id in ids_to_delete {
                let bone;
                if let Some(result) = armature.bones.iter().find(|b| b.id == id) {
                    bone = result;
                } else {
                    continue;
                }

                // remove all children of this bone as well
                let mut children = vec![bone.clone()];
                armature_window::get_all_children(&armature.bones, &mut children, &bone);
                children.reverse();
                for bone in &children {
                    let idx = armature.bones.iter().position(|b| b.id == bone.id);
                    armature.bones.remove(idx.unwrap());
                }

                // remove all references to this bone and it's children from all animations
                for bone in &children {
                    for a in 0..armature.animations.len() {
                        let anim = &mut armature.animations[a];

                        let mut temp_kfs = anim.keyframes.clone();
                        temp_kfs.retain(|kf| kf.bone_id != bone.id);
                        armature.animations[a].keyframes = temp_kfs;
                    }
                }

                // remove this bone from binds
                for bone in &mut armature.bones {
                    bone.binds.retain(|bind| bind.bone_id != id);
                    for child in &children {
                        bone.binds.retain(|bind| bind.bone_id != child.id);
                    }
                }

                // IK constraints that target this or its children lose their target
                for bone in &mut armature.bones {
                    if let Some(ik) = &mut bone.ik {
                        if children.iter().any(|c| c.id == ik.target_id) {
                            ik.target_id = -1;
                        }
                    }
                }

                // de-select bone(s)
                if selections.bone_idx == value as usize || selections.bone_ids.len() > 1 {
                    selections.bone_idx = usize::MAX;
                    selections.bone_ids = vec![];
                }
            }

            // keep only selected bones that still exist (a deleted hierarchy can hold
            // selected bones, e.g. a copy pasted under it), and re-find the selected one
            let exists = |id: &i32| armature.bones.iter().any(|b| b.id == *id);
            selections.bone_ids.retain(exists);
            selections.bone_idx = match selected_id.filter(exists) {
                Some(id) if selections.bone_idx != usize::MAX => {
                    armature.bones.iter().position(|b| b.id == id).unwrap_or(usize::MAX)
                }
                _ => usize::MAX,
            };
            if selections.bone_idx == usize::MAX {
                selections.bone_ids = vec![];
                selections.vert_ids = vec![];
                selections.bind = -1;
                if ui.last_selected == "bone" {
                    ui.last_selected = "".to_string();
                }
            }
        }
        Events::DeleteSelectedTextures => {
            let style = &mut armature.sel_style_mut(selections).unwrap();
            let mut t = -1;
            style.textures.retain(|_| {
                t += 1;
                !selections.tex_ids.contains(&t)
            });
            selections.tex_ids = vec![];
        }
        Events::DeleteStyle => {
            let styles = &mut armature.styles;
            let idx = styles.iter().position(|s| s.id == value as i32).unwrap();
            if selections.style_id == value as i32 {
                selections.style_id = -1;
            }
            styles.remove(idx);
        }
        Events::CopyBone => copy_bone(copy_buffer, selections, armature, value as usize),
        Events::PasteBone => paste_bone(copy_buffer, selections, armature, value as usize),
        Events::NewAnimation => {
            armature.new_animation();
            let idx = armature.animations.len() - 1;
            ui.rename_id = format!("anim_{}", idx.to_string());
            ui.edit_value = Some("".to_string());
        }
        Events::DuplicateAnim => {
            let anims = &armature.animations;
            let id = anims.iter().position(|a| a.id == value as i32).unwrap();
            let mut new_anim = anims[id].clone();
            let ids: Vec<i32> = anims.iter().map(|anim| anim.id).collect();
            new_anim.id = generate_id(ids);
            armature.animations.push(new_anim);
        }
        Events::SaveBone => {
            let bone = armature.bones[value as usize].clone();
            undo_states.new_undo_bone(&bone);
            *ui.saving.lock().unwrap() = Saving::Autosaving;
        }
        Events::SaveEditedBone => {
            // don't save if values are being edited to dragging the input.
            // dragging directly edits bones, so this event would be spammed
            if ui.edited_dragging {
                return;
            }

            let Some(bone) = armature.bones.get(value as usize).cloned() else { return };
            if ui.is_animating(&edit_mode, &selections) && !bone.locked {
                let anim = armature.animations[selections.anim as usize].clone();
                undo_states.new_undo_anim(&anim);
            } else {
                // save all bones, if multiple were edited
                if selections.bone_ids.len() > 0 {
                    undo_states.new_undo_bones(&armature.bones);
                } else {
                    undo_states.new_undo_bone(&bone);
                }
            }
            *ui.saving.lock().unwrap() = Saving::Autosaving;
        }
        Events::ApplySettings => {
            ui.scale = config.ui_scale;
            crate::utils::save_config(&config);
        }
        Events::NewBone => {
            let idx;
            if armature.sel_bone(&selections) == None {
                (_, idx) = armature.new_bone(-1);
            } else {
                let id = armature.sel_bone(&selections).unwrap().id;
                (_, idx) = armature.new_bone(id);
            }
            armature.bones[idx].name = "".to_string();
            let sel = selections;
            // don't select new bone until user has done it at least once this session.
            // This is to prevent user from being overwhelmed with bone panel
            if ui.selected_bone_first_time {
                select_bone(sel, ui, armature, edit_mode, input, idx, false);
            }
            ui.rename_id = "bone_".to_string() + &idx.to_string();

            // mark this bone as selected, so focus isn't taken away from bone name input
            ui.prev_selected_bone_idx = idx;
        }
        Events::SetBoneTexture => {
            let frame = selections.anim_frame;
            armature.set_bone_tex(value as i32, str_value.clone(), selections.anim, frame);
        }
        Events::DeleteVertex => {
            // right-click on a vertex dissolves it: the hole is filled from its neighbours
            let bone = armature.sel_bone_mut(&selections).unwrap();
            let vert_id = bone.vertices[value as usize].id as usize;
            if topology::dissolve_vertex(bone, value as u32).is_err() {
                open_modal(ui, false, ui.loc("vert_limit"));
                return;
            }
            selections.vert_ids.retain(|id| *id != vert_id);
        }
        Events::DragVertex => {
            let bones = &renderer.temp_bones;
            let sel_id = armature.sel_bone(selections).unwrap().id;
            let bone = bones.iter().find(|b| b.id == sel_id).clone().unwrap();
            let temp_vert = bone.vertices.iter().find(|v| v.id == value as u32);
            if bone.vertices.len() == 0 || temp_vert == None {
                return;
            }

            // total rotation to cancel out when dragging vertex
            let mut total_rot = temp_vert.unwrap().offset_rot;

            // if a bind has a weight of 1, all other binds have no effect
            let mut overridden = false;

            let mut binds = bone.binds.clone();
            let mut scale = Vec2::new(1., 1.);
            binds.reverse();
            for bind in &binds {
                if overridden {
                    break;
                }
                if bind.bone_id == -1 {
                    continue;
                }
                let vert = bind.verts.iter().find(|v| v.id == value as i32);
                if vert == None {
                    continue;
                }

                let bones = &renderer.temp_bones;
                let bind_bone = bones.iter().find(|b| b.id == bind.bone_id).unwrap();
                scale = bind_bone.scale;
                if !bind.is_path {
                    total_rot += bind_bone.rot * vert.unwrap().weight;
                }
                overridden |= vert.unwrap().weight == 1.;
            }
            // add mesh bone's own rotation, if this vert is not bound
            if !overridden {
                total_rot += bone.rot;
                scale = bone.scale;
            }
            total_rot += bone.pivot_rot;

            let mouse_vel = renderer::mouse_vel(&input, &camera);
            let zoom = camera.zoom;
            let og_bone = &mut armature.sel_bone_mut(&selections).unwrap();
            let moved = utils::rotate(&(mouse_vel * zoom), -total_rot) / scale;
            // sculpting a shape: the drag shapes its offset
            if let Some((b, s)) = edit_mode.sculpt_shape.filter(|(b, _)| *b == og_bone.id) {
                let _ = b;
                if let Some(shape) = og_bone.shapes.iter_mut().find(|sh| sh.id == s) {
                    let d = shapes::delta_of(shape, value as u32);
                    shapes::set_delta(shape, value as u32, d - moved);
                }
                return;
            }
            og_bone.verts_edited = true;
            let vert_mut = og_bone.vertices.iter_mut().find(|v| v.id == value as u32);
            vert_mut.unwrap().pos -= moved;
        }
        Events::ClickVertex => {
            if selections.bind == -1 {
                return;
            }
            let bone_mut = &mut armature.sel_bone_mut(&selections).unwrap();
            let id = bone_mut.id;
            let idx = selections.bind as usize;
            let vert_id = bone_mut.vertices[value as usize].id;

            // bind-posed: the vertex goes fully to this bind's bone, or leaves it (its
            // share goes to the other bones), keeping shares normalized
            let mesh = bone_mut.clone();
            if let Some((bind_bone, shares)) = skin::bind_shares(&armature.bones, &mesh, idx) {
                let share = shares.get(&(vert_id as i32)).copied().unwrap_or(0.);
                let value = if share > 0. { 0. } else { 1. };
                skin::edit_shares(armature, id, bind_bone, &[(vert_id, value)], false);
                return;
            }
            let bone_mut = &mut armature.sel_bone_mut(&selections).unwrap();
            let bind = &bone_mut.binds[idx];

            // add/remove vertex to bind
            let mut bound = false;
            let mut unbound_bone_id = -1; // track the unbound bone id, to adjust position later
            if let Some(v) = bind.verts.iter().position(|vert| vert.id == vert_id as i32) {
                bone_mut.binds[idx].verts.remove(v);
                unbound_bone_id = bone_mut.binds[idx].bone_id;
            } else {
                bound = true;
                bone_mut.binds[idx].verts.push(BoneBindVert {
                    id: vert_id as i32,
                    weight: 1.,
                });
            }
            skin::lock_vertex(bone_mut, vert_id);

            if bind_pose::is_bind_posed(armature, id) {
                return;
            }
            // classic binding misplaces a vertex that's in more than one bind: say so now
            if bound {
                let mesh = armature.sel_bone(&selections).unwrap();
                if bind_pose::classic_multi_bind_verts(armature, mesh) > 0 {
                    ui.warnings_open = true;
                    ui.flash_warn_timer = Some(Instant::now());
                }
            }
            let bone_mut = &mut armature.sel_bone_mut(&selections).unwrap();

            let temp_bone = renderer.temp_bones.iter().find(|b| b.id == id).unwrap();
            let temp_bones = &renderer.temp_bones;

            for bind in &mut bone_mut.binds {
                let ids: Vec<u32> = bind.verts.iter().map(|v| v.id as u32).collect();
                if !ids.contains(&vert_id) && unbound_bone_id != bind.bone_id {
                    continue;
                }

                if temp_bones.iter().find(|b| b.id == bind.bone_id) == None {
                    continue;
                }

                let bind_bone = temp_bones.iter().find(|b| b.id == bind.bone_id).unwrap();
                let verts = &mut bone_mut.vertices;
                let vert = verts.iter_mut().find(|v| v.id == vert_id).unwrap();

                // get the rotation to offset by, based on bind type (weight, path, etc)
                let bind_id = bind.bone_id;
                let bind_idx = temp_bone.binds.iter().position(|b| b.bone_id == bind_id);
                let rot = if bind.is_path {
                    renderer::get_path_normal_angle(temp_bones, temp_bone, bind_idx.unwrap())
                } else {
                    bind_bone.rot
                };

                // offset vertex such that it stays still after binding/unbinding
                // todo: this currently only works if vertex is in 1 bind.
                // Make it account for all of them
                if bound {
                    vert.pos /= bind_bone.scale / temp_bone.scale;
                    vert.pos = utils::rotate(&vert.pos, temp_bone.rot);
                    vert.pos -= (bind_bone.pos - temp_bone.pos) / bind_bone.scale;
                    vert.pos = utils::rotate(&vert.pos, -rot);
                } else {
                    vert.pos = utils::rotate(&vert.pos, rot);
                    vert.pos += (bind_bone.pos - temp_bone.pos) / bind_bone.scale;
                    vert.pos = utils::rotate(&vert.pos, -temp_bone.rot);
                    vert.pos *= bind_bone.scale / temp_bone.scale;
                }
            }
        }
        Events::DeleteTriangle => {
            let bone = armature.sel_bone_mut(&selections).unwrap();
            if topology::remove_triangle(bone, value as usize / 3).is_err() {
                open_modal(ui, false, ui.loc("indices_limit"));
                return;
            }
            let ids: Vec<usize> = bone.vertices.iter().map(|v| v.id as usize).collect();
            selections.vert_ids.retain(|id| ids.contains(id));
        }
        Events::NewVertex => {
            // remove drag vertex action, since it's always triggered
            undo_states.undo_actions.pop();
            undo_states.new_undo_bone(&armature.bones[selections.bone_idx]);

            // split the triangle or edge it was placed on (docs/TOPOLOGY_TOOLS.md §4)
            let bone_mut = armature.sel_bone_mut(&selections).unwrap();
            let site = renderer.new_vert_site;
            if topology::add_vertex(bone_mut, renderer.new_vert.unwrap(), site).is_ok() {
                bone_mut.verts_edited = true;
            }
        }
        Events::RetriangulateVerts => {
            // the old automatic behaviour, on demand: Delaunay over the vertices
            let tex_img = renderer::sel_tex_img(armature.sel_bone(&selections).unwrap(), &armature);
            let bone = armature.sel_bone_mut(&selections).unwrap();
            let indices = triangulate(&bone.vertices, &tex_img);
            if indices.is_empty() {
                open_modal(ui, false, ui.loc("retriangulate_failed"));
                return;
            }
            bone.indices = indices;
            bone.verts_edited = true;
            cleanup_vertices(bone);
            let ids: Vec<usize> = bone.vertices.iter().map(|v| v.id as usize).collect();
            selections.vert_ids.retain(|id| ids.contains(id));
        }
        Events::SetAnimEnd => {
            // 0 (or less) follows the last key again
            if let Some(anim) = armature.sel_anim_mut(selections) {
                anim.end_frame = (value as i32).max(0);
            }
        }
        Events::AdjustKeyframesByFPS => {
            let anim_mut = armature.sel_anim_mut(selections).unwrap();

            let mut old_unique_keyframes: Vec<i32> =
                anim_mut.keyframes.iter().map(|kf| kf.frame).collect();
            old_unique_keyframes.dedup();

            let mut anim_clone = anim_mut.clone();

            // adjust keyframes to maintain spacing
            let div = anim_mut.fps as f32 / value;
            for kf in &mut anim_clone.keyframes {
                kf.frame = ((kf.frame as f32) / div) as i32
            }

            let mut unique_keyframes: Vec<i32> =
                anim_clone.keyframes.iter().map(|kf| kf.frame).collect();
            unique_keyframes.dedup();

            if unique_keyframes.len() == old_unique_keyframes.len() {
                anim_mut.fps = value as i32;
                anim_mut.keyframes = anim_clone.keyframes;
            } else {
                open_modal(ui, value == 1., ui.loc("keyframe_editor.invalid_fps"));
            }
        }
        Events::PasteKeyframesOnFrame => {
            paste_keyframes_on_frame(copy_buffer, armature, selections, value as i32)
        }
        Events::InsertKeyframe => {
            let ids = selections.bone_ids.clone();
            insert_keyframes(armature, selections.anim, selections.anim_frame, &ids);
        }
        // undo is saved when the slider is grabbed (SaveAnimation), not per step
        Events::Breakdown => {
            let ids = selections.bone_ids.clone();
            breakdown_keyframes(armature, selections.anim, selections.anim_frame, &ids, value);
        }
        Events::InsertKeyframeAll => {
            let ids = rig_bones(armature);
            insert_keyframes(armature, selections.anim, selections.anim_frame, &ids);
        }
        Events::DeleteBoneKeyframes => {
            let ids = selections.bone_ids.clone();
            delete_keyframes_at(armature, selections.anim, selections.anim_frame, &ids);
        }
        Events::DeleteKeyframesByFrame => {
            let anim = armature.sel_anim_mut(&selections).unwrap();
            anim.keyframes.retain(|kf| kf.frame != value as i32);
        }
        Events::TogglePoseMode => {
            if edit_mode.pose_mode {
                exit_pose_mode(armature, edit_mode, undo_states);
            } else {
                edit_mode.pose_snapshot = bind_pose::pose_snapshot(armature);
                edit_mode.pose_undo_len = undo_states.undo_actions.len();
                edit_mode.pose_mode = true;
            }
        }
        // the bind pose is captured at rest, so these wait until Pose Mode is off
        Events::SetBindPose | Events::SetBindPoseAll | Events::ClearBindPose
            if edit_mode.pose_mode => {}
        Events::SetBindPose => {
            let bone_id = armature.sel_bone(&selections).unwrap().id;
            if let Err(err) = bind_pose::set_bind_pose(armature, bone_id) {
                ui.custom_error = err;
                let headline = ui.loc("bone_panel.bind_pose.error");
                open_modal(ui, false, headline);
            }
        }
        Events::SetBindPoseAll => {
            let report = bind_pose::set_bind_pose_all(armature);
            let mut summary = format!("{} mesh(es) use bind pose.", report.converted.len());
            if !report.skipped.is_empty() {
                summary += &format!("\n\n{} skipped:", report.skipped.len());
                for (name, reason) in &report.skipped {
                    summary += &format!("\n- {name}: {reason}");
                }
            }
            ui.custom_error = summary;
            let headline = ui.loc("bone_panel.bind_pose.all_result");
            open_modal(ui, false, headline);
        }
        Events::ClearBindPose => {
            let bone_id = armature.sel_bone(&selections).unwrap().id;
            bind_pose::clear_bind_pose(armature, bone_id);
        }
        Events::ResetVertices => {
            let sel_bone = armature.sel_bone(&selections).unwrap().clone();
            let tex_size = armature.tex_of(sel_bone.id).unwrap().size.clone();
            let (verts, indices) = renderer::create_tex_rect(&tex_size);
            let bone = armature.sel_bone_mut(&selections).unwrap();
            selections.vert_ids = vec![];
            bone.vertices = verts;
            bone.indices = indices;
            bone.binds = vec![];
            bone.verts_edited = false;
            selections.bind = -1;
        }
        Events::SelectBind => {
            if value == -3. {
                selections.bind = -1;
            } else if value == -2. {
                let binds = &mut armature.sel_bone_mut(&selections).unwrap().binds;
                binds.push(BoneBind {
                    bone_id: -1,
                    ..Default::default()
                });
                selections.bind = binds.len() as i32 - 1;
            } else if value != -1. {
                selections.bind = value as i32;
            }
        }
        Events::CenterBoneVerts => {
            let verts = &mut armature.sel_bone_mut(&selections).unwrap().vertices;
            center_verts(verts)
        }
        Events::TraceBoneVerts => {
            let bone = armature.sel_bone(&selections).unwrap().clone();
            let tex = &armature.tex_of(bone.id).unwrap();
            let tex_data = &armature.tex_data;
            let data = tex_data.iter().find(|d| tex.data_id == d.id).unwrap();
            let (w, h) = (data.image.width(), data.image.height());
            let mut opts = auto_mesh::TraceOptions::for_size(w, h);
            // unset options take this texture's defaults; the toolbar then shows them
            opts.detail = *ui.tracing_detail.get_or_insert(opts.detail);
            opts.padding = *ui.tracing_padding.get_or_insert(opts.padding);
            opts.density = *ui.tracing_density.get_or_insert(opts.density);
            if ui.tracing_outline_only {
                opts.density = 0.;
            }
            opts.alpha_threshold = ui.tracing_alpha;
            let (verts, indices) = auto_mesh::trace(&data.image, &opts);
            if verts.len() < 3 || indices.len() < 3 {
                open_modal(ui, false, ui.loc("tracing_empty"));
                return;
            }
            let bone = &mut armature.sel_bone_mut(&selections).unwrap();
            selections.vert_ids = vec![];
            bone.vertices = verts;
            bone.indices = indices;
            bone.binds = vec![];
            bone.verts_edited = true;
            cleanup_vertices(bone);
            shapes::prune(bone);
            selections.bind = -1;
        }
        Events::RenameTex => {
            let style = armature.sel_style_mut(&selections).unwrap();
            let t = value as usize;
            let og_name = style.textures[t].name.clone();
            let trimmed = str_value.trim_start().trim_end().to_string();
            style.textures[t].name = trimmed.clone();
            let tex_names: Vec<String> = style.textures.iter().map(|t| t.name.clone()).collect();

            let filter = tex_names.iter().filter(|name| **name == trimmed);
            if filter.count() > 1 {
                style.textures[t].name = og_name.clone();
                open_modal(ui, false, ui.loc("styles_modal.same_name"));
            }

            if !config.keep_tex_str {
                for bone in &mut armature.bones {
                    if bone.tex != "" && bone.tex == og_name {
                        bone.tex = trimmed.clone();
                    }
                }
            }
        }
        Events::OpenFileErrModal => {
            open_modal(ui, false, ui.loc("import_err") + &str_value);
        }
        Events::ToggleBakingIk => edit_mode.export_bake_ik = value == 1.,
        Events::ToggleExcludeIk => edit_mode.export_exclude_ik = value == 1.,
        Events::SetExportImgFormat => {
            edit_mode.export_img_format = ExportImgFormat::from_repr(value as usize).unwrap()
        }
        Events::OpenExportModal => {
            ui.export_modal = true;
            for _ in &armature.animations {
                ui.exporting_anims.push({
                    #[cfg(target_arch = "wasm32")]
                    {
                        // since web doesn't support multi-exports,
                        // don't select any anim
                        false
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        true
                    }
                });
            }
        }
        Events::UpdateConfig => *config = ui.updated_config.clone(),
        Events::CopySelectedKeyframes => {
            copy_selected_keyframes(copy_buffer, ui);
        }
        Events::SaveAnimation => {
            undo_states.new_undo_anim(&armature.sel_anim(&selections).unwrap());
        }
        Events::UpdateRenderOptions => {
            renderer.render_points = ui.render_points;
            renderer.render_kites = ui.render_kites;
            renderer.render_textures = ui.render_textures;
            renderer.render_mesh_wf = ui.render_mesh_wf;
            renderer.render_rects = ui.render_rects;
        }
        Events::SetTemporaryEditMode => {
            if value == 3. {
                edit_mode.temporary = None;
            } else {
                edit_mode.temporary = EditModes::from_repr(value as usize);
            }
        }
        Events::ToggleEditModifying => edit_mode.holding_edit_mod = value == 1.,
        Events::ToggleEditSnapping => edit_mode.holding_edit_snap = value == 1.,
        Events::ToggleEditAlt => edit_mode.editing_pivot = value == 1.,
        Events::UpdateCurrentEditing => {
            if value == 1. {
                edit_mode.is_moving = false;
                edit_mode.is_rotating = false;
                edit_mode.is_scaling = false;
            } else {
                ui.cursor_icon = egui::CursorIcon::Crosshair;
                let current_edit = if let Some(temporary) = &edit_mode.temporary {
                    temporary
                } else {
                    &edit_mode.current
                };
                if *current_edit == EditModes::Other {
                    ui.cursor_icon = egui::CursorIcon::Default;
                }
                edit_mode.is_moving = *current_edit == EditModes::Move;
                edit_mode.is_rotating = *current_edit == EditModes::Rotate;
                edit_mode.is_scaling = *current_edit == EditModes::Scale;
            }
        }
        Events::RaiseGlobalZindex => {
            let bones = &armature.bones;
            let zindex = bones.iter().find(|b| b.id == value as i32).unwrap().zindex;
            for bone in &mut armature.bones {
                if bone.zindex > zindex && bone.tex != "" {
                    bone.zindex += 1;
                }
            }
            let bones = &mut armature.bones;
            let bone = bones.iter_mut().find(|b| b.id == value as i32).unwrap();
            bone.zindex += 1;
        }
        Events::SetRotResistance => {
            let bone = armature.sel_bone_mut(selections).unwrap();
            bone.phys_sway = value;
            if value == 0. {
                let mut children = vec![];
                let bone = armature.sel_bone(selections).unwrap();
                get_all_children(&armature.bones, &mut children, bone);
                for child in children {
                    if child.parent_id != armature.sel_bone(selections).unwrap().id {
                        continue;
                    }
                    let bones = &mut armature.bones;
                    let bone = bones.iter_mut().find(|b| b.id == child.id).unwrap();
                    bone.pos.y = 0.;
                }
            }
        }
        Events::SetPosDamping => {
            let bone = armature.sel_bone_mut(selections).unwrap();
            bone.phys_pos_damping = value;
        }
        Events::SetScaleDamping => {
            let bone = armature.sel_bone_mut(selections).unwrap();
            bone.phys_scale_damping = value;
        }
        Events::SetRotDamping => {
            let bone = armature.sel_bone_mut(selections).unwrap();
            bone.phys_rot_damping = value;
        }
        Events::SetRotBounce => {
            let bone = armature.sel_bone_mut(selections).unwrap();
            bone.phys_rot_bounce = (value).max(0.).min(1.);
        }
        Events::SetPosRatio => {
            let bone = armature.sel_bone_mut(selections).unwrap();
            bone.phys_pos_ratio = (value).max(-1.).min(1.);
        }
        Events::SetScaleRatio => {
            let bone = armature.sel_bone_mut(selections).unwrap();
            bone.phys_scale_ratio = (value).max(-1.).min(1.);
        }
        Events::SetHoveringVertId => selections.hovering_vert_id = value as i32,
        Events::SetHoveringTri => selections.hovering_tri_dur = value as i32,
        Events::SetHoveringBoneId => selections.hovering_bone_id = value as i32,
        Events::SetHoveringLine => selections.hovering_line_dur = value as i32,
        Events::ImportPsdArmature => {
            armature.bones = psd_armature.bones.clone();
            armature.styles = psd_armature.styles.clone();
            armature.tex_data = psd_armature.tex_data.clone();
            *psd_armature = Armature::default();
        }
        Events::CreateParentBone => {
            // either get the bone ID from event, or selected bones if more than 1
            let mut bones_to_copy = vec![armature.bones[value as usize].id];
            if selections.bone_ids.len() > 1 {
                bones_to_copy = selections.bone_ids.clone();
            }

            let (parent, _) = armature.new_bone(bones_to_copy[0]);

            // move new bone(s) above target
            drag_bone(armature, bones_to_copy[0], &vec![parent.id], true);

            // set targets' parent as new bone
            for bone_id in &bones_to_copy {
                let bone = armature.bones.iter().find(|b| b.id == *bone_id);
                if bone == None || bones_to_copy.contains(&bone.unwrap().parent_id) {
                    continue;
                }
                armature.find_bone_mut(*bone_id).unwrap().parent_id = parent.id;
            }

            // activate renaming for new bone
            armature.find_bone_mut(parent.id).unwrap().name = "".to_string();
            let bones = &armature.bones;
            let idx = bones.iter().position(|b| b.id == parent.id).unwrap();
            ui.rename_id = format!("bone_{}", idx);

            // select new parent
            selections.bone_ids = vec![parent.id];
            selections.bone_idx = bones.iter().position(|b| b.id == parent.id).unwrap();
        }
        Events::MoveSelectedKeyframes => {
            if value as i32 == ui.dragged_keyframe.frame
                || ui.dragged_keyframe.frame == -1
                || value as i32 == -1
            {
                ui.dragged_keyframe.frame = -1;
                return;
            }

            undo_states.new_undo_anim(&armature.sel_anim(&selections).unwrap());
            let sel_anim = &mut armature.sel_anim_mut(&selections).unwrap();

            // shift the selected keys by the drag; what they land on is replaced
            let delta = value as i32 - ui.dragged_keyframe.frame;
            let (keys, moved, _) = timeline_rows::retime(&sel_anim.keyframes, &ui.selected_keyframes, |f| f + delta);
            sel_anim.keyframes = keys;
            ui.selected_keyframes = moved;
            ui.dragged_keyframe.frame = -1;
        }
        Events::GlobalCopy => match ui.last_selected.as_str() {
            "keyframe" => copy_selected_keyframes(copy_buffer, ui),
            "bone" => copy_bone(copy_buffer, selections, armature, selections.bone_idx),
            _ => {}
        },
        Events::GlobalPaste => match ui.last_selected.as_str() {
            "keyframe" => {
                undo_states.new_undo_anim(armature.sel_anim(&selections).unwrap());
                paste_keyframes_on_frame(copy_buffer, armature, selections, value as i32);
            }
            "bone" => {
                undo_states.new_undo_bones(&armature.bones);
                paste_bone(copy_buffer, selections, armature, selections.bone_idx);
            }
            _ => {}
        },
        Events::ToggleEditingPivot => edit_mode.editing_pivot = !edit_mode.editing_pivot,
        _ => {}
    }
}

/// Leave Pose Mode: put every bone back to its rest transform, and drop the pose edits from
/// undo history (undoing them later would write the test pose into the rest pose).
pub fn exit_pose_mode(armature: &mut Armature, edit_mode: &mut EditMode, undo: &mut UndoStates) {
    if !edit_mode.pose_mode {
        return;
    }
    bind_pose::apply_pose(armature, &edit_mode.pose_snapshot);
    let len = edit_mode.pose_undo_len.min(undo.undo_actions.len());
    undo.undo_actions.truncate(len);
    undo.redo_actions.clear();
    undo.unsaved_undo_actions = undo.unsaved_undo_actions.min(len);
    undo.prev_undo_actions = undo.prev_undo_actions.min(len);
    edit_mode.pose_mode = false;
    edit_mode.pose_snapshot = vec![];
}

pub fn center_verts(verts: &mut Vec<Vertex>) {
    if verts.is_empty() {
        return;
    }
    let mut min = verts[0].pos;
    let mut max = verts[0].pos;
    for v in &mut *verts {
        if v.pos.x < min.x {
            min.x = v.pos.x;
        }
        if v.pos.y < min.y {
            min.y = v.pos.y
        }
        if v.pos.x > max.x {
            max.x = v.pos.x;
        }
        if v.pos.y > max.y {
            max.y = v.pos.y;
        }
    }

    let avg = (min + max) / 2.;
    for v in verts {
        v.pos -= avg;
    }
}

/// Give the selected bone an IK constraint, with a new target bone where its head is
/// now drawn, so nothing moves (docs/IK.md §4.1).
pub fn add_ik(armature: &mut Armature, selections: &SelectionState) {
    let Some(tip) = armature.sel_bone(selections).cloned() else { return };
    let depth = ik::depth(&armature.bones, tip.id);
    if depth == 0 || tip.ik.is_some() {
        return;
    }

    let mut world = armature.bones.clone();
    renderer::construction(&mut world, &armature.bones);
    let tip_world = world.iter().find(|b| b.id == tip.id).unwrap().pos;

    let target_id = armature.new_bone(-1).0.id;
    let target = armature.find_bone_mut(target_id).unwrap();
    target.name = format!("{} Target", tip.name);
    target.pos = tip_world;

    armature.find_bone_mut(tip.id).unwrap().ik = Some(IkConstraint {
        target_id,
        chain: depth.min(2),
        ..Default::default()
    });
}

/// Bones the IK target of `tip_id` can be parented to: not in or under its chain (that
/// would feed back), not the target or under it, and not Bind Pose helpers. Ancestors of
/// the chain come first, nearest first.
pub fn ik_target_spaces(armature: &Armature, tip_id: i32) -> Vec<i32> {
    let Some(c) = armature.bones.iter().find(|b| b.id == tip_id).and_then(|b| b.ik.clone()) else { return vec![] };
    let target = c.target_id;
    let first = ik::chain_ids(&armature.bones, tip_id, c.chain).first().copied().unwrap_or(tip_id);
    let ancestors: Vec<i32> = armature.get_all_parents(false, first).iter().map(|b| b.id).collect();
    let under_target = |id: i32| id == target || armature.get_all_parents(false, id).iter().any(|p| p.id == target);
    let ok = |b: &Bone| {
        b.bind_owner.is_none() && !under_target(b.id) && ik::target_allowed(&armature.bones, tip_id, c.chain, b.id)
    };
    let mut ids: Vec<i32> = ancestors.iter().copied().filter(|id| armature.bones.iter().any(|b| b.id == *id && ok(b))).collect();
    ids.extend(armature.bones.iter().filter(|b| ok(b) && !ancestors.contains(&b.id)).map(|b| b.id));
    ids
}

/// Parent the IK target of `tip_id` to `parent_id` (-1: world), keeping it where it is,
/// so it follows that bone from then on (docs/IK.md §4.1). Returns whether it changed.
pub fn set_ik_target_space(armature: &mut Armature, tip_id: i32, parent_id: i32) -> bool {
    let Some(target) = armature.bones.iter().find(|b| b.id == tip_id).and_then(|b| b.ik.as_ref()).map(|c| c.target_id) else {
        return false;
    };
    let Some(current) = armature.bones.iter().find(|b| b.id == target).map(|b| b.parent_id) else { return false };
    if current == parent_id || (parent_id != -1 && !ik_target_spaces(armature, tip_id).contains(&parent_id)) {
        return false;
    }
    if parent_id != -1 {
        drag_bone(armature, parent_id, &vec![target], false);
        return true;
    }

    // world: a root bone, at the end of the list
    let before = armature.fk_world().into_iter().find(|b| b.id == target).unwrap();
    let bone = armature.bones.iter().find(|b| b.id == target).unwrap().clone();
    let mut moving = vec![bone.clone()];
    get_all_children(&armature.bones, &mut moving, &bone);
    let ids: Vec<i32> = moving.iter().map(|b| b.id).collect();
    let mut moved: Vec<Bone> = ids.iter().map(|id| armature.bones.iter().find(|b| b.id == *id).unwrap().clone()).collect();
    armature.bones.retain(|b| !ids.contains(&b.id));
    moved[0].parent_id = -1;
    armature.bones.extend(moved);
    armature.keep_world(target, &before);
    true
}

/// The transforms Set Key records.
const KEYED: [AnimElement; 5] = [
    AnimElement::PositionX,
    AnimElement::PositionY,
    AnimElement::Rotation,
    AnimElement::ScaleX,
    AnimElement::ScaleY,
];

/// Set Key: key the position, rotation and scale of `bone_ids` at `frame` of animation
/// `anim`, at the values shown there (so an in-between pose can be held). As with
/// editing a bone, an element with no key on frame 0 gets one with its setup value.
/// Locked bones and Bind Pose helpers are skipped.
pub fn insert_keyframes(armature: &mut Armature, anim: usize, frame: i32, bone_ids: &[i32]) {
    if anim >= armature.animations.len() || frame < 0 {
        return;
    }
    let shown = armature.animate(anim, frame, None);
    let a = &armature.animations[anim];
    let layer = a.layers.get(a.active_layer).filter(|_| a.active_layer > 0).cloned();
    if layer.as_ref().is_some_and(|l| l.locked || l.link.is_some()) {
        return;
    }
    // a Looped layer keys in its own time
    let key_frame = layer.as_ref().map_or(frame, |l| layers::layer_frame(a, l, frame));
    for id in bone_ids {
        let Some(setup) = armature.bones.iter().find(|b| b.id == *id).cloned() else { continue };
        let Some(now) = shown.iter().find(|b| b.id == *id) else { continue };
        if setup.locked || setup.bind_owner.is_some() {
            continue;
        }
        let values = |b: &Bone| [b.pos.x, b.pos.y, b.rot, b.scale.x, b.scale.y];
        // through a layer, the key that shows this pose
        let shown_values: Vec<f32> = KEYED
            .iter()
            .zip(values(now))
            .map(|(el, v)| match &layer {
                Some(_) => {
                    let ctx = layers::Ctx { anims: &armature.animations, clock: None };
                    layers::key_on_active_in(&armature.animations[anim], &ctx, &armature.bones, *id, el, 0, frame, v)
                }
                None => v,
            })
            .collect();
        let a = &mut armature.animations[anim];
        let first_key = layer.as_ref().map_or(true, |l| l.mode != LayerMode::Override);
        for ((el, value), setup_value) in KEYED.iter().zip(shown_values).zip(values(&setup)) {
            let has_first = a.keyframes.iter().any(|k| k.bone_id == *id && k.element == *el && k.frame == 0);
            if key_frame != 0 && !has_first && first_key {
                let (_, i) = a.check_if_in_keyframe(*id, 0, el.clone());
                a.keyframes[i].value = setup_value;
            }
            let (_, i) = a.check_if_in_keyframe(*id, key_frame, el.clone());
            a.keyframes[i].value = value;
        }
    }
}

/// Breakdown: key `bone_ids` at `frame` between their previous and next keys, `amount`
/// (0..1) of the way from the previous pose to the next, per transform. A transform
/// keyed on only one side holds that key; one keyed on neither side is left alone.
pub fn breakdown_keyframes(armature: &mut Armature, anim: usize, frame: i32, bone_ids: &[i32], amount: f32) {
    let Some(a) = armature.animations.get(anim) else { return };
    let mut sets: Vec<(i32, AnimElement, f32)> = vec![];
    for id in bone_ids {
        let Some(bone) = armature.bones.iter().find(|b| b.id == *id) else { continue };
        if bone.locked || bone.bind_owner.is_some() {
            continue;
        }
        for el in KEYED {
            let keys = a.keyframes.iter().filter(|k| k.bone_id == *id && k.element == el);
            let prev = keys.clone().filter(|k| k.frame < frame).max_by_key(|k| k.frame);
            let next = keys.filter(|k| k.frame > frame).min_by_key(|k| k.frame);
            let value = match (prev, next) {
                (Some(p), Some(n)) => p.value + (n.value - p.value) * amount.clamp(0., 1.),
                (Some(p), None) => p.value,
                (None, Some(n)) => n.value,
                (None, None) => continue,
            };
            sets.push((*id, el.clone(), value));
        }
    }
    let a = &mut armature.animations[anim];
    for (id, el, value) in sets {
        let (_, i) = a.check_if_in_keyframe(id, frame, el);
        a.keyframes[i].value = value;
    }
}

/// The bones Key All keys: every bone without a texture (the skeleton, and controls
/// such as IK targets), apart from Bind Pose helpers.
pub fn rig_bones(armature: &Armature) -> Vec<i32> {
    armature.bones.iter().filter(|b| b.tex.is_empty() && b.bind_owner.is_none()).map(|b| b.id).collect()
}

/// Delete Key: remove every keyframe of `bone_ids` at `frame` of animation `anim`, or
/// every keyframe at `frame` when no bones are given.
pub fn delete_keyframes_at(armature: &mut Armature, anim: usize, frame: i32, bone_ids: &[i32]) {
    let Some(a) = armature.animations.get_mut(anim) else { return };
    a.keyframes.retain(|k| k.frame != frame || (!bone_ids.is_empty() && !bone_ids.contains(&k.bone_id)));
}

pub fn open_modal(ui: &mut crate::Ui, forced: bool, headline: String) {
    ui.modal = true;
    ui.forced_modal = forced;
    ui.headline = headline.replace("$err", &ui.custom_error);
}

fn select_bone(
    sel: &mut SelectionState,
    ui: &mut crate::Ui,
    armature: &mut Armature,
    edit_mode: &mut EditMode,
    input: &InputStates,
    idx: usize,
    from_renderer: bool,
) {
    edit_mode.showing_mesh = false;
    edit_mode.editing_mesh = false;
    if edit_mode.tool.edits_mesh() {
        edit_mode.tool = Tool::Transform;
    }
    edit_mode.sel_time = 0.;
    edit_mode.temporary = None;
    sel.vert_ids = vec![];
    ui.selected_bone_first_time = true;
    edit_mode.editing_pivot = false;
    if sel.camera.take().is_some() && ui.last_selected == "camera" {
        ui.last_selected = "".to_string();
    }

    if idx == usize::MAX {
        sel.bone_idx = usize::MAX;
        sel.bone_ids = vec![];
        sel.bind = -1;
        edit_mode.setting_ik_target = false;
        edit_mode.setting_bind_bone = false;
        return;
    }

    // rename bone if already selected and in right-side panel
    if sel.bone_idx == idx && !from_renderer && ui.last_selected == "bone" {
        ui.rename_id = "bone_".to_string() + &sel.bone_idx.to_string().clone();
        ui.edit_value = Some(armature.sel_bone(&sel).unwrap().name.clone());
        return;
    }

    ui.last_selected = "bone".to_string();

    // set this bone as IK target if in IK target mode, unless it would drive itself
    if edit_mode.setting_ik_target {
        edit_mode.setting_ik_target = false;
        let target_id = armature.bones[idx].id;
        let tip = armature.sel_bone(&sel).unwrap().clone();
        let Some(c) = &tip.ik else { return };
        if !ik::target_allowed(&armature.bones, tip.id, c.chain, target_id) {
            open_modal(ui, false, ui.loc("bone_panel.inverse_kinematics.target_in_chain"));
            return;
        }
        armature.sel_bone_mut(&sel).unwrap().ik.as_mut().unwrap().target_id = target_id;
        return;
    }

    // set this bone as bind if in bind mode
    if edit_mode.setting_bind_bone {
        let mesh_id = armature.sel_bone(&sel).map(|b| b.id).unwrap_or(-1);
        let id = bind_pose::bind_target(armature, armature.bones[idx].id, mesh_id);
        if let Some(bind) = armature
            .sel_bone_mut(&sel)
            .and_then(|bone| bone.binds.get_mut(sel.bind as usize))
        {
            bind.bone_id = id;
        }
        edit_mode.setting_bind_bone = false;
        return;
    }

    sel.bind = -1;
    let bone_id = armature.bones[idx].id;
    // scroll to this bone in keyframe editor
    // (`height` is the bone row's top within the rows; scroll only if it's out of view)
    if let Some(bone) = ui.bone_tops.tops.iter().find(|b| b.id == bone_id) {
        let row_h = 18.;
        if bone.height < ui.timeline_offset.y || bone.height + row_h > ui.timeline_offset.y + ui.timeline_rows_h {
            ui.timeline_offset.y = (bone.height - row_h).max(0.);
        }
    }

    // select only this bone if not holding modifiers
    if !input.holding_mod && !input.holding_shift {
        sel.bone_idx = idx;
        sel.bone_ids = vec![armature.bones[idx].id];
        edit_mode.showing_mesh = false;

        // unfold this bone's parents to reveal it in the hierarchy
        let parents = armature.get_all_parents(false, armature.sel_bone(sel).unwrap().id);
        let bones = &mut armature.bones;
        for p in parents {
            bones.iter_mut().find(|b| b.id == p.id).unwrap().folded = false;
        }
    } else {
        if sel.bone_idx == usize::MAX {
            sel.bone_idx = idx;
            sel.bone_ids = vec![armature.bones[idx].id];
        }
        if input.holding_mod {
            let id = armature.bones[idx as usize].id;
            sel.bone_ids.push(id);
        } else {
            let mut first = sel.bone_idx;
            let mut second = idx as usize;
            if first > second {
                first = idx as usize;
                second = sel.bone_idx;
            }
            for i in first..second as usize + 1 {
                let bone = &armature.bones[i];
                sel.bone_ids.push(bone.id);
            }
        }

        // sort bone IDs by their order in the hierarchy
        sel.bone_ids.sort_by(|a, b| {
            let a_pos = armature.bones.iter().position(|bo| bo.id == *a).unwrap();
            let b_pos = armature.bones.iter().position(|bo| bo.id == *b).unwrap();
            a_pos.cmp(&b_pos)
        });

        // remove any duplicate IDs
        sel.bone_ids.dedup();
    }
}

fn unselect_all(selections: &mut SelectionState, edit_mode: &mut EditMode, ui: &mut crate::Ui) {
    selections.bone_idx = usize::MAX;
    selections.bone_ids = vec![];
    selections.anim_frame = -1;
    selections.anim = usize::MAX;
    selections.bind = -1;
    selections.style_id = -1;
    selections.camera = None;
    edit_mode.showing_mesh = false;
    edit_mode.setting_ik_target = false;
    edit_mode.setting_bind_bone = false;
    ui.last_selected = "".to_string();
}

pub fn undo_redo(
    undo: bool,
    undo_states: &mut UndoStates,
    armature: &mut Armature,
    selections: &mut SelectionState,
) {
    let action: Action;
    if undo {
        if undo_states.undo_actions.last() == None {
            return;
        }
        action = undo_states.undo_actions.last().unwrap().clone();
    } else {
        if undo_states.redo_actions.last() == None {
            return;
        }
        action = undo_states.redo_actions.last().unwrap().clone();
    }

    // store the state prior to undoing/redoing the action,
    // to add to the opposite stack later
    let mut new_action = action.clone();

    match &action.action {
        ActionType::Bone => {
            let bone = armature.find_bone_mut(action.bones[0].id).unwrap();
            new_action.bones = vec![bone.clone()];
            *bone = action.bones[0].clone();
            // remove selected vertices that no longer exist
            let vert_ids: Vec<usize> = bone.vertices.iter().map(|v| v.id as usize).collect();
            selections.vert_ids.retain(|id| vert_ids.contains(id));
        }
        ActionType::Bones => {
            new_action.bones = armature.bones.clone();
            armature.bones = action.bones.clone();
            if selections.bone_ids.len() == 0 {
                selections.bone_idx = usize::MAX;
            } else {
                let sel_id = selections.bone_ids[0];
                let sel_idx = armature.bones.iter().position(|b| b.id == sel_id);
                if sel_idx != None {
                    selections.bone_idx = sel_idx.unwrap();
                } else {
                    selections.bone_idx = usize::MAX
                }
            }
        }
        ActionType::Animation => {
            let anim_id = action.animations[0].id;
            let animations = &mut armature.animations;
            let anim = animations.iter_mut().find(|a| a.id == anim_id).unwrap();
            new_action.animations = vec![anim.clone()];
            *anim = action.animations[0].clone();
        }
        ActionType::Animations => {
            new_action.animations = armature.animations.clone();
            armature.animations = action.animations.clone();
            let animations = &mut armature.animations;
            if animations.len() == 0 || selections.anim > animations.len() - 1 {
                selections.anim = usize::MAX;
            }
        }
        ActionType::Style => {
            let id = action.styles[0].id;
            let style = armature.styles.iter_mut().find(|a| a.id == id).unwrap();
            new_action.styles = vec![style.clone()];
            *style = action.styles[0].clone();
        }
        ActionType::Cameras => {
            new_action.cameras = armature.cameras.clone();
            armature.cameras = action.cameras.clone();
            if !armature.cameras.iter().any(|c| Some(c.id) == selections.camera) {
                selections.camera = None;
            }
        }
        ActionType::Styles => {
            new_action.styles = armature.styles.clone();
            armature.styles = action.styles.clone();
            let style_ids: Vec<i32> = armature.styles.iter().map(|s| s.id).collect();
            if !style_ids.contains(&selections.style_id) {
                selections.style_id = -1;
            }
        }
        _ => {}
    }

    // add action(s) to opposing stack
    undo_states.temp_actions.push(new_action);
    if undo {
        undo_states.undo_actions.pop();
        if !action.continued {
            // reverse list to restore order of actions
            undo_states.temp_actions.reverse();
            let temp_actions = &mut undo_states.temp_actions;
            undo_states.redo_actions.append(temp_actions);
            undo_states.temp_actions = vec![];
        }
    } else {
        undo_states.redo_actions.pop();
        if !action.continued {
            // ditto
            undo_states.temp_actions.reverse();
            let temp_actions = &mut undo_states.temp_actions;
            undo_states.undo_actions.append(temp_actions);
            undo_states.temp_actions = vec![];
        }
    }

    undo_states.prev_undo_actions = undo_states.undo_actions.len();
    undo_states.unsaved_undo_actions = undo_states.undo_actions.len();

    // actions tagged with `continue` are part of an action chain
    if action.continued {
        undo_redo(undo, undo_states, armature, selections);
    }
}

pub fn move_bone(bones: &mut Vec<Bone>, old_idx: i32, new_idx: i32, is_setting_parent: bool) {
    let main = &bones[old_idx as usize];
    let anchor = bones[new_idx as usize].clone();

    // gather all bones to be moved (this and its children)
    let mut to_move: Vec<Bone> = vec![main.clone()];
    armature_window::get_all_children(bones, &mut to_move, main);

    // remove them
    for _ in &to_move {
        bones.remove(old_idx as usize);
    }

    // re-add them in the new positions
    if is_setting_parent {
        to_move.reverse();
    }
    for bone in to_move {
        let idx = bones.iter().position(|b| b.id == anchor.id).unwrap();
        bones.insert(idx + is_setting_parent as usize, bone.clone());
    }
}

pub fn drag_bone(
    armature: &mut Armature,
    pointing_id: i32,
    bone_ids: &Vec<i32>,
    is_above: bool,
) -> usize {
    if bone_ids.contains(&pointing_id) {
        return usize::MAX;
    }

    // ignore if pointing bone is a child of this
    if bone_ids.len() != 0 {
        let mut children: Vec<Bone> = vec![];
        let id = bone_ids[0];
        let dragged_bone = armature.bones.iter().find(|b| b.id == id).unwrap();
        let db = dragged_bone;
        armature_window::get_all_children(&armature.bones, &mut children, &db);
        let children_ids: Vec<i32> = children.iter().map(|c| c.id).collect();
        if children_ids.contains(&pointing_id) {
            return usize::MAX;
        }
    }

    let mut sorted_ids = bone_ids.clone();
    sorted_ids.sort_by(|a, b| {
        let mut first = *b;
        let mut second = *a;
        if is_above {
            first = *a;
            second = *b;
        }
        let first_idx = armature.bones.iter().position(|b| b.id == first);
        let second_idx = armature.bones.iter().position(|b| b.id == second);
        first_idx.unwrap().cmp(&second_idx.unwrap())
    });

    for id in sorted_ids {
        let before = armature.fk_world().into_iter().find(|b| b.id == id).unwrap();

        #[rustfmt::skip] macro_rules! dragged {()=>{armature.find_bone_mut(id).unwrap()}}
        #[rustfmt::skip] macro_rules! pointing{()=>{armature.find_bone_mut(pointing_id).unwrap()}}
        #[rustfmt::skip] macro_rules! bones   {()=>{&mut armature.bones}}

        #[rustfmt::skip] let drag_idx = bones!().iter().position(|b| b.id == id).unwrap() as i32;
        #[rustfmt::skip] let point_idx = bones!().iter().position(|b| b.id == pointing_id).unwrap() as i32;

        if is_above {
            // set pointed bone's parent as dragged bone's parent
            dragged!().parent_id = pointing!().parent_id;
            move_bone(bones!(), drag_idx, point_idx, false);
        } else {
            // set pointed bone as dragged bone's parent
            dragged!().parent_id = pointing!().id;
            move_bone(bones!(), drag_idx, point_idx, true);
            pointing!().folded = false;
        }

        // adjust dragged bone so it stays in place
        armature.keep_world(id, &before);
    }

    let bones = &armature.bones;
    bones.iter().position(|b| b.id == bone_ids[0]).unwrap()
}

fn edit_bone(
    armature: &mut Armature,
    config: &Config,
    bone_id: i32,
    element: AnimElement,
    value: f32,
    value_str: String,
    mut anim_id: usize,
    mut anim_frame: i32,
) {
    let bones = &mut armature.bones;
    let bone = bones.iter_mut().find(|b| b.id == bone_id).unwrap();

    let mut init_value = 0.;
    let mut init_value_str = "".to_string();

    // prevent recording into animation if bone is locked, or for setup-only fields
    if bone.locked || element == AnimElement::Length {
        anim_id = usize::MAX;
        anim_frame = -1;
    }

    // do nothing if anim is playing and 'edit while playing' config is false
    let anims = &armature.animations;
    let is_any_anim_playing = anims.iter().find(|anim| anim.elapsed != None) != None;
    if !config.edit_while_playing && is_any_anim_playing {
        return;
    }

    macro_rules! set {
        ($field:expr, $field_type:ident) => {{
            init_value = $field as f32;
            if anim_frame == -1 {
                $field = value as $field_type;
            }
        }};
    }
    macro_rules! set_str {
        ($field:expr, $enum:ident) => {{
            init_value_str = $field.to_string();
            if anim_frame == -1 {
                $field = $enum::from_str(&value_str).unwrap()
            }
        }};
    }
    macro_rules! set_bool {
        ($field:expr) => {{
            init_value = shared::bool_as_f32($field);
            if anim_frame == -1 {
                $field = shared::f32_as_bool(value)
            }
        }};
    }

    match element {
        AnimElement::PositionX => set!(bone.pos.x, f32),
        AnimElement::PositionY => set!(bone.pos.y, f32),
        AnimElement::Rotation => set!(bone.rot, f32),
        AnimElement::ScaleX => set!(bone.scale.x, f32),
        AnimElement::ScaleY => set!(bone.scale.y, f32),
        AnimElement::Zindex => set!(bone.zindex, i32),
        AnimElement::TintR => set!(bone.tint.r, f32),
        AnimElement::TintG => set!(bone.tint.g, f32),
        AnimElement::TintB => set!(bone.tint.b, f32),
        AnimElement::TintA => set!(bone.tint.a, f32),
        AnimElement::Texture => { /* handled in set_bone_tex() */ }
        AnimElement::Hidden => set_bool!(bone.hidden),
        AnimElement::Locked => set_bool!(bone.locked),
        AnimElement::GroupColorR => set!(bone.group_color.r, u8),
        AnimElement::GroupColorG => set!(bone.group_color.g, u8),
        AnimElement::GroupColorB => set!(bone.group_color.b, u8),
        AnimElement::GroupColorA => set!(bone.group_color.a, u8),
        AnimElement::IkBend => {
            if let Some(ik) = &mut bone.ik {
                set_str!(ik.bend, IkBend)
            }
        }
        AnimElement::IkWeight => {
            if let Some(ik) = &mut bone.ik {
                set!(ik.weight, f32)
            }
        }
        AnimElement::IkCopyRotation => {
            if let Some(ik) = &mut bone.ik {
                set_bool!(ik.copy_rotation)
            }
        }
        AnimElement::PivotX => set!(bone.pivot_pos.x, f32),
        AnimElement::PivotY => set!(bone.pivot_pos.y, f32),
        AnimElement::PivotRot => set!(bone.pivot_rot, f32),
        AnimElement::PivotScaleX => set!(bone.pivot_scale.x, f32),
        AnimElement::PivotScaleY => set!(bone.pivot_scale.y, f32),
        // shape weights are keyed through `shapes::edit_weight` (EditShapeWeight)
        AnimElement::ShapeWeight => return,
        AnimElement::Length => {
            init_value = bone.length;
            bone.length = value.max(0.);
        }
    };

    if anim_frame == -1 {
        return;
    }

    // on a layer above Base: nothing if it's locked, and the key is what the layer
    // needs for the pose to show `value` (docs/TIMELINE.md §5.3)
    let (value, layer_mode) = {
        let anim = &armature.animations[anim_id];
        match anim.layers.get(anim.active_layer).filter(|_| anim.active_layer > 0) {
            Some(layer) if layer.locked || layer.link.is_some() => return,
            Some(layer) => {
                let ctx = layers::Ctx { anims: &armature.animations, clock: None };
                let v = layers::key_on_active_in(anim, &ctx, &armature.bones, bone_id, &element, 0, anim_frame, value);
                (v, Some(layer.mode.clone()))
            }
            None => (value, None),
        }
    };
    // a Looped layer keys in its own time (docs/TIMELINE.md §5.7)
    let anim_frame = {
        let anim = &armature.animations[anim_id];
        anim.layers.get(anim.active_layer).filter(|_| anim.active_layer > 0).map_or(anim_frame, |l| layers::layer_frame(anim, l, anim_frame))
    };

    macro_rules! check_kf {
        ($kf:expr) => {
            $kf.frame == 0 && $kf.element == element && $kf.bone_id == bone_id
        };
    }

    let anim = &mut armature.animations;

    // a first key with the setup value (an override layer gets none: it would hold
    // the setup pose over Base)
    let has_0th = anim[anim_id].keyframes.iter().find(|kf| check_kf!(kf)) != None;
    if anim_frame != 0 && !has_0th && layer_mode != Some(LayerMode::Override) {
        anim[anim_id].check_if_in_keyframe(bone_id, 0, element.clone());
        let mut oth_frame = anim[anim_id].keyframes.iter_mut().find(|kf| check_kf!(kf));
        oth_frame.as_mut().unwrap().value = init_value;
        oth_frame.as_mut().unwrap().value_str = init_value_str;
    }
    let frame = anim[anim_id]
        .check_if_in_keyframe(bone_id, anim_frame, element.clone())
        .1;
    anim[anim_id].keyframes[frame].value = value;
    anim[anim_id].keyframes[frame].value_str = value_str;
}

fn clear_pen(renderer: &mut crate::Renderer) {
    renderer.pen_cuts.clear();
    renderer.pen_open = false;
    renderer.pen_rev += 1;
}

fn set_transform_tool(edit_mode: &mut EditMode, mode: EditModes) {
    edit_mode.current = mode;
    edit_mode.tool = Tool::Transform;
}

// remove vertices that are not in any triangle
pub fn cleanup_vertices(bone: &mut Bone) {
    for v in (0..bone.vertices.len()).rev() {
        let vert = bone.vertices[v];
        if bone.indices.contains(&(v as u32)) {
            continue;
        }
        bone.vertices.remove(v);

        // removed vertex causes an offset by +1 for indices higher than itself,
        // so adjust indices to correct this
        for idx in &mut bone.indices {
            *idx -= if *idx >= v as u32 { 1 } else { 0 };
        }

        // and from shapes
        for shape in &mut bone.shapes {
            shape.deltas.retain(|d| d.vert != vert.id);
        }

        // remove this vertex from binds
        for bind in &mut bone.binds {
            let vert_id = vert.id as i32;
            let idx = bind.verts.iter().position(|v| v.id == vert_id);
            if idx != None {
                bind.verts.remove(idx.unwrap());
            }
        }
    }
}

/// sort vertices in cw (or ccw?) order
pub fn sort_vertices(mut verts: Vec<Vertex>) -> Vec<Vertex> {
    let mut center = Vec2::default();
    for v in 0..verts.len() {
        center += verts[v].pos;
    }
    center /= verts.len() as f32;

    verts.sort_by(|a, b| {
        let angle_a = (a.pos.y - center.y).atan2(a.pos.x - center.x);
        let angle_b = (b.pos.y - center.y).atan2(b.pos.x - center.x);
        angle_a.partial_cmp(&angle_b).unwrap()
    });

    verts
}

pub fn triangulate(verts: &Vec<Vertex>, tex: &image::DynamicImage) -> Vec<u32> {
    let mut triangulation: spade::DelaunayTriangulation<_> = spade::DelaunayTriangulation::new();
    let size = Vec2::new(tex.width() as f32, tex.height() as f32);

    for vert in verts {
        let _ = triangulation.insert(spade::Point2::new(vert.uv.x, vert.uv.y));
    }

    let mut indices: Vec<u32> = Vec::new();
    for face in triangulation.inner_faces() {
        let tri_indices = face.vertices().map(|v| v.index()).to_vec();
        if tri_indices.len() != 3 {
            continue;
        }

        // check if this triangle is part of the texture, and ignore if not
        let v1 = verts[tri_indices[0]];
        let v2 = verts[tri_indices[1]];
        let v3 = verts[tri_indices[2]];
        let blt = Vec2::new(
            v1.uv.x.min(v2.uv.x).min(v3.uv.x),
            v1.uv.y.min(v2.uv.y).min(v3.uv.y),
        ) * size;
        let brb = Vec2::new(
            v1.uv.x.max(v2.uv.x).max(v3.uv.x),
            v1.uv.y.max(v2.uv.y).max(v3.uv.y),
        ) * size;
        'pixel_check: for x in (blt.x as i32)..(brb.x as i32) {
            for y in (blt.y as i32)..(brb.y as i32) {
                let pos = &Vec2::new(x as f32, y as f32);
                let bary = tri_point(pos, &(v1.uv * size), &(v2.uv * size), &(v3.uv * size));
                let uv = v1.uv * bary.3 + v2.uv * bary.1 + v3.uv * bary.2;
                let pos = Vec2::new(
                    (uv.x * tex.width() as f32).min(tex.width() as f32 - 1.),
                    (uv.y * tex.height() as f32).min(tex.height() as f32 - 1.),
                );
                let pixel_alpha =
                    image::GenericImageView::get_pixel(tex, pos.x as u32, pos.y as u32).0[3];
                if pixel_alpha > 125 {
                    indices.push(tri_indices[0] as u32);
                    indices.push(tri_indices[1] as u32);
                    indices.push(tri_indices[2] as u32);
                    break 'pixel_check;
                }
            }
        }
    }

    indices
}

fn tri_point(p: &Vec2, a: &Vec2, b: &Vec2, c: &Vec2) -> (f32, f32, f32, f32) {
    let s = a.y * c.x - a.x * c.y + (c.y - a.y) * p.x + (a.x - c.x) * p.y;
    let t = a.x * b.y - a.y * b.x + (a.y - b.y) * p.x + (b.x - a.x) * p.y;

    if (s < 0.0) != (t < 0.0) && s != 0.0 && t != 0.0 {
        return (-1., -1., -1., -1.);
    }

    let area = -b.y * c.x + a.y * (c.x - b.x) + a.x * (b.y - c.y) + b.x * c.y;
    if area == 0.0 {
        return (-1., -1., -1., -1.);
    }

    let s_normalized = s / area;
    let t_normalized = t / area;

    if s_normalized >= 0.0 && t_normalized >= 0.0 && (s_normalized + t_normalized) <= 1.0 {
        let third = 1. - (s_normalized + t_normalized);
        return (area, s_normalized, t_normalized, third);
    }

    (-1., -1., -1., -1.)
}

pub fn copy_bone(
    copy_buffer: &mut CopyBuffer,
    selections: &mut SelectionState,
    armature: &mut Armature,
    bone_idx: usize,
) {
    copy_buffer.bones = vec![];

    // ignore copying if there isn't a target bone
    if bone_idx == usize::MAX {
        return;
    }

    // either get the bone ID from event, or selected bones if more than 1
    let mut bones_to_copy = vec![armature.bones[bone_idx].id];
    if selections.bone_ids.len() > 1 {
        bones_to_copy = selections.bone_ids.clone();
    }

    // add appropriate bones to copy buffer
    for bone_id in &bones_to_copy {
        let bone = armature.bones.iter().find(|b| b.id == *bone_id);
        let not_root = bones_to_copy.contains(&bone.unwrap().parent_id);
        if bone == None || not_root {
            continue;
        }
        let mut bones = vec![];
        armature_window::get_all_children(&armature.bones, &mut bones, bone.unwrap());
        bones.insert(0, bone.unwrap().clone());
        copy_buffer.bones.extend_from_slice(&bones);
    }
}

fn paste_bone(
    copy_buffer: &mut CopyBuffer,
    selections: &mut SelectionState,
    armature: &mut Armature,
    pasted_idx: usize,
) {
    // determine which id to give the new bone(s), based on the highest current id
    let ids: Vec<i32> = armature.bones.iter().map(|bone| bone.id).collect();
    let mut highest_id = 0;
    for id in ids {
        highest_id = id.max(highest_id);
    }
    highest_id += 1;

    let mut insert_idx = usize::MAX;
    let mut id_refs: HashMap<i32, i32> = HashMap::new();

    for b in 0..copy_buffer.bones.len() {
        let bone = &mut copy_buffer.bones[b];

        highest_id += 1;
        let new_id = highest_id;

        id_refs.insert(bone.id, new_id);
        bone.id = highest_id;

        // selected bone's parent is also pasted bone's
        if bone.parent_id != -1 && id_refs.get(&bone.parent_id) != None {
            bone.parent_id = *id_refs.get(&bone.parent_id).unwrap();
        } else if pasted_idx != usize::MAX {
            insert_idx = pasted_idx;
            bone.parent_id = armature.bones[pasted_idx].parent_id;
        } else {
            bone.parent_id = -1;
        }
    }

    // re-set binds and IK targets that are pointing to pasted bones
    for b in 0..copy_buffer.bones.len() {
        let bone = &mut copy_buffer.bones[b];
        for bind in &mut bone.binds {
            if let Some(new_id) = id_refs.get(&bind.bone_id) {
                bind.bone_id = *new_id;
            }
        }
        if let Some(ik) = &mut bone.ik {
            if let Some(new_id) = id_refs.get(&ik.target_id) {
                ik.target_id = *new_id;
            }
        }
    }

    // insert pasted bones on proper position of the bone array
    if insert_idx == usize::MAX {
        armature.bones.extend_from_slice(&copy_buffer.bones);
    } else {
        for bone in &copy_buffer.bones {
            armature.bones.insert(insert_idx, bone.clone());
            insert_idx += 1;
        }
    }

    // select pasted bones
    selections.bone_ids = id_refs.into_values().collect();
    let sel_id = selections.bone_ids[0];
    selections.bone_idx = armature.bones.iter().position(|b| b.id == sel_id).unwrap();
}

fn copy_selected_keyframes(copy_buffer: &mut CopyBuffer, ui: &mut crate::Ui) {
    *copy_buffer = CopyBuffer::default();
    copy_buffer.keyframes = ui.selected_keyframes.clone();
}

fn paste_keyframes_on_frame(
    copy_buffer: &mut CopyBuffer,
    armature: &mut Armature,
    selections: &mut SelectionState,
    frame: i32,
) {
    if copy_buffer.keyframes.len() == 0 {
        return;
    }

    copy_buffer.keyframes.sort_by(|a, b| a.frame.cmp(&b.frame));
    let base_frame = copy_buffer.keyframes[0].frame;

    let mut buffer_frames = copy_buffer.keyframes.clone();
    let anim = &mut armature.sel_anim_mut(&selections).unwrap();

    // set copy buffer to new frames, for the retain() later
    for kf in &mut buffer_frames {
        let diff = kf.frame - base_frame;
        kf.frame = frame + diff;
    }

    // remove identical keyframes in the new frame
    anim.keyframes.retain(|kf| {
        buffer_frames
            .iter()
            .find(|bkf| bkf.frame == kf.frame && bkf.element == kf.element)
            == None
    });

    let base_frame = buffer_frames[0].frame;
    for k in 0..buffer_frames.len() {
        let keyframe = buffer_frames[k].clone();
        let diff = keyframe.frame - base_frame;
        anim.keyframes.push(Keyframe {
            frame: frame + diff,
            ..keyframe
        })
    }

    armature.sel_anim_mut(&selections).unwrap().sort_keyframes();
}
