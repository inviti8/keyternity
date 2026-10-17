//! Local mesh topology edits (docs/TOPOLOGY_TOOLS.md §4, §6, §9).

use keyternity_lib::shared::*;
use keyternity_lib::topology::{self, TopoError, VertSite};

const EPS: f32 = 1e-5;

fn vertex(id: u32, u: f32, v: f32) -> Vertex {
    Vertex {
        id,
        pos: Vec2::new(u * 100., -v * 100.),
        uv: Vec2::new(u, v),
        ..Default::default()
    }
}

fn mesh(verts: Vec<Vertex>, indices: Vec<u32>) -> Bone {
    Bone {
        vertices: verts,
        indices,
        verts_edited: true,
        ..Default::default()
    }
}

/// Unit quad: 0 (0,0), 1 (1,0), 2 (1,1), 3 (0,1); diagonal 0-2.
fn quad() -> Bone {
    let verts = vec![vertex(10, 0., 0.), vertex(11, 1., 0.), vertex(12, 1., 1.), vertex(13, 0., 1.)];
    mesh(verts, vec![0, 1, 2, 0, 2, 3])
}

/// 3x3 grid of vertices (2x2 cells, each split along its diagonal); vertex 4 is the centre.
fn grid() -> Bone {
    let mut verts = vec![];
    for y in 0..3 {
        for x in 0..3 {
            verts.push(vertex(100 + y * 3 + x, x as f32 / 2., y as f32 / 2.));
        }
    }
    let mut indices = vec![];
    for y in 0..2 {
        for x in 0..2 {
            let a = y * 3 + x;
            indices.extend([a, a + 1, a + 4, a, a + 4, a + 3]);
        }
    }
    mesh(verts, indices)
}

fn signed_area(bone: &Bone, t: &[u32]) -> f32 {
    let uv = |i: u32| bone.vertices[i as usize].uv;
    let (a, b, c) = (uv(t[0]), uv(t[1]), uv(t[2]));
    ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)) / 2.
}

fn total_area(bone: &Bone) -> f32 {
    bone.indices.chunks_exact(3).map(|t| signed_area(bone, t).abs()).sum()
}

/// Every triangle winds the same way and is non-degenerate.
fn assert_consistent(bone: &Bone) {
    let signs: Vec<f32> =
        bone.indices.chunks_exact(3).map(|t| signed_area(bone, t)).collect();
    assert!(signs.iter().all(|s| s.abs() > EPS), "degenerate triangle: {:?}", bone.indices);
    let first = signs[0].signum();
    assert!(signs.iter().all(|s| s.signum() == first), "mixed winding: {:?}", bone.indices);
    for i in &bone.indices {
        assert!((*i as usize) < bone.vertices.len());
    }
    // every vertex is in a triangle (no orphans)
    for v in 0..bone.vertices.len() as u32 {
        assert!(bone.indices.contains(&v), "orphan vertex {v}");
    }
}

fn ids(bone: &Bone) -> Vec<u32> {
    bone.vertices.iter().map(|v| v.id).collect()
}

fn tri_set(bone: &Bone) -> Vec<[u32; 3]> {
    let mut tris: Vec<[u32; 3]> = bone
        .indices
        .chunks_exact(3)
        .map(|t| {
            let mut ids = [t[0], t[1], t[2]].map(|i| bone.vertices[i as usize].id);
            ids.sort();
            ids
        })
        .collect();
    tris.sort();
    tris
}

// --- add_vertex ---

#[test]
fn split_triangle_into_three() {
    let mut bone = quad();
    let new = vertex(0, 0.75, 0.25);
    let n = topology::add_vertex(&mut bone, new, VertSite::Tri(0)).unwrap();

    assert_eq!(n, 4);
    assert_eq!(bone.indices.len(), 4 * 3);
    assert_eq!(&ids(&bone)[..4], &[10, 11, 12, 13], "existing ids and order are kept");
    assert!(!ids(&bone)[..4].contains(&bone.vertices[4].id), "new id is unique");
    assert_eq!(&bone.indices[9..12], &[0, 2, 3], "untouched triangle is unchanged");
    assert!((total_area(&bone) - 1.).abs() < EPS);
    assert_consistent(&bone);
}

#[test]
fn split_shared_edge_splits_both_triangles() {
    let mut bone = quad();
    let new = vertex(0, 0.5, 0.5);
    topology::add_vertex(&mut bone, new, VertSite::Edge(0, 2)).unwrap();

    assert_eq!(bone.indices.len(), 4 * 3);
    assert!((total_area(&bone) - 1.).abs() < EPS);
    assert_consistent(&bone);
}

