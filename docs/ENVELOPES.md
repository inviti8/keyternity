# Envelope skinning and bone length

**Goal:** skin a mesh in one click with **Bind to Skeleton**, weighted
automatically from bone **envelopes** the way 3ds Max's Skin modifier does, then
refine it by hand with **weight painting** and a **weight table**. Bones gain a **length**, which envelopes need and which IK and
DragonBones can use too.

**Status:** steps 1–4 implemented (§0): bone length, the envelope maths, Bind to
Skeleton with live envelope weights, and the Envelope tool with the weight view.
Step 5 in part: normalized hand weights and weight painting (Add, Subtract,
Smooth). Painting's Set and Blur modes and the weight table are still design.
Decisions are in §9.


## 0. As built

**Step 1, bone length (§2):**
- `Bone.length`, saved when not 0. `renderer::tail_pos` gives a bone's tail:
  where a child at local `(length, 0)` would sit, so scale and flips are included.
- The canvas draws a blade from head to tail with the existing kite shape, in the
  bone's color. The tail handle (a small diamond) shows for the only selected bone,
  with the Transform tool, in the setup pose. Pressing it beats the transform ring
  and doesn't deselect. Dragging is one undo step. Ctrl+drag also aims the bone,
  unless IK owns its rotation.
- Length is a non-animatable `AnimElement::Length`, so it goes through `EditBone`
  and never records keyframes. It's clamped at 0.
- Bone panel: *Length* with *To child* (distance to the first child), disabled
  while animating.
- **Departure, PSD import:** a group bone with exactly one child group is also
  **aimed** at it, not just given the length. Otherwise the length would point
  along +x, away from a child below it. Its direct children turn back by the same
  angle, so no art moves. This is safe only at import, before any keyframes exist,
  so the editor's *To child* doesn't aim.
- DragonBones export writes `length`; import keeps it.
- Tests: `tests/bone_length.rs` (4), plus a PSD import test.

**Step 2, envelope maths (§3.2–3.3)**, in `src/envelope.rs`, without UI yet:
- `Envelope` (deform, four radii, falloff, absolute) with `for_length` defaults
  (§5), `influence` and `gap` (distance past the outer radius).
- **Radii are world px at rest**, not scaled by the bone, so an envelope looks the
  same size whatever the bone's scale.
- Falloff curves on `x` (0 at the outer radius, 1 at the inner): Linear `x`,
  Smooth `x²(3 − 2x)`, Fast out `x²`, Slow out `1 − (1 − x)²`.
- `vertex_weights`: influences, the strongest *N* (ties keep bone order),
  normalized, or the nearest bone by `gap` with the outside flag.
- `bind_weights` and `shares` convert between shares and ordered bind weights.
- Tests: `tests/envelope.rs` (10), including the conversion through
  `renderer::construct_verts`.

**Step 3, Bind to Skeleton (§3.4–3.6, §4.2–4.3, §5)**, in `src/skin.rs`:
- `bind_to_skeleton` (Bind Pose on, skeleton, weights, binds through the Bind Pose
  helpers, parents first), `add_bones_in_reach`, `maintain` (live recompute) and
  `lock_vertex`. Envelopes, the per-mesh switch, *Max influences* and locked
  vertices are editor-only fields on `Bone`, saved in `editor.json` (locked
  vertices by index, since vertex ids are re-indexed on save).
- **Live recompute** runs every frame after `bind_pose::maintain`, but only
  rebuilds a mesh when a hash of its inputs changes: its rest transform and
  vertices, locks, *Max influences*, and each skin bone's head, tail and
  envelope. Weights are a pure function of those inputs, so undo stays
  consistent without extra undo steps. Skipped in Pose Mode.
- **Departure:** live recompute keeps a bind even when its bone stops weighting
  any vertex, so shrinking an envelope and growing it back doesn't drop the bone.
  Running Bind to Skeleton again prunes empty binds. Envelope-weighted meshes
  therefore skip the *NoVertsInBind* and *NoWeights* warnings.
- **Departure, point-bone defaults:** a bone without length gets an inner and
  outer radius of 0.25 and 0.75 times the distance to the nearest other default
  skeleton bone (20 / 60 px if it has none), rather than radii from mesh size.
  This scales with the rig, which matters for large Photoshop art.
- Hand edits that lock a vertex: the bind panel's weight slider, and adding or
  removing a vertex from a bind.
