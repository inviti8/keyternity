//! The standard face and the face rig (docs/LIP_SYNC.md §6.4–6.5).
//!
//! The standard face is FaceBot's Mixamo-based shape set, with shorter canonical
//! names and the old spellings as aliases. The face rig is a group of helper bones,
//! the controls, laid out like FaceBot's 13-control mask. Each control's offset from
//! its rest (and its twist) drives shape weights through its slots, every time the
//! pose is constructed, so the controls are keyed and layered like any bone and the
//! shape weights follow live.

use crate::shared::*;

/// A standard shape: its canonical name, its group, and other names it's known by.
pub struct StdShape {
    pub name: &'static str,
    pub group: &'static str,
    pub aliases: &'static [&'static str],
}

macro_rules! std_shapes {
    ($(($name:expr, $group:expr $(, $alias:expr)*)),* $(,)?) => {
        &[$(StdShape { name: $name, group: $group, aliases: &[$($alias),*] }),*]
    };
}

/// The core set (docs/LIP_SYNC.md §6.4). `_L` / `_R` are the character's left and
/// right; `Left` / `Right` spellings are matched automatically.
pub const STANDARD: &[StdShape] = std_shapes![
    ("Blink_L", "eyes"),
    ("Blink_R", "eyes"),
    ("BrowsUp_L", "brows"),
    ("BrowsUp_R", "brows"),
    ("BrowsDown_L", "brows"),
    ("BrowsDown_R", "brows"),
    ("BrowsIn_L", "brows"),
    ("BrowsIn_R", "brows"),
    ("BrowsOuterLower_L", "brows"),
    ("BrowsOuterLower_R", "brows"),
    ("NoseScrunch_L", "nose"),
    ("NoseScrunch_R", "nose"),
    ("MouthOpen", "mouth"),
    ("MouthUp", "mouth"),
    ("MouthDown", "mouth"),
    ("MidMouth_L", "mouth"),
    ("MidMouth_R", "mouth"),
    ("MouthNarrow_L", "mouth"),
    ("MouthNarrow_R", "mouth"),
    ("MouthWhistle_L", "mouth", "MouthWhistle_NarrowAdjust_Left", "MouthWhistleNarrowAdjust_Left"),
    ("MouthWhistle_R", "mouth", "MouthWhistle_NarrowAdjust_Right", "MouthWhistleNarrowAdjust_Right"),
    ("Smile_L", "mouth"),
    ("Smile_R", "mouth"),
    ("Frown_L", "mouth"),
    ("Frown_R", "mouth"),
    ("UpperLipUp_L", "mouth", "UpperLeftUp_Left"),
    ("UpperLipUp_R", "mouth", "UpperLeftUp_Right"),
    ("UpperLipIn", "mouth"),
    ("UpperLipOut", "mouth"),
    ("LowerLipDown_L", "mouth"),
    ("LowerLipDown_R", "mouth"),
    ("LowerLipIn", "mouth"),
    ("LowerLipOut", "mouth"),
    ("JawSide_L", "jaw", "JawRotateZ_Left"),
    ("JawSide_R", "jaw", "JawRotateZ_Right"),
    ("JawTwist_L", "jaw", "JawRotateY_Left", "Jaw_RotateY_Left"),
    ("JawTwist_R", "jaw", "JawRotateY_Right", "Jaw_RotateY_Right"),
    ("JawForward", "jaw", "JawForeward"),
    ("JawBackward", "jaw"),
    ("TongueUp", "mouth"),
];

/// Preston Blair's visemes, optional viseme shapes (§6.4).
pub const VISEMES: [&str; 10] = ["AI", "E", "O", "U", "etc", "WQ", "MBP", "L", "FV", "rest"];

/// A name reduced for matching: lower case, no separators, and a trailing
/// left/right as l/r (so `Blink_Left`, `blink_l` and `BlinkL` all match).
pub fn normalize(name: &str) -> String {
    let mut n: String = name.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    for (long, short) in [("left", "l"), ("right", "r")] {
        if let Some(stem) = n.strip_suffix(long) {
            n = format!("{stem}{short}");
        }
    }
    n
}

