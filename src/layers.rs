//! Animation layers (docs/TIMELINE.md §5): evaluation, keying through a layer, and
//! flattening.
//!
//! An animation's layers stack bottom to top: Base (index 0) is the animation as it
//! always was; each layer above it overrides or adds to the channels it has keys for.
//!
//! The active layer's keys live in `Animation::keyframes`, so every existing editing
//! tool works on the active layer unchanged; the other layers keep theirs in
//! `AnimLayer::keyframes`. `Animation::layer_keys(i)` hides the difference. Saving puts
//! Base back in `keyframes` (`normalized`), so files read as before.

use crate::shared::*;
use crate::utils;
use std::str::FromStr;

/// The numeric channels: interpolated, and blended by weight.
pub const NUMERIC: [AnimElement; 10] = [
    AnimElement::PositionX,
    AnimElement::PositionY,
    AnimElement::Rotation,
    AnimElement::ScaleX,
    AnimElement::ScaleY,
    AnimElement::TintR,
    AnimElement::TintG,
    AnimElement::TintB,
    AnimElement::TintA,
    AnimElement::IkWeight,
];

/// The discrete channels: stepped, and taken from the top layer that keys them.
pub const DISCRETE: [AnimElement; 5] =
    [AnimElement::Zindex, AnimElement::Hidden, AnimElement::Texture, AnimElement::IkCopyRotation, AnimElement::IkBend];

/// A channel's interpolated value at `frame` from `keys`, or `None` without keys. Before
/// the first key it holds the first key's value, after the last the last's (as Base
/// always has).
pub fn channel(keys: &[Keyframe], bone_id: i32, el: &AnimElement, frame: i32) -> Option<f32> {
    let matches = |kf: &&Keyframe| kf.bone_id == bone_id && kf.element == *el;
    let prev = keys.iter().filter(matches).filter(|kf| kf.frame <= frame).last();
    let next = keys.iter().filter(matches).find(|kf| kf.frame > frame);
    let (prev, next) = match (prev, next) {
        (None, None) => return None,
        (Some(p), None) => (p, p),
        (None, Some(n)) => (n, n),
        (Some(p), Some(n)) => (p, n),
    };
    Some(utils::interp(frame - prev.frame, next.frame - prev.frame, prev.value, next.value, next.start_handle, next.end_handle))
}

/// A discrete channel's key in effect at `frame` (the last one on or before it).
pub fn step<'a>(keys: &'a [Keyframe], bone_id: i32, el: &AnimElement, frame: i32) -> Option<&'a Keyframe> {
    keys.iter().filter(|kf| kf.bone_id == bone_id && kf.element == *el && kf.frame <= frame).last()
}

pub fn get(b: &Bone, el: &AnimElement) -> f32 {
    use AnimElement as E;
    match el {
        E::PositionX => b.pos.x,
        E::PositionY => b.pos.y,
        E::Rotation => b.rot,
        E::ScaleX => b.scale.x,
        E::ScaleY => b.scale.y,
        E::TintR => b.tint.r,
        E::TintG => b.tint.g,
        E::TintB => b.tint.b,
        E::TintA => b.tint.a,
        E::IkWeight => b.ik.as_ref().map_or(0., |ik| ik.weight),
        E::Zindex => b.zindex as f32,
        E::Hidden => bool_as_f32(b.hidden),
        E::IkCopyRotation => b.ik.as_ref().map_or(0., |ik| bool_as_f32(ik.copy_rotation)),
        _ => 0.,
    }
}

fn set(b: &mut Bone, el: &AnimElement, v: f32) {
    use AnimElement as E;
    match el {
        E::PositionX => b.pos.x = v,
        E::PositionY => b.pos.y = v,
        E::Rotation => b.rot = v,
        E::ScaleX => b.scale.x = v,
        E::ScaleY => b.scale.y = v,
        E::TintR => b.tint.r = v,
        E::TintG => b.tint.g = v,
        E::TintB => b.tint.b = v,
        E::TintA => b.tint.a = v,
        E::IkWeight => {
            if let Some(ik) = b.ik.as_mut() {
                ik.weight = v
            }
        }
        _ => {}
    }
}

