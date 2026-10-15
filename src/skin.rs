//! Bind to Skeleton and live envelope weights (docs/ENVELOPES.md §3.4–3.6, §5).
//!
//! A mesh skinned this way is bind-posed, binds to its skeleton bones through their
//! Bind Pose helpers, and has `envelope_weights` on: `maintain` recomputes its
//! weights whenever an input changes, except on vertices edited by hand (locked).

use crate::envelope::{self, Envelope, Skinner};
use crate::*;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Bones blending one vertex, unless a mesh sets its own limit.
pub const DEFAULT_MAX_INFLUENCES: u32 = 4;

fn find(bones: &[Bone], id: i32) -> Option<&Bone> {
    bones.iter().find(|b| b.id == id)
}

/// Constructed setup pose, including IK.
pub fn setup_world(armature: &Armature) -> Vec<Bone> {
    let local = armature.bones.clone();
    let mut world = local.clone();
    renderer::construction(&mut world, &local);
    world
}

fn is_ik_target(bones: &[Bone], id: i32) -> bool {
    bones.iter().any(|b| b.ik.as_ref().map_or(false, |c| c.target_id == id))
}

/// Whether a bone is part of the skeleton by default: untextured, and neither an IK
/// target nor a Bind Pose helper (docs/ENVELOPES.md §5).
fn deforms_by_default(bones: &[Bone], bone: &Bone) -> bool {
    bone.tex.is_empty() && bone.bind_owner.is_none() && !is_ik_target(bones, bone.id)
}

/// The envelope `bone_id` skins with: its own, or the default (§5). A point bone
/// reaches toward the nearest other skeleton bone. `world` is the setup pose.
pub fn envelope_of(world: &[Bone], bone_id: i32) -> Envelope {
    let Some(bone) = find(world, bone_id) else { return Envelope::default() };
    if let Some(e) = bone.envelope {
        return e;
    }
    let length = (renderer::tail_pos(bone) - bone.pos).mag();
    let mut e = if length > 0. {
        Envelope::for_length(length)
    } else {
        let nearest = world
            .iter()
            .filter(|b| b.id != bone.id && deforms_by_default(world, b))
            .map(|b| (b.pos - bone.pos).mag())
            .filter(|d| *d > 1e-3)
            .fold(f32::MAX, f32::min);
        if nearest < f32::MAX {
            let mut e = Envelope::for_length(0.);
            (e.head_inner, e.head_outer) = (0.25 * nearest, 0.75 * nearest);
            (e.tail_inner, e.tail_outer) = (e.head_inner, e.head_outer);
            e
        } else {
            Envelope::for_length(0.)
        }
    };
    e.deform = deforms_by_default(world, bone);
    e
}

fn skinner(world: &[Bone], id: i32) -> Skinner {
    let b = find(world, id).unwrap();
    Skinner { id, head: b.pos, tail: renderer::tail_pos(b), envelope: envelope_of(world, id) }
}

fn is_under(bones: &[Bone], id: i32, ancestor: i32) -> bool {
    let mut p = find(bones, id).map_or(-1, |b| b.parent_id);
    while p != -1 {
        if p == ancestor {
            return true;
        }
        p = find(bones, p).map_or(-1, |b| b.parent_id);
    }
    false
}

fn depth(bones: &[Bone], id: i32) -> usize {
    let mut d = 0;
    let mut p = find(bones, id).map_or(-1, |b| b.parent_id);
    while p != -1 {
        d += 1;
        p = find(bones, p).map_or(-1, |b| b.parent_id);
    }
    d
}

/// Sorted parents first (by depth, then bone order).
fn parents_first(bones: &[Bone], mut ids: Vec<i32>) -> Vec<i32> {
    let idx = |id: i32| bones.iter().position(|b| b.id == id).unwrap_or(usize::MAX);
    ids.sort_by_key(|id| (depth(bones, *id), idx(*id)));
    ids
}

