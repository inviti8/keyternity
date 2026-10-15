//! Hand weighting on normalized shares (docs/ENVELOPES.md §4.5–4.6).
//!
//! A vertex's **shares** are how much each bone moves it, summing to 1, with the
//! owner's share under the mesh's own id. Binds store blend weights instead
//! (`envelope::bind_weights`), where a later bind overrides an earlier one, so
//! hand edits go through shares: setting one bone's share scales the others to
//! make up the rest, as Max and Blender (Auto Normalize) do.

use crate::Vec2;
use std::collections::{BinaryHeap, HashMap};
use strum::FromRepr;

/// A vertex's shares: (bone id, share), summing to 1.
pub type Shares = Vec<(i32, f32)>;

/// Shares under this after an edit are dropped and the rest renormalized, so binds
/// don't fill with near-zero entries.
pub const PRUNE: f32 = 1e-3;

/// How far one dab of Add or Subtract at full strength and the brush centre moves
/// a share.
pub const DAB_RATE: f32 = 0.15;

/// Share of one dab of Smooth at full strength: how far toward the neighbours'
/// average a vertex moves.
pub const SMOOTH_RATE: f32 = 0.5;

/// Weight painting modes (§4.5).
#[derive(Clone, Copy, PartialEq, Debug, Default, FromRepr)]
pub enum PaintMode {
    #[default]
    Add,
    Subtract,
    Smooth,
}

pub fn share_of(shares: &Shares, bone: i32) -> f32 {
    shares.iter().filter(|s| s.0 == bone).map(|s| s.1).sum()
}

/// Set `bone`'s share to `value` (0..1) and scale the other bones to make up the
/// rest, keeping their ratios. With no other share to scale, the rest goes to
/// `fallback`; if that's `bone` itself, the share can't drop below 1.
pub fn set_share(shares: &mut Shares, bone: i32, value: f32, fallback: i32) {
    let mut value = value.clamp(0., 1.);
    let others: f32 = shares.iter().filter(|s| s.0 != bone).map(|s| s.1).sum();
    let rest = 1. - value;
    shares.retain(|s| s.0 != bone);
    if others > 1e-6 {
        for s in shares.iter_mut() {
            s.1 *= rest / others;
        }
    } else if rest > 0. {
        if fallback == bone {
            value = 1.;
        } else {
            shares.push((fallback, rest));
        }
    }
    shares.push((bone, value));
    tidy(shares);
}