#[test]
fn split_boundary_edge_splits_one_triangle() {
    let mut bone = quad();
    let new = vertex(0, 0.5, 0.);
    topology::add_vertex(&mut bone, new, VertSite::Edge(0, 1)).unwrap();

    assert_eq!(bone.indices.len(), 3 * 3);
    assert!((total_area(&bone) - 1.).abs() < EPS);
    assert_consistent(&bone);
}

#[test]
fn add_vertex_keeps_untouched_topology() {
    // the regression this design exists to fix: a split doesn't re-triangulate
    let mut bone = grid();
    let mut untouched = tri_set(&bone);
    untouched.retain(|t| *t != [100, 101, 104]); // triangle 0, the one being split
    topology::add_vertex(&mut bone, vertex(0, 0.25, 0.1), VertSite::Tri(0)).unwrap();
    let after = tri_set(&bone);
    for t in &untouched {
        assert!(after.contains(t), "triangle {t:?} was lost");
    }
    assert_eq!(after.len(), untouched.len() + 3);
    assert_consistent(&bone);
}

#[test]
fn add_vertex_bad_site() {
    let mut bone = quad();
    let v = vertex(0, 0.5, 0.5);
    assert_eq!(topology::add_vertex(&mut bone, v, VertSite::Tri(9)), Err(TopoError::NotFound));
    assert_eq!(topology::add_vertex(&mut bone, v, VertSite::Edge(1, 3)), Err(TopoError::NotFound));
    assert_eq!(topology::add_vertex(&mut bone, v, VertSite::None), Err(TopoError::NotFound));
    assert_eq!(bone.vertices.len(), 4);
}

#[test]
fn bind_posed_mesh_interpolates_weights() {
    let mut bone = quad();
    bone.bind_pose = true;
    bone.binds = vec![
        BoneBind {
            bone_id: 1,
            is_path: false,
            verts: vec![BoneBindVert { id: 10, weight: 1. }, BoneBindVert { id: 11, weight: 0.5 }],
        },
        BoneBind {
            bone_id: 2,
            is_path: false,
            verts: vec![BoneBindVert { id: 12, weight: 1. }],
        },
    ];
    let n = topology::add_vertex(&mut bone, vertex(0, 0.25, 0.), VertSite::Edge(0, 1)).unwrap();
    let id = bone.vertices[n as usize].id as i32;

    let w = bone.binds[0].verts.iter().find(|v| v.id == id).unwrap().weight;
    assert!((w - 0.875).abs() < EPS, "0.75 * 1 + 0.25 * 0.5, got {w}");
    assert!(
        bone.binds[1].verts.iter().all(|v| v.id != id),
        "no weight from corners outside the bind"
    );
}

#[test]
fn classic_binds_leave_new_vertex_unbound() {
    let mut bone = quad();
    bone.binds = vec![BoneBind {
        bone_id: 1,
        is_path: false,
        verts: vec![BoneBindVert { id: 10, weight: 1. }, BoneBindVert { id: 11, weight: 1. }],
    }];
    topology::add_vertex(&mut bone, vertex(0, 0.5, 0.), VertSite::Edge(0, 1)).unwrap();
    assert_eq!(bone.binds[0].verts.len(), 2);
}

// --- remove / delete ---

#[test]
fn remove_triangle_and_orphan() {
    let mut bone = quad();
    topology::remove_triangle(&mut bone, 1).unwrap();
    assert_eq!(bone.indices.len(), 3);
    assert_eq!(ids(&bone), vec![10, 11, 12], "vertex 13 was only in that triangle");
    assert_consistent(&bone);
}

#[test]
fn last_triangle_is_kept() {
    let mut bone = quad();
    topology::remove_triangle(&mut bone, 1).unwrap();
    assert_eq!(topology::remove_triangle(&mut bone, 0), Err(TopoError::LastTriangle));
    assert_eq!(topology::delete_vertex(&mut bone, 0), Err(TopoError::LastTriangle));
    assert_eq!(topology::delete_edge(&mut bone, 0, 1), Err(TopoError::LastTriangle));
    assert_eq!(topology::dissolve_vertex(&mut bone, 0), Err(TopoError::LastTriangle));
    assert_eq!(bone.indices.len(), 3);

    let mut bone = quad();
    assert_eq!(topology::delete_vertex(&mut bone, 0), Err(TopoError::LastTriangle));
}

