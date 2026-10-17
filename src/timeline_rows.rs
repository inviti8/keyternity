//! The timeline's rows and key retiming (docs/TIMELINE.md §3).
//!
//! Rows: a Summary row, then each keyed bone's header, its property groups (Position,
//! Scale, Tint…), and, for an opened group, its channels (X/Y, R/G/B/A). Each row shows
//! a mark on every frame where any of its keys sits, so folding a bone still shows its
//! timing. Retiming (moving and scaling keys in time) resolves collisions the same way
//! for both. Everything here is pure, so it's tested directly.

use crate::shared::{AnimElement, Animation, Bone, HandlePreset, Keyframe};

/// A property group: one row in the timeline, for all of its channels.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Group {
    Position,
    Rotation,
    Scale,
    Tint,
    Texture,
    Hidden,
    Zindex,
    Ik,
    Other,
}

impl Group {
    pub const ALL: [Group; 9] = [
        Group::Position,
        Group::Rotation,
        Group::Scale,
        Group::Tint,
        Group::Texture,
        Group::Hidden,
        Group::Zindex,
        Group::Ik,
        Group::Other,
    ];

    pub fn from_index(i: usize) -> Option<Group> {
        Group::ALL.get(i).copied()
    }

    pub fn of(element: &AnimElement) -> Group {
        use AnimElement as E;
        match element {
            E::PositionX | E::PositionY => Group::Position,
            E::Rotation => Group::Rotation,
            E::ScaleX | E::ScaleY => Group::Scale,
            E::TintR | E::TintG | E::TintB | E::TintA => Group::Tint,
            E::Texture => Group::Texture,
            E::Hidden => Group::Hidden,
            E::Zindex => Group::Zindex,
            E::IkBend | E::IkWeight | E::IkCopyRotation => Group::Ik,
            _ => Group::Other,
        }
    }

    /// Its name in `keyframe_editor.groups`.
    pub fn key(self) -> &'static str {
        match self {
            Group::Position => "Position",
            Group::Rotation => "Rotation",
            Group::Scale => "Scale",
            Group::Tint => "Tint",
            Group::Texture => "Texture",
            Group::Hidden => "Hidden",
            Group::Zindex => "Zindex",
            Group::Ik => "Ik",
            Group::Other => "Other",
        }
    }

    /// A muted hue per group, shown on its label and keys: (r, g, b).
    pub fn hue(self) -> (u8, u8, u8) {
        match self {
            Group::Position => (120, 175, 235),
            Group::Rotation => (130, 205, 140),
            Group::Scale => (230, 175, 95),
            Group::Tint => (215, 125, 185),
            Group::Texture => (175, 150, 230),
            Group::Hidden => (190, 190, 190),
            Group::Zindex => (110, 200, 200),
            Group::Ik => (225, 120, 110),
            Group::Other => (170, 170, 170),
        }
    }
}

/// Keys that snap rather than blend: texture, visibility, z-index and IK flags.
pub fn is_stepped(element: &AnimElement) -> bool {
    use AnimElement as E;
    matches!(element, E::Texture | E::Hidden | E::Zindex | E::IkBend | E::IkCopyRotation | E::Locked)
}

/// A key drawn as a square: stepped by its property, or set to snap.
pub fn is_stepped_key(kf: &Keyframe) -> bool {
    is_stepped(&kf.element) || kf.handle_preset == HandlePreset::Snap
}

#[derive(Clone, PartialEq, Debug)]
pub enum Row {
    /// every key in the animation
    Summary,
    /// a bone's header: all its keys
    Bone(i32),
    /// one property group of a bone
    Group(i32, Group),
    /// one channel of a group that has several (Position X, Tint G…)
    Channel(i32, AnimElement),
}

impl Row {
    pub fn matches(&self, kf: &Keyframe) -> bool {
        match self {
            Row::Summary => true,
            Row::Bone(id) => kf.bone_id == *id,
            Row::Group(id, g) => kf.bone_id == *id && Group::of(&kf.element) == *g,
            Row::Channel(id, el) => kf.bone_id == *id && kf.element == *el,
        }
    }

    pub fn bone_id(&self) -> Option<i32> {
        match self {
            Row::Summary => None,
            Row::Bone(id) | Row::Group(id, _) | Row::Channel(id, _) => Some(*id),
        }
    }

    /// The group whose hue the row's keys take, if any.
    pub fn group(&self) -> Option<Group> {
        match self {
            Row::Group(_, g) => Some(*g),
            Row::Channel(_, el) => Some(Group::of(el)),
            _ => None,
        }
    }
}

/// The channels a bone has keys for in `group`, in order.
pub fn channels(anim: &Animation, bone_id: i32, group: Group) -> Vec<AnimElement> {
    let mut els: Vec<AnimElement> = anim
        .keyframes
        .iter()
        .filter(|kf| kf.frame >= 0 && kf.bone_id == bone_id && Group::of(&kf.element) == group)
        .map(|kf| kf.element.clone())
        .collect();
    els.sort();
    els.dedup();
    els
}

