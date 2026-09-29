# Bind Pose Skinning — design

**Status:** design draft, not implemented. Design decisions are recorded in §10.
**Goal:** give SkelForm standard, 3D-style *bind pose* skinning as an opt-in
**Set Bind Pose** operation, with **no change to the `.skf` runtime format** and
**no change to any runtime**.

---

## 1. The problem

### 1.1 How SkelForm skins today

A mesh vertex has **one** stored position, `Vertex.pos`, in its owner bone's space.
Each bind places that same coordinate in the bind bone's current frame and lerps
toward it by the bind's weight, in bind order (`renderer::construct_verts`):

```rust
// start on the owner bone
vert.pos = inherit_vert(init_pos, &owner, pivot_rot, pivot_scale);
// each weight bind, in order
end_pos = inherit_vert(init_pos, &bind_bone, pivot_rot, pivot_scale) - vert.pos;
vert.pos += end_pos * weight;
```

There is no record of where the vertex sat relative to the bind bone when it was
bound. To stop the vertex jumping when you bind it, the editor rewrites
`Vertex.pos` into the bind bone's frame at bind time (`editor.rs`, `ClickVertex`).
That only works for one bind:

```rust
// offset vertex such that it stays still after binding/unbinding
// todo: this currently only works if vertex is in 1 bind.
// Make it account for all of them
```

### 1.2 What goes wrong

Take a two-bone arm on the x axis. The upper arm is at x=100 and the forearm at
x=200. A vertex at the elbow (world 200, 0) is weighted 50/50 to both:

| | via upper arm | via forearm | 50/50 blend |
|---|---|---|---|
| stored `Vertex.pos` = (+100, 0) (upper-arm-relative) | 100+100 = **200** | 200+100 = **300** | **250**: jumps 50 px *at rest* |

Consequences today:
- **Multi-bone blends drift at rest.** The vertex moves as soon as a second bind
  is added.
- **Changing a weight later moves the vertex** (`SetBindWeight` does no
  compensation).
- **External rigs can't be matched.** DragonBones, Spine, Unity and Godot all use
  standard linear blend skinning, where each (vertex, bone) pair keeps its own
  bind-time offset.

### 1.3 What bind pose skinning means

When a mesh is bound, record each bone's transform at that moment (the **bind
pose**). Afterwards, a vertex follows each bone *relative to where that bone was
at bind time*:

```
world(v) = Σⱼ wⱼ · Wⱼ · inverse(Bⱼ) · v_rest
```

- `Wⱼ` is bone j's current world transform.
- `Bⱼ` is bone j's bind-pose world transform.
- `v_rest` is the vertex's rest position in armature space.

At the bind pose, `Wⱼ = Bⱼ`, so every term is `v_rest`. That means **no jump for
any combination of weights**, and weights can be edited freely.

---

## 2. The idea: bind poses as ordinary helper bones

`Wⱼ · inverse(Bⱼ)` is the *skinning matrix* of bone j. SkelForm can already
represent it: it is the world transform of a **child bone of j whose local
transform is `inverse(Bⱼ)`**. That child is constant and never animated.

**Set Bind Pose** therefore:
1. For each bone the mesh uses (its owner plus every weight-bind bone), creates or
   refreshes a child **bind helper** `<bone>__bind`. The helper's local transform
   puts its world transform at the **identity** (origin, rotation 0, scale 1) in
   the current pose.
2. Rewrites every mesh vertex to its **current world position** (armature space,
   `v_rest`). Vertices that had no bind are bound to the owner's helper with
   weight 1.
3. Points the mesh's weight binds at the helpers.

With every helper at the identity, `inherit_vert(v_rest, helperⱼ) = v_rest` for all
j. Nothing moves, whatever the weights. When bone j later moves, its helper moves
by exactly `Wⱼ · inverse(Bⱼ)`, and the vertex follows standard skinning.

**Why this shape:**
- Helpers are **ordinary bones**. `armature.json` keeps its existing format, so every
  runtime (Rust, C, JS, Unity, Go, Python, …) plays bind-pose meshes **unchanged**.
