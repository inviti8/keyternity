//! Eraser picking and erasing (docs/TOPOLOGY_TOOLS.md §6).

use skelform_lib::mesh_tools::{self, Pick};
use skelform_lib::shared::*;

const WINDOW: Vec2 = Vec2 { x: 1000., y: 1000. };

/// Renderer-space vertex at screen pixel (x, y) of a 1000x1000 window.
fn at(x: f32, y: f32) -> Vertex {
    Vertex {
        pos: Vec2::new(x / 500. - 1., 1. - y / 500.),
        ..Default::default()
    }
}

fn mouse(x: f32, y: f32) -> Vec2 {
    at(x, y).pos
}

/// A 200 px square at (100, 100) split along 0-2.
fn square() -> (Vec<Vertex>, Vec<u32>) {
    let verts = vec![at(100., 100.), at(300., 100.), at(300., 300.), at(100., 300.)];
    (verts, vec![0, 1, 2, 0, 2, 3])
}

#[test]
fn to_screen_round_trips() {
    let p = mesh_tools::to_screen(at(123., 456.).pos, WINDOW);
    assert!((p.x - 123.).abs() < 1e-3 && (p.y - 456.).abs() < 1e-3);
}

#[test]
fn picks_the_nearest_vertex() {
    let (verts, indices) = square();
    let pick = mesh_tools::pick(&verts, &indices, mouse(296., 104.), WINDOW);
    assert_eq!(pick, Some(Pick::Vertex(1)));
}

#[test]
fn vertex_wins_over_its_edges() {
    let (verts, indices) = square();
    // on edge 0-1, but within reach of vertex 0
    let pick = mesh_tools::pick(&verts, &indices, mouse(106., 100.), WINDOW);
    assert_eq!(pick, Some(Pick::Vertex(0)));
}

#[test]
fn picks_an_edge() {
    let (verts, indices) = square();
    assert_eq!(mesh_tools::pick(&verts, &indices, mouse(200., 103.), WINDOW), Some(Pick::Edge(0, 1)));
    assert_eq!(mesh_tools::pick(&verts, &indices, mouse(202., 198.), WINDOW), Some(Pick::Edge(0, 2)));
}

#[test]
fn nothing_inside_a_face_or_outside() {
    let (verts, indices) = square();
    assert_eq!(mesh_tools::pick(&verts, &indices, mouse(260., 150.), WINDOW), None);
    assert_eq!(mesh_tools::pick(&verts, &indices, mouse(600., 600.), WINDOW), None);
}

fn quad_bone() -> Bone {
    let uv = |u: f32, v: f32, id: u32| Vertex {
        id,
        pos: Vec2::new(u * 100., -v * 100.),
        uv: Vec2::new(u, v),
        ..Default::default()
    };
    Bone {
        vertices: vec![uv(0., 0., 10), uv(1., 0., 11), uv(1., 1., 12), uv(0., 1., 13)],
        indices: vec![0, 1, 2, 0, 2, 3],
        ..Default::default()
    }
}

#[test]
fn erase_dissolves_or_deletes() {
    // dissolving the diagonal flips it
    let mut bone = quad_bone();
    mesh_tools::erase(&mut bone, Pick::Edge(0, 2), false).unwrap();
    assert_eq!(bone.indices.len(), 6);
    assert!(bone.indices.chunks_exact(3).all(|t| t.contains(&1) && t.contains(&3)));

    // deleting it removes both triangles, which here would empty the mesh
    let mut bone = quad_bone();
    let err = mesh_tools::erase(&mut bone, Pick::Edge(0, 2), true);
    assert_eq!(err, Err(skelform_lib::topology::TopoError::LastTriangle));
    assert_eq!(bone.indices.len(), 6);

    // deleting a corner removes its one triangle
    let mut bone = quad_bone();
    mesh_tools::erase(&mut bone, Pick::Vertex(1), true).unwrap();
    assert_eq!(bone.indices.len(), 3);
    assert_eq!(bone.vertices.len(), 3);
}

// --- Pen targeting ---

use skelform_lib::mesh_tools::{PenSnapping, PenTarget, UvMap};
use skelform_lib::pen::Snap;

/// The 200 px square, carrying UVs: (100,100) is uv (0,0), (300,300) is (1,1).
fn textured_square() -> (Vec<Vertex>, Vec<u32>) {
    let (mut verts, indices) = square();
    let uvs = [(0., 0.), (1., 0.), (1., 1.), (0., 1.)];
    for (v, (u, w)) in verts.iter_mut().zip(uvs) {
        v.uv = Vec2::new(u, w);
    }
    (verts, indices)
}

