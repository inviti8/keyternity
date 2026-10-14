//! Inverse kinematics (docs/IK.md): a constraint on a tip bone turns its
//! `chain` nearest ancestors so the tip's head reaches a target. Solving is
//! relative to the animated pose, so a target sitting on the tip moves nothing.

use crate::*;
use std::collections::HashMap;

/// FABRIK stops after this many passes, or once the tip is this close (px).
pub const FABRIK_ITERATIONS: usize = 20;
pub const FABRIK_TOLERANCE: f32 = 0.01;

/// Segments shorter than this (px) are treated as zero length.
const EPS: f32 = 1e-4;

/// What an IK constraint does to a bone.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Role {
    None,
    /// Turned by the constraint on this tip.
    Chain(i32),
    /// Has its own active constraint: on, with a target and a chain.
    Tip,
}

fn find(bones: &[Bone], id: i32) -> Option<&Bone> {
    bones.iter().find(|b| b.id == id)
}

/// The bones a constraint on `tip_id` turns, nearest the root first. Shorter
/// than `chain` when the tip isn't that deep.
pub fn chain_ids(bones: &[Bone], tip_id: i32, chain: u32) -> Vec<i32> {
    let mut ids = vec![];
    let mut id = find(bones, tip_id).map_or(-1, |b| b.parent_id);
    while id != -1 && (ids.len() as u32) < chain {
        ids.push(id);
        id = find(bones, id).map_or(-1, |b| b.parent_id);
    }
    ids.reverse();
    ids
}

/// Number of ancestors of `bone_id`: the longest chain a constraint on it can have.
pub fn depth(bones: &[Bone], bone_id: i32) -> u32 {
    chain_ids(bones, bone_id, u32::MAX).len() as u32
}

/// Constraints that are on and can do something: a target and at least one chain bone.
fn active(bones: &[Bone]) -> impl Iterator<Item = (&Bone, &IkConstraint)> {
    bones.iter().filter_map(|b| match &b.ik {
        Some(c) if c.enabled && c.chain > 0 && c.target_id != -1 && b.parent_id != -1 => Some((b, c)),
        _ => None,
    })
}

/// How the constraints in `bones` treat `bone_id`.
pub fn role(bones: &[Bone], bone_id: i32) -> Role {
    if active(bones).any(|(tip, _)| tip.id == bone_id) {
        return Role::Tip;
    }
    for (tip, c) in active(bones) {
        if chain_ids(bones, tip.id, c.chain).contains(&bone_id) {
            return Role::Chain(tip.id);
        }
    }
    Role::None
}

/// Whether the solver owns `bone_id`'s rotation and position (the transform
/// inputs and canvas handles are locked while it does).
pub fn locks(bones: &[Bone], bone_id: i32) -> (bool, bool) {
    match role(bones, bone_id) {
        Role::None => (false, false),
        Role::Tip => {
            let c = find(bones, bone_id).and_then(|b| b.ik.as_ref()).unwrap();
            (c.copy_rotation, true)
        }
        Role::Chain(tip) => {
            let c = find(bones, tip).and_then(|b| b.ik.as_ref()).unwrap();
            let first = chain_ids(bones, tip, c.chain).first() == Some(&bone_id);
            (true, !first)
        }
    }
}

/// A target may not be the tip, a chain bone, or anything under the chain's
/// first bone: moving the chain would move the target, which feeds back.
pub fn target_allowed(bones: &[Bone], tip_id: i32, chain: u32, target_id: i32) -> bool {
    if target_id == tip_id || find(bones, target_id).is_none() {
        return false;
    }
    let ids = chain_ids(bones, tip_id, chain.max(1));
    let Some(&first) = ids.first() else { return true };
    let mut id = target_id;
    while id != -1 {
        if id == first {
            return false;
        }
        id = find(bones, id).map_or(-1, |b| b.parent_id);
    }
    true
}

/// Order to solve in: parents first (by depth of the chain's first bone), then bone order.
fn order(bones: &[Bone]) -> Vec<i32> {
    let mut tips: Vec<(u32, usize, i32)> = active(bones)
        .map(|(tip, c)| {
            let first = chain_ids(bones, tip.id, c.chain).first().copied().unwrap_or(tip.id);
            let idx = bones.iter().position(|b| b.id == tip.id).unwrap();
            (depth(bones, first), idx, tip.id)
        })
        .collect();
    tips.sort();
    tips.into_iter().map(|t| t.2).collect()
}

/// World-rotation overrides from every active constraint in `local` (the
/// animated local pose), for `renderer::inheritance`.
pub fn solve(local: &Vec<Bone>) -> HashMap<i32, f32> {
    let mut rots = HashMap::new();
    for tip_id in order(local) {
        let mut world = local.clone();
        renderer::inheritance(&mut world, rots.clone(), &vec![]);
        solve_one(&world, tip_id, &mut rots);
    }
    rots
}

