//! Bind to Skeleton and live envelope weights (docs/ENVELOPES.md §3.4–3.6, §7).

use keyternity_lib::envelope;
use keyternity_lib::renderer::construction;
use keyternity_lib::shared::*;
use keyternity_lib::skin;
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

const UPPER: i32 = 0;
const LOWER: i32 = 1;
const MESH: i32 = 2;

/// An arm: upper (0..100) and lower (100..200) skeleton bones along +x, and a
/// textured strip mesh over both, 21 columns of 2 vertices at y = ±10.
fn arm() -> Armature {
    let mut upper = bone(UPPER, -1, "upper", v(0., 0.), 0.);
    upper.length = 100.;
    let mut lower = bone(LOWER, UPPER, "lower", v(100., 0.), 0.);
    lower.length = 100.;
    let mut mesh = bone(MESH, -1, "arm art", v(0., 0.), 0.);
    mesh.tex = "arm".into();
    mesh.verts_edited = true;
    for i in 0..21 {
        for (j, y) in [-10., 10.].into_iter().enumerate() {
            let id = (i * 2 + j) as u32;
            mesh.vertices.push(Vertex { id, pos: v(i as f32 * 10., y), uv: v(0., 0.), ..Default::default() });
        }
    }
    for i in 0..20u32 {
        let (a, b, c, d) = (i * 2, i * 2 + 1, i * 2 + 2, i * 2 + 3);
        mesh.indices.extend([a, b, c, b, d, c]);
    }
    Armature { bones: vec![upper, lower, mesh], ..Default::default() }
}

fn find(arm: &Armature, id: i32) -> &Bone {
    arm.bones.iter().find(|b| b.id == id).unwrap()
}

/// World vertex positions of the mesh in the setup pose.
fn drawn(arm: &Armature) -> Vec<Vec2> {
    let mut w = arm.bones.clone();
    construction(&mut w, &arm.bones);
    w.iter().find(|b| b.id == MESH).unwrap().vertices.iter().map(|v| v.pos).collect()
}

/// Each vertex's share per skeleton bone (owner under MESH).
fn shares(arm: &Armature) -> Vec<Vec<(i32, f32)>> {
    let mesh = find(arm, MESH);
    let binds: Vec<&BoneBind> = mesh.binds.iter().collect();
    let bone_of = |b: &BoneBind| {
        let h = find(arm, b.bone_id);
        if h.bind_owner == Some(MESH) { h.parent_id } else { h.id }
    };
    mesh.vertices
        .iter()
        .map(|vert| {
            let weights: Vec<f32> = binds
                .iter()
                .map(|b| b.verts.iter().find(|bv| bv.id == vert.id as i32).map_or(0., |bv| bv.weight))
                .collect();
            let (owner, s) = envelope::shares(&weights);
            let mut out = vec![(MESH, owner)];
            out.extend(binds.iter().zip(s).map(|(b, x)| (bone_of(b), x)));
            out
        })
        .collect()
}

fn share_of(s: &[(i32, f32)], id: i32) -> f32 {
    s.iter().filter(|x| x.0 == id).map(|x| x.1).sum()
}

#[test]
fn bind_to_skeleton_skins_the_mesh_in_one_step() {
    let mut a = arm();
    let before = drawn(&a);
    let outside = skin::bind_to_skeleton(&mut a, MESH).unwrap();
    assert!(outside.is_empty(), "outside: {outside:?}");

    let mesh = find(&a, MESH);
    assert!(mesh.bind_pose && mesh.envelope_weights);
    assert_eq!(skin::skin_bones(&a.bones, mesh), vec![UPPER, LOWER]);

    for (i, s) in shares(&a).iter().enumerate() {
        let sum: f32 = s.iter().map(|x| x.1).sum();
        assert!((sum - 1.).abs() < 1e-4, "vertex {i}: shares sum {sum}");
        assert!(share_of(s, MESH) < 1e-4, "vertex {i} kept on its owner");
    }
    // start of the arm on upper, end on lower, the elbow shared
    let s = shares(&a);
    assert!(share_of(&s[0], UPPER) > 0.99);
    assert!(share_of(&s[40], LOWER) > 0.99);
    let elbow = &s[20];
    assert!((share_of(elbow, UPPER) - 0.5).abs() < 0.05, "elbow {elbow:?}");

    // nothing moves at rest
    for (p, q) in before.iter().zip(drawn(&a)) {
        assert!((*p - q).mag() < 1e-3, "{p:?} moved to {q:?}");
    }
}

