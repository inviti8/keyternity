//! Pen cuts (docs/TOPOLOGY_TOOLS.md §5.3, §9).

use skelform_lib::pen::{self, PenCut, PenPoint, Snap};
use skelform_lib::shared::*;

const EPS: f32 = 1e-4;
const TEX: Vec2 = Vec2 { x: 100., y: 100. };

fn vertex(id: u32, u: f32, v: f32) -> Vertex {
    Vertex {
        id,
        pos: Vec2::new(u * 100., -v * 100.),
        uv: Vec2::new(u, v),
        ..Default::default()
    }
}

/// Unit quad: 0 (0,0), 1 (1,0), 2 (1,1), 3 (0,1); diagonal 0-2.
fn quad() -> Bone {
    Bone {
        vertices: vec![vertex(10, 0., 0.), vertex(11, 1., 0.), vertex(12, 1., 1.), vertex(13, 0., 1.)],
        indices: vec![0, 1, 2, 0, 2, 3],
        verts_edited: true,
        ..Default::default()
    }
}

/// n x n cells over 0..1, each split along its diagonal; `skip` cells (x, y) are left out.
fn grid(n: u32, skip: &[(u32, u32)]) -> Bone {
    let mut vertices = vec![];
    for y in 0..=n {
        for x in 0..=n {
            vertices.push(vertex(100 + y * (n + 1) + x, x as f32 / n as f32, y as f32 / n as f32));
        }
    }
    let mut indices = vec![];
    for y in 0..n {
        for x in 0..n {
            if skip.contains(&(x, y)) {
                continue;
            }
            let a = y * (n + 1) + x;
            let w = n + 1;
            indices.extend([a, a + 1, a + w + 1, a, a + w + 1, a + w]);
        }
    }
    Bone { vertices, indices, verts_edited: true, ..Default::default() }
}

fn free(u: f32, v: f32) -> PenPoint {
    PenPoint { uv: Vec2::new(u, v), snap: Snap::Free }
}

fn at_vertex(bone: &Bone, u: f32, v: f32) -> PenPoint {
    let i = bone
        .vertices
        .iter()
        .position(|vert| (vert.uv - Vec2::new(u, v)).mag() < 1e-6)
        .expect("no vertex there");
    PenPoint { uv: Vec2::new(u, v), snap: Snap::Vertex(i as u32) }
}

fn on_edge(u: f32, v: f32) -> PenPoint {
    PenPoint { uv: Vec2::new(u, v), snap: Snap::Edge }
}

fn open(points: Vec<PenPoint>) -> PenCut {
    PenCut { points, closed: false }
}

fn closed(points: Vec<PenPoint>) -> PenCut {
    PenCut { points, closed: true }
}

fn area(bone: &Bone) -> f32 {
    bone.indices
        .chunks_exact(3)
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| bone.vertices[t[k] as usize].uv);
            ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() / 2.
        })
        .sum()
}

/// Same winding everywhere, no degenerate triangles, no orphans, unique ids, and
/// every vertex's pos follows the test layout (pos = uv * (100, -100)).
fn assert_valid(bone: &Bone) {
    let signs: Vec<f32> = bone
        .indices
        .chunks_exact(3)
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| bone.vertices[t[k] as usize].uv);
            (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
        })
        .collect();
    assert!(signs.iter().all(|s| s.abs() > 1e-9), "degenerate triangle");
    assert!(signs.iter().all(|s| s.signum() == signs[0].signum()), "mixed winding");
    for v in 0..bone.vertices.len() as u32 {
        assert!(bone.indices.contains(&v), "orphan vertex {v}");
    }
    let mut ids: Vec<u32> = bone.vertices.iter().map(|v| v.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), bone.vertices.len(), "duplicate ids");
    for v in &bone.vertices {
        let expected = Vec2::new(v.uv.x * 100., -v.uv.y * 100.);
        assert!((v.pos - expected).mag() < 1e-2, "{:?} placed at {:?}", v.uv, v.pos);
    }
}

#[test]
fn knife_cut_across_the_mesh() {
    let mut bone = quad();
    let cut = open(vec![free(-0.5, 0.5), free(1.5, 0.5)]);
    let report = pen::apply_cuts(&mut bone, &[cut], TEX).unwrap();

    assert_eq!(report.added, 3, "crossings of the left edge, the diagonal and the right edge");
    assert_eq!(report.dropped, vec![(0, 0), (0, 1)], "the ends outside enclose nothing");
    assert!((area(&bone) - 1.).abs() < EPS, "no surface added or lost");
    // the cut line is now made of edges
    let on_cut = bone.vertices.iter().filter(|v| (v.uv.y - 0.5).abs() < EPS).count();
    assert_eq!(on_cut, 3);
    assert_valid(&bone);
}

