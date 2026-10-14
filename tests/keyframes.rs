//! Set Key and Delete Key (the timeline's buttons, I and Alt+I).

use keyternity_lib::editor;
use keyternity_lib::shared::*;

fn bone(id: i32, x: f32) -> Bone {
    Bone {
        id,
        parent_id: -1,
        name: format!("b{id}"),
        pos: Vec2::new(x, 0.),
        scale: Vec2::new(1., 1.),
        pivot_scale: Vec2::new(1., 1.),
        tint: TintColor::new(1., 1., 1., 1.),
        physics_id: -1,
        visuals_id: -1,
        ..Default::default()
    }
}

fn key(bone_id: i32, frame: i32, element: AnimElement, value: f32) -> Keyframe {
    Keyframe {
        frame,
        bone_id,
        element,
        value,
        start_handle: Vec2::new(1. / 3., 0.),
        end_handle: Vec2::new(1. / 3., 0.),
        next_kf: -1,
        handle_preset: HandlePreset::Linear,
        ..Default::default()
    }
}

fn armature(keys: Vec<Keyframe>) -> Armature {
    Armature {
        bones: vec![bone(0, 10.), bone(1, 20.)],
        animations: vec![Animation { name: "a".into(), fps: 60, keyframes: keys, ..Default::default() }],
        ..Default::default()
    }
}

fn value(arm: &Armature, id: i32, frame: i32, el: AnimElement) -> Option<f32> {
    arm.animations[0].keyframes.iter().find(|k| k.bone_id == id && k.frame == frame && k.element == el).map(|k| k.value)
}

#[test]
fn set_key_keys_every_transform_and_frame_zero() {
    let mut arm = armature(vec![]);
    editor::insert_keyframes(&mut arm, 0, 12, &[0]);
    for el in [AnimElement::PositionX, AnimElement::PositionY, AnimElement::Rotation, AnimElement::ScaleX, AnimElement::ScaleY] {
        assert!(value(&arm, 0, 12, el.clone()).is_some(), "{el:?} not keyed on frame 12");
        assert!(value(&arm, 0, 0, el.clone()).is_some(), "{el:?} has no frame 0 key");
    }
    assert_eq!(value(&arm, 0, 12, AnimElement::PositionX), Some(10.));
    assert_eq!(value(&arm, 0, 12, AnimElement::ScaleX), Some(1.));
    assert!(arm.animations[0].keyframes.iter().all(|k| k.bone_id == 0), "keyed an unselected bone");
}

#[test]
fn set_key_holds_the_in_between_pose() {
    let mut arm = armature(vec![key(0, 0, AnimElement::PositionX, 0.), key(0, 20, AnimElement::PositionX, 100.)]);
    let shown = arm.animate(0, 10, None)[0].pos.x;
    editor::insert_keyframes(&mut arm, 0, 10, &[0]);
    assert_eq!(value(&arm, 0, 10, AnimElement::PositionX), Some(shown));
    assert_eq!(value(&arm, 0, 0, AnimElement::PositionX), Some(0.), "existing frame 0 key changed");
    // keying again leaves it as it is
    editor::insert_keyframes(&mut arm, 0, 10, &[0]);
    assert_eq!(arm.animations[0].keyframes.iter().filter(|k| k.frame == 10 && k.element == AnimElement::PositionX).count(), 1);
}

#[test]
fn locked_bones_are_not_keyed() {
    let mut arm = armature(vec![]);
    arm.bones[1].locked = true;
    editor::insert_keyframes(&mut arm, 0, 5, &[0, 1]);
    assert!(arm.animations[0].keyframes.iter().all(|k| k.bone_id == 0));
}

#[test]
fn delete_key_removes_the_selected_bones_keys_on_the_frame() {
    let keys = vec![
        key(0, 0, AnimElement::PositionX, 0.),
        key(0, 10, AnimElement::PositionX, 5.),
        key(0, 10, AnimElement::Rotation, 1.),
        key(1, 10, AnimElement::PositionX, 7.),
    ];
    let mut arm = armature(keys.clone());
    editor::delete_keyframes_at(&mut arm, 0, 10, &[0]);
    let left: Vec<(i32, i32)> = arm.animations[0].keyframes.iter().map(|k| (k.bone_id, k.frame)).collect();
    assert_eq!(left, vec![(0, 0), (1, 10)]);

    // with nothing selected, every key on the frame
    let mut arm = armature(keys);
    editor::delete_keyframes_at(&mut arm, 0, 10, &[]);
    assert_eq!(arm.animations[0].keyframes.len(), 1);
}

#[test]
fn key_all_keys_the_untextured_bones() {
    let mut arm = armature(vec![]);
    arm.bones[1].tex = "art".into();
    let mut helper = bone(2, 0.);
    helper.bind_owner = Some(1);
    arm.bones.push(helper);
    arm.bones.push(bone(3, 50.));
    assert_eq!(editor::rig_bones(&arm), vec![0, 3]);
    let ids = editor::rig_bones(&arm);
    editor::insert_keyframes(&mut arm, 0, 8, &ids);
    assert_eq!(value(&arm, 3, 8, AnimElement::PositionX), Some(50.));
    assert!(arm.animations[0].keyframes.iter().all(|k| k.bone_id == 0 || k.bone_id == 3));
}

#[test]
fn breakdown_keys_between_the_neighbouring_poses() {
    let keys = vec![
        key(0, 0, AnimElement::PositionX, 0.),
        key(0, 20, AnimElement::PositionX, 100.),
        key(0, 0, AnimElement::Rotation, 1.),
    ];
    let mut arm = armature(keys);
    editor::breakdown_keyframes(&mut arm, 0, 10, &[0], 0.3);
    assert!((value(&arm, 0, 10, AnimElement::PositionX).unwrap() - 30.).abs() < 1e-4);
    assert_eq!(value(&arm, 0, 10, AnimElement::Rotation), Some(1.), "one-sided key isn't held");
    assert_eq!(value(&arm, 0, 10, AnimElement::ScaleX), None, "unkeyed transform was keyed");
    // moving the slider updates the same key
    editor::breakdown_keyframes(&mut arm, 0, 10, &[0], 0.8);
    assert!((value(&arm, 0, 10, AnimElement::PositionX).unwrap() - 80.).abs() < 1e-4);
}

#[test]
fn easing_presets_shape_the_transition() {
    use keyternity_lib::utils;
    for preset in HandlePreset::MENU {
        if preset == HandlePreset::Snap {
            continue;
        }
        let (a, b) = utils::interp_preset(preset.clone());
        let at = |f: i32| utils::interp(f, 100, 0., 1., a, b);
        assert!(at(0).abs() < 1e-3 && (at(100) - 1.).abs() < 1e-3, "{preset:?} doesn't run 0 to 1");
    }
    let (a, b) = utils::interp_preset(HandlePreset::CubicIn);
    assert!(utils::interp(50, 100, 0., 1., a, b) < 0.2, "Cubic-In starts slow");
    let (a, b) = utils::interp_preset(HandlePreset::BackOut);
    let peak = (1..100).map(|f| utils::interp(f, 100, 0., 1., a, b)).fold(0f32, f32::max);
    assert!(peak > 1.05, "Back-Out doesn't overshoot ({peak})");
}
