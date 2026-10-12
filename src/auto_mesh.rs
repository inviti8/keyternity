//! Trace: a mesh from a texture's opaque pixels (docs/AUTO_MESH.md).
//!
//! 1. Mask the pixels at or above the alpha threshold and grow it by `padding`.
//! 2. Follow the boundary of every opaque piece and every hole, in order.
//! 3. Simplify each outline (Ramer–Douglas–Peucker) and drop tiny pieces.
//! 4. Triangulate with the outlines as constraints, refine the inside to
//!    `density`, and keep the faces inside an odd number of outlines.

use crate::*;
use spade::{AngleLimit, ConstrainedDelaunayTriangulation, Point2, RefinementParameters, Triangulation};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceOptions {
    /// Outline simplification tolerance, in px. Higher means fewer vertices.
    pub detail: f32,
    /// How far the outline sits outside the art, in px.
    pub padding: f32,
    /// Target interior edge length, in px. 0 gives an outline-only mesh.
    pub density: f32,
    /// Pixels with alpha at or above this count as art.
    pub alpha_threshold: u8,
    /// Pieces and holes smaller than this fraction of the texture are dropped.
    pub min_piece: f32,
}

impl TraceOptions {
    /// The defaults for a `width` x `height` texture. Detail and density scale
    /// with the texture, so a large texture doesn't get a crowded mesh. Padding is
    /// at least the detail, because simplifying can cut that far into the art.
    pub fn for_size(width: u32, height: u32) -> TraceOptions {
        let short = width.min(height) as f32;
        let detail = (short / 100.).max(1.);
        TraceOptions {
            detail,
            padding: detail.max(2.),
            density: (short / 6.).max(4.),
            alpha_threshold: 16,
            min_piece: 0.001,
        }
    }
}

/// Angle limit for refinement: no interior triangle gets thinner than this.
pub const MIN_ANGLE_DEG: f64 = 25.;
/// Refinement stops adding vertices past this many, whatever the density.
pub const MAX_ADDED_VERTS: usize = 4000;

/// The mesh for `texture`: vertices (pos in px with y up, uv 0..1) and
/// triangle indices. Empty if nothing in the texture is opaque enough.
pub fn trace(texture: &image::DynamicImage, opts: &TraceOptions) -> (Vec<Vertex>, Vec<u32>) {
    let rgba = texture.to_rgba8();
    let (w, h) = rgba.dimensions();
    let alpha: Vec<u8> = rgba.pixels().map(|p| p.0[3]).collect();
    trace_alpha(&alpha, w, h, opts)
}

/// `trace` on a plain alpha channel (row-major, `w` x `h`).
pub fn trace_alpha(alpha: &[u8], w: u32, h: u32, opts: &TraceOptions) -> (Vec<Vertex>, Vec<u32>) {
    if w == 0 || h == 0 {
        return (vec![], vec![]);
    }
    let mask = grow(&threshold(alpha, opts.alpha_threshold), w as usize, h as usize, opts.padding);

    let min_area = opts.min_piece.max(0.) as f64 * w as f64 * h as f64;
    let loops: Vec<Vec<[f64; 2]>> = outlines(&mask, w as usize, h as usize)
        .into_iter()
        .filter(|l| polygon_area(l).abs() >= min_area.max(1.))
        .map(|l| simplify_loop(&l, opts.detail.max(0.) as f64))
        .filter(|l| l.len() >= 3)
        .collect();
    if loops.is_empty() {
        return (vec![], vec![]);
    }

    let Some((points, tris)) = triangulate(&loops, opts.density as f64) else {
        return (vec![], vec![]);
    };

    // same space as the default quad (`renderer::create_tex_rect`): the texture's
    // center at the origin, so the art stays where it was
    let size = Vec2::new(w as f32, h as f32);
    let half = size / 2.;
    let verts: Vec<Vertex> = points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let px = Vec2::new(p[0] as f32, p[1] as f32);
            Vertex { id: i as u32, pos: Vec2::new(px.x - half.x, half.y - px.y), uv: px / size, ..Default::default() }
        })
        .collect();
    (verts, tris)
}

/// Opaque pixels: alpha at or above `min`.
pub fn threshold(alpha: &[u8], min: u8) -> Vec<bool> {
    alpha.iter().map(|&a| a >= min.max(1)).collect()
}