#[test]
fn closed_loop_outside_makes_an_island() {
    let mut bone = quad();
    let before = bone.indices.clone();
    let cut = closed(vec![free(2., 0.), free(3., 0.), free(3., 1.), free(2., 1.)]);
    let report = pen::apply_cuts(&mut bone, &[cut], TEX).unwrap();

    assert_eq!(report.added, 4);
    assert!(report.dropped.is_empty());
    assert!((area(&bone) - 2.).abs() < EPS);
    assert_eq!(&bone.indices[..6], &before[..], "the original triangles are untouched");
    assert_valid(&bone);
}

#[test]
fn cut_out_and_back_grows_the_mesh() {
    let mut bone = quad();
    let cut = open(vec![at_vertex(&bone, 1., 0.), free(1.5, 0.5), at_vertex(&bone, 1., 1.)]);
    let report = pen::apply_cuts(&mut bone, &[cut], TEX).unwrap();

    assert_eq!(report.added, 1);
    assert!((area(&bone) - 1.25).abs() < EPS);
    assert_valid(&bone);
}

#[test]
fn line_across_a_hole_leaves_it() {
    let mut bone = grid(3, &[(1, 1)]);
    let third = 1. / 3.;
    let cut = open(vec![at_vertex(&bone, third, third), at_vertex(&bone, 2. * third, 2. * third)]);
    pen::apply_cuts(&mut bone, &[cut], TEX).unwrap();
    assert!((area(&bone) - 8. / 9.).abs() < EPS, "the hole stays empty");
    assert_valid(&bone);
}

#[test]
fn closed_loop_in_a_hole_fills_it() {
    let mut bone = grid(3, &[(1, 1)]);
    let (a, b) = (0.4, 0.6);
    let cut = closed(vec![free(a, a), free(b, a), free(b, b), free(a, b)]);
    pen::apply_cuts(&mut bone, &[cut], TEX).unwrap();
    assert!((area(&bone) - (8. / 9. + 0.04)).abs() < EPS);
    assert_valid(&bone);
}

#[test]
fn dangling_segment_changes_nothing() {
    let mut bone = quad();
    let before = bone.clone();
    let cut = open(vec![free(2., 0.), free(3., 0.5)]);
    let report = pen::apply_cuts(&mut bone, &[cut], TEX).unwrap();

    assert_eq!(report.dropped, vec![(0, 0), (0, 1)]);
    assert_eq!(report.added, 0);
    assert_eq!(bone.indices, before.indices);
    assert_eq!(bone.vertices.len(), 4);
}

#[test]
fn edge_snapped_point_splits_its_edge() {
    let mut bone = quad();
    let cut = open(vec![on_edge(0.5, 0.), at_vertex(&bone, 0., 1.)]);
    let report = pen::apply_cuts(&mut bone, &[cut], TEX).unwrap();

    // the snapped point, plus where the cut crosses the diagonal
    assert_eq!(bone.vertices.len(), 6);
    assert!(report.dropped.is_empty());
    assert!((area(&bone) - 1.).abs() < EPS);
    assert_valid(&bone);
}

#[test]
fn cut_inside_one_cell_keeps_the_rest_exactly() {
    let mut bone = grid(3, &[]);
    let before = bone.indices.clone();
    // a closed loop inside the top-left cell's first triangle
    let cut = closed(vec![free(0.2, 0.03), free(0.3, 0.03), free(0.3, 0.13)]);
    pen::apply_cuts(&mut bone, &[cut], TEX).unwrap();

    assert_eq!(&bone.indices[..before.len() - 3], &before[3..], "only triangle 0 changed");
    assert!((area(&bone) - 1.).abs() < EPS);
    assert_valid(&bone);
}

#[test]
fn bind_posed_mesh_interpolates_weights() {
    let mut bone = quad();
    bone.bind_pose = true;
    bone.binds = vec![BoneBind {
        bone_id: 1,
        is_path: false,
        verts: vec![BoneBindVert { id: 10, weight: 1. }, BoneBindVert { id: 13, weight: 1. }],
    }];
    // a vertical cut at u = 0.25: it crosses the top and bottom edges, a quarter of
    // the way from the bound side
    let cut = open(vec![free(0.25, -0.5), free(0.25, 1.5)]);
    pen::apply_cuts(&mut bone, &[cut], TEX).unwrap();

    let bottom = bone.vertices.iter().find(|v| (v.uv - Vec2::new(0.25, 0.)).mag() < EPS).unwrap();
    let w = bone.binds[0].verts.iter().find(|bv| bv.id == bottom.id as i32).unwrap().weight;
    assert!((w - 0.75).abs() < 1e-3, "got {w}");
}

#[test]
fn snapping_to_a_missing_vertex_fails_cleanly() {
    let mut bone = quad();
    let before = bone.clone();
    let cut = open(vec![PenPoint { uv: Vec2::ZERO, snap: Snap::Vertex(99) }, free(0.5, 0.5)]);
    assert!(pen::apply_cuts(&mut bone, &[cut], TEX).is_err());
    assert_eq!(bone.indices, before.indices);
}
