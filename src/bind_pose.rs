//! Bind Pose skinning: standard linear blend skinning built from ordinary bones.
//!
//! See `docs/BIND_POSE.md`. In short: every bone used by a bind-posed mesh gets a child
//! "bind helper" whose local transform puts it at the identity (origin, rotation 0,
//! scale 1) in the setup pose. Mesh vertices are stored at their rest position in
//! armature space and bound to the helpers, so each helper moves them by exactly
//! `bone world · inverse(bone bind pose)`. The bind pose always equals the setup pose:
//! `sync_helpers` re-derives the helpers after any setup-pose edit.
//!
//! Helpers are plain bones, so `.skf` files and runtimes need no changes. Only the
//! editor knows about them (`Bone::bind_helper`, saved in editor.json).

use crate::renderer::{construction, is_facing_left};
use crate::shared::*;
use crate::utils;

/// Helper bone of `bone_id`, if it has one.
pub fn helper_for(armature: &Armature, bone_id: i32) -> Option<i32> {
    armature
        .bones
        .iter()
        .find(|b| b.bind_helper && b.parent_id == bone_id)
        .map(|b| b.id)
}

/// Whether a mesh bone is bind-posed, ie: its binds target bind helpers.
pub fn is_bind_posed(armature: &Armature, bone_id: i32) -> bool {
    let Some(bone) = armature.bones.iter().find(|b| b.id == bone_id) else {
        return false;
    };
    bone.binds.iter().any(|bind| {
        let target = armature.bones.iter().find(|b| b.id == bind.bone_id);
        target.map(|t| t.bind_helper).unwrap_or(false)
    })
}

/// Constructed (world) setup pose, including IK.
fn setup_world(armature: &Armature) -> Vec<Bone> {
    let local = armature.bones.clone();
    let mut world = local.clone();
    construction(&mut world, &local);
    world
}

/// World-space vertex positions of a bone as drawn, incl. the pivot offset the renderer adds.
fn drawn_vertices(armature: &Armature, world: &Vec<Bone>, bone_id: i32) -> Vec<Vec2> {
    let bone = world.iter().find(|b| b.id == bone_id).unwrap();
    let size = armature
        .tex_of(bone_id)
        .map(|t| t.size)
        .unwrap_or(Vec2::ZERO);
    let left = if is_facing_left(bone.scale) { -1. } else { 1. };
    let pivot = utils::rotate(&(size * bone.pivot_pos), bone.rot * left) * bone.scale;
    bone.vertices.iter().map(|v| v.pos + pivot).collect()
}

/// Local transform that puts a child at the identity under a parent world transform.
/// Inverse of `renderer::inheritance` (docs/BIND_POSE.md §2.1).
fn identity_local(parent: &Bone) -> (Vec2, f32, Vec2) {
    let s = parent.scale;
    let rot = if is_facing_left(s) {
        parent.rot
    } else {
        -parent.rot
    };
    let pos = utils::rotate(&(Vec2::ZERO - parent.pos), -parent.rot) / s;
    (pos, rot, Vec2::new(1. / s.x, 1. / s.y))
}

/// Create the helper of `bone_id` if missing. Its transform is set by `sync_helpers`.
fn ensure_helper(armature: &mut Armature, bone_id: i32) -> i32 {
    if let Some(id) = helper_for(armature, bone_id) {
        return id;
    }

    let parent_idx = armature.bones.iter().position(|b| b.id == bone_id).unwrap();
    let parent = armature.bones[parent_idx].clone();

    // insert after the parent's subtree, keeping children contiguous
    let mut children = vec![];
    crate::armature_window::get_all_children(&armature.bones, &mut children, &parent);
    let idx = parent_idx + children.len() + 1;

    let ids = armature.bones.iter().map(|b| b.id).collect();
    let helper = Bone {
        id: generate_id(ids),
        name: format!("{}__bind", parent.name),
        parent_id: bone_id,
        scale: Vec2::new(1., 1.),
        pivot_scale: Vec2::new(1., 1.),
        tint: TintColor::new(1., 1., 1., 1.),
        ik_target_id: -1,
        ik_family_id: -1,
        physics_id: -1,
        visuals_id: -1,
        zindex: -1,
        group_color: Color::new(0, 0, 0, 0),
        bind_helper: true,
        ..Default::default()
    };
    let id = helper.id;
    armature.bones.insert(idx, helper);
    id
}

/// Re-derive every helper's local transform from the current setup pose, so each sits at
/// the identity. Call after any setup-pose edit (the bind pose follows the setup pose).
pub fn sync_helpers(armature: &mut Armature) {
    if !armature.bones.iter().any(|b| b.bind_helper) {
        return;
    }
    let world = setup_world(armature);
    for b in 0..armature.bones.len() {
        if !armature.bones[b].bind_helper {
            continue;
        }
        let parent_id = armature.bones[b].parent_id;
        let Some(parent) = world.iter().find(|w| w.id == parent_id) else {
            continue;
        };
        let (pos, rot, scale) = identity_local(parent);
        let helper = &mut armature.bones[b];
        helper.pos = pos;
        helper.rot = rot;
        helper.scale = scale;
    }
}

