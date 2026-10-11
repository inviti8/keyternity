//! Trace (docs/AUTO_MESH.md §5): synthetic alpha masks, checked for coverage,
//! holes, separate pieces, triangle quality and index sanity.

use keyternity_lib::auto_mesh::{self, TraceOptions};
use keyternity_lib::shared::*;

const W: u32 = 64;
const H: u32 = 64;

/// A W x H alpha channel, opaque where `f(x, y)`.
fn mask(f: impl Fn(u32, u32) -> bool) -> Vec<u8> {
    (0..H).flat_map(|y| (0..W).map(move |x| (x, y))).map(|(x, y)| if f(x, y) { 255 } else { 0 }).collect()
}

fn opts() -> TraceOptions {
    TraceOptions { detail: 1., padding: 0., density: 12., alpha_threshold: 16, min_piece: 0.001 }
}

/// Triangles as texture-px points (from the uvs).
fn triangles(verts: &[Vertex], indices: &[u32]) -> Vec<[Vec2; 3]> {
    let px = |i: u32| verts[i as usize].uv * Vec2::new(W as f32, H as f32);
    indices.chunks_exact(3).map(|t| [px(t[0]), px(t[1]), px(t[2])]).collect()
}

fn centroid(t: &[Vec2; 3]) -> Vec2 {
    (t[0] + t[1] + t[2]) / 3.
}

fn area(t: &[Vec2; 3]) -> f32 {
    ((t[1].x - t[0].x) * (t[2].y - t[0].y) - (t[2].x - t[0].x) * (t[1].y - t[0].y)).abs() / 2.
}

fn min_angle_deg(t: &[Vec2; 3]) -> f32 {
    (0..3)
        .map(|i| {
            let (a, b, c) = (t[i], t[(i + 1) % 3], t[(i + 2) % 3]);
            let (u, v) = (b - a, c - a);
            let cos = (u.x * v.x + u.y * v.y) / (u.mag() * v.mag());
            cos.clamp(-1., 1.).acos().to_degrees()
        })
        .fold(180., f32::min)
}

/// Checks every shape shares: indices in range, no degenerate triangles, every
/// centroid on the art (`on_art`), and the art's area covered (within `slack`).
fn check(alpha: &[u8], o: &TraceOptions, on_art: impl Fn(Vec2) -> bool, slack: f32) -> Vec<[Vec2; 3]> {
    let (verts, indices) = auto_mesh::trace_alpha(alpha, W, H, o);
    assert!(!indices.is_empty(), "no triangles");
    assert_eq!(indices.len() % 3, 0);
    assert!(indices.iter().all(|&i| (i as usize) < verts.len()), "index out of range");
    let tris = triangles(&verts, &indices);
    for t in &tris {
        assert!(area(t) > 1e-3, "degenerate triangle {t:?}");
        assert!(on_art(centroid(t)), "triangle off the art at {:?}", centroid(t));
    }
    let art = alpha.iter().filter(|&&a| a >= o.alpha_threshold).count() as f32;
    let covered: f32 = tris.iter().map(area).sum();
    assert!((covered - art).abs() <= art * slack, "covered {covered} px² of {art}");
    tris
}

#[test]
fn square_outline_only_is_two_triangles() {
    let alpha = mask(|x, y| (16..48).contains(&x) && (16..48).contains(&y));
    let o = TraceOptions { density: 0., ..opts() };
    let tris = check(&alpha, &o, |p| p.x > 16. && p.x < 48. && p.y > 16. && p.y < 48., 0.001);
    assert_eq!(tris.len(), 2);
}

#[test]
fn refined_square_has_interior_vertices_and_no_slivers() {
    let alpha = mask(|x, y| (8..56).contains(&x) && (8..56).contains(&y));
    let tris = check(&alpha, &opts(), |p| p.x > 8. && p.x < 56. && p.y > 8. && p.y < 56., 0.001);
    assert!(tris.len() > 8, "only {} triangles", tris.len());
    for t in &tris {
        assert!(min_angle_deg(t) >= 20., "sliver {t:?}: {}°", min_angle_deg(t));
    }
}

#[test]
fn concave_l_shape_leaves_the_cutout_empty() {
    // L: a 48x48 square with its top-right 24x24 quarter cut away
    let art = |x: f32, y: f32| x > 8. && x < 56. && y > 8. && y < 56. && !(x > 32. && y < 32.);
    let alpha = mask(|x, y| art(x as f32 + 0.5, y as f32 + 0.5));
    check(&alpha, &opts(), |p| art(p.x, p.y), 0.001);
}