- Everything new lives in the **editor**. Runtime maintainers have nothing to do.
- SkelForm's sequential bind lerps stay as they are. With every bind seeing the same
  `v_rest`, any lerp order gives a proper blend, so the weight UI keeps working.

### 2.1 Helper local transform

SkelForm composes transforms per component (`renderer::inheritance`):

```
world.rot   = parent.rot + (facing_left(parent) ? -local.rot : local.rot)
world.scale = parent.scale * local.scale
world.pos   = parent.pos + rotate(local.pos * parent.scale, parent.rot)
```

To get world = identity under parent world `(P, r, S)` at bind time:

```
local.scale = 1 / S
local.rot   = facing_left(parent) ? r : -r
local.pos   = rotate(-P, -r) / S
```

`S` must be non-zero on both axes. Set Bind Pose refuses a bone with zero scale.

### 2.2 The mesh bone's pivot

`inherit_vert` also applies the mesh bone's `pivot_rot`/`pivot_scale`. The
renderer adds `pivot_pos` as a world-space offset afterwards (`final_pivot` in
`renderer.rs`). Set Bind Pose **bakes the pivot into `v_rest`** (it's part of the
current world position) and **resets the mesh bone's pivot to neutral**. UVs are
untouched, so the texture mapping doesn't change.

---

## 3. Data

