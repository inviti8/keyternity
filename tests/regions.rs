//! Region weighting for the AI skinning tools and the lasso (docs/AI_SKINNING.md §4.1).

use keyternity_lib::renderer::construction;
use keyternity_lib::shared::*;
use keyternity_lib::skin;
use keyternity_lib::weights;

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

/// An outline covering x from `x0` to `x1` across the strip.
fn band(x0: f32, x1: f32) -> Vec<Vec2> {
    vec![v(x0, -50.), v(x1, -50.), v(x1, 50.), v(x0, 50.)]
}

/// Vertex id of the bottom vertex in the column at x (columns every 10).
fn at(x: f32) -> u32 {
    (x / 10.) as u32 * 2
}

#[test]
fn polygons_and_geodesic_distance() {
    let square = vec![v(0., 0.), v(10., 0.), v(10., 10.), v(0., 10.)];
    assert!(weights::in_polygon(v(5., 5.), &square));
    assert!(!weights::in_polygon(v(15., 5.), &square));
    assert!((weights::polygon_edge_distance(v(5., 2.), &square) - 2.).abs() < 1e-5);

    // two separate segments: distance doesn't jump the gap
    let pos = vec![v(0., 0.), v(10., 0.), v(12., 0.), v(22., 0.)];
    let d = weights::geodesic(&pos, &[(0, 1), (2, 3)], &[0]);
    assert_eq!(d[1], 10.);
    assert_eq!(d[2], f32::MAX);
}

#[test]
fn assign_parts_splits_and_blends_along_the_mesh() {
    let mut a = skinned_arm();
    // hand both halves to the opposite bones, to see the parts win over envelopes
    let parts = vec![(LOWER, band(-5., 95.)), (UPPER, band(95., 205.))];
    let n = skin::assign_parts(&mut a, MESH, &parts, 30.);
    assert_eq!(n, 42);
    assert!((share(&a, at(0.), LOWER) - 1.).abs() < 1e-4);
    assert!((share(&a, at(200.), UPPER) - 1.).abs() < 1e-4);
    let near = share(&a, at(90.), LOWER);
    assert!(near > 0.5 && near < 0.95, "boundary blend {near}");
    sums_to_one(&a);
    assert!(mesh(&a).locked_verts.contains(&at(0.)));

    // no blend: a hard seam
    let mut a = skinned_arm();
    skin::assign_parts(&mut a, MESH, &parts, 0.);
    assert!((share(&a, at(90.), LOWER) - 1.).abs() < 1e-4);
    assert!((share(&a, at(100.), UPPER) - 1.).abs() < 1e-4);
}

#[test]
fn set_region_feathers_from_the_edge() {
    let mut a = skinned_arm();
    let n = skin::set_region(&mut a, MESH, &band(-5., 105.), LOWER, 1., 40.);
    assert_eq!(n, 22);
    let deep = share(&a, at(50.), LOWER);
    let edge = share(&a, at(0.), LOWER);
    assert!((deep - 1.).abs() < 1e-4, "inside {deep}");
    assert!(edge < 0.5, "edge {edge}");
    sums_to_one(&a);
}

#[test]
fn smoothing_all_bones_softens_a_seam() {
    let mut a = skinned_arm();
    skin::assign_parts(&mut a, MESH, &[(UPPER, band(-5., 95.)), (LOWER, band(95., 205.))], 0.);
    let ids = skin::vertices_in(&a, MESH, Some(&band(65., 135.)));
    skin::smooth_vertices(&mut a, MESH, &ids, None, 4);
    let s = share(&a, at(90.), LOWER);
    assert!(s > 0.05 && s < 0.6, "seam after blur {s}");
    sums_to_one(&a);

    // one bone only
    let before = share(&a, at(100.), UPPER);
    skin::smooth_vertices(&mut a, MESH, &[at(100.)], Some(UPPER), 1);
    assert!(share(&a, at(100.), UPPER) != before);
}

#[test]
fn blend_joint_eases_across_the_joint() {
    let mut a = skinned_arm();
    skin::assign_parts(&mut a, MESH, &[(UPPER, band(-5., 95.)), (LOWER, band(95., 205.))], 0.);
    let n = skin::blend_joint(&mut a, MESH, UPPER, LOWER, 60.);
    assert!(n > 0);
    assert!((share(&a, at(100.), LOWER) - 0.5).abs() < 1e-3);
    assert!(share(&a, at(80.), LOWER) < 0.3);
    assert!(share(&a, at(120.), LOWER) > 0.7);
    // outside the band: untouched
    assert!((share(&a, at(50.), UPPER) - 1.).abs() < 1e-4);
    sums_to_one(&a);
}
