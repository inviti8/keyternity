//! Hand weighting on normalized shares (docs/ENVELOPES.md §4.5–4.6).
//!
//! A vertex's **shares** are how much each bone moves it, summing to 1, with the
//! owner's share under the mesh's own id. Binds store blend weights instead
//! (`envelope::bind_weights`), where a later bind overrides an earlier one, so
//! hand edits go through shares: setting one bone's share scales the others to
//! make up the rest, as Max and Blender (Auto Normalize) do.

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
