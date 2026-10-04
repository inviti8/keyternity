//! The Pen tool's cuts (docs/TOPOLOGY_TOOLS.md §5.3): turning placed points and
//! the segments between them into mesh topology, in one pass.
//!
//! Every existing edge goes into a constrained Delaunay triangulation (in UV space)
//! as a constraint, so untouched triangles come back exactly. The cut segments are
//! added with `add_constraint_and_split`, which inserts a vertex wherever a segment
//! crosses an edge (the knife cuts). Faces inside an original triangle are kept;
//! empty areas are filled only where the cuts enclose them.

use crate::shared::*;
use crate::topology::{self, TopoError, VertSite};
use spade::handles::{FixedFaceHandle, FixedVertexHandle, InnerTag};
use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, PositionInTriangulation, Triangulation};
use std::collections::HashMap;

/// UV distance within which a point counts as lying on a segment.
const ON_SEGMENT: f64 = 1e-6;

/// What a placed point is attached to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Snap {
    /// An existing vertex, by index.
    Vertex(u32),
    /// A point on an existing edge (found again by its UV when applied).
    Edge,
    /// Anywhere: inside a triangle, or outside the mesh.
    Free,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenPoint {
    pub uv: Vec2,
    pub snap: Snap,
}

/// A chain of points, each joined to the previous by a segment. A closed cut
/// also joins its last point to its first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PenCut {
    pub points: Vec<PenPoint>,
    pub closed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CutReport {
    /// (cut, point) of each placed point that ended up in no triangle: segments
    /// outside the mesh that enclose nothing are dropped (§5.4).
    pub dropped: Vec<(usize, usize)>,
    /// Vertices added to the mesh.
    pub added: usize,
}

#[derive(Clone, Copy)]
struct CVert {
    pos: Point2<f64>,
    /// The mesh vertex this is, if it was one before the cut.
    orig: Option<u32>,
}

impl HasPosition for CVert {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.pos
    }
}

type Cdt = ConstrainedDelaunayTriangulation<CVert>;