fn snap_on() -> PenSnapping {
    PenSnapping { snap: true, midpoint: false, angle_lock: false }
}

fn close_to(a: Vec2, b: Vec2) -> bool {
    (a - b).mag() < 1e-4
}

#[test]
fn uv_map_round_trips_inside_and_outside() {
    let (verts, indices) = textured_square();
    let map = UvMap::new(&verts, &indices);
    // inside: exact
    assert!(close_to(map.uv_at(mouse(150., 250.)).unwrap(), Vec2::new(0.25, 0.75)));
    // outside: from the fitted layout
    assert!(close_to(map.uv_at(mouse(400., 200.)).unwrap(), Vec2::new(1.5, 0.5)));
    assert!(close_to(map.pos_of(Vec2::new(1.5, 0.5)).unwrap(), mouse(400., 200.)));
}

#[test]
fn pen_snaps_to_vertices_then_edges() {
    let (verts, indices) = textured_square();
    let map = UvMap::new(&verts, &indices);
    let target = |x, y, s| mesh_tools::pen_target(&verts, &indices, &map, mouse(x, y), WINDOW, &[], s);

    match target(298., 302., snap_on()) {
        Some(PenTarget::Place(p, _)) => assert_eq!(p.snap, Snap::Vertex(2)),
        t => panic!("{t:?}"),
    }
    match target(150., 103., snap_on()) {
        Some(PenTarget::Place(p, pos)) => {
            assert_eq!(p.snap, Snap::Edge);
            assert!(close_to(p.uv, Vec2::new(0.25, 0.)));
            assert!(close_to(pos, mouse(150., 100.)), "drawn on the edge");
        }
        t => panic!("{t:?}"),
    }
    let midpoint = PenSnapping { midpoint: true, ..snap_on() };
    match target(150., 103., midpoint) {
        Some(PenTarget::Place(p, _)) => assert!(close_to(p.uv, Vec2::new(0.5, 0.))),
        t => panic!("{t:?}"),
    }
    // snapping off: a free point where the cursor is
    match target(150., 103., PenSnapping::default()) {
        Some(PenTarget::Place(p, _)) => {
            assert_eq!(p.snap, Snap::Free);
            assert!(close_to(p.uv, Vec2::new(0.25, 0.015)));
        }
        t => panic!("{t:?}"),
    }
}

#[test]
fn pen_free_points_stay_on_the_texture() {
    let (verts, indices) = textured_square();
    let map = UvMap::new(&verts, &indices);
    let t = mesh_tools::pen_target(&verts, &indices, &map, mouse(500., 200.), WINDOW, &[], snap_on());
    match t {
        Some(PenTarget::Place(p, pos)) => {
            assert!(close_to(p.uv, Vec2::new(1., 0.5)), "clamped to the texture's edge");
            assert!(close_to(pos, mouse(300., 200.)));
        }
        t => panic!("{t:?}"),
    }
}

#[test]
fn pen_closes_and_finishes_cuts() {
    let (verts, indices) = textured_square();
    let map = UvMap::new(&verts, &indices);
    let active = [mouse(150., 150.), mouse(250., 150.), mouse(250., 250.)];
    let target = |x, y| mesh_tools::pen_target(&verts, &indices, &map, mouse(x, y), WINDOW, &active, snap_on());
    assert_eq!(target(152., 151.), Some(PenTarget::Close));
    assert_eq!(target(249., 252.), Some(PenTarget::Finish));
    // two points can't close yet: the first point is just a place
    let two = &active[..2];
    let t = mesh_tools::pen_target(&verts, &indices, &map, mouse(152., 151.), WINDOW, two, snap_on());
    assert!(matches!(t, Some(PenTarget::Place(..))));
}

#[test]
fn pen_angle_lock_keeps_45_degree_steps() {
    let (verts, indices) = textured_square();
    let map = UvMap::new(&verts, &indices);
    let active = [mouse(150., 150.)];
    let lock = PenSnapping { snap: false, midpoint: false, angle_lock: true };
    // nearly diagonal: snaps onto the 45° line
    let t = mesh_tools::pen_target(&verts, &indices, &map, mouse(210., 200.), WINDOW, &active, lock);
    match t {
        Some(PenTarget::Place(_, pos)) => {
            let px = mesh_tools::to_screen(pos, WINDOW);
            assert!(((px.x - 150.) - (px.y - 150.)).abs() < 1e-2, "{px:?}");
        }
        t => panic!("{t:?}"),
    }
}
