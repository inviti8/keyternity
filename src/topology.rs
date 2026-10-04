//! Local mesh topology edits (docs/TOPOLOGY_TOOLS.md §4, §6).
//!
//! Authored `indices` are the mesh: every operation here changes only the triangles
//! it touches and never re-triangulates the whole mesh. Geometry tests run in UV
//! space, which is the stable authored layout (`pos` may have been dragged so that
//! faces overlap).

use crate::editor::cleanup_vertices;
use crate::shared::*;

const EPS: f32 = 1e-9;

/// Where a new vertex lands on the existing mesh.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VertSite {
    #[default]
    None,
    /// Inside triangle `n` (indices `n*3..n*3+3`).
    Tri(usize),
    /// On the edge between these two vertex indices.
    Edge(u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TopoError {
    /// The operation would leave the mesh without triangles.
    LastTriangle,
    /// The vertex, edge or triangle isn't in the mesh.
    NotFound,
    /// The edge isn't shared by exactly one or two triangles.
    NonManifold,
    /// Flipping the edge would fold its quad over (the quad is concave).
    Concave,
}

/// Add `vert` to the mesh at `site`, splitting the triangle (into 3) or the
/// triangles on the edge (each into 2). Returns the new vertex's index.
///
/// On a bind-posed mesh the new vertex gets weights interpolated from the corners
/// it was placed between. Classic binds store `pos` in the bind bone's frame, so
/// interpolating there would make the vertex jump; it's left unbound instead.
pub fn add_vertex(bone: &mut Bone, mut vert: Vertex, site: VertSite) -> Result<u32, TopoError> {
    let n = bone.vertices.len() as u32;
    let ids: Vec<i32> = bone.vertices.iter().map(|v| v.id as i32).collect();
    vert.id = generate_id(ids) as u32;

    let weights: Vec<(u32, f32)>;
    match site {
        VertSite::None => return Err(TopoError::NotFound),
        VertSite::Tri(t) => {
            if t * 3 + 2 >= bone.indices.len() {
                return Err(TopoError::NotFound);
            }
            let [a, b, c] = tri(&bone.indices, t);
            let uv = |i: u32| bone.vertices[i as usize].uv;
            let (wa, wb, wc) = barycentric(vert.uv, uv(a), uv(b), uv(c));
            weights = vec![(a, wa), (b, wb), (c, wc)];
            bone.indices.splice(t * 3..t * 3 + 3, [a, b, n, b, c, n, c, a, n]);
        }
        VertSite::Edge(i, j) => {
            let tris = tris_with_edge(&bone.indices, i, j);
            if tris.is_empty() {
                return Err(TopoError::NotFound);
            }
            let (ui, uj) = (bone.vertices[i as usize].uv, bone.vertices[j as usize].uv);
            let t = edge_param(vert.uv, ui, uj);
            weights = vec![(i, 1. - t), (j, t)];

            // replace each triangle (i, j, k) with (i, n, k) and (n, j, k), keeping winding
            for &t in tris.iter().rev() {
                let corners = tri(&bone.indices, t);
                let mut first = corners;
                let mut second = corners;
                for c in 0..3 {
                    if corners[c] == j {
                        first[c] = n;
                    }
                    if corners[c] == i {
                        second[c] = n;
                    }
                }
                bone.indices.splice(t * 3..t * 3 + 3, [first, second].concat());
            }
        }
    }

    bone.vertices.push(vert);
    if bone.bind_pose {
        interpolate_binds(bone, vert.id as i32, &weights);
    }
    Ok(n)
}

/// Remove triangle `t`. Vertices it alone used are removed too.
pub fn remove_triangle(bone: &mut Bone, t: usize) -> Result<(), TopoError> {
    if t * 3 + 2 >= bone.indices.len() {
        return Err(TopoError::NotFound);
    }
    remove_tris(bone, &[t])
}

/// Remove vertex `v` and every triangle that uses it, leaving a hole.
pub fn delete_vertex(bone: &mut Bone, v: u32) -> Result<(), TopoError> {
    remove_tris(bone, &tris_with_vertex(&bone.indices, v))
}

/// Remove the triangles on the edge between `i` and `j`, leaving a hole.
pub fn delete_edge(bone: &mut Bone, i: u32, j: u32) -> Result<(), TopoError> {
    remove_tris(bone, &tris_with_edge(&bone.indices, i, j))
}

/// Remove vertex `v` but keep the surface: the hole its triangles leave is filled
/// from its ring of neighbours. On the mesh boundary this only works where `v` is
/// a convex corner; otherwise (or if the ring isn't a simple loop/path) it falls
/// back to `delete_vertex`.
pub fn dissolve_vertex(bone: &mut Bone, v: u32) -> Result<(), TopoError> {
    let fan = tris_with_vertex(&bone.indices, v);
    if fan.is_empty() {
        return Err(TopoError::NotFound);
    }
    let sign = winding_sign(bone, fan[0]);
    let fill = ring_polygon(bone, v, &fan).and_then(|poly| ear_clip(bone, &poly, sign));

    let Some(fill) = fill else {
        return delete_vertex(bone, v);
    };
    if fill.is_empty() && fan.len() * 3 == bone.indices.len() {
        return Err(TopoError::LastTriangle);
    }
    let mut kept = without_tris(&bone.indices, &fan);
    kept.extend(fill);
    bone.indices = kept;
    cleanup_vertices(bone);
    Ok(())
}

/// Remove the edge between `i` and `j` but keep the surface. An interior edge is
/// flipped to the quad's other diagonal; a boundary edge has nothing to merge
/// with, so its triangle is removed.
pub fn dissolve_edge(bone: &mut Bone, i: u32, j: u32) -> Result<(), TopoError> {
    let tris = tris_with_edge(&bone.indices, i, j);
    match tris.len() {
        0 => Err(TopoError::NotFound),
        1 => remove_tris(bone, &tris),
        2 => {
            let k = third(tri(&bone.indices, tris[0]), i, j);
            let l = third(tri(&bone.indices, tris[1]), i, j);
            let uv = |x: u32| bone.vertices[x as usize].uv;
            if !segments_cross(uv(k), uv(l), uv(i), uv(j)) {
                return Err(TopoError::Concave);
            }
            let sign = winding_sign(bone, tris[0]);
            let a = oriented(bone, [k, i, l], sign);
            let b = oriented(bone, [l, j, k], sign);
            let mut kept = without_tris(&bone.indices, &tris);
            kept.extend(a);
            kept.extend(b);
            bone.indices = kept;
            Ok(())
        }
        _ => Err(TopoError::NonManifold),
    }
}

/// Every edge of the mesh once, as (lower index, higher index).
pub fn edges(indices: &[u32]) -> Vec<(u32, u32)> {
    let mut edges = vec![];
    for t in indices.chunks_exact(3) {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let e = (a.min(b), a.max(b));
            if !edges.contains(&e) {
                edges.push(e);
            }
        }
    }
    edges
}