/// Apply `cuts` to `bone`'s mesh as one operation. On error the mesh is unchanged.
/// `tex_size` places new vertices outside the mesh when the mesh's own layout
/// can't (fewer than 3 non-collinear vertices).
pub fn apply_cuts(bone: &mut Bone, cuts: &[PenCut], tex_size: Vec2) -> Result<CutReport, TopoError> {
    let mut work = bone.clone();
    let layout = Layout::new(bone, tex_size);
    let sign = majority_winding(&work);

    // edge-snapped points become real vertices first, splitting their edge exactly
    let mut resolved: Vec<Vec<Option<u32>>> = vec![];
    for cut in cuts {
        let mut points = vec![];
        for p in &cut.points {
            points.push(match p.snap {
                Snap::Vertex(v) if (v as usize) < work.vertices.len() => Some(v),
                Snap::Vertex(_) => return Err(TopoError::NotFound),
                Snap::Edge => Some(split_nearest_edge(&mut work, p.uv)?),
                Snap::Free => None,
            });
        }
        resolved.push(points);
    }

    // every vertex and placed point, with every existing edge as a constraint
    let mut cdt = Cdt::new();
    let mut handles = vec![];
    for (i, v) in work.vertices.iter().enumerate() {
        let p = point(v.uv);
        if let PositionInTriangulation::OnVertex(_) = cdt.locate(p) {
            return Err(TopoError::Degenerate);
        }
        let orig = Some(i as u32);
        handles.push(cdt.insert(CVert { pos: p, orig }).map_err(|_| TopoError::Degenerate)?);
    }
    let mut point_handles: Vec<Vec<FixedVertexHandle>> = vec![];
    for (c, cut) in cuts.iter().enumerate() {
        let mut hs = vec![];
        for (k, p) in cut.points.iter().enumerate() {
            hs.push(match resolved[c][k] {
                Some(v) => handles[v as usize],
                None => insert_free(&mut cdt, p.uv)?,
            });
        }
        point_handles.push(hs);
    }
    let orig_edges = topology::edges(&work.indices);
    for &(i, j) in &orig_edges {
        let (a, b) = (handles[i as usize], handles[j as usize]);
        if cdt.try_add_constraint(a, b).is_empty() && !cdt.exists_constraint(a, b) {
            // the mesh overlaps itself in UV space
            return Err(TopoError::Degenerate);
        }
    }

    // the cuts, split wherever they cross an edge or each other
    let mut stroke_segments = vec![];
    for (c, cut) in cuts.iter().enumerate() {
        let hs = &point_handles[c];
        let mut pairs: Vec<(usize, usize)> = (1..hs.len()).map(|k| (k - 1, k)).collect();
        if cut.closed && hs.len() > 2 {
            pairs.push((hs.len() - 1, 0));
        }
        for (a, b) in pairs {
            if hs[a] == hs[b] {
                continue;
            }
            let ctor = |pos: Point2<f64>| CVert { pos, orig: None };
            cdt.add_constraint_and_split(hs[a], hs[b], ctor);
            let pos = |h: FixedVertexHandle| cdt.vertex(h).position();
            stroke_segments.push((pos(hs[a]), pos(hs[b])));
        }
    }

    // which faces to keep
    let orig_segments: Vec<(Point2<f64>, Point2<f64>)> = orig_edges
        .iter()
        .map(|&(i, j)| (point(work.vertices[i as usize].uv), point(work.vertices[j as usize].uv)))
        .collect();
    let orig_tris: Vec<[Point2<f64>; 3]> = work
        .indices
        .chunks_exact(3)
        .map(|t| [0, 1, 2].map(|k| point(work.vertices[t[k] as usize].uv)))
        .collect();
    let kept = classify(&cdt, &orig_tris, &orig_segments, &stroke_segments);

    // write back: existing vertices stay put, new ones are appended
    let mut index_of: HashMap<FixedVertexHandle, u32> = HashMap::new();
    let mut new_faces: Vec<[u32; 3]> = vec![];
    let mut added = 0;
    for &face in &kept {
        let mut corners = [0u32; 3];
        for (k, v) in cdt.face(face).vertices().iter().enumerate() {
            let h = v.fix();
            let idx = match (v.data().orig, index_of.get(&h)) {
                (Some(orig), _) => orig,
                (None, Some(idx)) => *idx,
                (None, None) => {
                    let uv = Vec2::new(v.position().x as f32, v.position().y as f32);
                    let (pos, weights) = layout.place(uv);
                    let ids: Vec<i32> = work.vertices.iter().map(|v| v.id as i32).collect();
                    let vert = Vertex { id: generate_id(ids) as u32, pos, uv, ..Default::default() };
                    work.vertices.push(vert);
                    if work.bind_pose {
                        topology::interpolate_binds(&mut work, vert.id as i32, &weights);
                    }
                    added += 1;
                    work.vertices.len() as u32 - 1
                }
            };
            index_of.insert(h, idx);
            corners[k] = idx;
        }
        new_faces.push(corners);
    }

    // triangles that survived keep their original corners and order
    let key = |t: &[u32]| {
        let mut k = [t[0], t[1], t[2]];
        k.sort();
        k
    };
    let mut remaining: Vec<[u32; 3]> = new_faces.iter().map(|f| key(f)).collect();
    let mut indices = vec![];
    for t in work.indices.chunks_exact(3) {
        if let Some(pos) = remaining.iter().position(|r| *r == key(t)) {
            remaining.remove(pos);
            indices.extend_from_slice(t);
        }
    }
    for f in &new_faces {
        if remaining.contains(&key(f)) {
            indices.extend(oriented(&work, *f, sign));
        }
    }
    if indices.is_empty() {
        return Err(TopoError::LastTriangle);
    }
    work.indices = indices;
    if added > 0 || !cuts.is_empty() {
        work.verts_edited = true;
    }

    let mut dropped = vec![];
    for (c, hs) in point_handles.iter().enumerate() {
        for (k, h) in hs.iter().enumerate() {
            if !index_of.contains_key(h) {
                dropped.push((c, k));
            }
        }
    }

    *bone = work;
    Ok(CutReport { dropped, added })
}