/// Set a discrete channel from its key.
fn set_step(b: &mut Bone, kf: &Keyframe) {
    use AnimElement as E;
    match kf.element {
        E::Zindex => b.zindex = kf.value as i32,
        E::Hidden => b.hidden = kf.value != 0.,
        E::Texture => b.tex = kf.value_str.clone(),
        E::IkCopyRotation => {
            if let Some(ik) = b.ik.as_mut() {
                ik.copy_rotation = kf.value != 0.
            }
        }
        E::IkBend => {
            if let (Some(ik), Ok(bend)) = (b.ik.as_mut(), IkBend::from_str(&kf.value_str)) {
                ik.bend = bend
            }
        }
        _ => {}
    }
}

/// Combine a layer's value `k` over `below`. `rest` is the channel's setup value: an
/// additive layer adds its offset from it (scale multiplies by its ratio to it).
pub fn blend(below: f32, k: f32, rest: f32, mode: &LayerMode, w: f32, el: &AnimElement) -> f32 {
    use AnimElement as E;
    match mode {
        LayerMode::Override => below + (k - below) * w,
        LayerMode::Additive => match el {
            E::ScaleX | E::ScaleY if rest != 0. => below * (1. + w * (k / rest - 1.)),
            // tint and IK weight blend toward the layer's value, as an override
            E::TintR | E::TintG | E::TintB | E::TintA | E::IkWeight => below + (k - below) * w,
            _ => below + w * (k - rest),
        },
    }
}

/// The key a layer needs so that `blend` gives `target` over `below`: what editing
/// through a layer writes. With no weight the layer can't change anything, so the
/// target itself is keyed.
pub fn key_for(target: f32, below: f32, rest: f32, mode: &LayerMode, w: f32, el: &AnimElement) -> f32 {
    use AnimElement as E;
    if w.abs() < 1e-4 {
        return target;
    }
    match mode {
        LayerMode::Override => below + (target - below) / w,
        LayerMode::Additive => match el {
            E::ScaleX | E::ScaleY if rest != 0. && below != 0. => rest * (1. + (target / below - 1.) / w),
            E::TintR | E::TintG | E::TintB | E::TintA | E::IkWeight => below + (target - below) / w,
            _ => rest + (target - below) / w,
        },
    }
}

/// Whether layer `i` plays: Base always does; others unless muted, and only soloed
/// ones while any layer is soloed.
pub fn enabled(anim: &Animation, i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let Some(layer) = anim.layers.get(i) else { return false };
    let any_solo = anim.layers.iter().skip(1).any(|l| l.solo);
    !layer.muted && (!any_solo || layer.solo)
}

/// What evaluating a layer can need beyond its animation: the other animations (for
/// linked layers) and, while the editor plays, its clock (for free-running loops).
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub anims: &'a [Animation],
    pub clock: Option<f32>,
}

impl Ctx<'_> {
    pub const NONE: Ctx<'static> = Ctx { anims: &[], clock: None };
}

/// A Looped layer's length in seconds, if it loops.
pub fn loop_length(layer: &AnimLayer) -> Option<f32> {
    match layer.play {
        PlayMode::Looped { length, .. } if length > 0. => Some(length),
        _ => None,
    }
}

/// The layer's own time (seconds) when its animation is at `t` seconds
/// (docs/TIMELINE.md §5.7): `(t × speed + phase) mod length` for a loop, with `t` from
/// the editor's clock instead when it's free-running and the editor is playing.
pub fn local_time(layer: &AnimLayer, t: f32, clock: Option<f32>) -> f32 {
    match layer.play {
        PlayMode::Looped { length, speed, phase, free_running } if length > 0. => {
            let t = if free_running { clock.unwrap_or(t) } else { t };
            (t * speed + phase).rem_euclid(length)
        }
        _ => t,
    }
}

