//! Shapes (morph targets) on meshes (docs/LIP_SYNC.md §4).
//!
//! A shape is a set of per-vertex offsets in the mesh's own (rest) space, blended by
//! its weight before skinning: `v = rest + Σ weight · delta`. Offsets are keyed by
//! vertex id, like binds, so they survive topology edits (and are saved by index).
//! Weights are keyable (`AnimElement::ShapeWeight`, with `Keyframe::shape` naming the
//! shape) and go through animation layers like any channel.

use crate::shared::*;
use crate::layers;
use std::collections::HashMap;

/// Add the weighted shape offsets to the bone's vertices (rest space). Called on a
/// working copy each frame, before skinning (`renderer::construct_verts`).
pub fn apply(bone: &mut Bone) {
    if bone.shapes.iter().all(|s| s.weight == 0. || s.deltas.is_empty()) {
        return;
    }
    let index: HashMap<u32, usize> = bone.vertices.iter().enumerate().map(|(i, v)| (v.id, i)).collect();
    for shape in &bone.shapes {
        if shape.weight == 0. {
            continue;
        }
        for d in &shape.deltas {
            if let Some(&i) = index.get(&d.vert) {
                bone.vertices[i].pos += d.delta * shape.weight;
            }
        }
    }
}

/// The total shape offset of each vertex (by index), at the bone's current weights.
pub fn offsets(bone: &Bone) -> Vec<Vec2> {
    let mut out = vec![Vec2::ZERO; bone.vertices.len()];
    let index: HashMap<u32, usize> = bone.vertices.iter().enumerate().map(|(i, v)| (v.id, i)).collect();
    for shape in bone.shapes.iter().filter(|s| s.weight != 0.) {
        for d in &shape.deltas {
            if let Some(&i) = index.get(&d.vert) {
                out[i] += d.delta * shape.weight;
            }
        }
    }
    out
}

/// A shape's offset for one vertex (zero if it has none).
pub fn delta_of(shape: &MeshShape, vert: u32) -> Vec2 {
    shape.deltas.iter().find(|d| d.vert == vert).map_or(Vec2::ZERO, |d| d.delta)
}

/// Set a shape's offset for one vertex; a zero offset is dropped.
pub fn set_delta(shape: &mut MeshShape, vert: u32, delta: Vec2) {
    shape.deltas.retain(|d| d.vert != vert);
    if delta.x.abs() > 1e-6 || delta.y.abs() > 1e-6 {
        shape.deltas.push(ShapeDelta { vert, delta });
    }
}

/// Drop offsets of vertices that no longer exist (after vertices are removed).
pub fn prune(bone: &mut Bone) {
    let ids: Vec<u32> = bone.vertices.iter().map(|v| v.id).collect();
    for shape in &mut bone.shapes {
        shape.deltas.retain(|d| ids.contains(&d.vert));
    }
}

/// A new vertex's offsets: its corners' offsets blended by `weights` (vertex index,
/// weight), as `topology::interpolate_binds` does for binds.
pub fn interpolate(bone: &mut Bone, id: u32, weights: &[(u32, f32)]) {
    let corners: Vec<(u32, f32)> = weights.iter().filter_map(|(i, w)| bone.vertices.get(*i as usize).map(|v| (v.id, *w))).collect();
    for shape in &mut bone.shapes {
        let mut delta = Vec2::ZERO;
        for (corner, w) in &corners {
            delta += delta_of(shape, *corner) * *w;
        }
        set_delta(shape, id, delta);
    }
}

/// The next free shape id on a bone (ids start at 1: 0 means "not a shape" on keys).
pub fn next_id(bone: &Bone) -> i32 {
    bone.shapes.iter().map(|s| s.id).max().unwrap_or(0) + 1
}

/// A name not yet used on the bone: `base`, or `base 2`, `base 3`…
pub fn unique_name(bone: &Bone, base: &str) -> String {
    let taken = |n: &str| bone.shapes.iter().any(|s| s.name == n);
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).unwrap()
}