| what | where | runtime-visible? |
|---|---|---|
| Helper bones | `armature.json` `bones`, as normal bones: name `<bone>__bind`, constant transform, no texture | yes, as ordinary bones (that's the point) |
| Mesh vertices at `v_rest`, binds pointing at helpers | existing `visuals` vertices/binds | yes, existing fields |
| "This bone is a bind helper" | **`editor.json`** `EditorBone.bind_helper: bool` (next to `locked`, `folded`, …) | **no** |

- **Older SkelForm versions** open these files fine. `editor.json` fields they don't
  know are ignored, and the helpers show up as ordinary bones. They still deform
  meshes correctly, because the deformation needs nothing but the bones.
- **Runtimes** see a few extra bones with constant transforms. The cost is one
  transform per helper per frame.

---

## 4. Editor behaviour

### 4.1 Operations

| operation | behaviour |
|---|---|
| **Set Bind Pose** (mesh bone panel) | Steps 1–3 above for the selected mesh. Allowed only in the setup pose (not while animating). One undo step. From then on the bind pose **is** the setup pose, kept in sync automatically (§4.3), so there's no separate Rebind |
| **Clear Bind Pose** | Converts back to classic binds. Each vertex gets its owner-relative position; helpers are removed if no other mesh uses them. Exact only for single-bind vertices, same as today |
| Bind/unbind a vertex (bind-posed mesh) | No position rewrite is needed. All helpers share the identity frame at bind time, so the `ClickVertex` compensation branch is skipped. Binding to a bone with no helper yet creates one at the current setup pose |
| Edit a weight | Nothing to compensate, since every bind sees the same `v_rest`. This fixes the `SetBindWeight` drift. Weights stay SkelForm's existing **per-bind** values, with the same UI and storage as today |
| Edit mesh vertices (drag, add, center, trace) | For a bind-posed mesh, `Vertex.pos` is in armature space, so the vertex tools work in armature space instead of the owner bone's frame. This affects the drag conversion in `editor.rs` (~L759), which today divides by the owner's rotation and scale. Regenerating the mesh (Trace, reset to rect) keeps the bind pose and re-binds new vertices to the owner's helper |
| Path binds | Unchanged. Path binds keep their own semantics and aren't converted |

### 4.2 Helper hygiene

- **Shown greyed out** in the armature window, under their bone, so the rig
  structure is honest about what the runtimes will see. They can't be renamed,
  dragged or edited, and the bone panel shows them read-only with a note on what
  they are.
- Never selectable on the canvas.
- Excluded from keyframing, IK families and physics.
- **Delete bone:** deleting a bone deletes its children, including its helper.
  Before that, remove binds to the helper from every mesh, so no vertex points at
  a missing bone. Because vertices share one frame, removing a bind doesn't make
  them jump at the bind pose.
- **Reparent (drag) bone:** helpers are children, so they move with their bone.
  In the setup pose the bind re-syncs automatically (§4.3), so meshes stay put.
- **Copy/paste:** helpers are copied only with their parent. A pasted mesh's binds
  are remapped to pasted helpers when those were copied too, and otherwise keep
  pointing at the originals.
- **Rename:** helpers follow their parent's name (`<bone>__bind`).

### 4.3 Editing the setup pose after binding: Blender-style (decided)

**The bind pose always equals the setup pose.** Editing a bound bone's setup
transform re-captures the bind, so the mesh **stays put**, as in Blender's edit
mode. Deformation only appears when animating.

How:
- A helper's local transform is derived data: `inverse(parent's world transform in
  the setup pose)` (§2.1). `v_rest` never changes when bones move.
- After **any** setup-pose change (move, rotate, scale, reparent, or an IK target
  moved in the setup pose), the editor recomputes **every** helper's local transform
  from the new constructed setup pose.
  - Recomputing all of them, not just the edited bone's, covers edits to ancestors,
    which move descendants and their helpers.
  - Cost: one transform per helper, only on setup edits.
- The world transforms come from `renderer::construction`, **including IK**. A
  helper under an IK-driven bone must match where that bone is actually drawn.
- Animation edits never touch helpers. SkelForm keys are absolute values, and
  helpers only follow their bone.
- Undo: the helper recompute is part of the same undo step as the edit that
  triggered it.

Consequence: moving a bone in the setup pose never deforms a bind-posed mesh. To
see deformation, pose the bone in an animation, like pose mode in 3D software.

---

## 5. Limits

- **Non-uniform scale.** Exact while each bound bone's world scale changes
  **uniformly** relative to its bind pose (`sx/sx₀ == sy/sy₀`). SkelForm can't
  represent skew, so a non-uniform squash of a rotated bone can't be reproduced
  exactly by its helper. This is the same limit SkelForm's bone hierarchy already
  has.
- **Mirroring.** Negative scales go through SkelForm's `facing_left` rule (§2.1),
  which needs explicit tests.
- **Extra bones.** Exactly one helper per bone that has bind-posed meshes, shared
  by all of them. Because the bind pose always equals the setup pose (§4.3), no
  bone ever needs a second helper.

---

## 6. Alternative considered: native bind transforms in the format

Store each bind's bone transform at bind time in `BoneBind` (for example
`bind_pos/bind_rot/bind_scale`), and apply `inverse(B)` in `construct_verts`.

| | helper bones (this doc) | native field |
|---|---|---|
| `.skf` format | unchanged | new optional fields, with a version gate |
| Runtimes | unchanged | every official runtime must implement it |
| Data cleanliness | extra (hidden) bones | clean |
| Editor work | helper hygiene (§4.2) | bind UI only |

Helpers are the pragmatic first step. If SkelForm ever adopts the native field,
converting helpers is mechanical: read each helper's transform as the bind
transform, drop the helpers, and keep `v_rest`.

---

## 7. Knock-on benefits

- **DragonBones / Spine import.** Their weighted meshes are standard linear blend
  skinning, so an importer builds the same helpers through this code path, and
  blended meshes import exactly (see `DRAGONBONES_IMPORT.md` §4.5).
- **Export to bind-pose formats.** An exporter that recognises flagged helpers can
  write real `bonePose`/bind matrices instead of emitting the helpers as bones.

---

## 8. Implementation sketch

| area | change |
|---|---|
| `src/bind_pose.rs` (new) | `set_bind_pose(armature, mesh_bone_id)`, `clear_bind_pose`, `sync_helpers(armature)` (§4.3), `helper_for(bone_id)`, helper local-transform math (§2.1), pivot baking (§2.2). Pure functions on `Armature`, unit-tested without GPU |
| `shared.rs` | `Bone.bind_helper` (runtime-only, `#[serde(skip)]`), `EditorBone.bind_helper`; new events `SetBindPose` / `ClearBindPose` |
| `utils.rs` | save/load `bind_helper` through `editor.json` (`prepare_files` / `import`) |
| `editor.rs` | event handlers (with undo); call `sync_helpers` after setup-pose bone edits and reparenting (§4.3); skip the `ClickVertex` compensation for bind-posed meshes; armature-space vertex tools (§4.1); delete/paste hooks (§4.2) |
| `armature_window.rs` | draw helpers greyed out and read-only; block rename, drag, selection and keyframing |
| `bone_panel.rs` | Set Bind Pose / Clear buttons in the mesh section; a bind-pose indicator; read-only view for helpers |
| `assets/i18n/en.json` | strings |