/// Grow `mask` by `radius` px (round brush), using a 3-4 chamfer distance
/// transform: two passes, so the cost doesn't depend on the radius.
pub fn grow(mask: &[bool], w: usize, h: usize, radius: f32) -> Vec<bool> {
    if radius < 0.5 {
        return mask.to_vec();
    }
    const FAR: u32 = u32::MAX / 2;
    let mut d: Vec<u32> = mask.iter().map(|&m| if m { 0 } else { FAR }).collect();
    let idx = |x: usize, y: usize| y * w + x;
    for y in 0..h {
        for x in 0..w {
            let mut v = d[idx(x, y)];
            if x > 0 { v = v.min(d[idx(x - 1, y)] + 3); }
            if y > 0 {
                v = v.min(d[idx(x, y - 1)] + 3);
                if x > 0 { v = v.min(d[idx(x - 1, y - 1)] + 4); }
                if x + 1 < w { v = v.min(d[idx(x + 1, y - 1)] + 4); }
            }
            d[idx(x, y)] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let mut v = d[idx(x, y)];
            if x + 1 < w { v = v.min(d[idx(x + 1, y)] + 3); }
            if y + 1 < h {
                v = v.min(d[idx(x, y + 1)] + 3);
                if x + 1 < w { v = v.min(d[idx(x + 1, y + 1)] + 4); }
                if x > 0 { v = v.min(d[idx(x - 1, y + 1)] + 4); }
            }
            d[idx(x, y)] = v;
        }
    }
    let limit = (radius * 3.).round() as u32;
    d.iter().map(|&v| v <= limit).collect()
}

/// Every boundary between opaque and transparent pixels, as closed loops of
/// pixel-corner points (x right, y down). Outer outlines and holes run in
/// opposite directions. Pixels touching only at a corner count as separate.
pub fn outlines(mask: &[bool], w: usize, h: usize) -> Vec<Vec<[f64; 2]>> {
    let inside = |x: i64, y: i64| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && mask[y as usize * w + x as usize];

    // directed boundary edges with the opaque pixel on the right (y down),
    // keyed by their start corner
    let cw = w + 1;
    let key = |x: i64, y: i64| y as usize * cw + x as usize;
    let mut out: std::collections::HashMap<usize, Vec<(i64, i64)>> = Default::default();
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if !inside(x, y) {
                continue;
            }
            let mut add = |a: (i64, i64), b: (i64, i64)| out.entry(key(a.0, a.1)).or_default().push(b);
            if !inside(x, y - 1) { add((x, y), (x + 1, y)); }
            if !inside(x + 1, y) { add((x + 1, y), (x + 1, y + 1)); }
            if !inside(x, y + 1) { add((x + 1, y + 1), (x, y + 1)); }
            if !inside(x - 1, y) { add((x, y + 1), (x, y)); }
        }
    }

    let mut loops = vec![];
    let mut starts: Vec<usize> = out.keys().copied().collect();
    starts.sort_unstable();
    for start in starts {
        while out.get(&start).map_or(false, |v| !v.is_empty()) {
            let (sx, sy) = ((start % cw) as i64, (start / cw) as i64);
            let mut pts: Vec<[f64; 2]> = vec![];
            let (mut cx, mut cy) = (sx, sy);
            let mut dir = (0i64, 0i64);
            loop {
                let list = out.get_mut(&key(cx, cy)).unwrap();
                // at a corner with two ways out (diagonal pixels), turn right,
                // which keeps the two pixels in separate loops
                let pick = if list.len() > 1 {
                    let right = (-dir.1, dir.0);
                    list.iter().position(|&(nx, ny)| (nx - cx, ny - cy) == right).unwrap_or(0)
                } else {
                    0
                };
                let (nx, ny) = list.swap_remove(pick);
                let ndir = (nx - cx, ny - cy);
                // keep corners only: a straight run adds no point
                if ndir != dir {
                    pts.push([cx as f64, cy as f64]);
                }
                dir = ndir;
                cx = nx;
                cy = ny;
                if (cx, cy) == (sx, sy) {
                    break;
                }
            }
            // the start may be mid-run: drop it if it's collinear with its neighbours
            if pts.len() >= 3 {
                let (a, b, c) = (pts[pts.len() - 1], pts[0], pts[1]);
                if (b[0] - a[0]) * (c[1] - b[1]) == (b[1] - a[1]) * (c[0] - b[0]) {
                    pts.remove(0);
                }
            }
            if pts.len() >= 3 {
                loops.push(pts);
            }
        }
    }
    loops
}

