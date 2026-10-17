//! Generating the face from landmarks (docs/LIP_SYNC.md §6.7).

use keyternity_lib::face;
use keyternity_lib::face_gen::{self, Cylinder};
use keyternity_lib::shared::*;
use std::collections::HashMap;
use keyternity_lib::editor;

fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).mag() < 1e-3
}

#[test]
fn the_symmetry_axis_follows_a_tilted_head() {
    // midline points along a line tilted 10° from vertical
    let t = 10f32.to_radians();
    let dir = Vec2::new(t.sin(), t.cos());
    let pts: Vec<Vec2> = [-50., -10., 30., 80.].iter().map(|s| Vec2::new(5., 2.) + dir * *s).collect();
    let (o, d) = face_gen::symmetry_axis(&pts).unwrap();
    assert!((d.x - dir.x).abs() < 1e-4 && (d.y - dir.y).abs() < 1e-4, "{:?}", d);
    // a point reflected across it lands at the same distance, other side
    let p = o + dir * 20. + Vec2::new(dir.y, -dir.x) * 30.;
    let q = face_gen::reflect(p, o, d);
    assert!(close(q, o + dir * 20. - Vec2::new(dir.y, -dir.x) * 30.));
}

fn front_face() -> HashMap<String, Vec2> {
    let mut m = HashMap::new();
    for (n, p) in [
        ("UpperLipMid", (0., -40.)),
        ("LowerLipMid", (0., -50.)),
        ("NoseTip", (0., 0.)),
        ("Chin", (0., -90.)),
        ("MouthCorner_L", (30., -45.)),
        ("UpperLip_L1", (10., -41.)),
        ("Brow_L_Out", (55., 60.)),
    ] {
        m.insert(n.to_string(), Vec2::new(p.0, p.1));
    }
    m
}

#[test]
fn front_views_mirror_across_the_midline() {
    let known = front_face();
    let derived = face_gen::derive_side(&known, None);
    assert!(close(derived["MouthCorner_R"], Vec2::new(-30., -45.)));
    assert!(close(derived["UpperLip_R1"], Vec2::new(-10., -41.)));
    assert!(close(derived["Brow_R_Out"], Vec2::new(-55., 60.)));
    assert!(!derived.contains_key("NoseTip"), "midline points have no twin");
}

#[test]
fn three_quarter_views_follow_a_turned_cylinder() {
    // the truth: a head of radius 100 turned 30°, centred at 0
    let truth = Cylinder { r: 100., theta: 30f32.to_radians(), centre: 0. };
    let at = |phi: f32| truth.centre + truth.r * (phi + truth.theta).sin();
    let mid = at(0.);
    let corner = 25f32.to_radians();
    let (near, far) = (at(corner), at(-corner));
    // solve from the mouth corners
    let c = Cylinder::from_pair(near, far, mid, 100.).unwrap();
    assert!((c.theta - truth.theta).abs() < 0.01, "{}", c.theta.to_degrees());
    // other features land where the truth puts them, squeezed more far from the midline
    for deg in [10f32, 40., 55.] {
        let phi = deg.to_radians();
        assert!((c.twin(at(phi)) - at(-phi)).abs() < 0.5, "{deg}°: {} vs {}", c.twin(at(phi)), at(-phi));
    }
    // turned +30°, the +φ side turns away from the viewer: it's the squeezed one
    assert!(at(55f32.to_radians()) - mid < mid - at(-55f32.to_radians()), "the far side is squeezed");
}

#[test]
fn the_view_is_suggested_from_the_corners() {
    let mut known = front_face();
    known.insert("MouthCorner_R".into(), Vec2::new(-30., -45.));
    assert_eq!(face_gen::suggest_view(&known), FaceView::Front);
    known.insert("MouthCorner_R".into(), Vec2::new(-15., -45.));
    assert_eq!(face_gen::suggest_view(&known), FaceView::ThreeQuarter);
}

#[test]
fn right_side_templates_mirror_the_left() {
    let l = face_gen::template_for("Smile_L").unwrap();
    let r = face_gen::template_for("Smile_R").unwrap();
    assert_eq!(l.len(), r.len());
    assert_eq!(r[0].landmark, "MouthCorner_R");
    assert_eq!((r[0].out, r[0].up), (l[0].out, l[0].up), "out stays outward");
    assert!(face_gen::template_for("Nope").is_none());
}

/// a 21×21 grid face over -100..100, with the landmarks on grid vertices
fn grid_face() -> Bone {
    let n = 21;
    let step = 10.;
    let mut vertices = vec![];
    for j in 0..n {
        for i in 0..n {
            let id = (j * n + i) as u32;
            vertices.push(Vertex { id, pos: Vec2::new(-100. + i as f32 * step, -100. + j as f32 * step), ..Default::default() });
        }
    }
    let mut indices = vec![];
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let a = (j * n + i) as u32;
            let (b, c, d) = (a + 1, a + n as u32 + 1, a + n as u32);
            indices.extend([a, b, c, a, c, d]);
        }
    }
    let at = |x: f32, y: f32| ((((y + 100.) / step).round() as usize) * n + ((x + 100.) / step).round() as usize) as u32;
    let points = [
        ("UpperLipMid", (0., -30.)),
        ("LowerLipMid", (0., -50.)),
        ("NoseTip", (0., 10.)),
        ("Chin", (0., -80.)),
        ("MouthCorner_L", (30., -40.)),
        ("MouthCorner_R", (-30., -40.)),
        ("UpperLip_L1", (10., -30.)),
        ("UpperLip_L2", (20., -30.)),
        ("UpperLip_R1", (-10., -30.)),
        ("UpperLip_R2", (-20., -30.)),
        ("LowerLip_L1", (10., -50.)),
        ("LowerLip_L2", (20., -50.)),
        ("LowerLip_R1", (-10., -50.)),
        ("LowerLip_R2", (-20., -50.)),
        ("Brow_L_In", (20., 60.)),
        ("Brow_L_Mid", (40., 70.)),
        ("Brow_L_Out", (60., 60.)),
    ]
    .iter()
    .map(|(name, (x, y))| LandmarkPoint { name: name.to_string(), vert: at(*x, *y), derived: false })
    .collect();
    Bone { id: 1, vertices, indices, verts_edited: true, face_map: Some(FaceMap { view: FaceView::Front, points }), ..Default::default() }
}