#[test]
fn the_skinned_mesh_bends_with_the_skeleton() {
    let mut a = arm();
    skin::bind_to_skeleton(&mut a, MESH).unwrap();
    // bend the elbow 90°: the tip goes up, the start stays
    a.bones.iter_mut().find(|b| b.id == LOWER).unwrap().rot = std::f32::consts::FRAC_PI_2;
    bind_pose_rest(&mut a);
    let after = drawn(&a);
    assert!((after[0] - v(0., -10.)).mag() < 1e-3, "start moved to {:?}", after[0]);
    let tip = (after[40] + after[41]) / 2.;
    assert!((tip - v(100., 100.)).mag() < 1., "tip at {tip:?}");
}

/// Bind Pose helpers follow the setup pose (`bind_pose::maintain`) and the test
/// moves the setup pose, so pose the bone through an animation-free copy instead:
/// put helpers back at the original rest.
fn bind_pose_rest(a: &mut Armature) {
    let mut rest = a.clone();
    rest.bones.iter_mut().find(|b| b.id == LOWER).unwrap().rot = 0.;
    let snapshot = keyternity_lib::bind_pose::pose_snapshot(&rest);
    keyternity_lib::bind_pose::maintain_with_rest(a, Some(&snapshot));
}

#[test]
fn bones_out_of_reach_and_under_the_mesh_get_no_bind() {
    let mut a = arm();
    a.bones.push(bone(10, -1, "far away", v(5000., 5000.), 0.));
    a.bones.push(bone(11, MESH, "under the mesh", v(50., 0.), 0.));
    a.bones.push({
        let mut art = bone(12, UPPER, "other art", v(30., 0.), 0.);
        art.tex = "other".into();
        art
    });
    skin::bind_to_skeleton(&mut a, MESH).unwrap();
    assert_eq!(skin::skin_bones(&a.bones, find(&a, MESH)), vec![UPPER, LOWER]);
}

#[test]
fn envelope_edits_update_the_weights_live_but_not_locked_vertices() {
    let mut a = arm();
    skin::bind_to_skeleton(&mut a, MESH).unwrap();
    skin::maintain(&mut a, false);
    let before = shares(&a);

    // hand-edit vertex 30, then make upper reach much further
    let mesh = a.bones.iter_mut().find(|b| b.id == MESH).unwrap();
    skin::lock_vertex(mesh, 30);
    skin::set_envelope_field(&mut a, UPPER, EnvelopeField::TailOuter, 150.);
    skin::set_envelope_field(&mut a, UPPER, EnvelopeField::TailInner, 120.);
    skin::maintain(&mut a, false);
    let after = shares(&a);

    assert!(share_of(&after[28], UPPER) > share_of(&before[28], UPPER) + 0.2, "vertex 28 didn't follow upper's envelope");
    for id in [MESH, UPPER, LOWER] {
        assert!((share_of(&after[30], id) - share_of(&before[30], id)).abs() < 1e-4, "locked vertex changed");
    }
}

#[test]
fn binding_again_keeps_hand_weights() {
    let mut a = arm();
    skin::bind_to_skeleton(&mut a, MESH).unwrap();
    // put vertex 2 fully on lower by hand
    {
        let mesh = a.bones.iter_mut().find(|b| b.id == MESH).unwrap();
        for bind in &mut mesh.binds {
            bind.verts.retain(|bv| bv.id != 2);
        }
        let last = mesh.binds.len() - 1;
        mesh.binds[last].verts.push(BoneBindVert { id: 2, weight: 1. });
        skin::lock_vertex(mesh, 2);
    }
    let hand = shares(&a)[2].clone();
    skin::bind_to_skeleton(&mut a, MESH).unwrap();
    assert!((share_of(&shares(&a)[2], LOWER) - share_of(&hand, LOWER)).abs() < 1e-4);
    assert!(share_of(&shares(&a)[2], LOWER) > 0.99);
}