// --- helpers ---

fn tri(indices: &[u32], t: usize) -> [u32; 3] {
    [indices[t * 3], indices[t * 3 + 1], indices[t * 3 + 2]]
}

fn tris_with_vertex(indices: &[u32], v: u32) -> Vec<usize> {
    let chunks = indices.chunks_exact(3).enumerate();
    chunks.filter(|(_, c)| c.contains(&v)).map(|(t, _)| t).collect()
}

fn tris_with_edge(indices: &[u32], i: u32, j: u32) -> Vec<usize> {
    let chunks = indices.chunks_exact(3).enumerate();
    let has_edge = |c: &[u32]| i != j && c.contains(&i) && c.contains(&j);
    chunks.filter(|(_, c)| has_edge(c)).map(|(t, _)| t).collect()
}

fn without_tris(indices: &[u32], tris: &[usize]) -> Vec<u32> {
    let chunks = indices.chunks_exact(3).enumerate();
    let kept = chunks.filter(|(t, _)| !tris.contains(t));
    kept.flat_map(|(_, c)| c.to_vec()).collect()
}

fn remove_tris(bone: &mut Bone, tris: &[usize]) -> Result<(), TopoError> {
    if tris.is_empty() {
        return Err(TopoError::NotFound);
    }
    if tris.len() * 3 >= bone.indices.len() {
        return Err(TopoError::LastTriangle);
    }
    bone.indices = without_tris(&bone.indices, tris);
    cleanup_vertices(bone);
    Ok(())
}