- UI: the binds area has *Bind to Skeleton* (for every selected mesh; a modal
  lists failures, or how many vertices sat outside every envelope), and once
  skinned, *Envelope Weights*, *Max Influences*, *Add Bones in Reach*, the
  hand-weighted count with *Unlock Selected* / *Unlock All*, and the outside
  count with *Select*. *Clear All Binds* (shown once a mesh has binds, after a
  confirmation) removes every bind and weight, turns Bind Pose off and removes
  unused helpers, leaving the art as drawn, so skinning can start over. The bone panel has a collapsible *Envelope* section
  (Deform, head and tail radii, Falloff, Absolute, Copy / Paste). The skin
  controls are disabled in Pose Mode only, with a tooltip saying why: binding
  uses the rest pose, which animating leaves alone. (They were first also
  disabled while animating, which left Bind to Skeleton greyed out with no
  explanation after testing a mesh in an animation.)
- Tests: `tests/skin.rs` (9).

**Step 4, Envelope tool and weight view (§4.1, §4.4)**, in `renderer.rs`
(`envelope_tool`, `drag_envelope`, `capsule_outline`, `envelope_handles`):
- **Key: Shift+E**, not E, which is Scale (and W is Rotate, so weight painting
  becomes Shift+W). Toolbar button after the Eraser, with *Show All* and a
  "Shift: both ends" hint in the tool options.
- Capsules: the outline is the convex hull of a circle at each end. The outer
  shape is filled very faintly and outlined dashed, the inner shape filled a
  little stronger (not for Absolute envelopes), in the bone's color, or grey for a
  bone that doesn't deform. Shown for the selected bones and, for a selected
  mesh, the bones that skin it; *Show All* adds every deforming bone.
- Handles: diamonds at the selected bone's head and tail, inner radius on one
  side of the bone and outer on the other, so they never overlap. Dragging sets
  the radius to the distance from that end; Shift sets the other end too. One undo
  step per drag; only at rest. The tail (length) handle works in this tool too.
- The tool never moves bones: dragging the canvas does nothing, and the transform
  ring is hidden.
- Weight view (in this tool, with one bone selected): every mesh the bone skins
  shows the bone's share per vertex, blue (0) to red (1), with a white ring on
  hand-weighted vertices.
- **Bind labels** (added after review): with a mesh selected, each of its binds is
  labelled on its bone's envelope with the bind index and bone name, at the middle
  of the bone (above a point bone). Clicking a label selects that bind in the
  panel, so a stray vertex outside every envelope can be added to the right bind.
  The renderer computes the positions; egui draws them behind the panels.
- Tests: `tests/envelope_tool.rs` (5).

**Step 5, normalized hand weights and weight painting (§4.5)**, in `src/weights.rs`
(share maths) and `skin.rs` (`edit_shares`, `paint`, `bind_shares`):
- **Hand weights are shares** on bind-posed meshes. Before, the bind panel's
  sliders edited the raw blend weights, where a later bind overrides an earlier
  one: a vertex at 1 in two binds moved only with the later bone, and the earlier
  slider still read 1. The sliders (the bind's list and the selected vertex's
  weights) now show each vertex's share of the bind's bone and edit it with the
  other bones scaling to make up the rest. A vertex with no other bone gives the
  rest to the nearest other skin bone (by envelope), else its owner. Shares are
  converted back with §3.3's formula, so files and runtimes are unchanged. Classic
  (not bind-posed) meshes keep raw weights, since their vertex positions depend on
  the binds.
- Double-clicking a vertex with a bind selected gives it fully to that bind's bone,
  or, if it already has a share, removes it (the rest renormalizes). One undo step.
- **Fixed:** vertices added to a bind one after another disappeared. Each add
  locked the vertex, which re-ran the live recompute, which converts to shares; a
  vertex added at weight 1 to an earlier bind had a 0 share there if a later bind
  also held it, so it was dropped. Adds are now full shares, so they survive.