fn delta_at(deltas: &[ShapeDelta], bone: &Bone, x: f32, y: f32) -> Vec2 {
    let v = bone.vertices.iter().find(|v| (v.pos - Vec2::new(x, y)).mag() < 0.1).unwrap();
    deltas.iter().find(|d| d.vert == v.id).map_or(Vec2::ZERO, |d| d.delta)
}

#[test]
fn opening_the_mouth_moves_the_lower_lip_and_chin_only() {
    let bone = grid_face();
    let d = face_gen::generate(&bone, bone.face_map.as_ref().unwrap(), "MouthOpen", 1.).unwrap();
    assert!(delta_at(&d, &bone, 0., -50.).y < -10., "the lower lip drops: {:?}", delta_at(&d, &bone, 0., -50.));
    assert!(delta_at(&d, &bone, 0., -80.).y < -5., "the chin follows");
    assert!(delta_at(&d, &bone, 0., -20.).mag() < 0.5, "above the lips stays: {:?}", delta_at(&d, &bone, 0., -20.));
    assert_eq!(delta_at(&d, &bone, 40., 70.), Vec2::ZERO, "the brows don't move");
}

#[test]
fn a_smile_lifts_its_own_corner() {
    let bone = grid_face();
    let d = face_gen::generate(&bone, bone.face_map.as_ref().unwrap(), "Smile_L", 1.).unwrap();
    let corner = delta_at(&d, &bone, 30., -40.);
    assert!(corner.x > 1. && corner.y > 1., "out and up: {:?}", corner);
    assert!(delta_at(&d, &bone, -30., -40.).mag() < 0.5, "the other corner stays");
    // "Smile_Left" is the same shape
    assert!(face_gen::generate(&bone, bone.face_map.as_ref().unwrap(), "Smile_Left", 1.).is_some());
}

#[test]
fn regenerating_keeps_the_artists_corrections() {
    let mut shape = MeshShape { id: 1, name: "Smile_L".into(), ..Default::default() };
    face_gen::regenerate(&mut shape, vec![ShapeDelta { vert: 1, delta: Vec2::new(1., 1.) }]);
    // the artist pushes vertex 1 further, and adds vertex 2
    shape.deltas = vec![ShapeDelta { vert: 1, delta: Vec2::new(1., 3.) }, ShapeDelta { vert: 2, delta: Vec2::new(0., 5.) }];
    // regenerate with a stronger template
    face_gen::regenerate(&mut shape, vec![ShapeDelta { vert: 1, delta: Vec2::new(2., 2.) }]);
    let d = |v: u32| shape.deltas.iter().find(|d| d.vert == v).unwrap().delta;
    assert_eq!(d(1), Vec2::new(2., 4.), "new generated + the correction (0, 2)");
    assert_eq!(d(2), Vec2::new(0., 5.));
}

#[test]
fn generate_face_builds_the_set() {
    let mut bone = grid_face();
    let n = face_gen::generate_face(&mut bone, true, 1.);
    assert!(n >= 20, "{n}");
    let open = face::find_shape(&bone, "MouthOpen").unwrap();
    assert!(!bone.shapes.iter().find(|s| s.id == open).unwrap().deltas.is_empty());
    // the grid has no lip seam: the check says so
    assert!(face_gen::checks(&bone, bone.face_map.as_ref().unwrap()).contains(&"lip_seam"));
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
fn mapping_one_side_deriving_and_generating_through_events() {
    let mut s = Shared::default();
    let mut bone = grid_face();
    // keep only the midline and the left side: the right is derived
    let full = bone.face_map.take().unwrap();
    s.armature.bones = vec![bone];
    for p in full.points.iter().filter(|p| !p.name.ends_with("_R") && !p.name.contains("_R1") && !p.name.contains("_R2")) {
        s.events.face_landmarks(1, &format!("map|{}|{}", p.name, p.vert));
    }
    s.events.face_landmarks(1, "derive");
    run(&mut s);
    let map = s.armature.bones[0].face_map.clone().unwrap();
    let corner = map.points.iter().find(|p| p.name == "MouthCorner_R").unwrap();
    assert!(corner.derived);
    let truth = full.points.iter().find(|p| p.name == "MouthCorner_R").unwrap();
    assert_eq!(corner.vert, truth.vert, "mirrored onto the right vertex");

    s.events.face_landmarks(1, "generate|0|1");
    run(&mut s);
    let open = face::find_shape(&s.armature.bones[0], "MouthOpen").unwrap();
    assert!(!s.armature.bones[0].shapes.iter().find(|x| x.id == open).unwrap().deltas.is_empty());

    s.events.undo();
    run(&mut s);
    assert!(s.armature.bones[0].shapes.is_empty(), "undo takes the generated shapes back");
}
