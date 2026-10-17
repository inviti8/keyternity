//! Bone length (docs/ENVELOPES.md §2): the tail, saving, DragonBones in and out, and PSD
//! group bones pointing at their child group.

use keyternity_lib::dragonbones_export;
use keyternity_lib::dragonbones_import::{self, DbAtlas};
use keyternity_lib::renderer::{construction, tail_pos};
use keyternity_lib::shared::*;
use keyternity_lib::utils;

fn bone(id: i32, parent_id: i32, name: &str, pos: Vec2, rot: f32) -> Bone {
    Bone {
        id,
        parent_id,
        name: name.to_string(),
        pos,
        rot,
        scale: Vec2::new(1., 1.),
        pivot_scale: Vec2::new(1., 1.),
        tint: TintColor::new(1., 1., 1., 1.),
        physics_id: -1,
        visuals_id: -1,
        ..Default::default()
    }
}

fn near(a: Vec2, b: Vec2) -> bool {
    (a - b).mag() < 1e-3
}

fn world(bones: &Vec<Bone>) -> Vec<Bone> {
    let mut w = bones.clone();
    construction(&mut w, bones);
    w
}

#[test]
fn tail_is_where_a_child_at_the_length_would_sit() {
    for (scale, parent_scale) in [((1., 1.), (1., 1.)), ((2., 0.5), (1., 1.)), ((-1., 1.), (1., 1.)), ((1., 1.), (-1., 1.))] {
        let mut parent = bone(0, -1, "parent", Vec2::new(5., 7.), 0.4);
        parent.scale = Vec2::new(parent_scale.0, parent_scale.1);
        let mut b = bone(1, 0, "b", Vec2::new(10., -3.), 0.7);
        b.scale = Vec2::new(scale.0, scale.1);
        b.length = 25.;
        let child = bone(2, 1, "child", Vec2::new(25., 0.), 0.);
        let w = world(&vec![parent, b, child]);
        let tail = tail_pos(&w[1]);
        assert!(near(tail, w[2].pos), "scale {scale:?}/{parent_scale:?}: tail {tail:?}, child {:?}", w[2].pos);
    }
}

#[test]
fn point_bones_have_their_tail_on_the_head() {
    let b = bone(0, -1, "b", Vec2::new(3., 4.), 1.);
    assert!(near(tail_pos(&b), b.pos));
}

#[test]
fn length_is_saved_and_zero_is_omitted() {
    let mut a = bone(0, -1, "a", Vec2::ZERO, 0.);
    a.length = 42.5;
    let b = bone(1, 0, "b", Vec2::new(42.5, 0.), 0.);
    let arm = Armature { bones: vec![a, b], ..Default::default() };
    let (json, _) = utils::prepare_files(&arm, Camera::default(), vec![], &EditMode::default(), None);
    let raw: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(raw["bones"][0]["length"], 42.5);
    assert!(raw["bones"][1].get("length").is_none(), "zero length written");
    let root: Root = serde_json::from_str(&json).unwrap();
    assert_eq!((root.bones[0].length, root.bones[1].length), (42.5, 0.));
}

#[test]
fn dragonbones_export_writes_length_and_import_keeps_it() {
    let mut a = bone(0, -1, "upper", Vec2::new(10., 20.), 0.3);
    a.length = 60.;
    let mut b = bone(1, 0, "lower", Vec2::new(60., 0.), -0.5);
    b.length = 45.;
    let c = bone(2, 1, "hand", Vec2::new(45., 0.), 0.);
    let arm = Armature { bones: vec![a, b, c], ..Default::default() };

    let files = dragonbones_export::export(&arm, "len", Vec2::new(2., 2.)).unwrap();
    let ske: serde_json::Value = serde_json::from_str(&files.ske_json).unwrap();
    let bones = ske["armature"][0]["bone"].as_array().unwrap();
    let length = |name: &str| bones.iter().find(|b| b["name"] == name).unwrap().get("length").cloned();
    assert_eq!(length("upper"), Some(serde_json::json!(60.0)));
    assert_eq!(length("lower"), Some(serde_json::json!(45.0)));
    assert_eq!(length("hand"), None);

    let atlas = DbAtlas { json: files.tex_json.clone(), png: files.tex_png.clone() };
    let back = dragonbones_import::import(&files.ske_json, &[atlas]).unwrap().armature;
    let len = |name: &str| back.bones.iter().find(|b| b.name == name).unwrap().length;
    assert_eq!((len("upper"), len("lower"), len("hand")), (60., 45., 0.));
}