/// The frame (in its keys' time) a layer is on at its animation's `frame`: where
/// editing it keys (docs/TIMELINE.md §5.7). Without a loop, the same frame.
pub fn layer_frame(anim: &Animation, layer: &AnimLayer, frame: i32) -> i32 {
    if loop_length(layer).is_none() {
        return frame;
    }
    let fps = anim.fps.max(1) as f32;
    let f = (local_time(layer, frame as f32 / fps, None) * fps).round() as i32;
    // the loop's last frame is its first
    let len = loop_frames(anim, layer).unwrap_or(i32::MAX);
    if f >= len { 0 } else { f }
}

/// A Looped layer's length in its animation's frames.
pub fn loop_frames(anim: &Animation, layer: &AnimLayer) -> Option<i32> {
    loop_length(layer).map(|l| (l * anim.fps.max(1) as f32).round().max(1.) as i32)
}

/// A channel's value at a fractional `frame` (layers playing at another speed land
/// between frames).
fn channel_at(keys: &[Keyframe], bone_id: i32, el: &AnimElement, frame: f32) -> Option<f32> {
    let whole = frame.floor() as i32;
    if (frame - whole as f32).abs() < 1e-4 {
        return channel(keys, bone_id, el, whole);
    }
    let matches = |kf: &&Keyframe| kf.bone_id == bone_id && kf.element == *el;
    let prev = keys.iter().filter(matches).filter(|kf| kf.frame as f32 <= frame).last();
    let next = keys.iter().filter(matches).find(|kf| kf.frame as f32 > frame);
    let (prev, next) = match (prev, next) {
        (None, None) => return None,
        (Some(p), None) => (p, p),
        (None, Some(n)) => (n, n),
        (Some(p), Some(n)) => (p, n),
    };
    // `interp` takes whole steps: measure in thousandths of a frame
    let span = ((next.frame - prev.frame) * 1000).max(0);
    let at = ((frame - prev.frame as f32) * 1000.).round() as i32;
    Some(utils::interp(at, span, prev.value, next.value, next.start_handle, next.end_handle))
}

/// A generator's value at the layer's time `t` (seconds). On a loop of `loop_len`
/// seconds a wave's period snaps to a whole fraction of it, and noise repeats over it,
/// so the result always loops (docs/TIMELINE.md §5.8).
pub fn generator_value(g: &Generator, t: f32, loop_len: Option<f32>) -> f32 {
    let mut period = g.period.max(1e-3);
    let mut cells: Option<i64> = None;
    if let Some(len) = loop_len {
        let n = (len / period).round().max(1.);
        period = len / n;
        cells = Some(n as i64);
    }
    let x = (t + g.phase) / period;
    let wave = match g.wave {
        Wave::Sine => (x * std::f32::consts::TAU).sin(),
        Wave::Triangle => 4. * ((x - 0.25).rem_euclid(1.) - 0.5).abs() - 1.,
        Wave::Square => {
            if x.rem_euclid(1.) < 0.5 {
                1.
            } else {
                -1.
            }
        }
        Wave::Noise => noise(x, g.octaves.max(1), g.seed, cells),
    };
    g.offset + g.amplitude * wave
}