fn point(uv: Vec2) -> Point2<f64> {
    Point2::new(uv.x as f64, uv.y as f64)
}

fn insert_free(cdt: &mut Cdt, uv: Vec2) -> Result<FixedVertexHandle, TopoError> {
    let p = point(uv);
    if let PositionInTriangulation::OnVertex(h) = cdt.locate(p) {
        return Ok(h);
    }
    cdt.insert(CVert { pos: p, orig: None }).map_err(|_| TopoError::Degenerate)
}

/// Split the edge nearest `uv` at `uv`'s projection, returning the new vertex (or
/// the edge's end, if `uv` is on it).
fn split_nearest_edge(bone: &mut Bone, uv: Vec2) -> Result<u32, TopoError> {
    let mut best: Option<(f32, u32, u32, f32)> = None;
    for (i, j) in topology::edges(&bone.indices) {
        let (a, b) = (bone.vertices[i as usize].uv, bone.vertices[j as usize].uv);
        let d = b - a;
        let len2 = d.x * d.x + d.y * d.y;
        if len2 == 0. {
            continue;
        }
        let t = (((uv.x - a.x) * d.x + (uv.y - a.y) * d.y) / len2).clamp(0., 1.);
        let dist = (uv - (a + d * t)).mag();
        if best.map_or(true, |(bd, ..)| dist < bd) {
            best = Some((dist, i, j, t));
        }
    }
    let (_, i, j, t) = best.ok_or(TopoError::NotFound)?;
    if t < 1e-4 {
        return Ok(i);
    }
    if t > 1. - 1e-4 {
        return Ok(j);
    }
    let (vi, vj) = (bone.vertices[i as usize], bone.vertices[j as usize]);
    let vert = Vertex {
        pos: vi.pos + (vj.pos - vi.pos) * t,
        uv: vi.uv + (vj.uv - vi.uv) * t,
        ..Default::default()
    };
    topology::add_vertex(bone, vert, VertSite::Edge(i, j))
}