/// The key on `frame` for `bone_id`'s shape, creating it (sorted in) if missing.
pub fn key_at(anim: &mut Animation, bone_id: i32, shape: i32, frame: i32) -> usize {
    let same = |kf: &Keyframe| kf.frame == frame && kf.bone_id == bone_id && kf.element == AnimElement::ShapeWeight && kf.shape == shape;
    if let Some(i) = anim.keyframes.iter().position(same) {
        return i;
    }
    let (start_handle, end_handle) = crate::utils::interp_preset(HandlePreset::Linear);
    anim.keyframes.push(Keyframe {
        frame,
        bone_id,
        element: AnimElement::ShapeWeight,
        shape,
        start_handle,
        end_handle,
        ..Default::default()
    });
    anim.sort_keyframes();
    anim.keyframes.iter().position(same).unwrap()
}

/// Set a shape's weight: the setup weight without an animation frame (or on a locked
/// bone), otherwise a key on the active layer at `frame`, written so the pose shows
/// `value` (as `editor::edit_bone` does for transforms).
pub fn edit_weight(armature: &mut Armature, bone_id: i32, shape: i32, value: f32, anim_id: usize, frame: i32) {
    let Some(bone) = armature.bones.iter_mut().find(|b| b.id == bone_id) else { return };
    let locked = bone.locked;
    let Some(s) = bone.shapes.iter_mut().find(|s| s.id == shape) else { return };
    if anim_id >= armature.animations.len() || frame < 0 || locked {
        s.weight = value;
        return;
    }
    let setup = s.weight;
    let el = AnimElement::ShapeWeight;

    // through the active layer: nothing if locked or linked; the key it needs; its frame
    let (value, frame, override_layer) = {
        let anim = &armature.animations[anim_id];
        match anim.layers.get(anim.active_layer).filter(|_| anim.active_layer > 0) {
            Some(l) if l.locked || l.link.is_some() => return,
            Some(l) => {
                let ctx = layers::Ctx { anims: &armature.animations, clock: None };
                let v = layers::key_on_active_in(anim, &ctx, &armature.bones, bone_id, &el, shape, frame, value);
                (v, layers::layer_frame(anim, l, frame), l.mode == LayerMode::Override)
            }
            None => (value, frame, false),
        }
    };

    let anim = &mut armature.animations[anim_id];
    let has_first = anim.keyframes.iter().any(|k| k.frame == 0 && k.bone_id == bone_id && k.element == el && k.shape == shape);
    if frame != 0 && !has_first && !override_layer {
        let i = key_at(anim, bone_id, shape, 0);
        anim.keyframes[i].value = setup;
    }
    let i = key_at(anim, bone_id, shape, frame);
    anim.keyframes[i].value = value;
}

/// Remove shape `shape` from `bone_id`, and every key of it (on every layer).
pub fn delete(armature: &mut Armature, bone_id: i32, shape: i32) {
    if let Some(bone) = armature.bones.iter_mut().find(|b| b.id == bone_id) {
        bone.shapes.retain(|s| s.id != shape);
    }
    let is_key = |k: &Keyframe| k.bone_id == bone_id && k.element == AnimElement::ShapeWeight && k.shape == shape;
    for anim in &mut armature.animations {
        anim.keyframes.retain(|k| !is_key(k));
        for layer in &mut anim.layers {
            layer.keyframes.retain(|k| !is_key(k));
        }
    }
}