/// Smooth 1D value noise in -1..1, with `octaves` layers; with `wrap`, it repeats every
/// `wrap` units.
fn noise(x: f32, octaves: u32, seed: u32, wrap: Option<i64>) -> f32 {
    let hash = |i: i64, o: u32| -> f32 {
        let mut h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ ((seed as u64) << 32) ^ (o as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        h ^= h >> 31;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 29;
        (h & 0xFFFF) as f32 / 32767.5 - 1.
    };
    let (mut sum, mut norm, mut amp, mut freq) = (0., 0., 1., 1.);
    for o in 0..octaves.min(6) {
        let p = x * freq;
        let i = p.floor() as i64;
        let f = p - i as f32;
        let w = wrap.map(|w| w * freq as i64);
        let cell = |i: i64| w.map_or(i, |w| i.rem_euclid(w.max(1)));
        let s = f * f * (3. - 2. * f);
        sum += amp * (hash(cell(i), o) * (1. - s) + hash(cell(i + 1), o) * s);
        norm += amp;
        amp *= 0.5;
        freq *= 2.;
    }
    sum / norm
}

/// The keys and frame rate a layer plays: its own, or the Base keys of the animation
/// it links (never itself).
fn layer_source<'a>(anim: &'a Animation, ctx: &Ctx<'a>, i: usize) -> (&'a [Keyframe], i32, &'a [Generator]) {
    let layer = &anim.layers[i];
    if let Some(linked) = layer.link.filter(|id| *id != anim.id).and_then(|id| ctx.anims.iter().find(|a| a.id == id)) {
        let gens: &[Generator] = linked.layers.first().map_or(&[], |b| b.generators.as_slice());
        return (linked.layer_keys(0), linked.fps, gens);
    }
    (anim.layer_keys(i), anim.fps, &[])
}

/// The pose at `frame`: `setup` with Base applied, then each layer `include` allows.
pub fn evaluate(anim: &Animation, setup: &[Bone], frame: i32, include: &dyn Fn(usize) -> bool) -> Vec<Bone> {
    evaluate_in(anim, &Ctx::NONE, setup, frame, include)
}

pub fn evaluate_in(anim: &Animation, ctx: &Ctx, setup: &[Bone], frame: i32, include: &dyn Fn(usize) -> bool) -> Vec<Bone> {
    let mut bones = setup.to_vec();
    let base = anim.layer_keys(0);
    for b in &mut bones {
        for el in NUMERIC.iter() {
            if let Some(v) = channel(base, b.id, el, frame) {
                set(b, el, v);
            }
        }
        for el in DISCRETE.iter() {
            if let Some(kf) = step(base, b.id, el, frame) {
                set_step(b, &kf.clone());
            }
        }
    }
    let t = frame as f32 / anim.fps.max(1) as f32;
    for i in 1..anim.layers.len() {
        if !include(i) {
            continue;
        }
        let layer = &anim.layers[i];
        let (keys, fps, linked_gens) = layer_source(anim, ctx, i);
        if keys.is_empty() && layer.generators.is_empty() && linked_gens.is_empty() {
            continue;
        }
        let lt = local_time(layer, t, ctx.clock);
        let lf = lt * fps.max(1) as f32;
        let loop_len = loop_length(layer);
        for (b, rest) in bones.iter_mut().zip(setup) {
            for el in NUMERIC.iter() {
                let key = channel_at(keys, b.id, el, lf);
                let mut gens = layer.generators.iter().chain(linked_gens).filter(|g| g.bone_id == b.id && g.element == *el).peekable();
                if key.is_none() && gens.peek().is_none() {
                    continue;
                }
                // generators add to the keys, or to the setup value without any
                let k = key.unwrap_or(get(rest, el)) + gens.map(|g| generator_value(g, lt, loop_len)).sum::<f32>();
                let v = blend(get(b, el), k, get(rest, el), &layer.mode, layer.weight, el);
                set(b, el, v);
            }
            if layer.weight >= 0.5 {
                for el in DISCRETE.iter() {
                    if let Some(kf) = step(keys, b.id, el, lf.floor() as i32) {
                        set_step(b, &kf.clone());
                    }
                }
            }
        }
    }
    bones
}

/// The pose as the animation plays it (muted and solo layers respected).
pub fn pose(anim: &Animation, setup: &[Bone], frame: i32) -> Vec<Bone> {
    pose_in(anim, &Ctx::NONE, setup, frame)
}

pub fn pose_in(anim: &Animation, ctx: &Ctx, setup: &[Bone], frame: i32) -> Vec<Bone> {
    evaluate_in(anim, ctx, setup, frame, &|i| enabled(anim, i))
}

