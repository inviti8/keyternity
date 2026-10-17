//! The standard face and the face rig (docs/LIP_SYNC.md §6.4–6.5).

use keyternity_lib::editor;
use keyternity_lib::face;
use keyternity_lib::renderer;
use keyternity_lib::shared::*;
use keyternity_lib::utils;

fn vertex(id: u32, x: f32, y: f32) -> Vertex {
    Vertex { id, pos: Vec2::new(x, y), uv: Vec2::new(0.5, 0.5), init_pos: Vec2::new(x, y), ..Default::default() }
}

/// a head bone with a 140×220 face mesh under it
fn head() -> Armature {
    let head = Bone { id: 0, parent_id: -1, name: "head".into(), scale: Vec2::new(1., 1.), pivot_scale: Vec2::new(1., 1.), ..Default::default() };
    let face = Bone {
        id: 1,
        parent_id: 0,
        name: "face".into(),
        tex: "face".into(),
        scale: Vec2::new(1., 1.),
        pivot_scale: Vec2::new(1., 1.),
        vertices: vec![vertex(0, -70., -110.), vertex(1, 70., -110.), vertex(2, 70., 110.), vertex(3, -70., 110.)],
        indices: vec![0, 1, 2, 0, 2, 3],
        verts_edited: true,
        ..Default::default()
    };
    Armature { bones: vec![head, face], ..Default::default() }
}

fn run(s: &mut Shared) {
    while !s.events.events.is_empty() {
        #[rustfmt::skip]
        editor::iterate_events(
            &s.input, &mut s.config, &mut s.events, &mut s.camera, &mut s.edit_mode, &mut s.selections,
            &mut s.undo_states, &mut s.armature, &mut s.psd_armature, &mut s.copy_buffer, &mut s.ui, &mut s.renderer,
        );
    }
}

#[test]
fn names_match_their_standard_shape() {
    assert_eq!(face::canonical("Blink_Left"), Some("Blink_L"));
    assert_eq!(face::canonical("blink_l"), Some("Blink_L"));
    assert_eq!(face::canonical("JawRotateZ_Right"), Some("JawSide_R"));
    assert_eq!(face::canonical("Jaw_RotateY_Left"), Some("JawTwist_L"));
    assert_eq!(face::canonical("JawForeward"), Some("JawForward"));
    assert_eq!(face::canonical("MouthWhistle_NarrowAdjust_Left"), Some("MouthWhistle_L"));
    assert_eq!(face::canonical("Midmouth_Right"), Some("MidMouth_R"));
    assert_eq!(face::canonical("mbp"), Some("MBP"));
    assert_eq!(face::canonical("Lip"), None);
}

#[test]
fn adding_the_standard_shapes() {
    let mut face_bone = head().bones[1].clone();
    face_bone.shapes = vec![MeshShape { id: 1, name: "Smile_Left".into(), ..Default::default() }];
    let added = face::add_standard_shapes(&mut face_bone, true);
    assert_eq!(added, face::STANDARD.len() - 1 + 10, "all but the existing smile, plus the visemes");
    assert_eq!(face::find_shape(&face_bone, "Smile_L"), Some(1), "the existing one is kept");
    assert!(face_bone.shapes.iter().any(|s| s.name == "MouthOpen" && s.group == "mouth"));
    assert_eq!(face::add_standard_shapes(&mut face_bone, true), 0, "nothing twice");
}

fn rigged() -> (Armature, Vec<i32>) {
    let mut arm = head();
    face::add_standard_shapes(&mut arm.bones[1], false);
    let ids = face::set_up(&mut arm, 1, "Face");
    (arm, ids)
}

fn control<'a>(arm: &'a mut Armature, name: &str) -> &'a mut Bone {
    arm.bones.iter_mut().find(|b| b.face_control.as_ref().is_some_and(|c| c.name == name)).unwrap()
}

fn weight(bones: &[Bone], name: &str) -> f32 {
    let mesh = bones.iter().find(|b| b.id == 1).unwrap();
    let id = face::find_shape(mesh, name).unwrap();
    mesh.shapes.iter().find(|s| s.id == id).unwrap().weight
}

fn constructed(arm: &Armature) -> Vec<Bone> {
    let mut out = vec![];
    renderer::construction(&mut out, &arm.bones);
    out
}

#[test]
fn setting_up_a_face_lays_out_the_controls() {
    let (arm, ids) = rigged();
    assert_eq!(ids.len(), 1 + face::LAYOUT.len(), "a root and 13 controls");
    let root = arm.bones.iter().find(|b| b.id == ids[0]).unwrap();
    assert_eq!(root.parent_id, 0, "under the head, so it follows it");
    let by_name = |n: &str| arm.bones.iter().find(|b| b.face_control.as_ref().is_some_and(|c| c.name == n)).unwrap();
    assert_eq!(by_name("Corner_L").parent_id, by_name("Mouth").id);
    assert_eq!(by_name("LowerLip").parent_id, by_name("Jaw").id);
    assert!(by_name("Brow_L").pos.x > 0. && by_name("Brow_R").pos.x < 0., "character left is screen right");
    // mapped to the mesh's standard shapes
    let smile = &by_name("Corner_L").face_control.as_ref().unwrap().slots[0];
    assert_eq!(smile.axis, ControlAxis::Up);
    assert_eq!(smile.targets.len(), 1);
}