#[test]
fn outside_vertices_go_to_the_nearest_bone_and_are_reported() {
    let mut a = arm();
    // a vertex far below the arm
    a.bones.iter_mut().find(|b| b.id == MESH).unwrap().vertices[0].pos = v(150., -500.);
    let outside = skin::bind_to_skeleton(&mut a, MESH).unwrap();
    assert_eq!(outside, vec![0]);
    assert!(share_of(&shares(&a)[0], LOWER) > 0.99);
    assert_eq!(find(&a, MESH).skin_outside, vec![0]);
}

#[test]
fn meshes_with_path_binds_are_refused() {
    let mut a = arm();
    a.bones.iter_mut().find(|b| b.id == MESH).unwrap().binds.push(BoneBind { bone_id: UPPER, is_path: true, verts: vec![] });
    assert!(skin::bind_to_skeleton(&mut a, MESH).is_err());
}

#[test]
fn default_deform_is_the_untextured_skeleton() {
    let mut a = arm();
    a.bones.push(bone(20, -1, "target", v(200., 0.), 0.));
    a.bones.iter_mut().find(|b| b.id == LOWER).unwrap().ik =
        Some(IkConstraint { target_id: 20, chain: 1, ..Default::default() });
    let world = skin::setup_world(&a);
    assert!(skin::envelope_of(&world, UPPER).deform);
    assert!(!skin::envelope_of(&world, MESH).deform, "textured bones don't deform by default");
    assert!(!skin::envelope_of(&world, 20).deform, "IK targets don't deform");
    // a bone with length gets radii from it
    let e = skin::envelope_of(&world, UPPER);
    assert_eq!((e.head_inner, e.head_outer), (20., 50.));
}

#[test]
fn envelopes_and_hand_weights_are_saved() {
    let mut a = arm();
    skin::bind_to_skeleton(&mut a, MESH).unwrap();
    skin::set_envelope_field(&mut a, UPPER, EnvelopeField::HeadOuter, 77.);
    let mesh = a.bones.iter_mut().find(|b| b.id == MESH).unwrap();
    mesh.max_influences = 2;
    skin::lock_vertex(mesh, 5);
    let (_, editor) = utils::prepare_files(&a, Camera::default(), vec![], &EditMode::default());
    let editor: serde_json::Value = serde_json::from_str(&editor).unwrap();
    let bones = editor["bones"].as_array().unwrap();
    let upper = &bones[a.bones.iter().position(|b| b.id == UPPER).unwrap()];
    assert_eq!(upper["envelope"]["head_outer"], 77.);
    let mesh = &bones[a.bones.iter().position(|b| b.id == MESH).unwrap()];
    assert_eq!(mesh["envelope_weights"], true);
    assert_eq!(mesh["max_influences"], 2);
    assert_eq!(mesh["locked_verts"], serde_json::json!([5]));
}

#[test]
fn clear_all_binds_starts_the_skinning_over() {
    let mut a = arm();
    a.bones[MESH as usize].parent_id = UPPER;
    a.bones[UPPER as usize].rot = 0.4;
    let before = drawn(&a);
    skin::bind_to_skeleton(&mut a, MESH).unwrap();
    a.bones.iter_mut().find(|b| b.id == MESH).unwrap().locked_verts = vec![0, 1];

    skin::clear_all_binds(&mut a, MESH);
    let mesh = find(&a, MESH);
    assert!(mesh.binds.is_empty() && !mesh.bind_pose && !mesh.envelope_weights);
    assert!(mesh.locked_verts.is_empty());
    assert!(a.bones.iter().all(|b| b.bind_owner.is_none()), "helpers left behind");
    for (p, q) in drawn(&a).iter().zip(&before) {
        assert!((*p - *q).mag() < 1e-3, "art moved: {p:?} vs {q:?}");
    }
    // and it can be skinned again
    skin::bind_to_skeleton(&mut a, MESH).unwrap();
    assert!(!find(&a, MESH).binds.is_empty());
}