/// The value a key on the active layer needs for `bone_id`'s channel to show `target`
/// at `frame` (docs/TIMELINE.md §5.3): the playing layers without the active one give
/// what's below, and the active layer's mode and weight are inverted over it (and its
/// generators taken away).
pub fn key_on_active(anim: &Animation, setup: &[Bone], bone_id: i32, el: &AnimElement, frame: i32, target: f32) -> f32 {
    key_on_active_in(anim, &Ctx::NONE, setup, bone_id, el, frame, target)
}

pub fn key_on_active_in(anim: &Animation, ctx: &Ctx, setup: &[Bone], bone_id: i32, el: &AnimElement, frame: i32, target: f32) -> f32 {
    let active = anim.active_layer;
    let Some(layer) = anim.layers.get(active).filter(|_| active > 0) else { return target };
    if !NUMERIC.contains(el) {
        return target;
    }
    let below = evaluate_in(anim, ctx, setup, frame, &|i| i != active && enabled(anim, i));
    let (Some(b), Some(rest)) = (below.iter().find(|b| b.id == bone_id), setup.iter().find(|b| b.id == bone_id)) else {
        return target;
    };
    let lt = local_time(layer, frame as f32 / anim.fps.max(1) as f32, None);
    let gens: f32 = layer
        .generators
        .iter()
        .filter(|g| g.bone_id == bone_id && g.element == *el)
        .map(|g| generator_value(g, lt, loop_length(layer)))
        .sum();
    key_for(target, get(b, el), get(rest, el), &layer.mode, layer.weight, el) - gens
}

/// The channels (bone, element) layers `layers` drive: their keys, generators and
/// linked animations.
fn touched(anim: &Animation, ctx: &Ctx, layers: impl Iterator<Item = usize>) -> Vec<(i32, AnimElement)> {
    let mut out: Vec<(i32, AnimElement)> = vec![];
    let mut add = |b: i32, e: &AnimElement| {
        if !out.iter().any(|(ob, oe)| *ob == b && oe == e) {
            out.push((b, e.clone()));
        }
    };
    for i in layers {
        let (keys, _, linked_gens) = if i == 0 { (anim.layer_keys(0), anim.fps, &[][..]) } else { layer_source(anim, ctx, i) };
        for kf in keys {
            add(kf.bone_id, &kf.element);
        }
        let own: &[Generator] = anim.layers.get(i).map_or(&[], |l| l.generators.as_slice());
        for g in own.iter().chain(linked_gens) {
            add(g.bone_id, &g.element);
        }
    }
    out
}

/// Keys sampled on every frame, with the ones a straight line through their
/// neighbours would give dropped.
fn sampled(bone_id: i32, el: &AnimElement, values: &[(i32, f32)]) -> Vec<Keyframe> {
    let (start, end) = utils::interp_preset(HandlePreset::Linear);
    let key = |&(frame, value): &(i32, f32)| Keyframe {
        frame,
        bone_id,
        element: el.clone(),
        value,
        start_handle: start,
        end_handle: end,
        ..Default::default()
    };
    let mut keep: Vec<(i32, f32)> = vec![];
    for (i, s) in values.iter().enumerate() {
        let needed = i == 0 || i + 1 == values.len() || {
            let (a, b) = (keep.last().unwrap(), values[i + 1]);
            let t = (s.0 - a.0) as f32 / (b.0 - a.0) as f32;
            (a.1 + (b.1 - a.1) * t - s.1).abs() > 1e-4
        };
        if needed {
            keep.push(*s);
        }
    }
    keep.iter().map(key).collect()
}