- A slider dragged to 0 keeps its row (the vertex stays in the bind at weight 0,
  through recomputes, while it's locked), so it can be dragged back up.
- Bind rows keep their order when weights change.
- **Paint Weights tool (Shift+W)**, toolbar button after Envelope. Paints the
  selected mesh with the bone of the selected bind, picked in the tool options
  (the mesh's weight binds; choosing the tool picks the first if none is). Modes
  **Add**, **Subtract**, **Smooth** (toward the neighbours' average along mesh
  edges); Shift smooths and Ctrl swaps Add and Subtract while held. Radius in
  screen px (`[` and `]` scale it by 1.2), strength 0–1; falloff
  `(1 − (d/r)²)²`. A dab every 4 px of movement or every 0.05 s held still, each
  moving a share by up to 0.15 × strength (Smooth: half way to the average at full
  strength). Vertices are hit where drawn, so it works in Pose Mode. Each stroke
  is one undo step; painted vertices are locked; *Max influences* is kept by
  dropping the weakest other bone; shares under 0.001 are pruned.
- While painting, the mesh shows the painted bone's shares (blue to red, white
  rings on hand-weighted vertices) and the brush ring is colored by mode. The tool
  doesn't select or move bones.
- Not done: Set and Blur modes, painting a bone that has no bind yet (add the bind
  first), Alt-click to pick the bone on the canvas, the weight table (§4.6).
- Tests: `tests/weights.rs` (8).


## 1. Today

- A mesh is skinned by **binds**: each bind names a bone and lists vertices with a
  0–1 weight (`BoneBind`, `BoneBindVert`). Every bone, and every vertex weight in
  each bind, is set by hand.
- Weights blend **in bind order**. A vertex starts on its owner (the mesh bone),
  and each bind pulls it toward where that bind's bone would put it, by the
  weight (`renderer::construct_verts`):

  ```
  pos = owner(v)
  for each bind k:  pos += (bone_k(v) − pos) · w_k
  ```

  So bind k's effective share is `w_k · Π(1 − w_m)` over the later binds m, and
  the owner keeps `Π(1 − w_m)` over all of them.
- **Bind Pose** (`BIND_POSE.md`) makes this standard linear blend skinning: every
  bind agrees with the owner at rest, so weights can change without vertices
  jumping. Classic (non-bind-posed) meshes drift when a vertex is in more than one
  bind, and a warning says so.
- Bones are points: a position, rotation and scale, with no length. The canvas
  draws a kite from each parent to each child. DragonBones import reads bone
  `length` (its IK solver needs it) but doesn't keep it, and export writes none.


## 2. Bone length

### 2.1 Data
`Bone.length: f32`, in px along the bone's local +x, saved in `armature.json`
(omitted when 0). 0 means a point bone, as today. The **tail** is the point
`length` along the bone's axis. Length is part of the setup pose and not
animatable (as in DragonBones and Spine).

### 2.2 Display and editing
- A bone with length draws as a tapered blade from its head to its tail, in the
  bone color. The parent-to-child kites stay.
- **Tail handle:** with the Transform tool, the selected bone shows a handle at
  its tail. Dragging it changes the length only, projected onto the bone's axis,
  so the bone and its art don't turn. Ctrl+drag also aims the bone, which turns
  its art, as rotating it does today.
- **Bone panel:** a Length input under Rotation, and *Length to child*, which
  sets the length to the distance to the bone's single child (or its first child).
- **New bones** start at length 0. *Length to child* is quick to apply.
- **PSD import:** a group bone with exactly one child group gets the length to
  it, and is aimed at it (§0). Others stay at 0.

### 2.3 Other uses
- **DragonBones:** export writes `length`, and import keeps it.
- **IK:** a later *Use Tail* option could make the tip's tail reach the target, as
  in Blender and DragonBones. Then the tip bone would turn too, and DragonBones IK
  could map onto constraints directly (`DRAGONBONES_IMPORT.md` §4.6).


## 3. Envelopes

### 3.1 Shape
Every bone has an envelope, used only when the bone is set to **Deform**:
- a segment from the head to the tail (a circle at the head for length 0)
- an **inner** and an **outer** radius at the head and at the tail. Along the
  segment, each radius blends linearly from its head value to its tail value.

In 2D this is a flat capsule. Max also allows extra cross-sections along a bone;
two ends cover 2D cut-out limbs, and cross-sections can come later.

### 3.2 Influence
For a vertex at rest position `p` (the setup pose, including IK at setup):

```
t     = projection of p onto the segment, clamped to 0..1
d     = distance from p to the segment's point at t
r_in  = lerp(head_inner, tail_inner, t)
r_out = lerp(head_outer, tail_outer, t)

influence = 1                                   if d ≤ r_in
          = falloff((r_out − d) / (r_out − r_in)) if r_in < d < r_out
          = 0                                   if d ≥ r_out
```

**Falloff** curves, as in Max: *Linear*, *Smooth* (smoothstep, the default),
*Fast out* and *Slow out*. The curve is set per bone.

**Absolute** (per bone, off by default), as in Max: influence is 1 anywhere inside
the outer radius, and the inner radius is ignored. It suits rigid pieces, such as
a hand or a sword, that should follow one bone completely.

### 3.3 Weights
For each vertex of a mesh:
1. **Candidates:** the mesh's **skin bones** (§3.4) that deform.
2. **Influences** from §3.2.
3. **Limit:** keep the strongest *N* (*Max influences*, default 4, which is what
   game runtimes typically support).
4. **Normalize** to sum to 1.
5. **Outside every envelope:** the vertex goes fully to the nearest candidate,
   measured to its outer radius, and is listed so it can be fixed (§4.3).

Then convert to bind weights. With the owner's share `e₀` and the binds' shares
`e₁ … eₙ` in bind order, the blend in §1 gives each bind exactly `e_k` with:

```
w_k = e_k / (e₀ + e₁ + … + e_k)      (0 when the sum is 0)
```

This is exact, so the stored weights stay the ones the renderer, every runtime
and the DragonBones exporter already read. Nothing downstream changes.

### 3.4 Bind to Skeleton
Binding is one action, as with Blender's *With Automatic Weights* or adding
bones to Max's Skin modifier. **Bind to Skeleton**, in the mesh's binds area,
does all of it for every selected mesh, in one undo step:

1. Turns on Bind Pose for the mesh (§3.5).
2. Finds the **skeleton**: every bone set to Deform (§5), apart from the mesh's
   own descendants.
3. Computes every vertex's envelope weights (§3.3) against the skeleton.
4. Creates a bind for each bone that ends up with weight on at least one vertex,
   parents first, and stores the weights in them. Bones out of reach get no bind.
5. Turns on envelope weights (§3.6), so editing envelopes later keeps updating
   the mesh.
6. Reports vertices that sat outside every envelope and went to their nearest
   bone (§3.3 step 5), with *Select* to find them.

Nothing moves at rest, because Bind Pose binds agree with the owner there. The
mesh deforms as soon as a skeleton bone moves.

**Running it again** (after adding bones, or changing lengths or envelopes)
recomputes the unlocked vertices, adds binds for bones now in reach, and removes
binds that no longer weight anything. Hand-edited (locked) vertices keep their
weights. *Unlock all* first starts over.

**Skin bones:** afterwards, a mesh's binds are its skin bones, as in Max's bone
list, and envelope weights are spread over them (plus the owner, if it deforms).
For refinement:
- *Add bone*: the existing bind-bone picker
- *Add bones in reach*: every deforming bone whose outer envelope touches the mesh
- removing a bind: drops that bone, and its weight goes to the remaining bones

Path binds are left out: they aren't weights. Bind to Skeleton refuses a mesh
with path binds, and says so.

### 3.5 Requires Bind Pose
Envelope weights blend several bones per vertex, which only skins correctly with
Bind Pose (§1). Turning on envelope weights for a mesh turns on Bind Pose for it,
in the same undo step.

### 3.6 Live, with manual overrides
- Envelope weights are **live**: changing an envelope, a bone's length, the setup
  pose or a mesh's skin bones recomputes the weights of every mesh that uses
  those bones. It runs on setup edits only, not while animating.
- **Manual weights win**, as with Max's *modified* vertices. Editing a vertex's
  weight by hand (painting §4.5, the weight table §4.6, or the bind panel slider)
  **locks** that vertex. Recomputing leaves locked vertices alone.
- *Unlock* on selected vertices returns them to their envelope weights. *Unlock
  all* resets the mesh.
- Turning envelope weights off for a mesh keeps its current weights, as ordinary
  hand weights.


## 4. Editing

### 4.1 Envelope tool
A canvas tool, **Envelope** (Shift+E), for the selected bones:
- draws each deforming bone's capsule: the inner shape filled faintly, the outer
  shape as a dashed line, in the bone's color
- handles at the head and the tail for the inner and outer radii. Dragging a
  radius handle resizes it; Shift resizes both ends together
- the tail handle (§2.2) works here too
- *Show all envelopes* in the tool options shows every deforming bone's capsule,
  for checking overlaps at joints

### 4.2 Bone panel: *Envelope* section
Deform (on/off), Inner and Outer radius at Head and Tail, Falloff, Absolute,
and *Copy* / *Paste* envelope (to give a left limb's settings to the right one).

### 4.3 Mesh panel: binds area
The existing binds area becomes the mesh's skinning controls:
- **Bind to Skeleton** (§3.4), first, as the main action
- **Envelope weights** (on/off, §3.6), **Max influences**
- skin bones list (the binds, as today), *Add bone*, *Add bones in reach*
- *Unlock selected* / *Unlock all*, with the count of locked vertices
- vertices outside every envelope (§3.3 step 5): a count, and *Select* to find
  them

### 4.4 Seeing weights
The bind panel's colors (`MESH_DISPLAY.md` §3) already show a selected bind's
weights per vertex. Envelope editing adds a **weight view**: with a bone
selected, its mesh vertices are colored by their effective share, blue (0) to red
(1), as in Max. Locked vertices get a small outline, so hand-edited ones are easy
to spot.


### 4.5 Weight painting
A canvas tool, **Paint Weights** (Shift+W), on the selected mesh, painting the
**active bone**: the skin bone selected in the Skin list (§4.3), or clicked on the
canvas with Alt.

**Brush:**
- **Radius** in screen px (`[` and `]`), **strength** 0–1, and a soft falloff
  toward the brush edge. A circle shows the brush, with the radius scaled by the
  UI scale (as edge widths are).
- Vertices are hit where they're drawn, so painting works in the setup pose and
  in Pose mode, where the mesh deforms live as you paint. Painting is off while
  animating.
- Each stroke is one undo step (as with the Eraser), and painted vertices are
  locked (§3.6).

**Modes** (in the tool options):

| Mode | Effect on the active bone's share of each vertex under the brush |
|---|---|
| Add (default) | `+ strength · falloff` |
| Subtract (Ctrl inverts Add) | `− strength · falloff` |
| Set | moves toward the **value** slider by `strength · falloff` |
| Smooth (hold Shift) | moves toward the average of its edge neighbours (`topology::edges`) |
| Blur | like Smooth, but for **every** bone's share, so it evens out a seam between bones |

**Keeping weights valid:** painting edits **effective shares** (§3.3) and keeps
each vertex's shares summing to 1:
- When the active bone's share changes, the other bones' shares scale to make
  up the difference, keeping their ratios, as in Max.
- If a vertex has no other share to take from, subtracting gives the remainder
  to the owner (or the nearest other skin bone when the owner doesn't deform).
- Painting a bone onto a vertex that is already at *Max influences* drops its
  weakest share.
- Shares below 0.01 after a stroke are pruned and the rest renormalized, so
  binds don't fill with near-zero entries.

Shares convert back to bind weights with §3.3's formula. Painting a bone that
isn't a skin bone yet adds it as a bind (with Bind Pose, §3.5).

**Display:** while painting, the weight view (§4.4) shows the active bone's
shares, and the brush circle stays visible.

### 4.6 Weight table
For exact values, the mesh panel's **Vertex Weights** list shows the selected
vertices' shares for every skin bone, one row per bone, as in Max's weight
table:
- Each row has a 0–1 slider. With several vertices selected, it shows the mean,
  and editing it sets all of them.
- Editing renormalizes the other bones as painting does, and locks the vertices.
- *Normalize* fixes any vertex whose shares don't sum to 1 (hand-edited classic
  binds can drift). *Prune* drops shares under a threshold.
- The existing per-bind view (one bind, its vertices and weights) stays. It's
  the transposed view of the same data.


## 5. Defaults
**Deform** starts on for skeleton bones and off for the rest:
- **on:** bones without a texture (joints and groups, such as Photoshop group bones)
- **off:** textured bones (the art being skinned), IK targets, and Bind Pose
  helpers

So Bind to Skeleton works on a fresh rig without any setup, including rigs of
point bones. Deform can be changed per bone in the Envelope section.

A bone's envelope starts with:
- **With length L:** inner radius `0.2·L`, outer radius `0.5·L`, at both ends.
  Neighbouring limb bones then overlap around the joint between them.
- **Point bone:** inner and outer radii of 0.25 and 0.75 times the distance to the
  nearest other skeleton bone, or 20 / 60 px without any.
- Bones with lengths give better weights than point bones, because a segment
  follows a limb and a circle doesn't. *To child* (§2.2) and Photoshop import
  set them.


## 6. Data

| What | Where | Runtime-visible? |
|---|---|---|
| `length` | `armature.json` bone | yes, harmless (DragonBones uses it) |
| Envelope: deform flag, 4 radii, falloff | `editor.json` per bone | no. Weights are baked into the binds |
| Envelope weights on/off, max influences | `editor.json` per mesh | no |
| Locked vertices | `editor.json` per mesh (vertex ids) | no |

The skinning result stays plain binds and weights. Files play wherever they
did before, and the DragonBones export needs nothing new.


## 7. Tests
- **Influence:** inside inner = 1, outside outer = 0, each falloff at its
  midpoint, tapered radii along the segment, length 0 as a circle.
- **Conversion:** effective shares → bind weights → `construct_verts` shares,
  exact for random shares and bind orders, including a 0 owner share.
- **Weights:** max influences keeps the strongest and renormalizes; vertices
  outside every envelope go to the nearest bone and are reported.
- **Absolute:** influence 1 everywhere inside the outer radius.
- **Locks:** recompute skips locked vertices; unlock restores envelope weights.
- **Painting:** add, subtract, set, smooth and blur change the active bone's share
  by the brush falloff; shares always sum to 1, keep their ratios, respect max
  influences, and prune below 0.01; a stroke is one undo step and locks what it
  touched; painting a new bone adds its bind.
- **Shares ↔ bind weights:** converting both ways is exact.
- **Weight table:** editing a row renormalizes and locks; *Normalize* and *Prune*.
- **Live:** moving an envelope handle, changing a length, or reparenting updates
  only unlocked weights, in one undo step.
- **Bind to Skeleton:** on a two-bone arm mesh it turns on Bind Pose, makes one
  bind per bone in reach (none for bones out of reach or the mesh's descendants),
  weights every vertex to sum 1, moves nothing at rest, and is one undo step.
  Running it again keeps locked vertices and adds or removes binds as bones come
  into or go out of reach. A mesh with path binds is refused.
- **End to end:** that arm mesh bends smoothly at the elbow under a 90° turn;
  DragonBones export round-trips it within 0.01 px.
- **Length:** saved and loaded; *Length to child*; the tail drag doesn't turn the
  bone; DragonBones export writes it and import keeps it.


## 8. Work plan
1. **Bone length** (§2): data, panel, tail handle, display, PSD import, DragonBones
   in and out. Shippable alone.
2. **Envelope maths** (§3.2–3.3): influence, limit, normalize, conversion, plus
   tests. No UI.
3. **Bind to Skeleton** (§3.4–3.6, §4.2–4.3, §5): per-bone envelopes with their
   defaults, the button and binds area, live recompute, locks. Usable on its own:
   one-click skinning with default envelopes.
4. **Envelope tool and weight view** (§4.1, §4.4).
5. **Weight painting and the weight table** (§4.5–4.6).
6. **Docs:** README and an as-built section here.


## 9. Decisions
| # | Decision |
|---|---|
| E1 | Bones get a `length` (setup pose only, not animatable). Dragging the tail changes length without turning the bone. |
| E2 | Envelopes follow Max: segment capsules with inner/outer radii at both ends and a falloff curve. Max's extra cross-sections are left for later. |
| E3 | Envelopes are **per bone**, shared by every mesh, like Blender's bone envelopes. (Max's are per Skin modifier, so per mesh.) A per-mesh scale on top can come later if one piece needs to reach further. |
| E4 | **Bind to Skeleton** skins a mesh in one click: Bind Pose on, a bind per deforming bone in reach, envelope weights. Afterwards a mesh's binds are its skin bones. |
| E5 | Weights are computed live in the setup pose, stored as ordinary bind weights; hand-edited vertices are locked. |
| E6 | Envelope weights require, and turn on, Bind Pose. |
| E7 | Max influences defaults to 4. Vertices outside every envelope go to the nearest bone and are flagged. |
| E8 | *Absolute* is a per-bone envelope option. |
| E9 | Weight painting (add, subtract, set, smooth, blur) and a weight table are part of this feature. Both edit normalized shares and lock what they touch. |
| E10 | Deform defaults on for untextured bones (the skeleton) and off for textured ones, IK targets and helpers, so binding needs no setup. |