/// The standard name `name` stands for, if any: a canonical name, an alias, or a
/// viseme (visemes match exactly, ignoring case, as "L" and "E" are short).
pub fn canonical(name: &str) -> Option<&'static str> {
    if let Some(v) = VISEMES.iter().find(|v| v.eq_ignore_ascii_case(name.trim())) {
        return Some(v);
    }
    let n = normalize(name);
    STANDARD
        .iter()
        .find(|s| normalize(s.name) == n || s.aliases.iter().any(|a| normalize(a) == n))
        .map(|s| s.name)
}

/// The shape on `bone` that stands for the standard `name`, by id.
pub fn find_shape(bone: &Bone, name: &str) -> Option<i32> {
    bone.shapes.iter().find(|s| canonical(&s.name) == Some(name)).map(|s| s.id)
}

/// Add the standard shapes the mesh doesn't have yet (empty, named and grouped), with
/// the visemes too if `visemes`. Returns how many were added.
pub fn add_standard_shapes(bone: &mut Bone, visemes: bool) -> usize {
    let mut added = 0;
    let wanted = STANDARD.iter().map(|s| (s.name, s.group)).chain(VISEMES.iter().filter(|_| visemes).map(|v| (*v, "visemes")));
    for (name, group) in wanted {
        if find_shape(bone, name).is_some() {
            continue;
        }
        let id = crate::shapes::next_id(bone);
        bone.shapes.push(MeshShape { id, name: name.to_string(), group: group.to_string(), ..Default::default() });
        added += 1;
    }
    added
}

/// A control's value along `axis`, from its offset (in units of its range) and twist
/// (in units of 45°): the positive half of one direction, so opposite slots never mix.
pub fn axis_value(c: &FaceControl, pos: Vec2, rot: f32, axis: ControlAxis) -> f32 {
    let range = c.range.max(1e-4);
    let off = (pos - c.rest) / range;
    let twist = (rot - c.rest_rot) / std::f32::consts::FRAC_PI_4;
    let v = match axis {
        ControlAxis::Right => off.x,
        ControlAxis::Left => -off.x,
        ControlAxis::Up => off.y,
        ControlAxis::Down => -off.y,
        ControlAxis::TwistCcw => twist,
        ControlAxis::TwistCw => -twist,
    };
    v.clamp(0., 1.5)
}

/// Drive shape weights from the face controls (docs/LIP_SYNC.md §6.5): each control's
/// slots add their value × weight to their target shapes. Runs on local (unconstructed)
/// bones, before shapes are applied, so it's called by `renderer::construction`.
pub fn drive(bones: &mut [Bone]) {
    let mut adds: Vec<(i32, i32, f32)> = vec![];
    for b in bones.iter() {
        let Some(c) = &b.face_control else { continue };
        for slot in &c.slots {
            let v = axis_value(c, b.pos, b.rot, slot.axis);
            if v <= 0. {
                continue;
            }
            for t in &slot.targets {
                adds.push((t.mesh, t.shape, v * t.weight));
            }
        }
    }
    for (mesh, shape, w) in adds {
        if let Some(s) = bones.iter_mut().find(|b| b.id == mesh).and_then(|b| b.shapes.iter_mut().find(|s| s.id == shape)) {
            s.weight = (s.weight + w).min(1.5);
        }
    }
}