/// Bake layers `0..=top` into one set of keys, for the channels the layers above Base
/// touch (Base's other keys are kept as they are). Sampled per frame, which is exact:
/// the editor and runtimes play whole frames.
fn bake(anim: &Animation, ctx: &Ctx, setup: &[Bone], top: usize) -> Vec<Keyframe> {
    let channels = touched(anim, ctx, (1..=top).filter(|i| enabled(anim, *i)));
    let mut keys: Vec<Keyframe> = anim
        .layer_keys(0)
        .iter()
        .filter(|kf| !channels.iter().any(|(b, e)| *b == kf.bone_id && *e == kf.element))
        .cloned()
        .collect();
    let last = anim.last_frame();
    let poses: Vec<Vec<Bone>> = (0..=last).map(|f| evaluate_in(anim, ctx, setup, f, &|i| i <= top && enabled(anim, i))).collect();
    for (bone_id, el) in channels {
        let at = |f: usize| poses[f].iter().find(|b| b.id == bone_id);
        if NUMERIC.contains(&el) {
            let values: Vec<(i32, f32)> = (0..=last as usize).filter_map(|f| at(f).map(|b| (f as i32, get(b, &el)))).collect();
            keys.extend(sampled(bone_id, &el, &values));
        } else {
            // a key wherever the value changes
            let mut prev: Option<(f32, String)> = None;
            for f in 0..=last as usize {
                let Some(b) = at(f) else { continue };
                let now = match el {
                    AnimElement::Texture => (0., b.tex.clone()),
                    AnimElement::IkBend => (0., b.ik.as_ref().map_or(String::new(), |ik| ik.bend.to_string())),
                    _ => (get(b, &el), String::new()),
                };
                if prev.as_ref() != Some(&now) {
                    keys.push(Keyframe { frame: f as i32, bone_id, element: el.clone(), value: now.0, value_str: now.1.clone(), ..Default::default() });
                    prev = Some(now);
                }
            }
        }
    }
    keys.sort_by(|a, b| a.frame.cmp(&b.frame));
    keys
}

/// The animation with every playing layer baked into Base, and no layers: what
/// runtimes without layer support get (docs/TIMELINE.md §5.5). Loops are baked in the
/// animation's own time: they restart with it.
pub fn flatten(anim: &Animation, setup: &[Bone]) -> Animation {
    flatten_in(anim, &Ctx::NONE, setup)
}

pub fn flatten_in(anim: &Animation, ctx: &Ctx, setup: &[Bone]) -> Animation {
    let mut flat = anim.normalized();
    if flat.layers.len() > 1 {
        let ctx = Ctx { clock: None, ..*ctx };
        flat.keyframes = bake(&flat, &ctx, setup, flat.layers.len() - 1);
    }
    flat.layers = vec![];
    flat.active_layer = 0;
    flat
}

/// Merge layer `i` into the one below it, which then plays as the two did together.
/// Merging into Base bakes; merging into another layer keys it through its own mode
/// and weight (`key_for`). Returns false if there's nothing it can merge into: a
/// Looped or linked layer below only takes keys in its own time.
pub fn merge_down(anim: &mut Animation, setup: &[Bone], i: usize) -> bool {
    merge_down_in(anim, &Ctx::NONE, setup, i)
}