fn third(corners: [u32; 3], i: u32, j: u32) -> u32 {
    *corners.iter().find(|c| **c != i && **c != j).unwrap()
}

fn cross(o: Vec2, a: Vec2, b: Vec2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn winding_sign(bone: &Bone, t: usize) -> f32 {
    let [a, b, c] = tri(&bone.indices, t);
    let uv = |x: u32| bone.vertices[x as usize].uv;
    if cross(uv(a), uv(b), uv(c)) < 0. {
        -1.
    } else {
        1.
    }
}

/// `corners` reordered (if needed) so its UV winding has the given sign.
fn oriented(bone: &Bone, corners: [u32; 3], sign: f32) -> [u32; 3] {
    let uv = |x: u32| bone.vertices[x as usize].uv;
    let [a, b, c] = corners;
    if cross(uv(a), uv(b), uv(c)) * sign < 0. {
        [a, c, b]
    } else {
        corners
    }
}

/// Barycentric weights of `p` in triangle (a, b, c). Falls back to equal weights
/// for a degenerate triangle.
fn barycentric(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> (f32, f32, f32) {
    let area = cross(a, b, c);
    if area.abs() < EPS {
        return (1. / 3., 1. / 3., 1. / 3.);
    }
    let wa = cross(p, b, c) / area;
    let wb = cross(a, p, c) / area;
    (wa, wb, 1. - wa - wb)
}

/// How far along a→b the projection of `p` lies, clamped to 0..1.
fn edge_param(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    let len2 = d.x * d.x + d.y * d.y;
    if len2 < EPS {
        return 0.5;
    }
    (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / len2).clamp(0., 1.)
}

/// Whether segments p1-p2 and q1-q2 cross at a single interior point.
fn segments_cross(p1: Vec2, p2: Vec2, q1: Vec2, q2: Vec2) -> bool {
    let d1 = cross(q1, q2, p1);
    let d2 = cross(q1, q2, p2);
    let d3 = cross(p1, p2, q1);
    let d4 = cross(p1, p2, q2);
    d1 * d2 < 0. && d3 * d4 < 0.
}

/// Weight binds of a new vertex: per bind, the corners' weights blended by `weights`.
fn interpolate_binds(bone: &mut Bone, id: i32, weights: &[(u32, f32)]) {
    let corner_ids: Vec<(i32, f32)> =
        weights.iter().map(|(v, w)| (bone.vertices[*v as usize].id as i32, *w)).collect();
    for bind in &mut bone.binds {
        if bind.is_path {
            continue;
        }
        let mut weight = 0.;
        for (corner, w) in &corner_ids {
            if let Some(bv) = bind.verts.iter().find(|bv| bv.id == *corner) {
                weight += bv.weight * w;
            }
        }
        if weight > 0. {
            bind.verts.push(BoneBindVert { id, weight });
        }
    }
}

/// The polygon left when `v` and its fan are removed, as vertex indices in order:
/// the ring for an interior vertex, the open fan path (closed by a chord) for a
/// boundary vertex. None if the ring isn't one simple loop or path, or if `v` is a
/// reflex boundary corner (the chord would cover area the mesh never had).
fn ring_polygon(bone: &Bone, v: u32, fan: &[usize]) -> Option<Vec<u32>> {
    // the edge opposite `v` in each fan triangle
    let mut ring_edges = vec![];
    for &t in fan {
        let corners = tri(&bone.indices, t);
        let others: Vec<u32> = corners.into_iter().filter(|c| *c != v).collect();
        if others.len() != 2 {
            return None;
        }
        ring_edges.push((others[0], others[1]));
    }

    // walk the ring edges (undirected, so mixed winding is fine)
    let mut verts: Vec<u32> = ring_edges.iter().flat_map(|e| [e.0, e.1]).collect();
    verts.sort();
    verts.dedup();
    let degree = |x: u32| ring_edges.iter().filter(|e| e.0 == x || e.1 == x).count();
    if verts.iter().any(|x| degree(*x) > 2) {
        return None;
    }
    let ends: Vec<u32> = verts.iter().copied().filter(|x| degree(*x) == 1).collect();
    let is_loop = ends.is_empty();
    if !is_loop && ends.len() != 2 {
        return None;
    }

    let mut poly = vec![if is_loop { verts[0] } else { ends[0] }];
    let mut used = vec![false; ring_edges.len()];
    loop {
        let last = *poly.last().unwrap();
        let next = (0..ring_edges.len()).find(|e| {
            !used[*e] && (ring_edges[*e].0 == last || ring_edges[*e].1 == last)
        });
        let Some(e) = next else { break };
        used[e] = true;
        let (a, b) = ring_edges[e];
        let other = if a == last { b } else { a };
        if is_loop && other == poly[0] {
            break;
        }
        poly.push(other);
    }
    if used.iter().any(|u| !u) || poly.len() != verts.len() {
        return None;
    }

    let uv = |x: u32| bone.vertices[x as usize].uv;
    if !is_loop {
        // convex (or straight) corner: the closing chord's triangle (end, v, start)
        // doesn't wind against the fan
        let fan_area = polygon_area(&[vec![v], poly.clone()].concat(), &uv);
        let corner = cross(uv(*poly.last().unwrap()), uv(v), uv(poly[0]));
        if corner * fan_area < 0. {
            return None;
        }
    }
    if !is_simple(&poly, &uv) {
        return None;
    }
    Some(poly)
}

fn polygon_area(poly: &[u32], uv: &impl Fn(u32) -> Vec2) -> f32 {
    let mut area = 0.;
    for p in 0..poly.len() {
        let a = uv(poly[p]);
        let b = uv(poly[(p + 1) % poly.len()]);
        area += a.x * b.y - b.x * a.y;
    }
    area / 2.
}

fn is_simple(poly: &[u32], uv: &impl Fn(u32) -> Vec2) -> bool {
    let n = poly.len();
    for a in 0..n {
        for b in a + 1..n {
            let adjacent = b == a + 1 || (a == 0 && b == n - 1);
            if adjacent {
                continue;
            }
            let (p1, p2) = (uv(poly[a]), uv(poly[(a + 1) % n]));
            let (q1, q2) = (uv(poly[b]), uv(poly[(b + 1) % n]));
            if segments_cross(p1, p2, q1, q2) {
                return false;
            }
        }
    }
    true
}

/// Triangulate a simple polygon by ear clipping, winding each triangle with `sign`.
/// None if no ear can be found (a degenerate polygon).
fn ear_clip(bone: &Bone, poly: &[u32], sign: f32) -> Option<Vec<u32>> {
    let uv = |x: u32| bone.vertices[x as usize].uv;
    let orientation = polygon_area(poly, &uv).signum();
    let mut poly = poly.to_vec();
    let mut out = vec![];
    while poly.len() > 3 {
        let n = poly.len();
        let ear = (0..n).find(|&e| {
            let (a, b, c) = (poly[(e + n - 1) % n], poly[e], poly[(e + 1) % n]);
            let convex = cross(uv(a), uv(b), uv(c)) * orientation > EPS;
            let empty = poly.iter().all(|&p| {
                p == a || p == b || p == c || !in_triangle(uv(p), uv(a), uv(b), uv(c))
            });
            convex && empty
        })?;
        let (a, b, c) = (poly[(ear + n - 1) % n], poly[ear], poly[(ear + 1) % n]);
        out.extend(oriented(bone, [a, b, c], sign));
        poly.remove(ear);
    }
    if poly.len() == 3 && polygon_area(&poly, &uv).abs() > EPS {
        out.extend(oriented(bone, [poly[0], poly[1], poly[2]], sign));
    }
    Some(out)
}

fn in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let d1 = cross(a, b, p);
    let d2 = cross(b, c, p);
    let d3 = cross(c, a, p);
    let neg = d1 < 0. || d2 < 0. || d3 < 0.;
    let pos = d1 > 0. || d2 > 0. || d3 > 0.;
    !(neg && pos)
}