/// One control of the default layout: its name, parent control, rest position on
/// FaceBot's mask (x across, y up; relative to its parent's rest), and its slots as
/// (axis, [(standard shape, weight)]).
pub struct LayoutControl {
    pub name: &'static str,
    pub parent: Option<&'static str>,
    pub pos: (f32, f32),
    pub slots: &'static [(ControlAxis, &'static [(&'static str, f32)])],
}

use ControlAxis::*;

/// FaceBot's 13 manipulators in 2D (docs/LIP_SYNC.md §6.5). Rest positions come from
/// the zero groups in `FaceBot.FBX`; the jaw sits below the mouth, and the tongue
/// beside the jaw, so no two handles overlap. Character left (`_L`) is screen right.
pub const LAYOUT: &[LayoutControl] = &[
    LayoutControl { name: "Brow_L", parent: None, pos: (6.6, 7.6), slots: &[(Up, &[("BrowsUp_L", 1.)]), (Down, &[("BrowsDown_L", 1.)]), (Left, &[("BrowsIn_L", 1.)]), (TwistCw, &[("BrowsOuterLower_L", 1.)])] },
    LayoutControl { name: "Brow_R", parent: None, pos: (-6.6, 7.6), slots: &[(Up, &[("BrowsUp_R", 1.)]), (Down, &[("BrowsDown_R", 1.)]), (Right, &[("BrowsIn_R", 1.)]), (TwistCcw, &[("BrowsOuterLower_R", 1.)])] },
    LayoutControl { name: "Eye_L", parent: None, pos: (6.4, 4.6), slots: &[(Down, &[("Blink_L", 1.)])] },
    LayoutControl { name: "Eye_R", parent: None, pos: (-6.4, 4.6), slots: &[(Down, &[("Blink_R", 1.)])] },
    LayoutControl { name: "Nose_L", parent: None, pos: (2.0, -2.8), slots: &[(Up, &[("NoseScrunch_L", 1.)])] },
    LayoutControl { name: "Nose_R", parent: None, pos: (-2.0, -2.8), slots: &[(Up, &[("NoseScrunch_R", 1.)])] },
    LayoutControl { name: "Mouth", parent: None, pos: (0., -8.6), slots: &[(Up, &[("MouthUp", 1.)]), (Down, &[("MouthDown", 1.)]), (Right, &[("MidMouth_L", 1.)]), (Left, &[("MidMouth_R", 1.)])] },
    LayoutControl { name: "Corner_L", parent: Some("Mouth"), pos: (5.8, -0.3), slots: &[(Up, &[("Smile_L", 1.)]), (Down, &[("Frown_L", 1.)]), (Left, &[("MouthNarrow_L", 1.)]), (Right, &[("MouthWhistle_L", 1.)])] },
    LayoutControl { name: "Corner_R", parent: Some("Mouth"), pos: (-5.8, -0.3), slots: &[(Up, &[("Smile_R", 1.)]), (Down, &[("Frown_R", 1.)]), (Right, &[("MouthNarrow_R", 1.)]), (Left, &[("MouthWhistle_R", 1.)])] },
    LayoutControl { name: "UpperLip", parent: Some("Mouth"), pos: (0., 1.3), slots: &[(Up, &[("UpperLipUp_L", 1.), ("UpperLipUp_R", 1.)]), (TwistCcw, &[("UpperLipOut", 1.)]), (TwistCw, &[("UpperLipIn", 1.)])] },
    LayoutControl { name: "Jaw", parent: Some("Mouth"), pos: (0., -3.2), slots: &[(Down, &[("MouthOpen", 1.)]), (Up, &[("JawForward", 1.)]), (Right, &[("JawSide_L", 1.)]), (Left, &[("JawSide_R", 1.)]), (TwistCcw, &[("JawTwist_L", 1.)]), (TwistCw, &[("JawTwist_R", 1.)])] },
    LayoutControl { name: "LowerLip", parent: Some("Jaw"), pos: (0., 1.9), slots: &[(Down, &[("LowerLipDown_L", 1.), ("LowerLipDown_R", 1.)]), (TwistCcw, &[("LowerLipOut", 1.)]), (TwistCw, &[("LowerLipIn", 1.)])] },
    LayoutControl { name: "Tongue", parent: None, pos: (0., -14.4), slots: &[(Up, &[("TongueUp", 1.)])] },
];

/// The mask's extent in its own units (x half-width, y bottom and top), for fitting it
/// to a face.
pub const MASK_HALF_WIDTH: f32 = 7.;
pub const MASK_BOTTOM: f32 = -13.;
pub const MASK_TOP: f32 = 8.6;

/// A control's travel for full weight, in mask units.
pub const RANGE: f32 = 1.6;

/// Set Up Face (docs/LIP_SYNC.md §6.5): add a face rig for the mesh `mesh_id`: a root
/// bone (under the mesh's parent, at the mesh) with the 13 controls laid out over the
/// face, each slot mapped to the mesh's matching standard shapes. Slots whose shapes
/// the mesh lacks stay empty, to map later. Returns the new bones' ids, root first.
pub fn set_up(armature: &mut Armature, mesh_id: i32, root_name: &str) -> Vec<i32> {
    let Some(mesh) = armature.bones.iter().find(|b| b.id == mesh_id).cloned() else { return vec![] };
    if mesh.vertices.is_empty() {
        return vec![];
    }
    // fit the mask over the mesh's bounds (in the mesh's own space)
    let (mut lo, mut hi) = (Vec2::new(f32::MAX, f32::MAX), Vec2::new(f32::MIN, f32::MIN));
    for v in &mesh.vertices {
        lo = Vec2::new(lo.x.min(v.pos.x), lo.y.min(v.pos.y));
        hi = Vec2::new(hi.x.max(v.pos.x), hi.y.max(v.pos.y));
    }
    let size = hi - lo;
    let s = (size.x / (2. * MASK_HALF_WIDTH)).min(size.y / (MASK_TOP - MASK_BOTTOM)).max(1e-3);
    let centre = (lo + hi) / 2.;
    let origin = Vec2::new(centre.x, centre.y - (MASK_TOP + MASK_BOTTOM) / 2. * s);

    let mut ids: Vec<i32> = armature.bones.iter().map(|b| b.id).collect();
    let mut next = || {
        let id = generate_id(ids.clone());
        ids.push(id);
        id
    };
    let base = |id: i32, parent_id: i32, name: String, pos: Vec2| Bone {
        id,
        parent_id,
        name,
        pos,
        scale: Vec2::new(1., 1.),
        pivot_scale: Vec2::new(1., 1.),
        tint: default_tint(),
        group_color: Color::new(0, 0, 0, 0),
        ..Default::default()
    };

    let root_id = next();
    let mut new_bones = vec![base(root_id, mesh.parent_id, root_name.to_string(), mesh.pos + origin)];
    let mut placed: Vec<(&str, i32)> = vec![];
    for lc in LAYOUT {
        let parent = lc.parent.and_then(|p| placed.iter().find(|(n, _)| *n == p).map(|(_, id)| *id)).unwrap_or(root_id);
        let pos = Vec2::new(lc.pos.0, lc.pos.1) * s;
        let slots = lc
            .slots
            .iter()
            .map(|(axis, targets)| ControlSlot {
                axis: *axis,
                targets: targets
                    .iter()
                    .filter_map(|(name, w)| find_shape(&mesh, name).map(|shape| SlotTarget { mesh: mesh_id, shape, weight: *w }))
                    .collect(),
            })
            .collect();
        let id = next();
        let mut b = base(id, parent, format!("{root_name}_{}", lc.name), pos);
        b.face_control = Some(FaceControl { name: lc.name.to_string(), rest: pos, rest_rot: 0., range: RANGE * s, slots, mesh: Some(mesh_id) });
        new_bones.push(b);
        placed.push((lc.name, id));
    }
    let out: Vec<i32> = new_bones.iter().map(|b| b.id).collect();
    armature.bones.extend(new_bones);
    out
}

/// The rig's bones: every control, and each rig root (the parent of top-level
/// controls) whose children are all controls.
pub fn rig_bones(bones: &[Bone]) -> Vec<i32> {
    let mut ids: Vec<i32> = bones.iter().filter(|b| b.face_control.is_some()).map(|b| b.id).collect();
    let parents: Vec<i32> = bones.iter().filter(|b| b.face_control.is_some()).map(|b| b.parent_id).collect();
    for p in parents {
        if ids.contains(&p) || p == -1 {
            continue;
        }
        let children_all_controls = bones.iter().filter(|b| b.parent_id == p).all(|b| b.face_control.is_some());
        if children_all_controls {
            ids.push(p);
        }
    }
    ids
}

/// For exports to runtimes, which don't have face rigs (docs/LIP_SYNC.md §6.5): bake
/// each animation's driven shape weights into `ShapeWeight` keys (sampled per frame,
/// straight runs reduced), then remove the rig's bones and their keys. Animations
/// should already be flattened (no layers).
pub fn bake_for_export(armature: &mut Armature) {
    let rig = rig_bones(&armature.bones);
    if rig.is_empty() {
        return;
    }
    let mut driven: Vec<(i32, i32)> = vec![];
    for b in armature.bones.iter().filter(|b| b.face_control.is_some()) {
        for s in &b.face_control.as_ref().unwrap().slots {
            for t in &s.targets {
                if !driven.contains(&(t.mesh, t.shape)) {
                    driven.push((t.mesh, t.shape));
                }
            }
        }
    }
    let setup = armature.bones.clone();
    for anim in &mut armature.animations {
        let moves = anim.keyframes.iter().any(|k| rig.contains(&k.bone_id));
        if moves {
            let last = anim.last_frame();
            let poses: Vec<Vec<Bone>> = (0..=last)
                .map(|f| {
                    let mut p = crate::layers::pose(anim, &setup, f);
                    drive(&mut p);
                    p
                })
                .collect();
            for (mesh, shape) in &driven {
                let values: Vec<(i32, f32)> = poses
                    .iter()
                    .enumerate()
                    .filter_map(|(f, p)| {
                        p.iter().find(|b| b.id == *mesh).and_then(|b| b.shapes.iter().find(|s| s.id == *shape)).map(|s| (f as i32, s.weight))
                    })
                    .collect();
                // only channels the rig actually moves
                if values.iter().all(|(_, w)| (*w - values[0].1).abs() < 1e-5) && !anim.keyframes.iter().any(|k| k.bone_id == *mesh && k.shape == *shape) {
                    continue;
                }
                anim.keyframes.retain(|k| !(k.bone_id == *mesh && k.element == AnimElement::ShapeWeight && k.shape == *shape));
                anim.keyframes.extend(crate::layers::sampled(*mesh, &AnimElement::ShapeWeight, *shape, &values));
            }
        }
        anim.keyframes.retain(|k| !rig.contains(&k.bone_id));
        anim.sort_keyframes();
    }
    armature.bones.retain(|b| !rig.contains(&b.id));
}

/// Fill the empty slots of `mesh_id`'s face rig with its matching standard shapes
/// (after shapes are added, renamed or generated). Rigs set up before controls knew
/// their mesh are matched by their root sitting under the mesh's parent. Returns how
/// many slots were filled.
pub fn automap(armature: &mut Armature, mesh_id: i32) -> usize {
    let Some(mesh) = armature.bones.iter().find(|b| b.id == mesh_id).cloned() else { return 0 };
    let mut filled = 0;
    // which controls belong to this mesh's rig
    let rig_ids: Vec<i32> = crate::face_panel::rigs(armature)
        .into_iter()
        .filter(|(root, controls)| {
            controls.iter().any(|(_, id)| armature.bones.iter().find(|b| b.id == *id).and_then(|b| b.face_control.as_ref()).and_then(|f| f.mesh) == Some(mesh_id))
                || (controls.iter().all(|(_, id)| armature.bones.iter().find(|b| b.id == *id).and_then(|b| b.face_control.as_ref()).is_some_and(|f| f.mesh.is_none()))
                    && armature.bones.iter().find(|b| b.id == *root).is_some_and(|r| r.parent_id == mesh.parent_id))
        })
        .flat_map(|(_, controls)| controls.into_iter().map(|(_, id)| id))
        .collect();
    for b in armature.bones.iter_mut().filter(|b| rig_ids.contains(&b.id)) {
        let fc = b.face_control.as_mut().unwrap();
        fc.mesh = Some(mesh_id);
        let Some(lc) = LAYOUT.iter().find(|l| l.name == fc.name) else { continue };
        for slot in fc.slots.iter_mut().filter(|s| s.targets.is_empty()) {
            let Some((_, defaults)) = lc.slots.iter().find(|(a, _)| *a == slot.axis) else { continue };
            for (name, w) in defaults.iter() {
                if let Some(shape) = find_shape(&mesh, name) {
                    slot.targets.push(SlotTarget { mesh: mesh_id, shape, weight: *w });
                    filled += 1;
                }
            }
        }
    }
    filled
}