pub fn merge_down_in(anim: &mut Animation, ctx: &Ctx, setup: &[Bone], i: usize) -> bool {
    if i == 0 || i >= anim.layers.len() {
        return false;
    }
    let below = i - 1;
    let simple = |l: &AnimLayer| l.play == PlayMode::Linear && l.link.is_none() && l.generators.is_empty();
    if below > 0 && !simple(&anim.layers[below]) {
        return false;
    }
    let ctx = Ctx { clock: None, ..*ctx };
    let active = anim.active_layer;
    anim.set_active_layer(0);
    if below == 0 {
        // bake Base and this layer (only), keeping the other layers
        let mut two = anim.clone();
        for (j, l) in two.layers.iter_mut().enumerate() {
            l.muted = j > i;
            l.solo = false;
        }
        two.layers[i].muted = false;
        anim.keyframes = bake(&two, &ctx, setup, i);
    } else {
        let channels = touched(anim, &ctx, std::iter::once(i));
        let last = anim.last_frame();
        let (mode, w) = (anim.layers[below].mode.clone(), anim.layers[below].weight);
        let mut keys: Vec<Keyframe> = anim.layers[below]
            .keyframes
            .iter()
            .filter(|kf| !channels.iter().any(|(b, e)| *b == kf.bone_id && *e == kf.element))
            .cloned()
            .collect();
        for (bone_id, el) in channels {
            let Some(rest) = setup.iter().find(|b| b.id == bone_id) else { continue };
            if !NUMERIC.contains(&el) {
                keys.extend(anim.layers[i].keyframes.iter().filter(|k| k.bone_id == bone_id && k.element == el).cloned());
                continue;
            }
            let mut values = vec![];
            for f in 0..=last {
                let target = evaluate_in(anim, &ctx, setup, f, &|j| j <= i && enabled(anim, j) || j == i);
                let under = evaluate_in(anim, &ctx, setup, f, &|j| j < below && enabled(anim, j));
                let (Some(t), Some(u)) = (target.iter().find(|b| b.id == bone_id), under.iter().find(|b| b.id == bone_id)) else { continue };
                values.push((f, key_for(get(t, &el), get(u, &el), get(rest, &el), &mode, w, &el)));
            }
            keys.extend(sampled(bone_id, &el, &values));
        }
        keys.sort_by(|a, b| a.frame.cmp(&b.frame));
        anim.layers[below].keyframes = keys;
    }
    anim.layers.remove(i);
    let new_active = if active == i { below } else if active > i { active - 1 } else { active };
    if anim.layers.len() <= 1 {
        anim.layers = vec![];
    }
    anim.set_active_layer(new_active.min(anim.layers.len().saturating_sub(1)));
    true
}

/// Close Loop: copy each channel's first key to the loop's end, so it loops without a
/// pop. Keys of layer `i` only; returns how many keys it set.
pub fn close_loop(anim: &mut Animation, i: usize) -> usize {
    let Some(len) = anim.layers.get(i).and_then(|l| loop_frames(anim, l)) else { return 0 };
    let active = anim.active_layer;
    anim.set_active_layer(i);
    let mut firsts: Vec<Keyframe> = vec![];
    for kf in &anim.keyframes {
        match firsts.iter_mut().find(|f| f.bone_id == kf.bone_id && f.element == kf.element) {
            Some(f) if kf.frame < f.frame => *f = kf.clone(),
            Some(_) => {}
            None => firsts.push(kf.clone()),
        }
    }
    for first in &firsts {
        anim.keyframes.retain(|k| !(k.bone_id == first.bone_id && k.element == first.element && k.frame == len));
        anim.keyframes.push(Keyframe { frame: len, ..first.clone() });
    }
    anim.sort_keyframes();
    anim.set_active_layer(active);
    firsts.len()
}

/// Bake generator `g` of layer `i` into its keys (sampled every `step` frames, over
/// the loop or the animation), and remove it.
pub fn bake_generator(anim: &mut Animation, i: usize, g: usize, step: i32) {
    let Some(layer) = anim.layers.get(i) else { return };
    let Some(gen) = layer.generators.get(g).cloned() else { return };
    let loop_len = loop_length(layer);
    let fps = anim.fps.max(1) as f32;
    let end = loop_frames(anim, layer).unwrap_or(anim.last_frame().max(1));
    let active = anim.active_layer;
    anim.set_active_layer(i);
    let mut values: Vec<(i32, f32)> = vec![];
    let mut f = 0;
    while f <= end {
        let base = channel(&anim.keyframes, gen.bone_id, &gen.element, f);
        values.push((f, base.unwrap_or(0.) + generator_value(&gen, f as f32 / fps, loop_len)));
        f += step.max(1);
    }
    if values.last().map(|v| v.0) != Some(end) {
        let base = channel(&anim.keyframes, gen.bone_id, &gen.element, end);
        values.push((end, base.unwrap_or(0.) + generator_value(&gen, end as f32 / fps, loop_len)));
    }
    let had_keys = anim.keyframes.iter().any(|k| k.bone_id == gen.bone_id && k.element == gen.element);
    anim.keyframes.retain(|k| !(k.bone_id == gen.bone_id && k.element == gen.element));
    let (start, end_h) = utils::interp_preset(HandlePreset::SineInOut);
    for (frame, value) in values {
        anim.keyframes.push(Keyframe { frame, bone_id: gen.bone_id, element: gen.element.clone(), value, start_handle: start, end_handle: end_h, ..Default::default() });
    }
    anim.sort_keyframes();
    anim.layers[i].generators.remove(g);
    let _ = had_keys;
    anim.set_active_layer(active);
}