/// Solve the constraint on `tip_id` against the world pose `world`.
fn solve_one(world: &[Bone], tip_id: i32, rots: &mut HashMap<i32, f32>) {
    let tip = find(world, tip_id).unwrap();
    let c = tip.ik.as_ref().unwrap();
    let Some(target) = find(world, c.target_id) else { return };
    let weight = c.weight.clamp(0., 1.);
    let ids = chain_ids(world, tip_id, c.chain);
    if ids.is_empty() || weight <= 0. {
        return;
    }

    let mut joints: Vec<Vec2> = ids.iter().map(|id| find(world, *id).unwrap().pos).collect();
    joints.push(tip.pos);
    let solved = solve_joints(&joints, target.pos, c.bend);

    let mut last_delta = 0.;
    for (i, id) in ids.iter().enumerate() {
        let old = joints[i + 1] - joints[i];
        let new = solved[i + 1] - solved[i];
        let delta = if old.mag() < EPS || new.mag() < EPS {
            0.
        } else {
            utils::shortest_angle_delta(old.y.atan2(old.x), new.y.atan2(new.x))
        };
        // each bone's own world direction turns by its own delta; children follow
        // through inheritance, so the deltas don't accumulate
        rots.insert(*id, find(world, *id).unwrap().rot + delta * weight);
        last_delta = delta;
    }

    if c.copy_rotation {
        let rot = tip.rot + last_delta * weight;
        rots.insert(tip_id, rot + utils::shortest_angle_delta(rot, target.rot) * weight);
    }
}

/// New joint positions (the first stays put) so the last reaches `target`.
pub fn solve_joints(joints: &[Vec2], target: Vec2, bend: IkBend) -> Vec<Vec2> {
    let lengths: Vec<f32> = joints.windows(2).map(|w| (w[1] - w[0]).mag()).collect();
    match lengths.len() {
        0 => joints.to_vec(),
        1 => aim(joints, target),
        2 if lengths[0] > EPS && lengths[1] > EPS => two_bone(joints, target, bend),
        _ => {
            let mut q = fabrik(joints, target);
            if bend != IkBend::AsDrawn && turn(&q) * bend_sign(bend) < 0. {
                q = mirror(&q, target);
            }
            q
        }
    }
}

/// One bone: point it at the target.
fn aim(joints: &[Vec2], target: Vec2) -> Vec<Vec2> {
    let len = (joints[1] - joints[0]).mag();
    let dir = (target - joints[0]).normalize();
    if dir == Vec2::ZERO {
        return joints.to_vec();
    }
    vec![joints[0], joints[0] + dir * len]
}

/// Which way the chain turns at its first joint: + counter-clockwise, - clockwise, 0 straight.
fn turn(q: &[Vec2]) -> f32 {
    if q.len() < 3 {
        return 0.;
    }
    let (a, b) = ((q[1] - q[0]).normalize(), (q[2] - q[1]).normalize());
    let cross = a.x * b.y - a.y * b.x;
    if cross.abs() < 1e-4 { 0. } else { cross.signum() }
}

fn bend_sign(bend: IkBend) -> f32 {
    match bend {
        IkBend::Clockwise => -1.,
        IkBend::CounterClockwise => 1.,
        IkBend::AsDrawn => 0.,
    }
}

/// Two bones, by the law of cosines.
fn two_bone(joints: &[Vec2], target: Vec2, bend: IkBend) -> Vec<Vec2> {
    let root = joints[0];
    let a = (joints[1] - joints[0]).mag();
    let b = (joints[2] - joints[1]).mag();

    // side to bend to: forced, as drawn, or (straight chain) where FABRIK settles
    let mut side = bend_sign(bend);
    if side == 0. {
        side = turn(joints);
    }
    if side == 0. {
        side = turn(&fabrik(joints, target));
    }
    if side == 0. {
        side = 1.;
    }

    let to = target - root;
    let d = to.mag().clamp((a - b).abs() + EPS, a + b);
    if to.mag() < EPS {
        return joints.to_vec();
    }
    let base = to.y.atan2(to.x);
    let cos = ((a * a + d * d - b * b) / (2. * a * d)).clamp(-1., 1.);
    // a counter-clockwise turn at the joint puts the joint clockwise of the base line
    let angle = base - side * cos.acos();
    let joint = root + Vec2::new(angle.cos(), angle.sin()) * a;
    let end = joint + (target - joint).normalize() * b;
    vec![root, joint, end]
}

/// FABRIK, from the current pose, with the first joint fixed.
pub fn fabrik(joints: &[Vec2], target: Vec2) -> Vec<Vec2> {
    let lengths: Vec<f32> = joints.windows(2).map(|w| (w[1] - w[0]).mag()).collect();
    let root = joints[0];
    let mut q = joints.to_vec();
    let n = q.len() - 1;

    // out of reach: straighten toward the target
    if (target - root).mag() >= lengths.iter().sum::<f32>() {
        let dir = (target - root).normalize();
        for i in 0..n {
            q[i + 1] = q[i] + dir * lengths[i];
        }
        return q;
    }

    for _ in 0..FABRIK_ITERATIONS {
        if (q[n] - target).mag() < FABRIK_TOLERANCE {
            break;
        }
        q[n] = target;
        for i in (0..n).rev() {
            let dir = (q[i] - q[i + 1]).normalize();
            q[i] = q[i + 1] + dir * lengths[i];
        }
        q[0] = root;
        for i in 0..n {
            let dir = (q[i + 1] - q[i]).normalize();
            q[i + 1] = q[i] + dir * lengths[i];
        }
    }
    q
}

/// `q` mirrored across the line from its first joint to `target`.
fn mirror(q: &[Vec2], target: Vec2) -> Vec<Vec2> {
    let root = q[0];
    let axis = (target - root).normalize();
    if axis == Vec2::ZERO {
        return q.to_vec();
    }
    q.iter()
        .map(|p| {
            let v = *p - root;
            let along = axis * (v.x * axis.x + v.y * axis.y);
            root + along * 2. - v
        })
        .collect()
}
