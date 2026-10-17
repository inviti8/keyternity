//! Reparenting keeps a bone in place, and an IK target's space (docs/IK.md §4.1).

use keyternity_lib::editor;
use keyternity_lib::renderer::construction;
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

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn world(arm: &Armature) -> Vec<Bone> {
    let mut w = arm.bones.clone();
    construction(&mut w, &arm.bones);
    w
}

fn at(w: &[Bone], id: i32) -> &Bone {
    w.iter().find(|b| b.id == id).unwrap()
}

fn same_place(a: &Bone, b: &Bone) -> bool {
    (a.pos - b.pos).mag() < 1e-3
        && utils::shortest_angle_delta(a.rot, b.rot).abs() < 1e-4
        && (a.scale - b.scale).mag() < 1e-4
}

#[test]
fn reparenting_keeps_the_bone_and_its_children_in_place() {
    for (scale, flip) in [((1., 1.), false), ((2., 0.5), false), ((1., 1.), true)] {
        let mut new_parent = bone(0, -1, "new parent", v(40., -30.), 1.1);
        new_parent.scale = v(scale.0, scale.1);
        if flip {
            new_parent.scale.x = -1.;
        }
        let old_parent = bone(1, -1, "old parent", v(-20., 10.), -0.4);
        let mut moving = bone(2, 1, "moving", v(15., 5.), 0.3);
        moving.scale = v(1.5, 1.);
        let child = bone(3, 2, "child", v(10., 2.), 0.2);
        let mut arm = Armature { bones: vec![new_parent, old_parent, moving, child], ..Default::default() };
        let before = world(&arm);

        editor::drag_bone(&mut arm, 0, &vec![2], false);
        assert_eq!(at(&arm.bones, 2).parent_id, 0);
        let after = world(&arm);
        for id in [2, 3] {
            assert!(same_place(at(&before, id), at(&after, id)), "{scale:?} flip {flip}: bone {id} moved");
        }
    }
}

/// chest → upper arm → forearm → hand (IK tip, chain 2), and the hand's target.
fn arm_rig() -> Armature {
    let chest = bone(0, -1, "chest", v(0., 100.), 0.2);
    let upper = bone(1, 0, "upper arm", v(20., 0.), -1.2);
    let fore = bone(2, 1, "forearm", v(40., 0.), 0.5);
    let mut hand = bone(3, 2, "hand", v(40., 0.), 0.);
    hand.ik = Some(IkConstraint { target_id: 4, chain: 2, ..Default::default() });
    let mut arm = Armature { bones: vec![chest, upper, fore, hand], ..Default::default() };
    // target where the hand is, at the root
    let hand_world = at(&world(&arm), 3).pos;
    arm.bones.push(bone(4, -1, "hand Target", hand_world, 0.));
    arm
}

#[test]
fn a_target_in_chest_space_follows_the_chest() {
    let mut arm = arm_rig();
    let before = world(&arm);
    assert!(editor::set_ik_target_space(&mut arm, 3, 0));
    assert_eq!(at(&arm.bones, 4).parent_id, 0);
    let after = world(&arm);
    assert!(same_place(at(&before, 4), at(&after, 4)), "target moved when switching space");
    assert!((at(&after, 3).pos - at(&after, 4).pos).mag() < 0.01, "hand left its target");

    // turning the chest carries the target, and the hand still reaches it
    arm.bones.iter_mut().find(|b| b.id == 0).unwrap().rot += 0.5;
    let turned = world(&arm);
    assert!((at(&turned, 4).pos - at(&after, 4).pos).mag() > 1., "target didn't follow the chest");
    assert!((at(&turned, 3).pos - at(&turned, 4).pos).mag() < 0.01, "hand left its target");
}

#[test]
fn back_to_world_space() {
    let mut arm = arm_rig();
    editor::set_ik_target_space(&mut arm, 3, 0);
    let before = world(&arm);
    assert!(editor::set_ik_target_space(&mut arm, 3, -1));
    assert_eq!(at(&arm.bones, 4).parent_id, -1);
    assert_eq!(arm.bones.last().unwrap().id, 4);
    assert!(same_place(at(&before, 4), at(&world(&arm), 4)));
    // and the chest no longer carries it
    arm.bones.iter_mut().find(|b| b.id == 0).unwrap().rot += 0.5;
    assert!(same_place(at(&before, 4), at(&world(&arm), 4)));
}

#[test]
fn spaces_inside_the_chain_are_refused() {
    let mut arm = arm_rig();
    arm.bones.push(bone(5, 1, "under the upper arm", v(5., 5.), 0.));
    arm.bones.push(bone(6, 4, "under the target", v(5., 5.), 0.));
    arm.bones.push(bone(7, -1, "prop", v(200., 0.), 0.));
    let spaces = editor::ik_target_spaces(&arm, 3);
    assert_eq!(spaces, vec![0, 7], "ancestors first, then other bones outside the chain");
    for id in [1, 2, 3, 4, 5, 6] {
        assert!(!editor::set_ik_target_space(&mut arm, 3, id), "bone {id} accepted");
    }
}
