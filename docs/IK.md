# Inverse kinematics: constraints on the tip bone

**Goal:** replace SkelForm's IK families with IK that works the way Blender,
Spine and DragonBones do. Setting up IK should never move a bone or its art, and
chains may be drawn bent.

**Status:** implemented (§0). Decisions are in §8.

**Format:** this changes the file format (§6). There's no backward
compatibility: Keyternity has no user base yet, so older files simply lose their
IK, and SkelForm runtimes can't play rigs with IK.


## 0. As built

- The solver is `src/ik.rs`: `solve` returns world-rotation overrides for
  `renderer::inheritance`, used by `construction` and `runtime_construction`.
  `chain_ids`, `depth`, `role`, `locks` and `target_allowed` serve the editor,
  warnings and exporters.
- The constraint is `Bone.ik: Option<IkConstraint>` (`shared.rs`). Family ids,
  `JointEffector`, `bone_eff`, the per-bone IK fields and the root
  `inverse_kinematics` array are gone.
- `src/backwards_compat.rs` (SkelForm's version migrations) is deleted, which
  also fixes #10. Loading deserializes `armature.json` directly.
- Events: `AddIk`, `RemoveIk`, `SetIkChain`, `ToggleIkEnabled` and
  `DeleteIkTarget`, each with an undo step. Bend, weight and copy rotation go
  through `EditBone` as the animatable elements `IkBend`, `IkWeight` and
  `IkCopyRotation`, which reuse the slots, and so the icons, of the old IK
  elements.
- **Found on the way:** deleting a bone cleared the IK target of constraints
  whose target id equalled the deleted bone's *list index*. It now clears
  targets that are the deleted bone or one of its children.
- **Straight two-bone chains** with bend *As drawn* bend the way FABRIK settles
  from the current pose (§3.3), as the old solver did.
- Tests: `tests/all/ik.rs` (16), and the PSD import IK test was rewritten.
- **Target Space** (§4.1): `editor::ik_target_spaces` and `set_ik_target_space`.
  It builds on a fix to reparenting: dragging a bone onto another in the hierarchy
  kept only its position, by adding and subtracting parent positions, so under a
  rotated or scaled parent it jumped (and PSD import aims group bones, so most
  parents are rotated). `Armature::keep_world` now keeps the world position,
  rotation and scale, as the inverse of `renderer::inheritance`. Tests:
  `tests/all/ik_space.rs` (4).


## 1. What was wrong

The review (2026-10-13) found these. Each was confirmed by a test or by reading
the code.

| # | Problem | Cause |
|---|---|---|
| 1 | Save and export crash when family ids have a gap (pasting IK into a file without IK, deleting a family, a PSD with only `$ik_1`) | `inverse_kinematics[family_id]` indexes an array sized by family count |
| 2 | Joining a family moves the bone and its art | The solver sets each joint's world rotation to the direction of the next joint, so chains must be straight along +x. Joining a family forces `pos = (distance, 0)` and `rot = 0`; rotation edits on chain bones are forced to 0; loading a file zeroes `pos.y` |
| 3 | A chain that hangs off another chain misses its target | Every family is solved against the pose from before any IK |
| 4 | *Mimic Target Rotation* does nothing in the editor | The editor reads the flag from the first bone, the UI sets it on the last |
| 5 | Arc mode is wrong away from the origin, and NaN with the target on the root | Mixes absolute and relative coordinates; divides by the target distance |
| 6 | Baked IK keyframes are wrong under a rotated or flipped parent | Converts world rotations to local by subtracting the chain's rotations only |
| 7 | Families aren't validated | Any bones can share a number; list order decides the start; a target can sit inside its own chain |
| 8 | Renumbering shifts every family when the only family-less bone joins one | `reduce(-1)` |
| 9 | Pasted chains keep pointing at the original target | Paste doesn't remap `ik_target_id` |
| 10 | When Keyternity reaches 0.3 or 0.5, its own files get SkelForm's 0.3/0.5 migrations | Migrations match `version.contains("0.3")` on the app version |


## 2. Model

An **IK constraint** lives on one bone, the **tip**, as in Blender:

```rust
pub struct IkConstraint {
    pub target_id: i32,     // bone the tip reaches for, -1 for none
    pub chain: u32,         // bones above the tip that the solver turns (1 or more)
    pub bend: IkBend,       // AsDrawn, Clockwise, CounterClockwise
    pub weight: f32,        // 0..1: blend from the animated pose to the solved one
    pub copy_rotation: bool,// the tip takes the target's world rotation
    pub enabled: bool,      // off: the chain shows its animated (FK) pose
}
```

- The **chain** is the tip's `chain` nearest ancestors. It's always a real
  parent-to-child path, so it can't be invalid, and bone order in the list
  doesn't matter.
- The **joints** are the heads (origins) of the chain bones plus the tip's head.
  The solver moves the tip's head onto the target. Keyternity bones have no
  length, so this is Blender's IK with *Use Tail* off.
- `chain` is capped at the tip's depth. A constraint whose chain has no bones,
  or with no target, does nothing (and shows a warning).


## 3. Solving

### 3.1 Relative to the animated pose
Solving starts from the animated (FK) world pose: the setup pose plus any
keyframes. For each chain bone, the solver finds a new direction from its joint
to the next joint, and turns the bone by the **change** in that direction:

```
world_rot' = world_rot + weight · angle_between(old_dir, new_dir)
```

Nothing assumes a straight chain. A chain drawn bent stays bent until the
target asks otherwise. If the target sits where the tip already is, every
change is 0 and nothing moves. Local positions, scales and art offsets are never
touched.

The tip keeps its own rotation relative to its parent, unless `copy_rotation` is
on. Then its world rotation blends to the target's by `weight`.

### 3.2 Solvers
| Chain | Solver |
|---|---|
| 1 bone | Aim: turn the bone so the tip's head points at the target |
| 2 bones | Two-bone analytic (law of cosines). Exact, stable, no iteration |
| 3+ bones | FABRIK, starting from the current pose, up to 20 iterations or 0.01 px |

An out-of-reach target straightens the chain toward it. Zero-length segments are
skipped.

### 3.3 Bend direction
- **As drawn** (default): a two-bone chain bends the way it bends in the current
  pose. A straight one bends the way FABRIK settles from it. Longer chains keep
  their current bend naturally, because FABRIK starts from the current pose.
- **Clockwise / Counter-clockwise:** forces the side. For FABRIK, a solution
  bent the wrong way at the first joint is mirrored across the line from the
  chain's first joint to the target.

### 3.4 Order
Constraints are solved one after another, **parents first** (by the depth of the
chain's first bone, then by bone order). After each one the world pose is
rebuilt, so a chain hanging off another chain solves against where its parent
chain actually ended up.

A target inside the chain it drives would feed back into itself. The target
picker refuses the tip, chain bones and any descendant of the chain's first
bone, and loading flags it as a warning.

### 3.5 Where it runs
`renderer::construction` and `runtime_construction` call `ik::apply`, which
returns the world-rotation overrides that `inheritance` already accepts. The
editor canvas, preview, screenshots, sprite bounds, bind pose, `.skf` IK baking
and the DragonBones exporter all go through these two functions, so they all
change together.


## 4. Editing

### 4.1 Bone panel
The IK section on any bone with a parent:
- **No constraint:** an *Add IK* button. It creates the constraint with
  `chain = 1` (2 if the bone has a grandparent), weight 1, bend As drawn, and a
  new target bone at the bone's current world position, parented to the root
  and named "<bone> Target". The art doesn't move, because the target is where
  the tip already is.
- **With a constraint:** Enabled, Target (pick, select, clear), **Target Space**,
  Chain length, Bend, Weight and Copy target rotation, plus *Remove IK*.
- **Target Space** (added after review): what the target follows. *World* (the
  default) keeps it in place when the body moves, which grounds feet; a bone such
  as the chest makes it move with that bone, so a hand follows the upper body.
  Switching reparents the target bone and keeps it where it is. The list offers the
  chain's ancestors first (nearest first), then every other bone outside the chain;
  bones in or under the chain, or under the target, would feed back and aren't
  offered. Keyframes already on the target are kept as local values.
- **Bones in someone else's chain:** "Turned by IK on <tip>", which selects the
  tip when clicked.

### 4.2 What can be edited while IK is on
The solver owns:
- the rotation of every chain bone
- the position of every chain bone below the first, and the tip's position
- the tip's rotation, when copying the target's rotation

Those inputs and the canvas rotate/move handles are disabled, with a hint to
turn the constraint off. Bones IK drives (the chain and the tip) can't be picked by
clicking their point on the canvas, so a target sitting on its tip is the one picked;
they're selected in the hierarchy, or on the canvas once their constraint is off. With it off, everything is editable, and that is how
the rest layout of a chain is changed. Scale, the first chain bone's position,
and everything else stay editable.

Rotation edits are no longer forced to 0, and nothing writes positions.

### 4.3 Animation
Animatable: `IkWeight` (a 0–1 slider, for IK/FK switching), `IkBend`,
`IkCopyRotation`. Keyframes of chain bones' rotation still apply. They're the
FK pose that weight < 1 blends from.

### 4.4 Other editor paths
- **Paste:** the constraint is copied with the bone. If its target is pasted
  too, it points to the copy.
- **Delete:** deleting a target clears `target_id` on constraints that use it.
- **Canvas** (added after review): an IK target draws as a control, not a bone: a
  circle with a cross in the IK target color (brighter when selected), and a
  dotted line to the bone it follows, instead of the bone kite from its parent.
  Its point is invisible but still selects it.
- **Armature panel:** IK icon on tips (tooltip shows the chain length) and the
  target icon on targets.
- **Warnings:** *NoIkTarget* (constraint without a target) and *IkTargetInChain*
  (§3.4). *OnlyIk* and *BoneOutOfFamily* go, since families no longer exist.


## 5. Import and export

- **PSD import:** `$ik_N` markers still group bones into a chain. The last marked
  group becomes the tip, with `chain` = the number of marked groups − 1, and a
  target at its world position. The straightening and art counter-rotation
  workaround goes.
- **DragonBones export:** unchanged, still baked. IK chain and tip bones get
  per-frame rotation from the solved pose, and the setup pose uses the solved
  rotations.
- **DragonBones import:** unchanged, still baked with the ported DragonBones
  solver (exact). Mapping DragonBones IK to native constraints is possible later
  (DragonBones IK aims a bone's *tail*, which needs bone length).
- **`.skf` *Bake IK*:** rewritten to take local rotation from the parent's
  solved world rotation (fixes #6).


## 6. File format

The constraint is saved on its tip bone, as `"ik": { "target_id", "chain",
"bend", "weight", "copy_rotation", "enabled" }`, with `target_id` as the saved
bone index like every other bone reference. The root `inverse_kinematics` array
and the per-bone family fields are no longer written or read.

No migration: a file from SkelForm or an older Keyternity loads without its IK.
A file with keyframes of the old IK elements (*IkConstraint*, *IkMode*,
*IkFamilyId*, *MimicTarget*) doesn't load.

SkelForm's version migrations are removed too. They matched substrings of the
app version, so Keyternity 0.3 and 0.5 would have run them on their own files
(#10).


## 7. Tests (`tests/all/ik.rs`)
- solving with the target at the tip changes nothing, for bent chains of 1, 2 and 4 bones
- 1-bone aim, 2-bone analytic and FABRIK all reach a reachable target; out of reach, the chain straightens toward it
- bend: as-drawn keeps the side, and CW/CCW force it
- weight 0 is the FK pose and weight 0.5 is halfway
- copy rotation gives the tip the target's world rotation
- a nested chain reaches its target (#3)
- a flipped parent (negative scale) still reaches the target
- translation invariance (#5 class)
- saving keeps constraints, with the target remapped to its saved index (#1)
- baked IK under a rotated parent replays the solved pose (#6)
- roles and locks; a disabled constraint shows the animated pose
- target picker rejects chain bones and their descendants
- PSD import: an `$ik_` chain keeps its art in place once solved, with no counter-rotation


## 8. Decisions

| # | Decision |
|---|---|
| D1 | IK follows Blender/Spine/DragonBones: a constraint on the tip bone with a chain length, replacing family indices. |
| D2 | Solving is relative to the animated pose, so adding IK never moves anything and chains may be drawn bent. |
| D3 | Solvers: aim for 1 bone, analytic for 2, FABRIK for 3+. Arc mode is dropped. |
| D4 | Bend defaults to *As drawn*; Clockwise/Counter-clockwise force a side. |
| D5 | Weight (animatable) and Copy target rotation, as in Blender's Influence and Rotation options. |
| D6 | No backward compatibility (no user base yet): no IK conversion, and SkelForm's version migrations are removed. SkelForm runtimes are no longer a target. |
| D7 | The setup pose shows IK, as in Spine and DragonBones; turn a constraint off to edit its chain's rest layout. |
| D8 | DragonBones import and export keep baking IK. |
