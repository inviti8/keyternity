//! Layers through the editor (docs/TIMELINE.md §5): adding, keying through a layer,
//! locking, undo, and what saving and exporting write.

use keyternity_lib::editor;
use keyternity_lib::shared::*;
use keyternity_lib::utils;

fn run(s: &mut Shared) {
    while !s.events.events.is_empty() {
        #[rustfmt::skip]
        editor::iterate_events(
            &s.input, &mut s.config, &mut s.events, &mut s.camera, &mut s.edit_mode, &mut s.selections,
            &mut s.undo_states, &mut s.armature, &mut s.psd_armature, &mut s.copy_buffer, &mut s.ui, &mut s.renderer,
        );
    }
}

fn walk() -> Shared {
    let mut s = Shared::default();
    s.armature.bones = vec![Bone { id: 0, parent_id: -1, name: "chest".into(), rot: 0.5, scale: Vec2::new(1., 1.), ..Default::default() }];
    let (start_handle, end_handle) = utils::interp_preset(HandlePreset::Linear);
    let key = |frame, value| Keyframe { frame, bone_id: 0, element: AnimElement::Rotation, value, start_handle, end_handle, ..Default::default() };
    s.armature.animations = vec![Animation { id: 0, name: "walk".into(), fps: 24, keyframes: vec![key(0, 0.), key(10, 1.)], ..Default::default() }];
    s.edit_mode.anim_open = true;
    s.selections.anim = 0;
    s.selections.bone_idx = 0;
    s
}

fn rot(s: &mut Shared, frame: i32) -> f32 {
    s.armature.animate(0, frame, None)[0].rot
}

fn set_rot(s: &mut Shared, frame: i32, value: f32) {
    s.events.save_edited_bone(0);
    s.events.edit_bone(0, &AnimElement::Rotation, value, "", 0, frame);
    run(s);
}

#[test]
fn adding_a_layer_makes_it_the_one_edited() {
    let mut s = walk();
    s.events.add_layer(LayerMode::Additive);
    run(&mut s);
    let a = &s.armature.animations[0];
    assert_eq!((a.layers.len(), a.active_layer), (2, 1));
    assert!(a.keyframes.is_empty(), "the new layer has no keys yet");
    assert_eq!(a.layer_keys(0).len(), 2, "Base keeps the walk");
    assert!((rot(&mut s, 5) - 0.5).abs() < 1e-4, "an empty layer changes nothing");

    s.events.undo();
    run(&mut s);
    assert!(s.armature.animations[0].layers.is_empty());
    assert_eq!(s.armature.animations[0].keyframes.len(), 2);
}

#[test]
fn keying_on_an_additive_layer_shows_what_was_set() {
    let mut s = walk();
    s.events.add_layer(LayerMode::Additive);
    run(&mut s);
    set_rot(&mut s, 5, 0.8); // the walk shows 0.5 there: +0.3
    assert!((rot(&mut s, 5) - 0.8).abs() < 1e-4, "{}", rot(&mut s, 5));
    // it's an offset: on frame 10 the walk's 1.0 gets it too (the first key holds)
    assert!((rot(&mut s, 10) - 1.3).abs() < 1e-3, "{}", rot(&mut s, 10));
    // the setup-value first key is a zero offset
    assert!((rot(&mut s, 0) - 0.).abs() < 1e-4);
    assert_eq!(s.armature.animations[0].layer_keys(0).len(), 2, "Base is untouched");
}

#[test]
fn an_override_layer_gets_no_setup_pose_first_key() {
    let mut s = walk();
    s.events.add_layer(LayerMode::Override);
    run(&mut s);
    set_rot(&mut s, 5, 0.2);
    let a = &s.armature.animations[0];
    assert_eq!(a.keyframes.len(), 1, "no frame-0 key holding the setup pose over Base");
    assert!((rot(&mut s, 5) - 0.2).abs() < 1e-4);
}

#[test]
fn locked_layers_take_no_keys() {
    let mut s = walk();
    s.events.add_layer(LayerMode::Additive);
    run(&mut s);
    let layer = AnimLayer { locked: true, ..s.armature.animations[0].layers[1].clone() };
    s.events.set_layer(1, &layer, true);
    run(&mut s);
    set_rot(&mut s, 5, 0.9);
    assert!(s.armature.animations[0].keyframes.is_empty());
}

#[test]
fn saving_keeps_layers_and_exporting_flattens_them() {
    let mut s = walk();
    s.events.add_layer(LayerMode::Additive);
    run(&mut s);
    set_rot(&mut s, 5, 0.8);
    let shown: Vec<f32> = (0..=10).map(|f| rot(&mut s, f)).collect();

    let (json, _) = utils::prepare_files(&s.armature, Camera::default(), vec![], &EditMode::default(), None);
    let root: serde_json::Value = serde_json::from_str(&json).unwrap();
    let anim = &root["animations"][0];
    assert_eq!(anim["keyframes"].as_array().unwrap().len(), 2, "Base in keyframes, as before layers");
    assert_eq!(anim["layers"].as_array().unwrap().len(), 2);
    assert!(anim.get("active_layer").is_none());
    let loaded: Animation = serde_json::from_value(anim.clone()).unwrap();
    let mut arm = s.armature.clone();
    arm.animations = vec![loaded];
    for f in 0..=10 {
        assert!((arm.animate(0, f, None)[0].rot - shown[f as usize]).abs() < 1e-4, "reloaded, frame {f}");
    }

    let export = EditMode { export_flatten_layers: true, ..Default::default() };
    let (json, _) = utils::prepare_files(&s.armature, Camera::default(), vec![], &export, None);
    let root: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(root["animations"][0].get("layers").is_none(), "runtimes get one set of keys");
    let flat: Animation = serde_json::from_value(root["animations"][0].clone()).unwrap();
    arm.animations = vec![flat];
    for f in 0..=10 {
        assert!((arm.animate(0, f, None)[0].rot - shown[f as usize]).abs() < 1e-3, "flattened, frame {f}");
    }
}

#[test]
fn layer_settings_keep_its_id_and_keys() {
    let mut s = walk();
    s.events.add_layer(LayerMode::Additive);
    run(&mut s);
    set_rot(&mut s, 5, 0.8);
    let id = s.armature.animations[0].layers[1].id;
    let settings = AnimLayer {
        name: "Breathing".into(),
        play: PlayMode::Looped { length: 1., speed: 1., phase: 0., free_running: true },
        generators: vec![Generator { bone_id: 0, ..Default::default() }],
        ..Default::default()
    };
    s.events.set_layer(1, &settings, true);
    run(&mut s);
    let a = &s.armature.animations[0];
    assert_eq!(a.layers[1].id, id);
    assert_eq!(a.layers[1].name, "Breathing");
    assert_eq!(a.layers[1].generators.len(), 1);
    assert!(!a.layer_keys(1).is_empty(), "its keys stay");
}
