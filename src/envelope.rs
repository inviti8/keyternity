//! Envelope skinning maths (docs/ENVELOPES.md §3): how much a bone's envelope
//! influences a point, the weights of a vertex across several bones, and the
//! conversion between those weights and SkelForm's ordered bind weights.

use crate::*;
use strum::{EnumString, FromRepr};

/// How influence fades from the inner radius (1) to the outer radius (0).
#[derive(
    serde::Serialize, serde::Deserialize, Clone, Copy, Default, PartialEq, Debug, EnumString, FromRepr,
)]
#[repr(i32)]
pub enum Falloff {
    Linear,
    /// smoothstep: eases in and out (the default)
    #[default]
    Smooth,
    /// drops quickly past the inner radius
    FastOut,
    /// stays high until near the outer radius
    SlowOut,
}

impl std::fmt::Display for Falloff {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Falloff {
    /// `x` runs from 0 at the outer radius to 1 at the inner radius.
    pub fn curve(self, x: f32) -> f32 {
        let x = x.clamp(0., 1.);
        match self {
            Falloff::Linear => x,
            Falloff::Smooth => x * x * (3. - 2. * x),
            Falloff::FastOut => x * x,
            Falloff::SlowOut => 1. - (1. - x) * (1. - x),
        }
    }
}

/// A bone's envelope: a capsule along the bone from head to tail, with an inner
/// and an outer radius at each end (world px at rest).
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(default)]
pub struct Envelope {
    /// whether this bone skins meshes at all
    pub deform: bool,
    pub head_inner: f32,
    pub head_outer: f32,
    pub tail_inner: f32,
    pub tail_outer: f32,
    pub falloff: Falloff,
    /// full influence anywhere inside the outer radius (rigid pieces)
    pub absolute: bool,
}

impl Default for Envelope {
    fn default() -> Self {
        Envelope::for_length(0.)
    }
}

impl Envelope {
    /// Default radii for a bone of `length` (docs/ENVELOPES.md §5); a point bone
    /// gets 20 / 60 px.
    pub fn for_length(length: f32) -> Envelope {
        let (inner, outer) = if length > 0. { (0.2 * length, 0.5 * length) } else { (20., 60.) };
        Envelope {
            deform: true,
            head_inner: inner,
            head_outer: outer,
            tail_inner: inner,
            tail_outer: outer,
            falloff: Falloff::Smooth,
            absolute: false,
        }
    }

    /// Distance from `p` to the segment, and the inner and outer radii there.
    fn measure(&self, head: Vec2, tail: Vec2, p: Vec2) -> (f32, f32, f32) {
        let seg = tail - head;
        let len2 = seg.x * seg.x + seg.y * seg.y;
        let t = if len2 > 0. { (((p - head).x * seg.x + (p - head).y * seg.y) / len2).clamp(0., 1.) } else { 0. };
        let d = (p - (head + seg * t)).mag();
        let inner = self.head_inner + (self.tail_inner - self.head_inner) * t;
        let outer = self.head_outer + (self.tail_outer - self.head_outer) * t;
        (d, inner.max(0.), outer.max(0.))
    }

    /// Influence 0..1 of this envelope, along the segment `head`..`tail`, on `p`.
    pub fn influence(&self, head: Vec2, tail: Vec2, p: Vec2) -> f32 {
        let (d, inner, outer) = self.measure(head, tail, p);
        if d >= outer {
            return 0.;
        }
        if self.absolute || d <= inner {
            return 1.;
        }
        self.falloff.curve((outer - d) / (outer - inner))
    }

    /// How far `p` lies outside the outer radius (negative inside).
    pub fn gap(&self, head: Vec2, tail: Vec2, p: Vec2) -> f32 {
        let (d, _, outer) = self.measure(head, tail, p);
        d - outer
    }
}

/// A bone that can skin a vertex: its id, head and tail at rest, and envelope.
#[derive(Clone, Copy, Debug)]
pub struct Skinner {
    pub id: i32,
    pub head: Vec2,
    pub tail: Vec2,
    pub envelope: Envelope,
}

/// Weights of the vertex at `p` across `bones`: the strongest `max_influences`
/// influences, normalized to sum to 1, as (bone id, share), strongest first. A
/// vertex outside every envelope goes fully to the nearest bone, and the flag is
/// true. Empty only when `bones` is.
pub fn vertex_weights(p: Vec2, bones: &[Skinner], max_influences: usize) -> (Vec<(i32, f32)>, bool) {
    let mut found: Vec<(i32, f32)> = bones
        .iter()
        .map(|b| (b.id, b.envelope.influence(b.head, b.tail, p)))
        .filter(|(_, w)| *w > 0.)
        .collect();

    if found.is_empty() {
        let nearest = bones.iter().min_by(|a, b| {
            let ga = a.envelope.gap(a.head, a.tail, p);
            let gb = b.envelope.gap(b.head, b.tail, p);
            ga.partial_cmp(&gb).unwrap_or(std::cmp::Ordering::Equal)
        });
        return match nearest {
            Some(b) => (vec![(b.id, 1.)], true),
            None => (vec![], false),
        };
    }

    // stable: equal influences keep the bones' order
    found.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    found.truncate(max_influences.max(1));
    let sum: f32 = found.iter().map(|(_, w)| w).sum();
    for (_, w) in &mut found {
        *w /= sum;
    }
    (found, false)
}

/// Ordered bind weights that give each bind exactly its share (docs/ENVELOPES.md §3.3).
///
/// `owner` is the share the vertex keeps on its owner bone and `shares` the binds'
/// shares in bind order; together they sum to 1. Binds blend in order, each pulling
/// the vertex toward its bone by its weight (`renderer::construct_verts`), so bind k
/// needs `w_k = e_k / (e_0 + e_1 + … + e_k)`.
pub fn bind_weights(owner: f32, shares: &[f32]) -> Vec<f32> {
    let mut so_far = owner;
    shares
        .iter()
        .map(|&e| {
            so_far += e;
            if so_far > 0. { (e / so_far).clamp(0., 1.) } else { 0. }
        })
        .collect()
}

/// The inverse of `bind_weights`: each bind's effective share, and the owner's,
/// from ordered bind weights. Bind k keeps `w_k` times what every later bind leaves.
pub fn shares(weights: &[f32]) -> (f32, Vec<f32>) {
    let mut out = vec![0.; weights.len()];
    let mut left = 1.;
    for k in (0..weights.len()).rev() {
        let w = weights[k].clamp(0., 1.);
        out[k] = w * left;
        left *= 1. - w;
    }
    (left, out)
}