/// Loops that won't line up when baked into their animation (docs/TIMELINE.md §5.7):
/// (layer name, loop seconds, animation seconds) where the animation isn't a whole
/// number of loops long.
pub fn seams(anim: &Animation) -> Vec<(String, f32, f32)> {
    let secs = anim.last_frame() as f32 / anim.fps.max(1) as f32;
    anim.layers
        .iter()
        .filter_map(|l| {
            let (PlayMode::Looped { speed, .. }, Some(len)) = (&l.play, loop_length(l)) else { return None };
            let per = len / speed.abs().max(1e-3);
            let n = secs / per;
            ((n - n.round()).abs() > 0.02 && secs > 0.).then(|| (l.name.clone(), per, secs))
        })
        .collect()
}

/// Apply a layer event (`Events::AddLayer` etc) to `anim`.
pub fn layer_event(event: &Events, value: f32, str_value: &str, anim: &mut Animation, ctx: &Ctx, setup: &[Bone]) {
    let i = value as usize;
    match event {
        Events::AddLayer => {
            let mode = if value == 1. { LayerMode::Additive } else { LayerMode::Override };
            let n = anim.layers.len().max(1);
            let new = anim.add_layer(&format!("Layer {n}"), mode);
            anim.set_active_layer(new);
        }
        Events::SelectLayer => anim.set_active_layer(i),
        Events::SetLayer => {
            let Ok((i, settings)) = serde_json::from_str::<(usize, AnimLayer)>(str_value) else { return };
            let anim_id = anim.id;
            if let Some(layer) = anim.layers.get_mut(i) {
                let keyframes = std::mem::take(&mut layer.keyframes);
                // a layer can't link its own animation
                let link = settings.link.filter(|id| *id != anim_id);
                *layer = AnimLayer { id: layer.id, keyframes, link, weight: settings.weight.clamp(0., 1.), ..settings };
            }
        }
        Events::DeleteLayer if i > 0 && i < anim.layers.len() => {
            let active = anim.active_layer;
            anim.set_active_layer(0);
            anim.layers.remove(i);
            if anim.layers.len() <= 1 {
                anim.layers = vec![];
            } else {
                let active = if active == i { i - 1 } else if active > i { active - 1 } else { active };
                anim.set_active_layer(active);
            }
        }
        Events::DuplicateLayer if i > 0 && i < anim.layers.len() => {
            let mut copy = anim.layers[i].clone();
            copy.keyframes = anim.layer_keys(i).to_vec();
            copy.id = anim.layers.iter().map(|l| l.id).max().unwrap_or(0) + 1;
            copy.name = format!("{} copy", copy.name);
            copy.solo = false;
            copy.generated = None;
            anim.set_active_layer(0);
            anim.layers.insert(i + 1, copy);
            anim.set_active_layer(i + 1);
        }
        Events::MergeLayerDown => {
            merge_down_in(anim, ctx, setup, i);
        }
        Events::FlattenLayers => {
            *anim = flatten_in(anim, ctx, setup);
        }
        Events::CloseLoop => {
            close_loop(anim, i);
        }
        Events::BakeGenerator => {
            // value: layer; str: the generator's index
            if let Ok(g) = str_value.parse::<usize>() {
                bake_generator(anim, i, g, 2);
            }
        }
        _ => {}
    }
}