/// Keep at most `max` bones besides `owner`, dropping the weakest other than `keep`,
/// then renormalize.
pub fn limit(shares: &mut Shares, max: usize, keep: i32, owner: i32) {
    loop {
        let bones = shares.iter().filter(|s| s.0 != owner).count();
        if bones <= max.max(1) {
            break;
        }
        let weakest = shares
            .iter()
            .enumerate()
            .filter(|(_, s)| s.0 != owner && s.0 != keep)
            .min_by(|a, b| a.1 .1.partial_cmp(&b.1 .1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i);
        match weakest {
            Some(i) => {
                shares.remove(i);
            }
            None => break,
        }
    }
    tidy(shares);
}

/// Prune tiny shares and renormalize to 1.
fn tidy(shares: &mut Shares) {
    shares.retain(|s| s.1 >= PRUNE);
    let sum: f32 = shares.iter().map(|s| s.1).sum();
    if sum > 0. {
        for s in shares.iter_mut() {
            s.1 /= sum;
        }
    }
}

/// The share a dab of `mode` gives a vertex now at `current`, with `amount` the
/// brush strength times its falloff there and `neighbours` the average share of the
/// vertex's edge neighbours (for Smooth).
pub fn painted(mode: PaintMode, current: f32, amount: f32, neighbours: f32) -> f32 {
    match mode {
        PaintMode::Add => current + amount * DAB_RATE,
        PaintMode::Subtract => current - amount * DAB_RATE,
        PaintMode::Smooth => current + (neighbours - current) * amount * SMOOTH_RATE,
    }
    .clamp(0., 1.)
}

/// The brush falloff at `d` from the centre of a brush of `radius`: 1 at the centre,
/// easing to 0 at the edge.
pub fn falloff(d: f32, radius: f32) -> f32 {
    if radius <= 0. || d >= radius {
        return 0.;
    }
    let t = 1. - (d / radius).powi(2);
    t * t
}

// --- regions (docs/AI_SKINNING.md §4.1) ----------------------------------------


/// Whether `p` is inside `polygon` (even-odd rule; any winding).
pub fn in_polygon(p: Vec2, polygon: &[Vec2]) -> bool {
    let mut inside = false;
    let n = polygon.len();
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

/// Distance from `p` to the nearest edge of `polygon`.
pub fn polygon_edge_distance(p: Vec2, polygon: &[Vec2]) -> f32 {
    let n = polygon.len();
    (0..n)
        .map(|i| {
            let (a, b) = (polygon[i], polygon[(i + 1) % n]);
            let ab = b - a;
            let len2 = ab.x * ab.x + ab.y * ab.y;
            let t = if len2 > 0. { (((p - a).x * ab.x + (p - a).y * ab.y) / len2).clamp(0., 1.) } else { 0. };
            (p - (a + ab * t)).mag()
        })
        .fold(f32::MAX, f32::min)
}

/// Distance from the nearest of `sources` to every vertex, along mesh `edges`
/// (vertex index pairs) with vertices at `pos`: geodesic, so it goes round gaps in
/// the mesh instead of across them. Unreachable vertices get `f32::MAX`.
pub fn geodesic(pos: &[Vec2], edges: &[(u32, u32)], sources: &[usize]) -> Vec<f32> {
    let mut next: Vec<Vec<usize>> = vec![vec![]; pos.len()];
    for &(a, b) in edges {
        next[a as usize].push(b as usize);
        next[b as usize].push(a as usize);
    }
    let mut dist = vec![f32::MAX; pos.len()];
    // distances are never negative, so their bits order like the floats
    let mut heap = BinaryHeap::new();
    for &s in sources {
        dist[s] = 0.;
        heap.push(std::cmp::Reverse((0u32, s)));
    }
    while let Some(std::cmp::Reverse((bits, v))) = heap.pop() {
        let d = f32::from_bits(bits);
        if d > dist[v] {
            continue;
        }
        for &u in &next[v] {
            let nd = d + (pos[u] - pos[v]).mag();
            if nd < dist[u] {
                dist[u] = nd;
                heap.push(std::cmp::Reverse((nd.to_bits(), u)));
            }
        }
    }
    dist
}

/// Shares from a segmentation (§4.1 `assign_parts`): `labels[v]` is the bone vertex
/// `v` was drawn into (or `None`), `pos` the rest positions and `edges` the mesh's.
/// A labelled vertex goes fully to its bone, except within `blend` (measured along
/// the mesh) of another part, where the two blend, half and half at the boundary.
/// Unlabelled vertices are left out of the result.
pub fn part_shares(pos: &[Vec2], edges: &[(u32, u32)], labels: &[Option<i32>], blend: f32) -> HashMap<usize, Shares> {
    let mut bones: Vec<i32> = labels.iter().flatten().copied().collect();
    bones.sort();
    bones.dedup();
    let dist: Vec<(i32, Vec<f32>)> = bones
        .iter()
        .map(|b| {
            let sources: Vec<usize> = (0..labels.len()).filter(|v| labels[*v] == Some(*b)).collect();
            (*b, geodesic(pos, edges, &sources))
        })
        .collect();

    let mut out = HashMap::new();
    for v in 0..labels.len() {
        let Some(own) = labels[v] else { continue };
        let mut shares: Shares = vec![];
        for (bone, d) in &dist {
            let raw = if *bone == own {
                1.
            } else if blend > 0. && d[v] < blend {
                (1. - d[v] / blend).powi(2)
            } else {
                0.
            };
            if raw > 0. {
                shares.push((*bone, raw));
            }
        }
        tidy(&mut shares);
        out.insert(v, shares);
    }
    out
}

/// The share of the child bone across a joint (§4.1 `blend_joint`): `x` is how far
/// along the child bone's direction a point is from the joint; the share eases from
/// 0 at `-width / 2` to 1 at `width / 2`.
pub fn joint_share(x: f32, width: f32) -> f32 {
    if width <= 0. {
        return if x >= 0. { 1. } else { 0. };
    }
    let t = (x / width + 0.5).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Every vertex's shares averaged with its edge neighbours', `iterations` times, for
/// the vertices in `only` (§4.1 `smooth_region` over all bones). `all` holds every
/// vertex's shares by index.
pub fn blur(all: &mut Vec<Shares>, edges: &[(u32, u32)], only: &[usize], iterations: u32) {
    let mut next: Vec<Vec<usize>> = vec![vec![]; all.len()];
    for &(a, b) in edges {
        next[a as usize].push(b as usize);
        next[b as usize].push(a as usize);
    }
    for _ in 0..iterations {
        let before = all.clone();
        for &v in only {
            let mut sum: HashMap<i32, f32> = HashMap::new();
            let around = std::iter::once(v).chain(next[v].iter().copied());
            let mut n = 0.;
            for u in around {
                n += 1.;
                for (bone, s) in &before[u] {
                    *sum.entry(*bone).or_default() += s;
                }
            }
            let mut shares: Shares = sum.into_iter().map(|(b, s)| (b, s / n)).collect();
            shares.sort_by_key(|s| s.0);
            tidy(&mut shares);
            all[v] = shares;
        }
    }
}