/// The rows for `anim`: Summary, then each keyed bone (in `bones` order) with its
/// groups unless the bone is folded, and the channels of groups in `opened`.
pub fn build_rows(anim: &Animation, bones: &[Bone], opened: &[(i32, Group)]) -> Vec<Row> {
    let keyed = |id: i32| anim.keyframes.iter().any(|kf| kf.frame >= 0 && kf.bone_id == id);
    if !anim.keyframes.iter().any(|kf| kf.frame >= 0 && bones.iter().any(|b| b.id == kf.bone_id)) {
        return vec![];
    }
    let mut rows = vec![Row::Summary];
    for bone in bones.iter().filter(|b| keyed(b.id)) {
        rows.push(Row::Bone(bone.id));
        if bone.anim_folded {
            continue;
        }
        let mut groups: Vec<Group> = anim
            .keyframes
            .iter()
            .filter(|kf| kf.frame >= 0 && kf.bone_id == bone.id)
            .map(|kf| Group::of(&kf.element))
            .collect();
        groups.sort();
        groups.dedup();
        for g in groups {
            rows.push(Row::Group(bone.id, g));
            let els = channels(anim, bone.id, g);
            if els.len() > 1 && opened.contains(&(bone.id, g)) {
                rows.extend(els.into_iter().map(|el| Row::Channel(bone.id, el)));
            }
        }
    }
    rows
}

/// The frames where `row` has keys, in order.
pub fn row_frames(anim: &Animation, row: &Row) -> Vec<i32> {
    let mut frames: Vec<i32> =
        anim.keyframes.iter().filter(|kf| kf.frame >= 0 && row.matches(kf)).map(|kf| kf.frame).collect();
    frames.sort();
    frames.dedup();
    frames
}

/// `row`'s keys on `frame`.
pub fn keys_at<'a>(anim: &'a Animation, row: &Row, frame: i32) -> Vec<&'a Keyframe> {
    anim.keyframes.iter().filter(|kf| kf.frame == frame && row.matches(kf)).collect()
}

/// One key, wherever it is: a channel has one key per frame.
pub fn same_key(a: &Keyframe, b: &Keyframe) -> bool {
    a.frame == b.frame && a.bone_id == b.bone_id && a.element == b.element
}

/// Move the `selected` keys to `to(frame)` (never below 0), keeping the others.
///
/// A moved key landing on an unselected key of its channel replaces it. Selected keys
/// of one channel landing on one frame merge, keeping the one that moved farther (so a
/// motion's extremes survive scaling). Returns the keys, the moved keys (the new
/// selection) and how many selected keys merged away.
pub fn retime(keys: &[Keyframe], selected: &[Keyframe], to: impl Fn(i32) -> i32) -> (Vec<Keyframe>, Vec<Keyframe>, usize) {
    let is_selected = |k: &Keyframe| selected.iter().any(|s| same_key(s, k));
    let mut moved: Vec<(Keyframe, i32)> = vec![];
    let mut merged = 0;
    for k in keys.iter().filter(|k| is_selected(k)) {
        let frame = to(k.frame).max(0);
        let dist = (frame - k.frame).abs();
        let key = Keyframe { frame, ..k.clone() };
        match moved.iter_mut().find(|(m, _)| same_key(m, &key)) {
            Some(existing) => {
                merged += 1;
                if dist > existing.1 {
                    *existing = (key, dist);
                }
            }
            None => moved.push((key, dist)),
        }
    }
    let mut out: Vec<Keyframe> = keys
        .iter()
        .filter(|k| !is_selected(k) && !moved.iter().any(|(m, _)| same_key(m, k)))
        .cloned()
        .collect();
    let moved: Vec<Keyframe> = moved.into_iter().map(|(k, _)| k).collect();
    out.extend(moved.iter().cloned());
    out.sort_by(|a, b| a.frame.cmp(&b.frame));
    (out, moved, merged)
}

/// Scaling keys in time, while it's under way (docs/TIMELINE.md §3.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleDrag {
    /// keys on this frame stay put
    pub pivot: i32,
    /// the pointer's x when scaling started: there, the factor is 1
    pub start_x: f32,
    /// started by Alt-dragging a key: releasing applies it (otherwise a click does)
    pub from_drag: bool,
}

/// The scale factor for the pointer at `x`, with the pivot at `pivot_x`. Starting
/// right on the pivot would make tiny moves huge, so the reference distance is at
/// least `min_ref` points.
pub fn scale_factor(drag: &ScaleDrag, pivot_x: f32, x: f32, min_ref: f32) -> f32 {
    let mut d0 = drag.start_x - pivot_x;
    if d0.abs() < min_ref {
        d0 = if d0 < 0. { -min_ref } else { min_ref };
    }
    (x - pivot_x) / d0
}

/// Where `frame` goes when scaling by `factor` around `pivot` (docs/TIMELINE.md §3.5).
pub fn scale_frame(frame: i32, pivot: i32, factor: f32) -> i32 {
    (pivot as f32 + (frame - pivot) as f32 * factor).round() as i32
}

/// Limit `factor` so that no key in `frames` goes below frame 0.
pub fn clamp_scale(frames: &[i32], pivot: i32, factor: f32) -> f32 {
    let mut factor = factor;
    for &f in frames {
        let d = (f - pivot) as f32;
        // pivot + d * factor >= 0
        if d > 0. && factor < 0. {
            factor = factor.max(-(pivot as f32) / d);
        } else if d < 0. && factor > 0. {
            factor = factor.min(pivot as f32 / -d);
        }
    }
    factor
}
