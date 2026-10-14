//! IK constraints (docs/IK.md §7): solving relative to the animated pose,
//! the solvers, bend, weight, copy rotation, nesting, flips, saving and baking.

use keyternity_lib::ik;
use keyternity_lib::renderer::construction;
use keyternity_lib::shared::*;
use keyternity_lib::utils;

const EPS: f32 = 1e-2;

fn bone(id: i32, parent_id: i32, pos: Vec2, rot: f32) -> Bone {
    Bone {
        id,
        parent_id,
        name: format!("b{id}"),
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

fn world(bones: &Vec<Bone>) -> Vec<Bone> {
    let mut w = bones.clone();
    construction(&mut w, bones);
    w
}

fn at(bones: &[Bone], id: i32) -> &Bone {
    bones.iter().find(|b| b.id == id).unwrap()
}

fn near(a: Vec2, b: Vec2) -> bool {
    (a - b).mag() < EPS
}

/// A bent chain: bone 0 at `root`, then `n` more bones, each offset off its
/// parent's axis and rotated, the last of which is the tip (id n). A free
/// target bone (id 100) sits at `target`.
fn chain(root: Vec2, n: i32, target: Vec2, c: IkConstraint) -> Vec<Bone> {
    let mut bones = vec![bone(0, -1, root, 0.3)];
    for i in 1..=n {
        bones.push(bone(i, i - 1, v(20. + i as f32, 6. - 4. * i as f32), 0.25 * i as f32));
    }
    bones[n as usize].ik = Some(IkConstraint { target_id: 100, chain: n as u32, ..c });
    bones.push(bone(100, -1, target, 0.));
    bones
}

fn no_ik(bones: &Vec<Bone>) -> Vec<Bone> {
    let mut b = bones.clone();
    for bone in &mut b {
        bone.ik = None;
    }
    world(&b)
}

#[test]
fn target_on_the_tip_moves_nothing() {
    for n in [1, 2, 4] {
        let mut bones = chain(v(5., -3.), n, v(0., 0.), IkConstraint::default());
        let tip = at(&no_ik(&bones), n).pos;
        bones.iter_mut().find(|b| b.id == 100).unwrap().pos = tip;
        let (fk, solved) = (no_ik(&bones), world(&bones));
        for (a, b) in fk.iter().zip(&solved) {
            assert!(near(a.pos, b.pos), "chain {n}: bone {} moved {:?} -> {:?}", a.id, a.pos, b.pos);
            let d = utils::shortest_angle_delta(a.rot, b.rot).abs();
            assert!(d < 1e-3, "chain {n}: bone {} turned {d}", a.id);
        }
    }
}

#[test]
fn reachable_targets_are_reached() {
    for n in [2, 3, 4] {
        let target = v(18., 25.);
        let w = world(&chain(v(0., 0.), n, target, IkConstraint::default()));
        assert!(near(at(&w, n).pos, target), "chain {n}: tip at {:?}", at(&w, n).pos);
    }
}

#[test]
fn one_bone_aims_at_the_target() {
    let w = world(&chain(v(0., 0.), 1, v(0., 50.), IkConstraint::default()));
    let tip = at(&w, 1).pos;
    assert!(tip.x.abs() < EPS && tip.y > 0., "tip at {tip:?}");
}

#[test]
fn out_of_reach_straightens_toward_the_target() {
    for n in [2, 4] {
        let bones = chain(v(0., 0.), n, v(500., 0.), IkConstraint::default());
        let w = world(&bones);
        for i in 0..=n {
            assert!(at(&w, i).pos.y.abs() < EPS, "chain {n}: joint {i} at {:?}", at(&w, i).pos);
        }
        assert!(at(&w, n).pos.x < 500.);
    }
}

/// Which way the solved chain turns at its first joint.
fn turn(w: &[Bone]) -> f32 {
    let (a, b, c) = (at(w, 0).pos, at(w, 1).pos, at(w, 2).pos);
    let (s, t) = (b - a, c - b);
    (s.x * t.y - s.y * t.x).signum()
}

#[test]
fn bend_as_drawn_keeps_the_side_and_cw_ccw_force_it() {
    for n in [2, 3] {
        let drawn = turn(&no_ik(&chain(v(0., 0.), n, v(0., 0.), IkConstraint::default())));
        let solve = |bend| world(&chain(v(0., 0.), n, v(15., 20.), IkConstraint { bend, ..Default::default() }));
        assert_eq!(turn(&solve(IkBend::AsDrawn)), drawn, "chain {n}: as drawn flipped");
        assert_eq!(turn(&solve(IkBend::Clockwise)), -1., "chain {n}: not clockwise");
        assert_eq!(turn(&solve(IkBend::CounterClockwise)), 1., "chain {n}: not counter-clockwise");
        for bend in [IkBend::Clockwise, IkBend::CounterClockwise] {
            assert!(near(at(&solve(bend), n).pos, v(15., 20.)), "chain {n} {bend:?} missed");
        }
    }
}

#[test]
fn weight_blends_from_the_animated_pose() {
    let c = |weight| IkConstraint { weight, ..Default::default() };
    let fk = no_ik(&chain(v(0., 0.), 2, v(10., 30.), c(1.)));
    let full = world(&chain(v(0., 0.), 2, v(10., 30.), c(1.)));
    let none = world(&chain(v(0., 0.), 2, v(10., 30.), c(0.)));
    let half = world(&chain(v(0., 0.), 2, v(10., 30.), c(0.5)));
    assert!(near(at(&none, 2).pos, at(&fk, 2).pos));
    let d_full = utils::shortest_angle_delta(at(&fk, 0).rot, at(&full, 0).rot);
    let d_half = utils::shortest_angle_delta(at(&fk, 0).rot, at(&half, 0).rot);
    assert!((d_half - d_full / 2.).abs() < 1e-3, "half {d_half} vs full {d_full}");
}

#[test]
fn copy_rotation_gives_the_tip_the_targets_rotation() {
    let mut bones = chain(v(0., 0.), 2, v(10., 30.), IkConstraint { copy_rotation: true, ..Default::default() });
    bones.iter_mut().find(|b| b.id == 100).unwrap().rot = 1.;
    let w = world(&bones);
    assert!(utils::shortest_angle_delta(at(&w, 2).rot, 1.).abs() < 1e-3, "tip rot {}", at(&w, 2).rot);
}

#[test]
fn tip_keeps_its_rotation_relative_to_its_parent() {
    let bones = chain(v(0., 0.), 2, v(10., 30.), IkConstraint::default());
    let w = world(&bones);
    let local = utils::shortest_angle_delta(at(&w, 1).rot, at(&w, 2).rot);
    assert!((local - at(&bones, 2).rot).abs() < 1e-3);
}

#[test]
fn a_chain_on_a_chain_reaches_its_target() {
    // chain A: 0 -> 1 -> 2 (tip), target 100; chain B hangs off bone 2: 3 -> 4 -> 5 (tip), target 101
    let mut bones = chain(v(0., 0.), 2, v(-10., 30.), IkConstraint::default());
    bones.push(bone(3, 2, v(8., 0.), 0.));
    bones.push(bone(4, 3, v(12., 3.), 0.2));
    bones.push(bone(5, 4, v(12., -3.), 0.));
    bones.iter_mut().find(|b| b.id == 5).unwrap().ik = Some(IkConstraint { target_id: 101, chain: 2, ..Default::default() });
    bones.push(bone(101, -1, v(-5., 40.), 0.));
    let w = world(&bones);
    assert!(near(at(&w, 2).pos, v(-10., 30.)), "A tip at {:?}", at(&w, 2).pos);
    assert!(near(at(&w, 5).pos, v(-5., 40.)), "B tip at {:?}", at(&w, 5).pos);
}

#[test]
fn flipped_parent_still_reaches() {
    let mut bones = chain(v(0., 0.), 2, v(-20., 15.), IkConstraint::default());
    let mut parent = bone(50, -1, v(3., 4.), 0.4);
    parent.scale = v(-1., 1.);
    bones[0].parent_id = 50;
    bones[0].pos = v(0., 0.);
    bones.push(parent);
    let w = world(&bones);
    assert!(near(at(&w, 2).pos, v(-20., 15.)), "tip at {:?}", at(&w, 2).pos);
}

#[test]
fn solving_is_translation_invariant() {
    let solve = |o: Vec2| {
        let w = world(&chain(o, 3, o + v(15., 22.), IkConstraint::default()));
        (0..=3).map(|i| at(&w, i).pos - o).collect::<Vec<_>>()
    };
    for (a, b) in solve(v(0., 0.)).iter().zip(&solve(v(300., -120.))) {
        assert!(near(*a, *b), "{a:?} vs {b:?}");
    }
}

#[test]
fn chain_is_capped_at_the_tips_depth() {
    let mut bones = chain(v(0., 0.), 2, v(10., 30.), IkConstraint::default());
    bones[2].ik.as_mut().unwrap().chain = 9;
    assert_eq!(ik::chain_ids(&bones, 2, 9), vec![0, 1]);
    assert_eq!(ik::depth(&bones, 2), 2);
    assert!(near(at(&world(&bones), 2).pos, v(10., 30.)));
}

#[test]
fn targets_inside_the_chain_are_refused() {
    let mut bones = chain(v(0., 0.), 2, v(10., 30.), IkConstraint::default());
    bones.push(bone(7, 1, v(4., 4.), 0.)); // under a chain bone
    bones.push(bone(8, -1, v(4., 4.), 0.)); // elsewhere
    for id in [0, 1, 2, 7] {
        assert!(!ik::target_allowed(&bones, 2, 2, id), "bone {id} allowed");
    }
    assert!(ik::target_allowed(&bones, 2, 2, 8));
    assert!(ik::target_allowed(&bones, 2, 1, 7) == false, "bone 7 sits under bone 1");
}

#[test]
fn roles_and_locks() {
    let bones = chain(v(0., 0.), 2, v(10., 30.), IkConstraint::default());
    assert_eq!(ik::role(&bones, 0), ik::Role::Chain(2));
    assert_eq!(ik::role(&bones, 2), ik::Role::Tip);
    assert_eq!(ik::role(&bones, 100), ik::Role::None);
    assert_eq!(ik::locks(&bones, 0), (true, false)); // first chain bone can still be moved
    assert_eq!(ik::locks(&bones, 1), (true, true));
    assert_eq!(ik::locks(&bones, 2), (false, true));

    let mut off = bones.clone();
    off[2].ik.as_mut().unwrap().enabled = false;
    assert_eq!(ik::locks(&off, 1), (false, false));
    assert!(near(at(&world(&off), 2).pos, at(&no_ik(&bones), 2).pos));
}

fn armature(bones: Vec<Bone>, keyframes: Vec<Keyframe>) -> Armature {
    Armature {
        bones,
        animations: vec![Animation { name: "a".into(), fps: 60, keyframes, ..Default::default() }],
        ..Default::default()
    }
}

fn key(bone_id: i32, frame: i32, element: AnimElement, value: f32) -> Keyframe {
    Keyframe {
        frame,
        bone_id,
        element,
        value,
        start_handle: v(1. / 3., 0.),
        end_handle: v(1. / 3., 0.),
        next_kf: -1,
        handle_preset: HandlePreset::Linear,
        ..Default::default()
    }
}

#[test]
fn saved_constraints_point_at_saved_bone_indices() {
    // ids that aren't indices, so the save has to remap the target
    let mut bones = chain(v(0., 0.), 2, v(10., 30.), IkConstraint { bend: IkBend::Clockwise, ..Default::default() });
    bones.rotate_left(1);
    let arm = armature(bones, vec![]);
    let (json, _) = utils::prepare_files(&arm, Camera::default(), vec![], &EditMode::default());
    let root: Root = serde_json::from_str(&json).unwrap();
    let tip = root.bones.iter().find(|b| b.ik.is_some()).expect("constraint not saved");
    let c = tip.ik.as_ref().unwrap();
    assert_eq!(root.bones[c.target_id as usize].name, "b100");
    assert_eq!((c.chain, c.bend), (2, IkBend::Clockwise));
}

#[test]
fn baked_rotations_match_the_solved_pose_under_a_rotated_parent() {
    let mut bones = chain(v(0., 0.), 2, v(10., 30.), IkConstraint::default());
    bones.push(bone(50, -1, v(7., -2.), 0.9));
    bones[0].parent_id = 50;
    let keys = vec![key(100, 0, AnimElement::PositionX, 10.), key(100, 10, AnimElement::PositionX, -15.)];
    let arm = armature(bones, keys);

    let edit = EditMode { export_bake_ik: true, export_exclude_ik: true, ..Default::default() };
    let (json, _) = utils::prepare_files(&arm, Camera::default(), vec![], &edit);
    let baked: Root = serde_json::from_str(&json).unwrap();
    assert!(baked.bones.iter().all(|b| b.ik.is_none()), "IK kept despite exclude");
    let mut baked_arm = Armature { bones: baked.bones.clone(), animations: baked.animations.clone(), ..Default::default() };

    for frame in [0, 10] {
        let solved = world(&arm.clone().animate(0, frame, None));
        let replay = world(&baked_arm.animate(0, frame, None));
        // saved ids are indices; compare by name
        for name in ["b1", "b2"] {
            let a = solved.iter().find(|b| b.name == name).unwrap().pos;
            let b = replay.iter().find(|b| b.name == name).unwrap().pos;
            assert!(near(a, b), "frame {frame} {name}: solved {a:?}, baked {b:?}");
        }
    }
}
