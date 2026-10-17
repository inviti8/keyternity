//! Hand weighting on normalized shares, and weight painting (docs/ENVELOPES.md §4.5).

use keyternity_lib::renderer::construction;
use keyternity_lib::shared::*;
use keyternity_lib::skin;
use keyternity_lib::weights::{self, PaintMode};

fn bone(id: i32, parent_id: i32, name: &str, pos: Vec2) -> Bone {
    Bone {
        id,
        parent_id,
        name: name.to_string(),
        pos,
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

/// An arm skinned with Bind to Skeleton: upper (0..100) and lower (100..200) bones
/// along +x under a strip mesh of 21 columns of 2 vertices at y = ±10.
fn skinned_arm() -> Armature {
    let mut upper = bone(UPPER, -1, "upper", v(0., 0.));
    upper.length = 100.;
    let mut lower = bone(LOWER, UPPER, "lower", v(100., 0.));
    lower.length = 100.;
    let mut mesh = bone(MESH, -1, "arm art", v(0., 0.));
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
    let mut a = Armature { bones: vec![upper, lower, mesh], ..Default::default() };
    skin::bind_to_skeleton(&mut a, MESH).unwrap();
    a
}

fn mesh(a: &Armature) -> &Bone {
    a.bones.iter().find(|b| b.id == MESH).unwrap()
}

fn share(a: &Armature, vert: u32, bone: i32) -> f32 {
    let all = skin::vertex_shares(&a.bones, mesh(a));
    weights::share_of(&all[&vert], bone)
}

fn sums_to_one(a: &Armature) {
    for (vid, s) in skin::vertex_shares(&a.bones, mesh(a)) {
        let sum: f32 = s.iter().map(|x| x.1).sum();
        assert!((sum - 1.).abs() < 1e-4, "vertex {vid}: shares sum {sum} ({s:?})");
    }
}

fn drawn(a: &Armature) -> Vec<Vec2> {
    let mut w = a.bones.clone();
    construction(&mut w, &a.bones);
    w.iter().find(|b| b.id == MESH).unwrap().vertices.iter().map(|v| v.pos).collect()
}

#[test]
fn setting_a_share_scales_the_others() {
    let mut s = vec![(1, 0.5), (2, 0.3), (3, 0.2)];
    weights::set_share(&mut s, 1, 0.8, 9);
    assert!((weights::share_of(&s, 1) - 0.8).abs() < 1e-5);
    assert!((weights::share_of(&s, 2) - 0.12).abs() < 1e-5, "{s:?}");
    assert!((weights::share_of(&s, 3) - 0.08).abs() < 1e-5, "{s:?}");

    // nothing else to take the rest: the fallback gets it
    let mut s = vec![(1, 1.)];
    weights::set_share(&mut s, 1, 0.25, 9);
    assert!((weights::share_of(&s, 9) - 0.75).abs() < 1e-5, "{s:?}");

    // a new bone at 1 takes everything
    let mut s = vec![(1, 0.6), (2, 0.4)];
    weights::set_share(&mut s, 5, 1., 9);
    assert_eq!(s, vec![(5, 1.)]);
}

#[test]
fn limit_drops_the_weakest_other_bones() {
    let mut s = vec![(0, 0.1), (1, 0.4), (2, 0.05), (3, 0.3), (4, 0.15)];
    weights::limit(&mut s, 2, 2, 0);
    let bones: Vec<i32> = s.iter().map(|x| x.0).collect();
    assert!(bones.contains(&2) && bones.contains(&1) && bones.contains(&0), "{s:?}");
    assert_eq!(bones.len(), 3);
    let sum: f32 = s.iter().map(|x| x.1).sum();
    assert!((sum - 1.).abs() < 1e-5);
}

#[test]
fn full_share_in_two_binds_is_not_double() {
    // the elbow is shared; hand-set it fully to upper, then fully to lower
    let mut a = skinned_arm();
    let elbow = 20;
    skin::edit_shares(&mut a, MESH, UPPER, &[(elbow, 1.)], false);
    assert!((share(&a, elbow, UPPER) - 1.).abs() < 1e-4);
    assert!(share(&a, elbow, LOWER) < 1e-4);
    skin::edit_shares(&mut a, MESH, LOWER, &[(elbow, 1.)], false);
    assert!((share(&a, elbow, LOWER) - 1.).abs() < 1e-4);
    assert!(share(&a, elbow, UPPER) < 1e-4);
    sums_to_one(&a);
    assert!(mesh(&a).locked_verts.contains(&elbow), "hand edit didn't lock");
}

#[test]
fn editing_shares_leaves_the_art_in_place() {
    let mut a = skinned_arm();
    let before = drawn(&a);
    skin::edit_shares(&mut a, MESH, UPPER, &[(20, 0.9), (30, 0.7), (2, 0.)], false);
    skin::maintain(&mut a, false);
    for (p, q) in before.iter().zip(drawn(&a)) {
        assert!((*p - q).mag() < 1e-3, "{p:?} moved to {q:?}");
    }
    sums_to_one(&a);
}

#[test]
fn added_vertices_survive_adding_the_next() {
    // double-clicking vertices into a bind one after another (each a full share)
    // used to drop earlier ones when the live recompute ran
    let mut a = skinned_arm();
    let picks = [30u32, 32, 34, 36];
    for (n, vid) in picks.iter().enumerate() {
        skin::edit_shares(&mut a, MESH, UPPER, &[(*vid, 1.)], false);
        skin::maintain(&mut a, false);
        for earlier in &picks[..=n] {
            assert!((share(&a, *earlier, UPPER) - 1.).abs() < 1e-4, "vertex {earlier} lost after adding {vid}");
        }
    }
}

#[test]
fn a_slider_dragged_to_zero_keeps_its_row() {
    let mut a = skinned_arm();
    let upper_bind = mesh(&a).binds.iter().position(|b| skin::bind_bone(&a.bones, MESH, b) == UPPER).unwrap();
    let rows = |a: &Armature| mesh(a).binds[upper_bind].verts.iter().map(|bv| bv.id).collect::<Vec<i32>>();
    let before = rows(&a);
    assert!(before.contains(&20));

    skin::edit_shares(&mut a, MESH, UPPER, &[(20, 0.)], true);
    skin::maintain(&mut a, false);
    assert_eq!(rows(&a), before, "rows changed");
    assert!(share(&a, 20, UPPER) < 1e-6);

    // without keep (painting, or removing a vertex) it leaves the bind
    skin::edit_shares(&mut a, MESH, UPPER, &[(22, 0.)], false);
    assert!(!rows(&a).contains(&22));
}

#[test]
fn painting_adds_subtracts_and_smooths() {
    let mut a = skinned_arm();
    let vid = 30; // on lower
    let start = share(&a, vid, UPPER);
    skin::paint(&mut a, MESH, UPPER, &[(vid, 1.)], PaintMode::Add, 1.);
    let added = share(&a, vid, UPPER);
    assert!((added - start - weights::DAB_RATE).abs() < 1e-4, "{start} -> {added}");
    skin::paint(&mut a, MESH, UPPER, &[(vid, 1.)], PaintMode::Subtract, 0.5);
    let sub = share(&a, vid, UPPER);
    assert!((added - sub - weights::DAB_RATE * 0.5).abs() < 1e-4, "{added} -> {sub}");
    sums_to_one(&a);

    // a spike smooths toward its neighbours
    skin::edit_shares(&mut a, MESH, UPPER, &[(40, 1.)], false);
    let spike = share(&a, 40, UPPER);
    skin::paint(&mut a, MESH, UPPER, &[(40, 1.)], PaintMode::Smooth, 1.);
    assert!(share(&a, 40, UPPER) < spike - 0.1, "smooth didn't lower the spike");
    sums_to_one(&a);
}

#[test]
fn brush_falloff_eases_to_the_edge() {
    assert_eq!(weights::falloff(0., 10.), 1.);
    assert!(weights::falloff(5., 10.) > 0.5);
    assert_eq!(weights::falloff(10., 10.), 0.);
    assert_eq!(weights::falloff(12., 10.), 0.);
}