#[test]
fn moving_a_control_drives_its_shapes() {
    let (mut arm, _) = rigged();
    assert_eq!(weight(&constructed(&arm), "Smile_L"), 0., "at rest, nothing");
    let c = control(&mut arm, "Corner_L");
    let range = c.face_control.as_ref().unwrap().range;
    c.pos.y += range * 0.5;
    let posed = constructed(&arm);
    assert!((weight(&posed, "Smile_L") - 0.5).abs() < 1e-4);
    assert_eq!(weight(&posed, "Frown_L"), 0., "opposite slots never mix");

    // the upper lip drives both sides; twisting the lower lip rolls it
    control(&mut arm, "UpperLip").pos.y += range;
    control(&mut arm, "LowerLip").rot = -std::f32::consts::FRAC_PI_4;
    let posed = constructed(&arm);
    assert_eq!((weight(&posed, "UpperLipUp_L"), weight(&posed, "UpperLipUp_R")), (1., 1.));
    assert!((weight(&posed, "LowerLipIn") - 1.).abs() < 1e-4);
}

#[test]
fn moving_the_mouth_carries_its_controls_without_driving_them() {
    let (mut arm, _) = rigged();
    let range = control(&mut arm, "Mouth").face_control.as_ref().unwrap().range;
    control(&mut arm, "Mouth").pos.y -= range;
    let posed = constructed(&arm);
    assert!((weight(&posed, "MouthDown") - 1.).abs() < 1e-4);
    assert_eq!(weight(&posed, "Smile_L") + weight(&posed, "Frown_L") + weight(&posed, "MouthOpen"), 0.);
}

#[test]
fn driven_weights_add_to_keyed_ones_and_cap() {
    let (mut arm, _) = rigged();
    let mesh = arm.bones.iter_mut().find(|b| b.id == 1).unwrap();
    let id = face::find_shape(mesh, "MouthOpen").unwrap();
    mesh.shapes.iter_mut().find(|s| s.id == id).unwrap().weight = 1.2;
    let range = control(&mut arm, "Jaw").face_control.as_ref().unwrap().range;
    control(&mut arm, "Jaw").pos.y -= range;
    assert_eq!(weight(&constructed(&arm), "MouthOpen"), 1.5);
}

#[test]
fn set_up_face_through_events_and_undo() {
    let mut s = Shared::default();
    s.armature = head();
    s.events.add_standard_shapes(1, false);
    s.events.set_up_face(1, "Face");
    run(&mut s);
    assert_eq!(s.armature.bones.len(), 2 + 14);
    s.events.undo();
    run(&mut s);
    assert_eq!(s.armature.bones.len(), 2);
}

#[test]
fn controls_save_their_targets_by_bone_index() {
    let (mut arm, _) = rigged();
    // ids that aren't indices: give the mesh a high id
    let old = 1;
    let new = 500;
    for b in &mut arm.bones {
        if b.id == old {
            b.id = new;
        }
        if b.parent_id == old {
            b.parent_id = new;
        }
        if let Some(fc) = &mut b.face_control {
            for slot in &mut fc.slots {
                for t in &mut slot.targets {
                    t.mesh = new;
                }
            }
        }
    }
    let (json, _) = utils::prepare_files(&arm, Camera::default(), vec![], &EditMode::default(), None);
    let root: serde_json::Value = serde_json::from_str(&json).unwrap();
    let bones = root["bones"].as_array().unwrap();
    let control = bones.iter().find(|b| b["face_control"]["name"] == "Corner_L").unwrap();
    assert_eq!(control["face_control"]["slots"][0]["targets"][0]["mesh"], 1, "the face mesh's index");
}

#[test]
fn exports_bake_the_rig_into_shape_keys() {
    let (mut arm, ids) = rigged();
    let jaw = arm.bones.iter().find(|b| b.face_control.as_ref().is_some_and(|c| c.name == "Jaw")).unwrap().clone();
    let range = jaw.face_control.as_ref().unwrap().range;
    let key = |frame, value| Keyframe { frame, bone_id: jaw.id, element: AnimElement::PositionY, value, start_handle: utils::interp_preset(HandlePreset::Linear).0, end_handle: utils::interp_preset(HandlePreset::Linear).1, ..Default::default() };
    arm.animations = vec![Animation { id: 0, name: "talk".into(), fps: 10, keyframes: vec![key(0, jaw.pos.y), key(10, jaw.pos.y - range)], ..Default::default() }];
    let shown: Vec<f32> = (0..=10).map(|f| { let mut a = arm.clone(); let p = a.animate(0, f, None); let mut c = vec![]; renderer::construction(&mut c, &p); weight(&c, "MouthOpen") }).collect();
    assert!((shown[10] - 1.).abs() < 1e-4);

    let mut out = arm.clone();
    face::bake_for_export(&mut out);
    assert!(out.bones.iter().all(|b| !ids.contains(&b.id)), "the rig's bones are gone");
    let open = face::find_shape(&out.bones[1], "MouthOpen").unwrap();
    assert!(out.animations[0].keyframes.iter().all(|k| k.element == AnimElement::ShapeWeight && k.shape == open));
    for f in 0..=10 {
        let p = out.animate(0, f, None);
        assert!((weight(&p, "MouthOpen") - shown[f as usize]).abs() < 1e-4, "frame {f}");
    }
}