#[test]
fn delete_vertex_leaves_hole_and_cleans_binds() {
    let mut bone = grid();
    bone.binds = vec![BoneBind {
        bone_id: 1,
        is_path: false,
        verts: vec![BoneBindVert { id: 104, weight: 1. }, BoneBindVert { id: 102, weight: 0.3 }],
    }];
    topology::delete_vertex(&mut bone, 4).unwrap();

    assert!(!ids(&bone).contains(&104));
    assert_eq!(bone.indices.len(), 2 * 3, "only the 2 triangles away from the centre remain");
    assert!((total_area(&bone) - 0.25).abs() < EPS);
    assert_eq!(bone.binds[0].verts.len(), 1);
    assert_eq!(bone.binds[0].verts[0].id, 102);
    assert!((bone.binds[0].verts[0].weight - 0.3).abs() < EPS, "other weights unchanged");
    assert_consistent(&bone);
}

#[test]
fn delete_edge_removes_both_triangles() {
    let mut bone = grid();
    topology::delete_edge(&mut bone, 0, 4).unwrap();
    assert_eq!(bone.indices.len(), 6 * 3);
    assert!((total_area(&bone) - 0.75).abs() < EPS);
    assert_consistent(&bone);
}

// --- dissolve ---

#[test]
fn dissolve_interior_vertex_keeps_surface() {
    let mut bone = grid();
    topology::dissolve_vertex(&mut bone, 4).unwrap();

    assert!(!ids(&bone).contains(&104));
    assert_eq!(bone.vertices.len(), 8);
    assert!((total_area(&bone) - 1.).abs() < EPS, "surface is kept");
    assert_consistent(&bone);
}

#[test]
fn dissolve_straight_boundary_vertex_keeps_surface() {
    let mut bone = grid();
    // vertex 1 is the middle of the top edge
    topology::dissolve_vertex(&mut bone, 1).unwrap();
    assert!(!ids(&bone).contains(&101));
    assert!((total_area(&bone) - 1.).abs() < EPS);
    assert_consistent(&bone);
}

#[test]
fn dissolve_reflex_boundary_vertex_falls_back_to_delete() {
    // an L shape: removing the inner corner (vertex 4 of a grid missing its
    // top-right cell) would fill area the mesh never had
    let mut bone = grid();
    topology::remove_triangle(&mut bone, 2).unwrap();
    topology::remove_triangle(&mut bone, 2).unwrap();
    assert!((total_area(&bone) - 0.75).abs() < EPS);
    let centre = bone.vertices.iter().position(|v| v.id == 104).unwrap() as u32;

    topology::dissolve_vertex(&mut bone, centre).unwrap();
    assert!(!ids(&bone).contains(&104));
    assert!(total_area(&bone) < 0.75, "nothing was filled outside the L");
    assert_consistent(&bone);
}

#[test]
fn dissolve_interior_edge_flips_it() {
    let mut bone = quad();
    topology::dissolve_edge(&mut bone, 0, 2).unwrap();

    assert_eq!(bone.indices.len(), 6);
    for t in bone.indices.chunks_exact(3) {
        assert!(t.contains(&1) && t.contains(&3), "new diagonal is 1-3: {t:?}");
    }
    assert!((total_area(&bone) - 1.).abs() < EPS);
    assert_consistent(&bone);
}

#[test]
fn dissolve_edge_of_concave_quad_is_refused() {
    // dart: 2 is pulled in past the 1-3 diagonal, so 0-2 can't flip
    let verts = vec![vertex(10, 0., 0.), vertex(11, 1., 0.), vertex(12, 0.3, 0.3), vertex(13, 0., 1.)];
    let mut bone = mesh(verts, vec![0, 1, 2, 0, 2, 3]);
    let before = bone.indices.clone();
    assert_eq!(topology::dissolve_edge(&mut bone, 0, 2), Err(TopoError::Concave));
    assert_eq!(bone.indices, before);
}

#[test]
fn dissolve_boundary_edge_removes_its_triangle() {
    let mut bone = quad();
    topology::dissolve_edge(&mut bone, 0, 1).unwrap();
    assert_eq!(bone.indices.len(), 3);
    assert_consistent(&bone);
}

#[test]
fn mixed_winding_input_still_dissolves() {
    // imported meshes may not wind consistently; the ring walk is undirected
    let mut bone = grid();
    for t in bone.indices.chunks_exact_mut(6) {
        t.swap(1, 2);
    }
    topology::dissolve_vertex(&mut bone, 4).unwrap();
    assert!((total_area(&bone) - 1.).abs() < EPS);
}

#[test]
fn edges_are_unique() {
    let bone = grid();
    // 12 outer/inner axis edges + 4 diagonals
    assert_eq!(topology::edges(&bone.indices).len(), 16);
}