/// The faces of `cdt` to keep: those inside an original triangle, and empty
/// regions the cuts enclose (§5.3 step 4).
fn classify(
    cdt: &Cdt,
    orig_tris: &[[Point2<f64>; 3]],
    orig_segments: &[(Point2<f64>, Point2<f64>)],
    stroke_segments: &[(Point2<f64>, Point2<f64>)],
) -> Vec<FixedFaceHandle<InnerTag>> {
    let faces: Vec<_> = cdt.inner_faces().collect();
    let index: HashMap<FixedFaceHandle<InnerTag>, usize> =
        faces.iter().enumerate().map(|(i, f)| (f.fix(), i)).collect();

    let inside_orig: Vec<bool> = faces
        .iter()
        .map(|f| {
            let c = centroid(f.positions());
            orig_tris.iter().any(|t| in_triangle(c, t))
        })
        .collect();

    // edge kinds: constraint (any), original (on a mesh edge), stroke (on a cut)
    struct Side {
        constraint: bool,
        stroke_only: bool,
        stroke: bool,
        neighbour: Option<usize>, // None: the outer face
    }
    let sides: Vec<Vec<Side>> = faces
        .iter()
        .map(|f| {
            f.adjacent_edges()
                .iter()
                .map(|e| {
                    let [a, b] = e.positions();
                    let mid = Point2::new((a.x + b.x) / 2., (a.y + b.y) / 2.);
                    let on = |segs: &[(Point2<f64>, Point2<f64>)]| {
                        segs.iter().any(|(p, q)| segment_distance(mid, *p, *q) < ON_SEGMENT)
                    };
                    let constraint = e.as_undirected().is_constraint_edge();
                    let original = constraint && on(orig_segments);
                    let stroke = constraint && on(stroke_segments);
                    let neighbour = e.rev().face().as_inner().map(|n| index[&n.fix()]);
                    Side { constraint, stroke_only: stroke && !original, stroke, neighbour }
                })
                .collect()
        })
        .collect();

    // flood the empty faces: `region` doesn't cross constraints; `area` crosses
    // cut-only edges too, recovering the empty area as it was before the cut
    let flood = |passable: &dyn Fn(&Side) -> bool| -> Vec<usize> {
        let mut label = vec![usize::MAX; faces.len()];
        for start in 0..faces.len() {
            if inside_orig[start] || label[start] != usize::MAX {
                continue;
            }
            let mut stack = vec![start];
            label[start] = start;
            while let Some(f) = stack.pop() {
                for side in &sides[f] {
                    let Some(n) = side.neighbour else { continue };
                    if passable(side) && !inside_orig[n] && label[n] == usize::MAX {
                        label[n] = start;
                        stack.push(n);
                    }
                }
            }
        }
        label
    };
    let region = flood(&|s: &Side| !s.constraint);
    let area = flood(&|s: &Side| !s.constraint || s.stroke_only);

    // per label: touches the outer face (through a passable side)
    let mut area_exterior: HashMap<usize, bool> = HashMap::new();
    let mut region_open: HashMap<usize, bool> = HashMap::new();
    let mut region_touches_stroke: HashMap<usize, bool> = HashMap::new();
    let mut region_all_stroke: HashMap<usize, bool> = HashMap::new();
    for f in 0..faces.len() {
        if inside_orig[f] {
            continue;
        }
        for side in &sides[f] {
            if side.neighbour.is_none() && (!side.constraint || side.stroke_only) {
                area_exterior.insert(area[f], true);
            }
            if side.neighbour.is_none() && !side.constraint {
                region_open.insert(region[f], true);
            }
            if side.constraint {
                if side.stroke {
                    region_touches_stroke.insert(region[f], true);
                }
                if !side.stroke_only {
                    region_all_stroke.insert(region[f], false);
                }
            }
        }
    }

    let mut kept = vec![];
    for f in 0..faces.len() {
        let keep = if inside_orig[f] {
            true
        } else {
            let r = region[f];
            let enclosed = !region_open.get(&r).copied().unwrap_or(false);
            let touches_stroke = region_touches_stroke.get(&r).copied().unwrap_or(false);
            let all_stroke = region_all_stroke.get(&r).copied().unwrap_or(true);
            let exterior = area_exterior.get(&area[f]).copied().unwrap_or(false);
            enclosed && touches_stroke && (exterior || all_stroke)
        };
        if keep {
            kept.push(faces[f].fix());
        }
    }
    kept
}

fn centroid(p: [Point2<f64>; 3]) -> Point2<f64> {
    Point2::new((p[0].x + p[1].x + p[2].x) / 3., (p[0].y + p[1].y + p[2].y) / 3.)
}

fn cross(o: Point2<f64>, a: Point2<f64>, b: Point2<f64>) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn in_triangle(p: Point2<f64>, t: &[Point2<f64>; 3]) -> bool {
    let d = [cross(t[0], t[1], p), cross(t[1], t[2], p), cross(t[2], t[0], p)];
    let neg = d.iter().any(|x| *x < 0.);
    let pos = d.iter().any(|x| *x > 0.);
    !(neg && pos)
}

fn segment_distance(p: Point2<f64>, a: Point2<f64>, b: Point2<f64>) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0. {
        0.
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0., 1.)
    };
    let (cx, cy) = (a.x + dx * t - p.x, a.y + dy * t - p.y);
    (cx * cx + cy * cy).sqrt()
}