#[test]
fn the_panel_art_matches_the_rig() {
    let art = keyternity_lib::face_panel::art();
    assert_eq!(art.controls.len(), face::LAYOUT.len());
    for c in &art.controls {
        assert!(face::LAYOUT.iter().any(|l| l.name == c.name), "{} has no control", c.name);
        assert!(!c.tris.is_empty());
    }
    assert!(!art.mask.is_empty());
}

#[test]
fn the_panel_places_controls_with_their_parents() {
    let (mut arm, ids) = rigged();
    let range = control(&mut arm, "Mouth").face_control.as_ref().unwrap().range;
    control(&mut arm, "Mouth").pos.y -= range;
    control(&mut arm, "Corner_L").pos.y += range * 0.5;
    let corner = arm.bones.iter().find(|b| b.face_control.as_ref().is_some_and(|c| c.name == "Corner_L")).unwrap().id;
    let posed = arm.bones.clone();
    let (off, twist) = keyternity_lib::face_panel::placement(&arm, &posed, corner);
    // in panel units (the rig's RANGE per full travel): mouth down 1 range, corner up half
    assert!((off.y - (-face::RANGE + face::RANGE * 0.5)).abs() < 1e-3, "{:?}", off);
    assert_eq!(twist, 0.);
    assert!(keyternity_lib::face_panel::rigs(&arm).iter().any(|(r, cs)| *r == ids[0] && cs.len() == 13));
}

#[test]
fn a_rig_set_up_before_its_shapes_maps_them_when_they_arrive() {
    let mut s = Shared::default();
    s.armature = head();
    s.events.set_up_face(1, "Face");
    run(&mut s);
    let unmapped = |s: &Shared| {
        s.armature.bones.iter().filter_map(|b| b.face_control.as_ref()).all(|fc| fc.slots.iter().all(|sl| sl.targets.is_empty()))
    };
    assert!(unmapped(&s), "no shapes yet: nothing to drive");

    s.events.add_standard_shapes(1, false);
    run(&mut s);
    assert!(!unmapped(&s), "adding the shapes fills the slots");
    let smile = |s: &Shared| {
        let c = s.armature.bones.iter().find(|b| b.face_control.as_ref().is_some_and(|c| c.name == "Corner_L")).unwrap();
        c.face_control.as_ref().unwrap().slots.iter().find(|sl| sl.axis == ControlAxis::Up).unwrap().targets.len()
    };
    assert_eq!(smile(&s), 1);

    // a rig from before controls knew their mesh: found by sitting under the mesh's parent
    for b in &mut s.armature.bones {
        if let Some(fc) = &mut b.face_control {
            fc.mesh = None;
            for sl in &mut fc.slots {
                sl.targets.clear();
            }
        }
    }
    assert_eq!(face::automap(&mut s.armature, 1) > 0, true);
    assert_eq!(smile(&s), 1);
}

#[test]
fn the_panel_face_deforms_with_its_controls() {
    use keyternity_lib::face_panel;
    let art = face_panel::art();
    let face = art.face.as_ref().expect("the skinned face");
    let jaw_rest = art.controls.iter().find(|c| c.name == "Jaw").unwrap().rest;
    let mut xf = std::collections::HashMap::new();
    xf.insert("Jaw".to_string(), (egui::vec2(0., -2.), 0., jaw_rest));
    let moved = face_panel::deform(face, &xf);
    for (i, ws) in face.weights.iter().enumerate() {
        let jaw: f32 = ws.iter().filter(|(n, _)| n == "Jaw").map(|(_, w)| w).sum();
        let total: f32 = ws.iter().map(|(_, w)| w).sum();
        let expect = -2. * jaw / total.max(1.);
        assert!((moved[i][1] - face.verts[i][1] - expect).abs() < 1e-3, "vertex {i}");
        if jaw == 0. {
            let d = ((moved[i][0] - face.verts[i][0]).powi(2) + (moved[i][1] - face.verts[i][1]).powi(2)).sqrt();
            assert!(d < 1e-4, "vertices the jaw doesn't weight stay put: {i}");
        }
    }
    assert!(face.weights.iter().any(|ws| ws.iter().any(|(n, _)| n == "Jaw")));
}