/// Signed area (shoelace); the sign gives the winding.
pub fn polygon_area(pts: &[[f64; 2]]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.
}

/// Ramer–Douglas–Peucker on a closed loop: split at the point farthest from
/// the first, simplify both halves, and join them.
pub fn simplify_loop(pts: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    let n = pts.len();
    if n < 4 || tolerance <= 0. {
        return pts.to_vec();
    }
    let dist2 = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
    let far = (1..n).max_by(|&i, &j| dist2(pts[0], pts[i]).total_cmp(&dist2(pts[0], pts[j]))).unwrap();

    let mut keep = vec![false; n];
    keep[0] = true;
    keep[far] = true;
    rdp(pts, 0, far, tolerance, &mut keep);
    rdp(pts, far, n, tolerance, &mut keep);
    (0..n).filter(|&i| keep[i]).map(|i| pts[i]).collect()
}

/// Marks the points to keep between `from` and `to` (exclusive; `to` may be
/// `pts.len()`, meaning back to the first point).
fn rdp(pts: &[[f64; 2]], from: usize, to: usize, tolerance: f64, keep: &mut [bool]) {
    let n = pts.len();
    let mut stack = vec![(from, to)];
    while let Some((s, e)) = stack.pop() {
        if e <= s + 1 {
            continue;
        }
        let (a, b) = (pts[s], pts[e % n]);
        let mut best = (0., s);
        for i in s + 1..e {
            let d = segment_distance(pts[i], a, b);
            if d > best.0 {
                best = (d, i);
            }
        }
        if best.0 > tolerance {
            keep[best.1] = true;
            stack.push((s, best.1));
            stack.push((best.1, e));
        }
    }
}

fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 == 0. { 0. } else { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0., 1.) };
    let q = [a[0] + d[0] * t, a[1] + d[1] * t];
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
}

/// Constrained triangulation of `loops`, refined to `density` px, keeping the
/// faces inside an odd number of loops. Points and triangle indices.
fn triangulate(loops: &[Vec<[f64; 2]>], density: f64) -> Option<(Vec<[f64; 2]>, Vec<u32>)> {
    let mut cdt: ConstrainedDelaunayTriangulation<Point2<f64>> = ConstrainedDelaunayTriangulation::new();
    for l in loops {
        let handles: Vec<_> = l.iter().map(|p| cdt.insert(Point2::new(p[0], p[1]))).collect::<Result<_, _>>().ok()?;
        for i in 0..handles.len() {
            let (a, b) = (handles[i], handles[(i + 1) % handles.len()]);
            if a != b {
                // outlines simplified separately can cross; split where they do
                cdt.add_constraint_and_split(a, b, |p| p);
            }
        }
    }

    if density > 0. {
        let area = 0.5 * density * density;
        let params = RefinementParameters::new()
            .exclude_outer_faces(true)
            .with_angle_limit(AngleLimit::from_deg(MIN_ANGLE_DEG))
            .with_max_allowed_area(area)
            .with_min_required_area(area / 64.)
            .with_max_additional_vertices(MAX_ADDED_VERTS);
        cdt.refine(params);
    }

    let mut index_of: std::collections::HashMap<usize, u32> = Default::default();
    let mut points = vec![];
    let mut tris = vec![];
    for face in cdt.inner_faces() {
        let vs = face.vertices();
        let ps = vs.map(|v| v.position());
        let c = [(ps[0].x + ps[1].x + ps[2].x) / 3., (ps[0].y + ps[1].y + ps[2].y) / 3.];
        if !inside_loops(c, loops) {
            continue;
        }
        for v in vs {
            let i = *index_of.entry(v.fix().index()).or_insert_with(|| {
                points.push([v.position().x, v.position().y]);
                points.len() as u32 - 1
            });
            tris.push(i);
        }
    }
    if tris.is_empty() {
        return None;
    }
    Some((points, tris))
}

/// Even-odd point-in-polygon over all loops: inside a piece, not in its holes.
pub fn inside_loops(p: [f64; 2], loops: &[Vec<[f64; 2]>]) -> bool {
    let mut inside = false;
    for l in loops {
        let n = l.len();
        for i in 0..n {
            let (a, b) = (l[i], l[(i + 1) % n]);
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                if p[0] < x {
                    inside = !inside;
                }
            }
        }
    }
    inside
}