## 9. Tests

1. **No-jump:** world vertex positions are identical before and after Set Bind Pose,
   for meshes with 1–4 blended binds and arbitrary weights.
2. **Standard skinning:** pose the bones, and compare `construction()` output with
   an independent computation of `Σ wⱼ · Wⱼ · inverse(Bⱼ) · v_rest`. They should
   match to float precision for uniform scale, and fail predictably for the §5
   non-uniform case.
3. **Weight edits:** changing any weight at the bind pose moves nothing.
4. **Setup edits (Blender-style):** moving, rotating, scaling or reparenting a bound
   bone, or one of its ancestors, in the setup pose leaves every bind-posed vertex
   where it was.
5. **Round-trip:** save → load keeps the helpers, the `bind_helper` flags and the
   deformation.
6. **Runtime parity:** play a bind-posed rig in `rusty_skelform` (unchanged) and
   compare with the editor.
7. **Hygiene:** delete, paste, reparent and undo keep binds valid (no dangling
   helper ids).

## 10. Decisions

| # | question | decision |
|---|---|---|
| 1 | Setup-pose edits after binding | **Blender-style.** The bind pose tracks the setup pose automatically, so meshes stay put (§4.3) |
| 2 | Weight UI | **Keep SkelForm's per-bind weights** for now. Normalised 3D-style per-vertex weights can come later as a view on the same data |
| 3 | Helper visibility | **Greyed out** under their bone, read-only (§4.2) |
| 4 | Helper bones vs native format field | **Helper bones first** (no format change). The native field (§6) remains a possible later migration |

## 11. Work plan (this branch)

This branch carries three related pieces of work. Bind Pose comes first, and the
other two build on it.

| # | task | depends on | doc |
|---|---|---|---|
| 1 | **Bind Pose core:** `bind_pose.rs` (set/clear, `sync_helpers`, helper math, pivot baking), `editor.json` flag, tests §9.1–9.5 | — | this doc |
| 2 | **Bind Pose editor:** Set/Clear buttons, auto-sync after setup-pose edits, armature-space vertex tools, greyed-out helpers, delete/paste hooks, test §9.7 | 1 | this doc |
| 3 | **Exporter update:** bind-posed meshes export as native DragonBones weights with real `bonePose` data, and their helpers are not emitted as bones | 1 | `DRAGONBONES_EXPORT.md` §4a |
| 4 | **Importer:** parser, atlas, bones/slots/skins, animations, IK, warnings modal | — (cutout) | `DRAGONBONES_IMPORT.md` |
| 5 | **Importer meshes:** weighted meshes imported as bind-posed meshes | 1, 4 | `DRAGONBONES_IMPORT.md` §4.5 |
| 6 | **Round-trip verification:** SkelForm → DragonBones → SkelForm, and DragonBones samples, through `tools/dragonbones_verify` | 3, 5 | `DRAGONBONES_VERIFY.md` |

Upstream note: task 1–2 (Bind Pose) and 4–5 (importer) are candidates for PRs to
`Retropaint/SkelForm`. The exporter (3) stays in this fork (upstream declined it).