/// Mirror a shape across the mesh's local X axis: each vertex takes the offset of the
/// vertex nearest its mirror image (within `tolerance`), with x flipped. Returns how
/// many vertices found no mirror partner.
pub fn mirrored(bone: &Bone, shape: &MeshShape, tolerance: f32) -> (Vec<ShapeDelta>, usize) {
    let mut out = vec![];
    let mut missing = 0;
    for v in &bone.vertices {
        let target = Vec2::new(-v.pos.x, v.pos.y);
        let partner = bone
            .vertices
            .iter()
            .map(|o| (o, (o.pos - target).mag()))
            .filter(|(_, d)| *d <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match partner {
            Some((o, _)) => {
                let d = delta_of(shape, o.id);
                if d.x.abs() > 1e-6 || d.y.abs() > 1e-6 {
                    out.push(ShapeDelta { vert: v.id, delta: Vec2::new(-d.x, d.y) });
                }
            }
            None => missing += 1,
        }
    }
    (out, missing)
}

/// How close a vertex must be to another's mirror image to be its partner: a few
/// percent of the mesh's size.
pub fn mirror_tolerance(bone: &Bone) -> f32 {
    let (mut lo, mut hi) = (Vec2::new(f32::MAX, f32::MAX), Vec2::new(f32::MIN, f32::MIN));
    for v in &bone.vertices {
        lo = Vec2::new(lo.x.min(v.pos.x), lo.y.min(v.pos.y));
        hi = Vec2::new(hi.x.max(v.pos.x), hi.y.max(v.pos.y));
    }
    ((hi - lo).mag() * 0.03).max(0.5)
}

/// The other side's name: `_L` ↔ `_R`, `Left` ↔ `Right`, or "(mirror)" added.
pub fn mirror_name(name: &str) -> String {
    for (a, b) in [("_L", "_R"), ("_R", "_L"), ("Left", "Right"), ("Right", "Left"), ("left", "right"), ("right", "left")] {
        if let Some(stem) = name.strip_suffix(a) {
            return format!("{stem}{b}");
        }
    }
    format!("{name} (mirror)")
}

/// Capture as shape (docs/LIP_SYNC.md §4.4): the offsets that make the setup pose show
/// the mesh as it is posed now. `rest` and `posed` are the armature's bones posed by
/// `renderer::construction`, at the setup pose (shapes at 0) and as shown. Both meshes
/// are taken back into the mesh's own space through its setup transform, so applying
/// the shape at the setup pose reproduces the pose exactly for vertices that follow
/// the mesh bone, and closely for vertices bound to others.
pub fn capture(rest: &[Bone], posed: &[Bone], bone_id: i32) -> Vec<ShapeDelta> {
    let (Some(r), Some(p)) = (rest.iter().find(|b| b.id == bone_id), posed.iter().find(|b| b.id == bone_id)) else {
        return vec![];
    };
    // the inverse of `renderer::inherit_vert` for the mesh bone at its setup pose
    let scale = r.scale * r.pivot_scale;
    let rot = r.rot + r.pivot_rot;
    let local = |w: Vec2| {
        let v = crate::utils::rotate(&(w - r.pos), -rot);
        Vec2::new(if scale.x != 0. { v.x / scale.x } else { 0. }, if scale.y != 0. { v.y / scale.y } else { 0. })
    };
    let mut out = vec![];
    for rv in &r.vertices {
        let Some(pv) = p.vertices.iter().find(|v| v.id == rv.id) else { continue };
        let delta = local(pv.pos) - local(rv.pos);
        if delta.mag() > 1e-4 {
            out.push(ShapeDelta { vert: rv.id, delta });
        }
    }
    out
}

/// Soft sculpting: each unselected vertex within `radius` of a selected one (rest
/// positions), with a smooth falloff weight (1 at the selection, 0 at the radius).
pub fn soft_weights(vertices: &[Vertex], selected: &[u32], radius: f32) -> Vec<(u32, f32)> {
    let picked: Vec<Vec2> = vertices.iter().filter(|v| selected.contains(&v.id)).map(|v| v.pos).collect();
    if picked.is_empty() || radius <= 0. {
        return vec![];
    }
    vertices
        .iter()
        .filter(|v| !selected.contains(&v.id))
        .filter_map(|v| {
            let d = picked.iter().map(|p| (*p - v.pos).mag()).fold(f32::MAX, f32::min);
            let t = 1. - d / radius;
            (t > 0.).then(|| (v.id, t * t * (3. - 2. * t)))
        })
        .collect()
}
