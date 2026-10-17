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

/// The pose at `frame`: `setup` with Base applied, then each layer `include` allows.
pub fn evaluate(anim: &Animation, setup: &[Bone], frame: i32, include: &dyn Fn(usize) -> bool) -> Vec<Bone> {
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
    for i in 1..anim.layers.len() {
        if !include(i) {
            continue;
        }
        let layer = &anim.layers[i];
        let keys = anim.layer_keys(i);
        if keys.is_empty() {
            continue;
        }
        for (b, rest) in bones.iter_mut().zip(setup) {
            for el in NUMERIC.iter() {
                if let Some(k) = channel(keys, b.id, el, frame) {
                    let v = blend(get(b, el), k, get(rest, el), &layer.mode, layer.weight, el);
                    set(b, el, v);
                }
            }
            if layer.weight >= 0.5 {
                for el in DISCRETE.iter() {
                    if let Some(kf) = step(keys, b.id, el, frame) {
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
    evaluate(anim, setup, frame, &|i| enabled(anim, i))
}

/// The value a key on the active layer needs for `bone_id`'s channel to show `target`
/// at `frame` (docs/TIMELINE.md §5.3): the playing layers without the active one give
/// what's below, and the active layer's mode and weight are inverted over it.
pub fn key_on_active(anim: &Animation, setup: &[Bone], bone_id: i32, el: &AnimElement, frame: i32, target: f32) -> f32 {
    let active = anim.active_layer;
    let Some(layer) = anim.layers.get(active).filter(|_| active > 0) else { return target };
    if !NUMERIC.contains(el) {
        return target;
    }
    let below = evaluate(anim, setup, frame, &|i| i != active && enabled(anim, i));
    let (Some(b), Some(rest)) = (below.iter().find(|b| b.id == bone_id), setup.iter().find(|b| b.id == bone_id)) else {
        return target;
    };
    key_for(target, get(b, el), get(rest, el), &layer.mode, layer.weight, el)
}

/// The channels (bone, element) layers `from..` have keys on.
fn touched(anim: &Animation, layers: impl Iterator<Item = usize>) -> Vec<(i32, AnimElement)> {
    let mut out: Vec<(i32, AnimElement)> = vec![];
    for i in layers {
        for kf in anim.layer_keys(i) {
            if !out.iter().any(|(b, e)| *b == kf.bone_id && *e == kf.element) {
                out.push((kf.bone_id, kf.element.clone()));
            }
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
fn bake(anim: &Animation, setup: &[Bone], top: usize) -> Vec<Keyframe> {
    let channels = touched(anim, (1..=top).filter(|i| enabled(anim, *i)));
    let mut keys: Vec<Keyframe> = anim
        .layer_keys(0)
        .iter()
        .filter(|kf| !channels.iter().any(|(b, e)| *b == kf.bone_id && *e == kf.element))
        .cloned()
        .collect();
    let last = anim.last_frame();
    let poses: Vec<Vec<Bone>> = (0..=last).map(|f| evaluate(anim, setup, f, &|i| i <= top && enabled(anim, i))).collect();
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
/// runtimes without layer support get (docs/TIMELINE.md §5.5).
pub fn flatten(anim: &Animation, setup: &[Bone]) -> Animation {
    let mut flat = anim.normalized();
    if flat.layers.len() > 1 {
        flat.keyframes = bake(&flat, setup, flat.layers.len() - 1);
    }
    flat.layers = vec![];
    flat.active_layer = 0;
    flat
}

/// Merge layer `i` into the one below it, which then plays as the two did together.
/// Merging into Base bakes; merging into another layer keys it through its own mode
/// and weight (`key_for`). Returns false if there's nothing below to merge into.
pub fn merge_down(anim: &mut Animation, setup: &[Bone], i: usize) -> bool {
    if i == 0 || i >= anim.layers.len() {
        return false;
    }
    let active = anim.active_layer;
    anim.set_active_layer(0);
    let below = i - 1;
    if below == 0 {
        // bake Base and this layer (only), keeping the other layers
        let mut two = anim.clone();
        for (j, l) in two.layers.iter_mut().enumerate() {
            l.muted = j > i;
            l.solo = false;
        }
        two.layers[i].muted = false;
        anim.keyframes = bake(&two, setup, i);
    } else {
        let channels = touched(anim, std::iter::once(i));
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
                let target = evaluate(anim, setup, f, &|j| j <= i && enabled(anim, j) || j == i);
                let under = evaluate(anim, setup, f, &|j| j < below && enabled(anim, j));
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

/// Apply a layer event (`Events::AddLayer` etc) to `anim`.
pub fn layer_event(event: &Events, value: f32, str_value: &str, anim: &mut Animation, setup: &[Bone]) {
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
            if let Some(layer) = anim.layers.get_mut(i) {
                let keyframes = std::mem::take(&mut layer.keyframes);
                *layer = AnimLayer { keyframes, weight: settings.weight.clamp(0., 1.), ..settings };
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
            let active = anim.active_layer;
            anim.set_active_layer(0);
            anim.layers.insert(i + 1, copy);
            anim.set_active_layer(if active > i { active + 1 } else { active });
            anim.set_active_layer(i + 1);
        }
        Events::MergeLayerDown => {
            merge_down(anim, setup, i);
        }
        Events::FlattenLayers => {
            *anim = flatten(anim, setup);
        }
        _ => {}
    }
}