/// The skeleton for `mesh_id`: every deforming bone except the mesh and its
/// descendants, parents first.
pub fn skeleton(world: &[Bone], mesh_id: i32) -> Vec<i32> {
    let ids = world
        .iter()
        .filter(|b| b.id != mesh_id && b.bind_owner.is_none() && !is_under(world, b.id, mesh_id))
        .filter(|b| envelope_of(world, b.id).deform)
        .map(|b| b.id)
        .collect();
    parents_first(world, ids)
}

/// The bone a bind of `mesh` skins with: a Bind Pose helper stands in for its parent.
pub fn bind_bone(bones: &[Bone], mesh_id: i32, bind: &BoneBind) -> i32 {
    match find(bones, bind.bone_id) {
        Some(h) if h.bind_owner == Some(mesh_id) => h.parent_id,
        _ => bind.bone_id,
    }
}

/// The mesh's skin bones: the bones its weight binds skin with, in bind order.
pub fn skin_bones(bones: &[Bone], mesh: &Bone) -> Vec<i32> {
    let mut ids = vec![];
    for bind in mesh.binds.iter().filter(|b| !b.is_path && b.bone_id != -1) {
        let id = bind_bone(bones, mesh.id, bind);
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// Each vertex's shares (bone id, share) from the mesh's current binds; the owner's
/// share is under the mesh's own id.
pub fn vertex_shares(bones: &[Bone], mesh: &Bone) -> HashMap<u32, Vec<(i32, f32)>> {
    current_shares(bones, mesh)
}

fn current_shares(bones: &[Bone], mesh: &Bone) -> HashMap<u32, Vec<(i32, f32)>> {
    let binds: Vec<&BoneBind> = mesh.binds.iter().filter(|b| !b.is_path && b.bone_id != -1).collect();
    let mut out = HashMap::new();
    for v in &mesh.vertices {
        let weights: Vec<f32> = binds
            .iter()
            .map(|b| b.verts.iter().find(|bv| bv.id == v.id as i32).map_or(0., |bv| bv.weight))
            .collect();
        let (owner, shares) = envelope::shares(&weights);
        let mut list = vec![(mesh.id, owner)];
        for (bind, share) in binds.iter().zip(shares) {
            let id = bind_bone(bones, mesh.id, bind);
            match list.iter_mut().find(|s| s.0 == id) {
                Some(s) => s.1 += share,
                None => list.push((id, share)),
            }
        }
        list.retain(|s| s.1 > 1e-6);
        out.insert(v.id, list);
    }
    out
}

/// Recompute `mesh_id`'s weights over `bone_ids` (plus its owner if that deforms),
/// keeping locked vertices as they are, and rewrite its binds. With `keep_empty`,
/// every bone keeps a bind even if it weights nothing. Returns the vertices that sat
/// outside every envelope.
fn apply(armature: &mut Armature, world: &[Bone], mesh_id: i32, bone_ids: &[i32], keep_empty: bool) -> Vec<u32> {
    let mesh = find(&armature.bones, mesh_id).unwrap().clone();
    let world_mesh = find(world, mesh_id).unwrap();
    let rest = bind_pose::drawn_vertices(armature, &world.to_vec(), mesh_id);

    let mut skinners: Vec<Skinner> = bone_ids.iter().map(|id| skinner(world, *id)).collect();
    if envelope_of(world, mesh_id).deform {
        skinners.push(Skinner {
            id: mesh_id,
            head: world_mesh.pos,
            tail: renderer::tail_pos(world_mesh),
            envelope: envelope_of(world, mesh_id),
        });
    }
    let max = if mesh.max_influences == 0 { DEFAULT_MAX_INFLUENCES } else { mesh.max_influences };
    let current = current_shares(&armature.bones, &mesh);

    let mut outside = vec![];
    let mut per_vert: Vec<(u32, Vec<(i32, f32)>)> = vec![];
    for (v, p) in mesh.vertices.iter().zip(&rest) {
        let shares = if mesh.locked_verts.contains(&v.id) {
            current.get(&v.id).cloned().unwrap_or_default()
        } else {
            let (w, out) = envelope::vertex_weights(*p, &skinners, max as usize);
            if out {
                outside.push(v.id);
            }
            w
        };
        per_vert.push((v.id, shares));
    }

    // bones that get a bind: in skeleton order, then any only locked vertices use
    let weighted = |id: i32| per_vert.iter().any(|(_, s)| s.iter().any(|x| x.0 == id && x.1 > 0.));
    let mut used: Vec<i32> = bone_ids.iter().copied().filter(|id| keep_empty || weighted(*id)).collect();
    for (_, shares) in &per_vert {
        for (id, _) in shares {
            if *id != mesh_id && !used.contains(id) {
                used.push(*id);
            }
        }
    }

    // locked vertices set to 0 by hand stay listed in their binds (`edit_shares`)
    let listed: Vec<(i32, i32)> = mesh
        .binds
        .iter()
        .filter(|b| !b.is_path && b.bone_id != -1)
        .flat_map(|b| {
            let bone = bind_bone(&armature.bones, mesh_id, b);
            b.verts.iter().map(move |bv| (bone, bv.id))
        })
        .collect();

    let targets: Vec<i32> = used.iter().map(|id| bind_pose::bind_target(armature, *id, mesh_id)).collect();
    let mut binds: Vec<BoneBind> =
        targets.iter().map(|t| BoneBind { bone_id: *t, is_path: false, verts: vec![] }).collect();
    for (vid, shares) in &per_vert {
        let share = |id: i32| shares.iter().find(|s| s.0 == id).map_or(0., |s| s.1);
        let seq: Vec<f32> = used.iter().map(|id| share(*id)).collect();
        let locked = mesh.locked_verts.contains(vid);
        for (k, w) in envelope::bind_weights(share(mesh_id), &seq).into_iter().enumerate() {
            if w > 1e-6 || locked && listed.contains(&(used[k], *vid as i32)) {
                binds[k].verts.push(BoneBindVert { id: *vid as i32, weight: w });
            }
        }
    }

    let mesh = armature.find_bone_mut(mesh_id).unwrap();
    mesh.binds = binds;
    mesh.skin_outside = outside.clone();
    bind_pose::remove_unused_helpers(armature);
    outside
}

/// Bind to Skeleton (§3.4): Bind Pose on, a bind per skeleton bone in reach, envelope
/// weights, live from then on. Returns the vertices outside every envelope.
pub fn bind_to_skeleton(armature: &mut Armature, mesh_id: i32) -> Result<Vec<u32>, String> {
    let Some(mesh) = find(&armature.bones, mesh_id) else { return Err("bone not found".into()) };
    if mesh.bind_owner.is_some() {
        return Err("bind helpers can't be skinned".into());
    }
    if mesh.vertices.is_empty() {
        return Err("bone has no mesh".into());
    }
    if mesh.binds.iter().any(|b| b.is_path) {
        return Err("meshes with path binds can't use envelopes".into());
    }
    bind_pose::set_bind_pose(armature, mesh_id)?;

    let world = setup_world(armature);
    let bones = skeleton(&world, mesh_id);
    if bones.is_empty() && !envelope_of(&world, mesh_id).deform {
        return Err("no skeleton bones: set Deform on the bones that should skin it".into());
    }
    let outside = apply(armature, &world, mesh_id, &bones, false);

    let mesh = armature.find_bone_mut(mesh_id).unwrap();
    mesh.envelope_weights = true;
    mesh.skin_sig = 0;
    Ok(outside)
}

/// Clear All Binds: remove every bind of `mesh_id` and its envelope skinning, so it can be
/// skinned again from scratch. The mesh stays as drawn; Bind Pose goes off and unused
/// helpers are removed.
pub fn clear_all_binds(armature: &mut Armature, mesh_id: i32) {
    let Some(mesh) = armature.find_bone_mut(mesh_id) else { return };
    mesh.binds.clear();
    mesh.envelope_weights = false;
    mesh.skin_sig = 0;
    mesh.locked_verts.clear();
    mesh.skin_outside.clear();
    // with no binds left, this puts every vertex back in the mesh's own frame
    bind_pose::clear_bind_pose(armature, mesh_id);
    bind_pose::remove_unused_helpers(armature);
}

/// Add every skeleton bone whose outer envelope reaches the mesh to its skin bones.
pub fn add_bones_in_reach(armature: &mut Armature, mesh_id: i32) {
    let Some(mesh) = find(&armature.bones, mesh_id).cloned() else { return };
    let world = setup_world(armature);
    let rest = bind_pose::drawn_vertices(armature, &world, mesh_id);
    let mut ids = skin_bones(&armature.bones, &mesh);
    for id in skeleton(&world, mesh_id) {
        let s = skinner(&world, id);
        if !ids.contains(&id) && rest.iter().any(|p| s.envelope.gap(s.head, s.tail, *p) < 0.) {
            ids.push(id);
        }
    }
    let ids = parents_first(&world, ids);
    apply(armature, &world, mesh_id, &ids, true);
    armature.find_bone_mut(mesh_id).unwrap().skin_sig = 0;
}

/// Everything envelope weights depend on, hashed, so `maintain` can tell when to
/// recompute.
fn signature(world: &[Bone], mesh: &Bone, bone_ids: &[i32]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let f = |h: &mut std::collections::hash_map::DefaultHasher, x: f32| x.to_bits().hash(h);
    let wm = find(world, mesh.id).unwrap();
    for x in [wm.pos.x, wm.pos.y, wm.rot, wm.scale.x, wm.scale.y, mesh.pivot_rot, mesh.pivot_scale.x, mesh.pivot_scale.y] {
        f(&mut h, x);
    }
    for v in &mesh.vertices {
        v.id.hash(&mut h);
        f(&mut h, v.pos.x);
        f(&mut h, v.pos.y);
    }
    mesh.locked_verts.hash(&mut h);
    mesh.max_influences.hash(&mut h);
    for id in bone_ids.iter().chain([&mesh.id]) {
        let s = skinner(world, *id);
        id.hash(&mut h);
        let e = s.envelope;
        for x in [s.head.x, s.head.y, s.tail.x, s.tail.y, e.head_inner, e.head_outer, e.tail_inner, e.tail_outer] {
            f(&mut h, x);
        }
        (e.deform, e.absolute, e.falloff as i32).hash(&mut h);
    }
    h.finish().max(1)
}

/// Keep envelope-weighted meshes up to date (§3.6): recompute any whose inputs changed.
/// Cheap when no mesh uses envelope weights. Skipped in Pose Mode, where the bones
/// aren't at rest.
pub fn maintain(armature: &mut Armature, pose_mode: bool) {
    if pose_mode || !armature.bones.iter().any(|b| b.envelope_weights) {
        return;
    }
    let world = setup_world(armature);
    let meshes: Vec<i32> = armature.bones.iter().filter(|b| b.envelope_weights).map(|b| b.id).collect();
    for id in meshes {
        let mesh = find(&armature.bones, id).unwrap().clone();
        // envelope weights need Bind Pose; clearing it turns them off
        if !mesh.bind_pose || mesh.vertices.is_empty() {
            armature.find_bone_mut(id).unwrap().envelope_weights = false;
            continue;
        }
        let bones = skin_bones(&armature.bones, &mesh);
        let sig = signature(&world, &mesh, &bones);
        if sig != mesh.skin_sig {
            apply(armature, &world, id, &bones, true);
            armature.find_bone_mut(id).unwrap().skin_sig = sig;
        }
    }
}

/// The bone nearest the rest position `p` among `bone_ids` other than `exclude`, by
/// distance to its outer envelope, for shares that have nowhere else to go.
fn nearest_bone(world: &[Bone], bone_ids: &[i32], exclude: i32, p: Vec2) -> Option<i32> {
    bone_ids
        .iter()
        .filter(|id| **id != exclude)
        .map(|id| (*id, skinner(world, *id)))
        .min_by(|a, b| {
            let ga = a.1.envelope.gap(a.1.head, a.1.tail, p);
            let gb = b.1.envelope.gap(b.1.head, b.1.tail, p);
            ga.partial_cmp(&gb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(id, _)| id)
}

/// Set `bone_id`'s share of each vertex in `edits` (vertex id, share), keeping every
/// vertex's shares summing to 1 (docs/ENVELOPES.md §4.5): the other bones scale to
/// make up the rest, and a vertex with no other bone gives the rest to the nearest
/// other skin bone (or the owner). Edited vertices are locked. With `keep_listed`, a
/// vertex set to 0 stays listed in the bind (at 0), so a slider can be dragged
/// through 0 without its row vanishing. Painting a bone the mesh has no bind for adds
/// one.
pub fn edit_shares(armature: &mut Armature, mesh_id: i32, bone_id: i32, edits: &[(u32, f32)], keep_listed: bool) {
    let Some(mesh) = find(&armature.bones, mesh_id).cloned() else { return };
    let world = setup_world(armature);
    let rest = bind_pose::drawn_vertices(armature, &world, mesh_id);
    let skin = skin_bones(&armature.bones, &mesh);
    let max = if mesh.max_influences == 0 { DEFAULT_MAX_INFLUENCES } else { mesh.max_influences } as usize;

    let mut all = current_shares(&armature.bones, &mesh);
    let mut edited = vec![];
    for (vid, value) in edits {
        let Some(i) = mesh.vertices.iter().position(|v| v.id == *vid) else { continue };
        let fallback = nearest_bone(&world, &skin, bone_id, rest[i]).unwrap_or(mesh_id);
        let shares = all.entry(*vid).or_insert_with(|| vec![(mesh_id, 1.)]);
        weights::set_share(shares, bone_id, *value, fallback);
        if mesh.envelope_weights {
            weights::limit(shares, max, bone_id, mesh_id);
        }
        edited.push(*vid);
    }
    let keep = if keep_listed { edited.clone() } else { vec![] };
    write_shares(armature, mesh_id, &all, &keep);

    let mesh = armature.find_bone_mut(mesh_id).unwrap();
    for vid in edited {
        lock_vertex(mesh, vid);
    }
}

/// One dab of weight painting on `mesh_id`'s vertices `dab` (vertex id, brush
/// falloff there), painting `bone_id` in `mode` at `strength` (§4.5).
pub fn paint(armature: &mut Armature, mesh_id: i32, bone_id: i32, dab: &[(u32, f32)], mode: weights::PaintMode, strength: f32) {
    let Some(mesh) = find(&armature.bones, mesh_id).cloned() else { return };
    let all = current_shares(&armature.bones, &mesh);
    let share = |vid: u32| all.get(&vid).map_or(0., |s| weights::share_of(s, bone_id));

    let mut neighbours: HashMap<u32, Vec<u32>> = HashMap::new();
    if mode == weights::PaintMode::Smooth {
        for (a, b) in topology::edges(&mesh.indices) {
            let (Some(va), Some(vb)) = (mesh.vertices.get(a as usize), mesh.vertices.get(b as usize)) else { continue };
            neighbours.entry(va.id).or_default().push(vb.id);
            neighbours.entry(vb.id).or_default().push(va.id);
        }
    }

    let edits: Vec<(u32, f32)> = dab
        .iter()
        .map(|(vid, f)| {
            let around = neighbours.get(vid).filter(|n| !n.is_empty());
            let avg = around.map_or(share(*vid), |n| n.iter().map(|v| share(*v)).sum::<f32>() / n.len() as f32);
            (*vid, weights::painted(mode, share(*vid), strength * f, avg))
        })
        .filter(|(vid, value)| (value - share(*vid)).abs() > 1e-5)
        .collect();
    if !edits.is_empty() {
        edit_shares(armature, mesh_id, bone_id, &edits, false);
    }
}

/// `mesh_id`'s vertices where they're drawn in the setup pose (world units), in
/// vertex order: what region outlines are drawn against.
pub fn rest_positions(armature: &Armature, mesh_id: i32) -> Vec<Vec2> {
    let world = setup_world(armature);
    bind_pose::drawn_vertices(armature, &world, mesh_id)
}

/// Replace the shares of the vertices in `new` (vertex id → shares) and lock them,
/// keeping *Max influences* by dropping each vertex's weakest bones.
pub fn set_shares(armature: &mut Armature, mesh_id: i32, new: HashMap<u32, weights::Shares>) {
    let Some(mesh) = find(&armature.bones, mesh_id).cloned() else { return };
    let max = if mesh.max_influences == 0 { DEFAULT_MAX_INFLUENCES } else { mesh.max_influences } as usize;
    let mut all = current_shares(&armature.bones, &mesh);
    let edited: Vec<u32> = new.keys().copied().collect();
    for (vid, mut shares) in new {
        let strongest = shares.iter().fold((mesh_id, -1.), |b, s| if s.1 > b.1 { *s } else { b }).0;
        weights::limit(&mut shares, max, strongest, mesh_id);
        all.insert(vid, shares);
    }
    write_shares(armature, mesh_id, &all, &[]);
    let mesh = armature.find_bone_mut(mesh_id).unwrap();
    for vid in edited {
        lock_vertex(mesh, vid);
    }
}

/// Assign drawn parts to bones (docs/AI_SKINNING.md §4.1): each part is a bone and an
/// outline in world units at rest; a vertex inside an outline goes to its bone (the
/// last part wins where outlines overlap), blending with neighbouring parts over
/// `blend` measured along the mesh. Vertices in no part keep their weights. Returns
/// how many vertices were assigned.
pub fn assign_parts(armature: &mut Armature, mesh_id: i32, parts: &[(i32, Vec<Vec2>)], blend: f32) -> usize {
    let Some(mesh) = find(&armature.bones, mesh_id).cloned() else { return 0 };
    let pos = rest_positions(armature, mesh_id);
    let labels: Vec<Option<i32>> = pos
        .iter()
        .map(|p| parts.iter().rev().find(|(_, outline)| weights::in_polygon(*p, outline)).map(|(bone, _)| *bone))
        .collect();
    let edges = topology::edges(&mesh.indices);
    let shares = weights::part_shares(&pos, &edges, &labels, blend);
    let count = shares.len();
    let new = shares.into_iter().map(|(v, s)| (mesh.vertices[v].id, s)).collect();
    set_shares(armature, mesh_id, new);
    count
}

/// Set `bone`'s share to `share` for the vertices inside `outline` (world units at
/// rest), fading in over `feather` from the outline's edge (§4.1 `set_region`).
/// Returns how many vertices changed.
pub fn set_region(armature: &mut Armature, mesh_id: i32, outline: &[Vec2], bone: i32, share: f32, feather: f32) -> usize {
    let Some(mesh) = find(&armature.bones, mesh_id).cloned() else { return 0 };
    let pos = rest_positions(armature, mesh_id);
    let all = current_shares(&armature.bones, &mesh);
    let edits: Vec<(u32, f32)> = mesh
        .vertices
        .iter()
        .zip(&pos)
        .filter(|(_, p)| weights::in_polygon(**p, outline))
        .map(|(v, p)| {
            let current = all.get(&v.id).map_or(0., |s| weights::share_of(s, bone));
            let t = if feather > 0. { (weights::polygon_edge_distance(*p, outline) / feather).min(1.) } else { 1. };
            (v.id, current + (share - current) * t)
        })
        .collect();
    edit_shares(armature, mesh_id, bone, &edits, false);
    edits.len()
}

/// Smooth weights over the vertices in `vertex_ids` (§4.1 `smooth_region`): one bone's
/// share toward its neighbours' average, or with `bone` `None`, every bone's at once
/// (a blur that evens out seams). Returns how many vertices were smoothed.
pub fn smooth_vertices(armature: &mut Armature, mesh_id: i32, vertex_ids: &[u32], bone: Option<i32>, iterations: u32) -> usize {
    let Some(mesh) = find(&armature.bones, mesh_id).cloned() else { return 0 };
    if let Some(bone) = bone {
        let dab: Vec<(u32, f32)> = vertex_ids.iter().map(|v| (*v, 1.)).collect();
        for _ in 0..iterations {
            paint(armature, mesh_id, bone, &dab, weights::PaintMode::Smooth, 1.);
        }
        return dab.len();
    }
    let current = current_shares(&armature.bones, &mesh);
    let mut all: Vec<weights::Shares> =
        mesh.vertices.iter().map(|v| current.get(&v.id).cloned().unwrap_or_else(|| vec![(mesh_id, 1.)])).collect();
    let only: Vec<usize> = (0..mesh.vertices.len()).filter(|i| vertex_ids.contains(&mesh.vertices[*i].id)).collect();
    weights::blur(&mut all, &topology::edges(&mesh.indices), &only, iterations);
    let new = only.iter().map(|i| (mesh.vertices[*i].id, all[*i].clone())).collect();
    set_shares(armature, mesh_id, new);
    only.len()
}

/// The vertices of `mesh_id` inside `outline` (world units at rest), or every vertex
/// with no outline.
pub fn vertices_in(armature: &Armature, mesh_id: i32, outline: Option<&[Vec2]>) -> Vec<u32> {
    let Some(mesh) = find(&armature.bones, mesh_id) else { return vec![] };
    let pos = rest_positions(armature, mesh_id);
    mesh.vertices
        .iter()
        .zip(&pos)
        .filter(|(_, p)| outline.map_or(true, |o| weights::in_polygon(**p, o)))
        .map(|(v, _)| v.id)
        .collect()
}

/// Re-blend the seam between `parent` and `child` (§4.1 `blend_joint`): across `width`
/// centred on the child's head, each vertex's combined share of the two eases from
/// the parent to the child along the child's direction. Vertices outside that band,
/// or with no share of either, are left alone. Returns how many vertices changed.
pub fn blend_joint(armature: &mut Armature, mesh_id: i32, parent: i32, child: i32, width: f32) -> usize {
    let Some(mesh) = find(&armature.bones, mesh_id).cloned() else { return 0 };
    let world = setup_world(armature);
    let Some(c) = find(&world, child) else { return 0 };
    let joint = c.pos;
    let tail = renderer::tail_pos(c);
    let dir = if (tail - joint).mag() > 1e-3 {
        (tail - joint) / (tail - joint).mag()
    } else {
        Vec2::new(c.rot.cos(), c.rot.sin())
    };
    let pos = bind_pose::drawn_vertices(armature, &world, mesh_id);
    let all = current_shares(&armature.bones, &mesh);

    let mut new = HashMap::new();
    for (v, p) in mesh.vertices.iter().zip(&pos) {
        let Some(shares) = all.get(&v.id) else { continue };
        let total = weights::share_of(shares, parent) + weights::share_of(shares, child);
        let x = (p.x - joint.x) * dir.x + (p.y - joint.y) * dir.y;
        if total < 0.01 || x.abs() > width / 2. {
            continue;
        }
        let to_child = weights::joint_share(x, width) * total;
        let mut s: weights::Shares = shares.iter().filter(|s| s.0 != parent && s.0 != child).copied().collect();
        s.push((parent, total - to_child));
        s.push((child, to_child));
        s.retain(|x| x.1 > 0.);
        new.insert(v.id, s);
    }
    let count = new.len();
    set_shares(armature, mesh_id, new);
    count
}

/// Write every vertex's shares (`all`; vertices missing from it are left unbound) back
/// as bind weights. Weight binds keep their order and their vertices' order, so the
/// bind panel's rows stay put; a bone with no bind yet gets one at the end. Vertices in
/// `keep_zero` stay listed in binds they were in, at weight 0.
fn write_shares(armature: &mut Armature, mesh_id: i32, all: &HashMap<u32, weights::Shares>, keep_zero: &[u32]) {
    let mesh = find(&armature.bones, mesh_id).unwrap().clone();
    let mut binds = mesh.binds.clone();
    let mut slots: Vec<usize> = (0..binds.len()).filter(|k| !binds[*k].is_path && binds[*k].bone_id != -1).collect();
    let mut bones: Vec<i32> = slots.iter().map(|k| bind_bone(&armature.bones, mesh_id, &binds[*k])).collect();

    // bones without a bind, in armature order
    let mut new_bones: Vec<i32> = vec![];
    for shares in all.values() {
        for (id, _) in shares {
            if *id != mesh_id && !bones.contains(id) && !new_bones.contains(id) {
                new_bones.push(*id);
            }
        }
    }
    let order = |id: &i32| armature.bones.iter().position(|b| b.id == *id).unwrap_or(usize::MAX);
    new_bones.sort_by_key(order);
    for id in new_bones {
        let target = bind_pose::bind_target(armature, id, mesh_id);
        binds.push(BoneBind { bone_id: target, is_path: false, verts: vec![] });
        slots.push(binds.len() - 1);
        bones.push(id);
    }

    let mut weights_of: Vec<HashMap<i32, f32>> = vec![HashMap::new(); slots.len()];
    for v in &mesh.vertices {
        let Some(shares) = all.get(&v.id) else { continue };
        // a bone with several binds keeps its share on the first
        let seq: Vec<f32> = (0..slots.len())
            .map(|p| if bones.iter().position(|b| *b == bones[p]) == Some(p) { weights::share_of(shares, bones[p]) } else { 0. })
            .collect();
        let owner = weights::share_of(shares, mesh_id);
        for (p, w) in envelope::bind_weights(owner, &seq).into_iter().enumerate() {
            weights_of[p].insert(v.id as i32, w);
        }
    }

    for (p, k) in slots.iter().enumerate() {
        let listed: Vec<i32> = binds[*k].verts.iter().map(|bv| bv.id).collect();
        let keep = |id: i32, w: f32| w > 1e-6 || (listed.contains(&id) && keep_zero.contains(&(id as u32)));
        let mut verts: Vec<BoneBindVert> = vec![];
        // existing rows first, in their order, then new ones in vertex order
        for id in &listed {
            let w = weights_of[p].get(id).copied().unwrap_or(0.);
            if keep(*id, w) && !verts.iter().any(|bv| bv.id == *id) {
                verts.push(BoneBindVert { id: *id, weight: w });
            }
        }
        for v in &mesh.vertices {
            let id = v.id as i32;
            let w = weights_of[p].get(&id).copied().unwrap_or(0.);
            if w > 1e-6 && !listed.contains(&id) {
                verts.push(BoneBindVert { id, weight: w });
            }
        }
        binds[*k].verts = verts;
    }

    armature.find_bone_mut(mesh_id).unwrap().binds = binds;
}

/// For a weight bind of a bind-posed mesh: its bone, and each vertex's share of that
/// bone, which hand weighting shows and edits instead of the raw blend weights.
/// `None` for path binds, unassigned binds and classic (not bind-posed) meshes.
pub fn bind_shares(bones: &[Bone], mesh: &Bone, bind_idx: usize) -> Option<(i32, HashMap<i32, f32>)> {
    let bind = mesh.binds.get(bind_idx)?;
    if !mesh.bind_pose || bind.is_path || bind.bone_id == -1 {
        return None;
    }
    let bone = bind_bone(bones, mesh.id, bind);
    let shares = current_shares(bones, mesh)
        .into_iter()
        .map(|(vid, s)| (vid as i32, weights::share_of(&s, bone)))
        .collect();
    Some((bone, shares))
}

/// Lock a vertex of an envelope-weighted mesh after a hand edit, so recomputing keeps it.
pub fn lock_vertex(mesh: &mut Bone, vert_id: u32) {
    if mesh.envelope_weights && !mesh.locked_verts.contains(&vert_id) {
        mesh.locked_verts.push(vert_id);
    }
}

/// Set one field of `bone_id`'s envelope, starting from its default if it has none.
pub fn set_envelope_field(armature: &mut Armature, bone_id: i32, field: EnvelopeField, value: f32) {
    let world = setup_world(armature);
    let mut e = envelope_of(&world, bone_id);
    match field {
        EnvelopeField::Deform => e.deform = value == 1.,
        EnvelopeField::HeadInner => e.head_inner = value.max(0.),
        EnvelopeField::HeadOuter => e.head_outer = value.max(0.),
        EnvelopeField::TailInner => e.tail_inner = value.max(0.),
        EnvelopeField::TailOuter => e.tail_outer = value.max(0.),
        EnvelopeField::Falloff => e.falloff = envelope::Falloff::from_repr(value as i32).unwrap_or_default(),
        EnvelopeField::Absolute => e.absolute = value == 1.,
    }
    // the inner radius never exceeds the outer
    e.head_inner = e.head_inner.min(e.head_outer);
    e.tail_inner = e.tail_inner.min(e.tail_outer);
    if let Some(bone) = armature.find_bone_mut(bone_id) {
        bone.envelope = Some(e);
    }
}
