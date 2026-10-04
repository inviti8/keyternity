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