fn majority_winding(bone: &Bone) -> f32 {
    let uv = |i: u32| bone.vertices[i as usize].uv;
    let sum: f32 = bone
        .indices
        .chunks_exact(3)
        .map(|t| topology::cross(uv(t[0]), uv(t[1]), uv(t[2])).signum())
        .sum();
    if sum < 0. {
        -1.
    } else {
        1.
    }
}

fn oriented(bone: &Bone, t: [u32; 3], sign: f32) -> [u32; 3] {
    let uv = |i: u32| bone.vertices[i as usize].uv;
    if topology::cross(uv(t[0]), uv(t[1]), uv(t[2])) * sign < 0. {
        [t[0], t[2], t[1]]
    } else {
        t
    }
}

/// Where a new vertex goes in bone space, given its UV: interpolated inside the
/// mesh (with the corners' indices and weights, for binds), and from the mesh's
/// overall UV-to-position layout outside it.
struct Layout {
    verts: Vec<Vertex>,
    indices: Vec<u32>,
    /// pos = [a b c; d e f] · (u, v, 1), least-squares fit over the vertices.
    affine: Option<[f32; 6]>,
    tex_size: Vec2,
}

impl Layout {
    fn new(bone: &Bone, tex_size: Vec2) -> Self {
        Layout {
            verts: bone.vertices.clone(),
            indices: bone.indices.clone(),
            affine: fit_affine(&bone.vertices.iter().map(|v| (v.uv, v.pos)).collect::<Vec<_>>()),
            tex_size,
        }
    }

    fn place(&self, uv: Vec2) -> (Vec2, Vec<(u32, f32)>) {
        for t in self.indices.chunks_exact(3) {
            let v = [t[0], t[1], t[2]].map(|i| self.verts[i as usize]);
            let (wa, wb, wc) = topology::barycentric(uv, v[0].uv, v[1].uv, v[2].uv);
            if wa >= -1e-5 && wb >= -1e-5 && wc >= -1e-5 {
                let pos = v[0].pos * wa + v[1].pos * wb + v[2].pos * wc;
                return (pos, vec![(t[0], wa), (t[1], wb), (t[2], wc)]);
            }
        }
        let pos = match self.affine {
            Some([a, b, c, d, e, f]) => Vec2::new(a * uv.x + b * uv.y + c, d * uv.x + e * uv.y + f),
            // the texture rect's layout (`renderer::create_tex_rect`)
            None => Vec2::new((uv.x - 0.5) * self.tex_size.x, (0.5 - uv.y) * self.tex_size.y),
        };
        (pos, vec![])
    }
}

/// Least-squares affine map from each pair's first point to its second:
/// `to = [a b c; d e f] · (from.x, from.y, 1)`. None if the points are collinear.
pub(crate) fn fit_affine(pairs: &[(Vec2, Vec2)]) -> Option<[f32; 6]> {
    // normal equations: (Σ x xᵀ) a = Σ x p, with x = (from.x, from.y, 1)
    let mut m = [[0f64; 3]; 3];
    let mut bx = [0f64; 3];
    let mut by = [0f64; 3];
    for (from, to) in pairs {
        let x = [from.x as f64, from.y as f64, 1.];
        for r in 0..3 {
            for c in 0..3 {
                m[r][c] += x[r] * x[c];
            }
            bx[r] += x[r] * to.x as f64;
            by[r] += x[r] * to.y as f64;
        }
    }
    let det = |m: &[[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(&m);
    if d.abs() < 1e-12 {
        return None;
    }
    // Cramer's rule
    let solve = |b: &[f64; 3]| {
        let mut out = [0f32; 3];
        for col in 0..3 {
            let mut mc = m;
            for r in 0..3 {
                mc[r][col] = b[r];
            }
            out[col] = (det(&mc) / d) as f32;
        }
        out
    };
    let [a, b, c] = solve(&bx);
    let [d2, e, f] = solve(&by);
    Some([a, b, c, d2, e, f])
}