/// Convert a mesh bone to bind pose skinning at the current setup pose. Vertices don't move.
pub fn set_bind_pose(armature: &mut Armature, mesh_id: i32) -> Result<(), String> {
    let Some(mesh) = armature.bones.iter().find(|b| b.id == mesh_id) else {
        return Err("bone not found".into());
    };
    if mesh.bind_helper {
        return Err("bind helpers can't be bind-posed".into());
    }
    if mesh.vertices.is_empty() {
        return Err("bone has no mesh".into());
    }
    if mesh.binds.iter().any(|b| b.is_path) {
        return Err("meshes with path binds can't be bind-posed".into());
    }
    if is_bind_posed(armature, mesh_id) {
        return Ok(());
    }

    // bones whose frames the mesh uses: the owner, then every (valid) bind bone
    let mut used = vec![mesh_id];
    for bind in &mesh.binds {
        let valid = armature.bones.iter().any(|b| b.id == bind.bone_id);
        if valid && !used.contains(&bind.bone_id) {
            used.push(bind.bone_id);
        }
    }
    for id in &used {
        let bone = armature.bones.iter().find(|b| b.id == *id).unwrap();
        if bone.scale.x == 0. || bone.scale.y == 0. {
            return Err(format!("bone '{}' has zero scale", bone.name));
        }
    }

    // where the vertices are drawn now, before anything changes
    let world = setup_world(armature);
    let rest = drawn_vertices(armature, &world, mesh_id);

    let helpers: Vec<(i32, i32)> = used
        .iter()
        .map(|id| (*id, ensure_helper(armature, *id)))
        .collect();
    let helper_of = |id: i32| helpers.iter().find(|h| h.0 == id).map(|h| h.1);

    let mesh = armature.bones.iter_mut().find(|b| b.id == mesh_id).unwrap();

    // Every vertex first snaps to its rest position via the owner's helper; the original
    // binds then lerp from there with their original weights, which keeps the owner's
    // original share of each blend.
    let owner_bind = BoneBind {
        bone_id: helper_of(mesh_id).unwrap(),
        is_path: false,
        verts: mesh
            .vertices
            .iter()
            .map(|v| BoneBindVert {
                id: v.id as i32,
                weight: 1.,
            })
            .collect(),
    };
    let mut binds = vec![owner_bind];
    for bind in &mesh.binds {
        if let Some(helper) = helper_of(bind.bone_id) {
            binds.push(BoneBind {
                bone_id: helper,
                ..bind.clone()
            });
        }
    }
    mesh.binds = binds;

    for (v, pos) in mesh.vertices.iter_mut().zip(&rest) {
        v.pos = *pos;
        v.init_pos = *pos;
    }
    mesh.verts_edited = true;

    // the pivot is baked into the rest positions
    mesh.pivot_pos = Vec2::ZERO;
    mesh.pivot_rot = 0.;
    mesh.pivot_scale = Vec2::new(1., 1.);

    sync_helpers(armature);
    Ok(())
}

/// Convert a bind-posed mesh back to classic binds. Each vertex is stored in the frame of
/// the bone with its largest effective weight, which is exact for single-bone vertices
/// (as with classic binding). Helpers no longer used by any mesh are removed.
pub fn clear_bind_pose(armature: &mut Armature, mesh_id: i32) {
    if !is_bind_posed(armature, mesh_id) {
        return;
    }
    let world = setup_world(armature);
    let rest = drawn_vertices(armature, &world, mesh_id);

    let parent_of = |id: i32| {
        armature
            .bones
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.parent_id)
    };
    let mesh = armature
        .bones
        .iter()
        .find(|b| b.id == mesh_id)
        .unwrap()
        .clone();

    // drop the owner's snap bind, retarget the rest to the helpers' bones
    let owner_helper = helper_for(armature, mesh_id);
    let mut binds = vec![];
    for bind in &mesh.binds {
        if Some(bind.bone_id) == owner_helper {
            continue;
        }
        match parent_of(bind.bone_id) {
            Some(bone_id) if bone_id != -1 => binds.push(BoneBind {
                bone_id,
                ..bind.clone()
            }),
            _ => {}
        }
    }

    let mut vertices = mesh.vertices.clone();
    for (i, v) in vertices.iter_mut().enumerate() {
        // effective weights: the owner starts at 1, each bind lerps
        let mut weights: Vec<(i32, f32)> = vec![(mesh_id, 1.)];
        for bind in &binds {
            let Some(bv) = bind.verts.iter().find(|bv| bv.id == v.id as i32) else {
                continue;
            };
            for w in weights.iter_mut() {
                w.1 *= 1. - bv.weight;
            }
            weights.push((bind.bone_id, bv.weight));
        }
        let dominant = weights
            .iter()
            .fold(
                (mesh_id, -1.),
                |best, w| if w.1 > best.1 { *w } else { best },
            )
            .0;
        let frame = world.iter().find(|b| b.id == dominant).unwrap();
        let local = utils::rotate(&(rest[i] - frame.pos), -frame.rot) / frame.scale;
        v.pos = local;
        v.init_pos = local;
    }

    let mesh_mut = armature.bones.iter_mut().find(|b| b.id == mesh_id).unwrap();
    mesh_mut.binds = binds;
    mesh_mut.vertices = vertices;

    remove_unused_helpers(armature);
}

/// Remove helpers that no mesh binds to.
pub fn remove_unused_helpers(armature: &mut Armature) {
    let used: Vec<i32> = armature
        .bones
        .iter()
        .flat_map(|b| b.binds.iter().map(|bind| bind.bone_id))
        .collect();
    armature
        .bones
        .retain(|b| !b.bind_helper || used.contains(&b.id));
}