#[test]
fn ring_keeps_its_hole() {
    let r = |x: f32, y: f32| ((x - 32.).powi(2) + (y - 32.).powi(2)).sqrt();
    let alpha = mask(|x, y| {
        let d = r(x as f32 + 0.5, y as f32 + 0.5);
        d < 28. && d > 12.
    });
    // detail 1 px: allow the simplified outline to cut a pixel either side
    let tris = check(&alpha, &opts(), |p| r(p.x, p.y) < 29. && r(p.x, p.y) > 11., 0.04);
    assert!(tris.iter().all(|t| r(centroid(t).x, centroid(t).y) > 11.));
}

#[test]
fn separate_pieces_are_both_meshed_and_not_bridged() {
    let left = |x: f32, y: f32| x > 4. && x < 24. && y > 20. && y < 44.;
    let right = |x: f32, y: f32| x > 40. && x < 60. && y > 20. && y < 44.;
    let alpha = mask(|x, y| {
        let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
        left(x, y) || right(x, y)
    });
    let tris = check(&alpha, &opts(), |p| left(p.x, p.y) || right(p.x, p.y), 0.001);
    assert!(tris.iter().any(|t| left(centroid(t).x, centroid(t).y)));
    assert!(tris.iter().any(|t| right(centroid(t).x, centroid(t).y)));
}

#[test]
fn specks_below_the_minimum_size_are_dropped() {
    let block = |x: u32, y: u32| (16..48).contains(&x) && (16..48).contains(&y);
    let alpha = mask(|x, y| block(x, y) || (x, y) == (2, 2) || (x, y) == (60, 5));
    let o = TraceOptions { min_piece: 0.01, ..opts() };
    check(&alpha, &o, |p| p.x > 16. && p.x < 48. && p.y > 16. && p.y < 48., 0.01);
}

#[test]
fn padding_grows_the_outline() {
    let alpha = mask(|x, y| (16..48).contains(&x) && (16..48).contains(&y));
    let o = TraceOptions { padding: 4., density: 0., ..opts() };
    let (verts, _) = auto_mesh::trace_alpha(&alpha, W, H, &o);
    let xs: Vec<f32> = verts.iter().map(|v| v.uv.x * W as f32).collect();
    let min = xs.iter().cloned().fold(f32::MAX, f32::min);
    let max = xs.iter().cloned().fold(f32::MIN, f32::max);
    assert!((min - 12.).abs() <= 1. && (max - 52.).abs() <= 1., "x range {min}..{max}");
}

#[test]
fn alpha_threshold_ignores_faint_pixels() {
    let alpha: Vec<u8> = mask(|x, y| (16..48).contains(&x) && (16..48).contains(&y))
        .into_iter()
        .enumerate()
        .map(|(i, a)| if a == 0 && i % 7 == 0 { 8 } else { a })
        .collect();
    let o = TraceOptions { density: 0., ..opts() };
    check(&alpha, &o, |p| p.x > 16. && p.x < 48. && p.y > 16. && p.y < 48., 0.001);
}

#[test]
fn fully_transparent_gives_an_empty_mesh() {
    let (verts, indices) = auto_mesh::trace_alpha(&mask(|_, _| false), W, H, &opts());
    assert!(verts.is_empty() && indices.is_empty());
}

#[test]
fn diagonal_pixels_trace_as_closed_loops() {
    let mut m = vec![false; 16];
    m[0] = true; // (0,0)
    m[5] = true; // (1,1)
    let loops = auto_mesh::outlines(&m, 4, 4);
    assert_eq!(loops.len(), 2, "{loops:?}");
    for l in loops {
        assert_eq!(auto_mesh::polygon_area(&l).abs(), 1.);
    }
}

#[test]
fn holes_wind_opposite_to_outer_outlines() {
    // a 4x4 block with a 2x2 hole
    let m: Vec<bool> = (0..36).map(|i| {
        let (x, y) = (i % 6, i / 6);
        (1..5).contains(&x) && (1..5).contains(&y) && !((2..4).contains(&x) && (2..4).contains(&y))
    }).collect();
    let loops = auto_mesh::outlines(&m, 6, 6);
    assert_eq!(loops.len(), 2);
    let areas: Vec<f64> = loops.iter().map(|l| auto_mesh::polygon_area(l)).collect();
    assert!(areas[0].signum() != areas[1].signum(), "{areas:?}");
    assert_eq!(areas.iter().map(|a| a.abs()).sum::<f64>(), 20.);
}
