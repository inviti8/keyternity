# Shapes, dialog and auto lip sync

Status: **research and scope.** Nothing is built. Decisions to confirm are in §11.

## 1. Goal

Get a talking character from a line of dialog in minutes, then animate expression on
top of it by hand. It works the way Facebot did in MotionBuilder:
1. An audio file goes to a phoneme analyser, which gives timed, weighted phonemes.
2. The phonemes map onto a fixed set of Preston Blair visemes.
3. The visemes become keys on the timeline, driving a face that follows a known
   convention.
4. The artist then keys brows, eyes and smiles over it.

For Keyternity this breaks down into building blocks, each useful on its own:

0. **Animation layers:** keys in stacked layers (override or additive) that combine,
   so generated lip sync and hand animation never fight over the same keys. They're
   designed with the timeline pass in [TIMELINE.md](TIMELINE.md).
1. **Shapes:** morph targets for 2D meshes, with keyable weights. They're useful for
   any facial or squash-and-stretch animation, not just speech.
2. **Dialog audio** in animations: import a clip, see its waveform, hear it in sync.
3. **A standard face, the face rig and visemes:**
   - FaceBot's shape convention (§6.4);
   - a 2D face rig of draggable controls after FaceBot's (§6.5);
   - a per-character mapping from each viseme to rig values, shapes or a mouth
     texture.
4. **Auto lip sync:** audio (and optional text) gives a viseme track, which gives
   keys. Regenerating it doesn't touch the artist's own keys.
5. **Voice generation:** type a line, pick a voice, get the audio and its timing.
6. **Agent tools,** so the MCP agent can do all of the above.

## 2. How 2D lip sync is done, and where Keyternity stands

| approach | how | Keyternity today |
|---|---|---|
| **Mouth swaps** (Moho switch layers, Spine attachments, the classic "mouth chart") | One drawing per viseme, swapped on the beat | **Possible now:** Texture keys swap a bone's texture. No lip sync drives them yet. |
| **Mesh morphs / blend shapes** (Live2D parameters, Spine mesh deform keys, 3D blend shapes) | Sculpted vertex offsets per shape, blended by weight. Shapes add up: "O" + "smile". | **Missing:** meshes have no per-vertex animation. Topology, binds and weights exist. |
| **Bone-driven mouths** | Jaw and lip-corner bones posed per viseme | Possible with keys, but there's no pose library. |

Facebot's power came from the morph approach: visemes and expressions are separate
shapes that add together. That's why lip sync can be generated and then left alone
while the artist animates on top. So Shapes come first (as you suggested). Mouth swaps
are supported too, because many 2D artists draw mouths, and they work with the
existing Texture keys.

**Delivering it to games:**
- Frame sequences and spritesheets render shapes as-is.
- DragonBones has per-vertex deform (FFD) timelines for meshes, so shape weights can
  be baked to deform keys on export.
- Native `.skf` runtimes would need the new data (see §4.5).

## 3. Animation layers

Layers are designed with the timeline pass in [TIMELINE.md](TIMELINE.md) §5, and built
before Shapes. What lip sync needs from them:
- A generated **Lip Sync** layer that *Regenerate* replaces without touching any other
  layer (TIMELINE.md §5.4).
- **Additive** shape weights, so expressions on other layers add to the visemes.
- **Override** for drawn mouths.

## 4. Shapes (morph targets)

### 4.1 Data

A mesh bone gets:

```rust
pub struct MeshShape {
    pub id: i32,
    pub name: String,            // "AI", "smile", "blink_L"
    pub deltas: Vec<Vec2>,       // per vertex, in the mesh's own (rest / bind) space
    pub weight: f32,             // setup-pose weight, usually 0
    pub group: String,           // optional: "mouth", "brows"; for the UI and for exclusivity (§6.3)
}
```

- **Saving:** shapes are stored by vertex **index**, like `locked_verts`, because
  vertex ids are re-numbered on save.
- **Mesh edits keep them valid:**
  - Pen/Eraser and Retriangulate remap deltas: new vertices get the delta
    interpolated from their neighbours, and removed ones are dropped.
  - Trace and Reset replace the mesh, so they ask first, as they do for binds.

### 4.2 Evaluation

```
v = rest + Σ weightᵢ · deltaᵢ      (in the mesh's own space)
then binds / envelope skinning / bind pose as today
```

Shapes apply **before** skinning, as blend shapes do in 3D. That way a viseme sculpted
at rest still follows the head bone when it turns. Bind-posed meshes take the deltas
in bind space.

### 4.3 Animating weights

- A new `AnimElement::ShapeWeight`. A keyframe carries which shape in a new
  `Keyframe.shape` field (serde default -1). It's interpolated per (bone, shape) with
  the existing curves and easing.
- The timeline shows one row per shape under its mesh.
- Weights are normally 0–1, but over-driving (up to about 1.5) is allowed for
  exaggeration.

### 4.4 Editing

- A **Shapes** section in the mesh panel lists each shape with a weight slider and a
  key button. It has *Add Shape* (an empty one, or "from the current pose"),
  Duplicate, Mirror (L↔R across the mesh's local X) and Delete.
- **Sculpt mode** (an Edit button on a shape): the existing vertex move, rotate and
  scale and the soft-select brush edit that shape's deltas. All other shapes are
  shown at 0 while it's being sculpted. A "Show others" toggle sculpts on top of the
  current mix.
- **Shape from pose:** pose the bones (e.g. jaw down), then *Capture as shape*. It
  takes the current skinned vertex positions back into rest-space deltas, so a
  bone-built mouth can become a reusable shape.

**As built (phase 1):**
- **Storage and channels:**
  - Shapes live on the mesh bone (`Bone::shapes`), with offsets by vertex id, saved by
    index in the mesh's visuals.
  - Weights key as `ShapeWeight`, and a new `Keyframe::shape` (the shape's id, from 1)
    gives each shape its own channel through layers, loops, flattening and retiming.
- **Applying:** offsets apply in `renderer::construct_verts`, before binds, so every
  posed-vertex path (view, onions, exports, camera fit, DragonBones sampling) shows
  them.
- **The bone panel's Shapes section:**
  - weight sliders that key while animating (one undo step per drag), with a ◆/◇ key
    button;
  - rename, Duplicate, *Mirror to Other Side* (flipped across x = 0, with `_L`↔`_R`
    names), Delete (with the shape's keys), *+ Add Shape* and *Capture Pose*.
- **Sculpt:**
  - Sculpting shows the shape alone at full weight and turns on vertex editing.
  - Vertex drags, of one vertex or a selection, then edit its offsets. A **soft
    radius** carries nearby unselected vertices along with a smooth falloff.
- **Mesh edits:** Pen and New Vertex interpolate offsets, removals drop them, and
  Trace and Reset prune them.
- **Not yet:** the "Show others" sculpt toggle, shape groups and exclusivity (§6.3).

### 4.5 Export

| target | shapes |
|---|---|
| `.skf` | New fields (`shapes` on the mesh, `ShapeWeight` keys). The runtime spec (`skf_readme.md`) gets a section. SkelForm's runtimes ignore them, so meshes stay at rest there. |
| DragonBones | Each key of any shape weight becomes a `ffd`/deform key: offsets = Σ w·delta, rotated and flipped into the mesh's DB space. Between keys, the weights' curves are sampled at a set rate, because DB tweens offsets linearly. |
| Frame sequence, spritesheet, video | Rendered directly. |

## 5. Dialog audio in animations

- **Clips:** `AudioClip { id, name, file, start_frame, gain, muted }` on an
  animation. The audio file is **embedded** in the `.skf` zip (`audio/<name>.wav`),
  so rigs stay portable.
- **Decoding:** Symphonia (pure Rust; WAV, MP3, OGG, FLAC). rodio has used it as its
  default decoder since 0.21. It's MPL-2.0, which is compatible with Keyternity's
  GPL-3.0.
- **Playback:** rodio/cpal on desktop. On the web, Web Audio through wasm-bindgen,
  later.
- **Timeline:** the clip's waveform is drawn under the timeline rows. Playback stays
  in sync with the animation clock. Scrubbing plays a short slice (audio scrub), which
  is essential for checking sync by ear.
- **Video export** muxes the clip into the MP4. FFmpeg is already there.
- **Exports:** the manifest/Phaser/Godot files can reference the clip with its start
  time, so a game can play the line in sync.

## 6. Visemes, mouth rigs and the face rig

### 6.1 Viseme sets

**Internally**, lip sync speaks **Oculus's 15 visemes**, the set TalkingHead's text
rules produce (§7.3). It's also the standard for ARKit-style and VRM avatars.
Keyternity adds two to it:
- **L**, which Oculus folds into `nn`;
- **W**, which Oculus folds into `U`.

The ported rules emit those two separately, because Preston Blair draws them
differently. That gives **17 sounds**.

Each **mouth rig chooses the set it draws**, and the 17 sounds map onto it.
**Preston Blair (10)** is the default, as in Facebot:

| sound (internal) | Preston Blair | Simple 5 |
|---|---|---|
| `sil` | rest | closed |
| `PP` (p, b, m) | MBP | closed |
| `FF` (f, v) | FV | mid |
| `TH` (th) | etc | mid |
| `DD` (t, d) | etc | mid |
| `kk` (k, g) | etc | mid |
| `CH` (ch, j, sh) | etc | mid |
| `SS` (s, z) | etc | mid |
| `nn` (n, ng) | etc | mid |
| **`L`** (l) | **L** | mid |
| `RR` (r) | etc | mid |
| `aa` (a, ah) | AI | open |
| `E` (e, eh) | E | wide |
| `I` (i, ee) | E | wide |
| `O` (o, aw) | O | round |
| `U` (oo, u) | U | round |
| **`W`** (w, wh) | **WQ** | round |

The sets offered are:
- **Preston Blair 10** (default);
- **Oculus 15**, for mouths drawn or sculpted to that standard (`L`→`nn`, `W`→`U`);
- **Simple 5** (closed, mid, open, wide, round), for simple or chibi styles.

A rig can also override any row, e.g. send `TH` to L if that character's "th" shows
the tongue.

### 6.2 Mouth rig

A per-character mapping from each viseme to a **mouth pose**:

```rust
pub struct MouthRig {
    pub id: i32,
    pub name: String,               // "Alice"
    pub set: VisemeSet,             // PrestonBlair10 (default), Oculus15, Simple5
    pub overrides: Vec<(String, String)>, // (sound, viseme): per-character tweaks to §6.1
    pub poses: Vec<MouthPose>,      // one per viseme in the set
}
pub struct MouthPose {
    pub viseme: String,
    pub controls: Vec<(String, Vec2, f32)>,  // (face-rig control, offset, twist): rig-built (§6.5, default)
    pub shapes: Vec<(i32, i32, f32)>,        // (mesh bone id, shape id, weight): viseme shapes
    pub texture: Option<(i32, String)>,      // (bone id, texture name): drawn mouths
}
```

- A pose can mix both kinds: a jaw shape plus a teeth texture.
- A **Mouth Rig panel** has a row per viseme with a mouth preview. *Auto-fill* matches
  shapes or textures by name (a shape called "MBP", or Oculus's "viseme_PP", fills
  MBP), which is what made Facebot's naming convention work. *Test* plays a phrase through the rig.
- A rig file can hold several mouth rigs, one per character.

### 6.3 Combining with expressions

- **Morph mouths:** viseme shapes and expression shapes (smile, frown, sneer) add up.
  The lip sync keys stay untouched on their own layer (TIMELINE.md §5.4) while the artist keys
  the expression on another.
- **Exclusive groups:** shapes in the same group as the mouth rig's visemes are
  normalised, so a viseme crossfade never stacks two visemes past full. Expression
  shapes in other groups stay additive.
- **Drawn mouths** can't add up. For those, an expression swaps the whole mouth set,
  e.g. a "smiling" variant of each viseme texture via a Style. Styles are already
  hot-swappable.

### 6.4 Standard face shapes (FaceBot's convention)

Facebot used the **Mixamo blend-shape set**, which became its convention, with an
optional set of Preston Blair viseme shapes added. Keyternity adopts the same
structure as its standard face. A mesh whose shapes follow it gets a working face
rig, viseme poses and lip sync with no setup. It's a convention, not a requirement:
other names are mapped (§6.6).

**Core set:** expression and articulation shapes, paired Left and Right where
they're sided.

| region | shapes |
|---|---|
| Eyes | `Blink_L`, `Blink_R` |
| Brows | `BrowsUp_L/R`, `BrowsDown_L/R`, `BrowsIn_L/R`, `BrowsOuterLower_L/R` |
| Nose | `NoseScrunch_L/R` |
| Mouth | `MouthOpen`, `MouthUp`, `MouthDown`, `MidMouth_L/R` (shift sideways), `MouthNarrow_L/R`, `MouthWhistle_L/R` |
| Corners | `Smile_L/R`, `Frown_L/R` |
| Upper lip | `UpperLipUp_L/R`, `UpperLipIn`, `UpperLipOut` |
| Lower lip | `LowerLipDown_L/R`, `LowerLipIn`, `LowerLipOut` |
| Jaw | `JawSide_L/R` (Mixamo's JawRotateZ), `JawTwist_L/R` (JawRotateY), `JawForward`, `JawBackward` |
| Tongue | `TongueUp` |

**Viseme shapes (optional):** `AI`, `E`, `O`, `U`, `etc`, `WQ`, `MBP`, `L`, `FV`,
`rest`. These are Preston Blair's ten, sculpted or drawn directly (§6.5 explains when
to use them).

- **Names:** Keyternity's canonical names are shorter (`_L`/`_R`, and a jaw "side"
  and "twist" for a 2D view). Mixamo's own spellings, and FaceBot's variants, are
  accepted as **aliases**: `Blink_Left`, `JawRotateZ_Left`, `JawForeward`,
  `Jaw_RotateY_Left`, `MouthWhistle_NarrowAdjust_Left`, and so on. Auto-fill and the
  rig match both, case-insensitively.
- **Creating the set:** *Generate Face* (§6.7) builds the set from a landmark map.
  *Add Standard Face Shapes* instead creates them empty, named and grouped (eyes,
  brows, mouth…), for sculpting by hand. Mirror (§4.4) makes each `_R` from its
  `_L`.

### 6.5 The face rig (2D, after FaceBot)

FaceBot's rig was a mask with **13 manipulators** sitting in front of the
character's face. Dragging one drove its shapes, so a face could be posed by feel
rather than by hunting through sliders. Keyternity's face rig keeps that structure,
in 2D.

**Controls are helper bones.** The rig is a small group of hidden helper bones, the
controls, plus a **driver table** that turns their offsets into shape weights each
frame. Because controls are bones:
- they're keyed, layered, looped, copied, scaled in time and given generators like
  any bone, with nothing new for the timeline;
- the shape weights are *derived*, so there's no bake-to-rig or bake-to-face round
  trip (§6.6);
- exports leave the controls out, as they already do for Bind Pose helpers. They bake
  the driven weights into shape weights, or into DragonBones FFD keys (§4.5).

**Layout.** The default layout is generated once from `FaceBot.FBX`, using its zero
groups' rest positions and its hierarchy, and ships as built-in data. The FBX isn't
needed at runtime. Units are the mask's, x across and y up:

| control | parent | rest (x, y) | drives (default mapping) |
|---|---|---|---|
| `Brow_L` / `Brow_R` | face | (±6.6, 7.6) | +y BrowsUp, −y BrowsDown, x inward BrowsIn, twist BrowsOuterLower |
| `Eye_L` / `Eye_R` | face | (±6.4, 4.6) | −y Blink. Look direction moves the eye bone or pupil (below). |
| `Nose_L` / `Nose_R` | face | (±2.0, −2.8) | +y NoseScrunch |
| `Mouth` | face | (0, −8.6) | +y MouthUp, −y MouthDown, ±x MidMouth_L/R; carries the four below |
| `Corner_L` / `Corner_R` | Mouth | (±5.8, −0.3) | +y Smile, −y Frown, x inward MouthNarrow, x outward MouthWhistle |
| `UpperLip` | Mouth | (0, 1.3) | +y UpperLipUp_L+R, twist UpperLipIn/Out (a lip roll) |
| `Jaw` | Mouth | (0, 5.2)* | −y MouthOpen, ±x JawSide, twist JawTwist, +y JawForward |
| `LowerLip` | Jaw | (0, −6.4) | −y LowerLipDown_L+R, twist LowerLipIn/Out |
| `Tongue` | face | (0, −8.6) | +y TongueUp |

\* FaceBot's jaw pivots behind the mouth: its zero sits 5.2 above it and 6.2 back.
The 2D rig places the handle below the mouth instead, and keeps the pivot for jaw
twist.

- **How values work:**
  - Each control moves within a box, ±1 by default and set per control, shown as its
    track.
  - An axis's positive half drives one shape and its negative half another
    (`weight = max(±v, 0)`). Twist is the handle's rotation, ±45° for full.
  - Each slot can drive several shapes with weights, for example `UpperLip` +y →
    UpperLipUp_L 1.0 and UpperLipUp_R 1.0.
  - Slots without a matching shape are hidden, so a simpler face gets a simpler rig.
- **Parenting:** moving `Mouth` carries the corners, lips and jaw, and moving `Jaw`
  carries the lower lip, as in FaceBot. Their own offsets stay local.
- **Showing it:** the rig draws as an overlay panel, a stylised mask with its
  handles, like FaceBot's `vizFace`. It docks beside the canvas, or floats near the
  character's head. Handles show their track and current value; double-click one to
  zero it.
- **Editing the mapping:** a *Face Rig* section lists each control's slots and the
  shapes they drive, with weights. Any slot can be re-pointed, including to texture
  swaps for drawn faces, and the result saves with the character.
- **Eyes:**
  - Blinks are shapes, driven by `Eye_L/R`.
  - Where the face has eye bones, or pupils as their own bones, an **Eye Look**
    control moves them within limits, as FaceBot's look target did.
  - FaceBot keyed random blinks and saccades from buttons. Here they become
    generators on a layer (TIMELINE.md §5.8, the planned *Random event* generator).
    FaceBot's timings are the defaults: a blink closes in 80 ms, holds 60–80 ms, and
    is open again by 180–250 ms. A saccade darts in 10–40 ms, holds 40–220 ms, and
    alternates axes.

**Visemes on the rig.** A mouth pose (§6.2) can be either kind:
- **Built from rig values** (the default): AI is "Jaw −0.6, Corner_L/R +0.2,
  UpperLip +0.1". Lip sync then keys the controls, and the artist adjusts any viseme
  with the same handles afterwards. This is what FaceBot recommended.
- **A viseme shape** (`AI`…`rest`), when the face has them. The result is crisper,
  and good for cartoon mouths, but the rig can't adjust it beyond the shape's weight,
  as FaceBot noted.

A rig can mix the two. Auto-fill picks viseme shapes when they exist; otherwise it
uses the built-in rig-value poses for the standard set.

### 6.6 What changes from FaceBot

The structure comes from FaceBot. The mechanics are reworked where the original was
fragile:

| FaceBot | Keyternity | why |
|---|---|---|
| Remapped other shape sets by a "config model": frame *n* held the equivalent shape at 100% | A **mapping table**: each standard shape lists the mesh shape it uses, auto-matched by name and alias, then picked by hand | No round trip through another tool. A mistake in one keyed frame no longer shifts every mapping after it. |
| Phoneme configs keyed as frames 0–9 of a scene | Viseme poses edited on the rig and saved with the mouth rig (§6.2) | Same idea, visible and editable in place |
| Bake to rig, edit, bake back to face, delete rig (a modal, destructive round trip) | Controls are the keyed channels, and shape weights are derived live | Nothing to bake, so nothing can get out of step |
| Found the face mesh, Hips and eyes from fixed name lists and prefixes, and allowed one character per scene | The artist picks the face mesh(es), and optionally the eye bones, in a *Set Up Face* step. Any number of characters, each with its own rig. | Fixed naming was the most common setup failure |
| Shape-name inconsistencies (`JawForeward`/`JawForward`, `Jaw_RotateY` vs `JawRotateY`, `Midmouth_Left` listed twice) | One canonical list, plus aliases | |
| Lip sync through a Windows SAPI aligner, with fixed 0.2 s ramps | The engine and keyer in §7: per-language rules, word timings, lead, hold and smoothing | Cross-platform, and tunable |
| Random blinks and saccades keyed in place by buttons | Generators on a layer | Non-destructive, and they loop with idles |

The recovered FaceBot source and docs are kept for reference in `REFERENCE/FaceBot_Free`
(not part of the build). `source_recovered/` holds the code decompiled from its
`.pyc` files.

| FaceBot | Keyternity | why |
|---|---|---|
| Shapes sculpted in another 3D app | Generated from a landmark map and templates, then refined (§6.7) | In 2D the artist authors everything. |

### 6.7 Generating the face: landmarks and automatic shapes

FaceBot assumed the shapes were sculpted in another 3D app. In Keyternity the artist
authors everything, so a full set of 40-odd shapes by hand would be the bottleneck.
Instead, the artist **maps a small set of landmarks** on the mesh once. Keyternity
then **generates** the standard shapes (§6.4) from templates, and the artist refines
them.

**Landmark map.** About 30 named points, shown on the face-rig mask (§6.5) as dots to
pick:

| region | landmarks | count |
|---|---|---|
| Mouth | `MouthCorner_L/R`; upper lip `UpperLip_L2, L1, R1, R2`; lower lip `LowerLip_L2, L1, R1, R2` | 10 |
| Lip centres (midline) | `UpperLipMid`, `LowerLipMid`. Optional: they're derived from the inner pairs when not mapped. | (2) |
| Eyes | upper lid `Lid_L_Up1/2`, lower lid `Lid_L_Low1/2`, corners `EyeIn_L`, `EyeOut_L`, and the same on R | 12 |
| Brows | `Brow_L_In/Mid/Out`, the same on R | 6 |
| Nose | `NoseWing_L/R`, and `NoseTip` on the midline | 3 |
| Jaw | `Chin` (midline), and optionally `Jaw_L/R` | 1–3 |

The mouth's 10 points are the core: 4 on the top lip, 4 on the bottom, and the 2
corners. A face can map only the regions it animates: a character that never blinks
skips the eyes.

#### Setup: map one side, derive the other

1. *Set Up Face* asks which **view** the drawing is (front, three-quarter or profile),
   and suggests one (below).
2. **Midline first:** the artist maps the midline points (`UpperLipMid`,
   `LowerLipMid`, `NoseTip`, `Chin`, about four). They fix the face's symmetry
   axis, including any head tilt.
3. **One side:** the artist maps the left points. Clicking a dot on the mask, then a
   vertex on the mesh, maps it. The next dot is selected automatically, so mapping is
   a run of clicks.
4. **Derived side:** the right side is computed (below), snapped to vertices, and
   shown as hollow dots. The artist confirms or drags any that are off. It's always
   a suggestion, never silently final.
5. **A check** lists what generating needs (mesh requirements, below), with a fix for
   each.

**Front view.** The symmetry axis is the line best fitting the midline points (least
squares, so a tilted head gives a tilted axis). Each right point is its left twin
reflected across that axis.

**Three-quarter view.** On a turned head the far side is foreshortened: it's a
mirror, squeezed. Two ways to measure the squeeze:
- **Automatically,** when a few far-side points are also mapped. Compare a pair's
  distances from the axis. If the far mouth corner is 0.6 as far from the midline as
  the near one, the face is turned. (Your observation: the far corner sits close to
  its neighbours, the near one far from them.)
- **From one far anchor:** the artist maps a single far-side point, usually the far
  mouth corner, and everything else on that side follows from it.

The model treats the head as a **cylinder** seen at a yaw angle θ. A point at angle φ
around the head appears at `x = R·sin(φ + θ)` from the head's centre. Its mirrored
twin at −φ appears at `R·sin(θ − φ)`.
- **Inputs:** the head radius R (from the face outline, or the eye-corner span) and
  the midline position on screen (`R·sin θ`).
- **Solving:** one mapped far anchor gives θ. Every near point's φ then gives its far
  twin.
- **Why a cylinder:** points far from the midline (corners, outer brows) compress
  more than points near it (inner lips, nose wings), which is what a turned face
  does. A flat squeeze would misplace them.
- **Heights:** vertical positions are carried across along the tilted axis.

Derived points then **snap** to a vertex near the computed spot. The search prefers a
vertex on the same feature line (the lip seam for lip points, the lid seam for lid
points) and keeps the points in order along it. The result is a suggestion for the
artist to confirm.

**Suggesting the view.** Once the midline and a pair are mapped:
- **Front:** both sides within about 10%.
- **Three-quarter:** clearly uneven.
- **Profile:** the midline lies on the mesh's outer edge.

The artist can always override it.

**Profile view** (unique to 2D). Only one side is visible, and the midline is the
silhouette.
- **Landmarks:** a reduced set with no mirroring: upper and lower lip front points,
  the one visible corner, `Chin`, `NoseTip`, the visible eye's lids and brow.
- **Shapes:** paired shapes collapse to one. `Smile` drives the visible corner, and
  the rig hides the far-side controls.
- **Templates:** each has a profile variant. O pushes the lips forward, smile pulls
  the corner back and up, MBP presses the lips together at the front.
- **Facing:** a profile facing the other way is the same map flipped in x.

**Several views of one character** (front, three-quarter and profile drawings,
swapped by Styles or textures) each get their own landmark map and generated shapes,
under the **same shape and control names**. Animation, lip sync and viseme poses
then carry across when the drawing switches views.

#### Templates

Each standard shape is a **template**: how each landmark moves, in face-relative
units, so it fits any face:
- horizontal motion in **mouth widths** (corner to corner) for the mouth, and in eye
  widths for the eyes;
- vertical motion in **nose-to-chin** distances.

Examples:
- `Smile_L`: the left corner +0.12 out and +0.10 up; the left lip points follow at
  half.
- `MouthOpen`: the lower lip points down 0.25 and `Chin` down 0.2.
- `MBP`: the upper and lower lip points meet at the lip line.

Templates are written for a front view, in head space. For three-quarter views
they're projected with the same yaw (horizontal motion scales with the cylinder's
local squeeze). Profile uses its own variants.
- **Style presets** (Subtle, Standard, Cartoon) scale every template.
- Each shape has its own **strength** too, as a cartoon "AI" opens far wider than a
  realistic one.
- Templates are data (JSON), so studios can ship their own.

#### Spreading landmark motion to the mesh

Uses existing machinery:
1. Each mapped landmark becomes a hidden helper bone.
2. The helpers get automatic weights, limited to the landmark's **region**. A
   region is the area around its feature group, padded and with its edge held fixed,
   so mouth shapes never move the eyes and brow shapes never move the mouth.
3. For each template, the helpers are posed and *Capture as shape* (§4.4) bakes the
   result into that shape's deltas.

The artist can fix a falloff with the weight tools they already know, and regenerate.
A smoother interpolation through the landmark motion (radial basis functions) can
replace the helper bones later without changing the workflow.

#### Mesh requirements (checked at setup)

- **A lip seam:** the upper and lower lip must be separate edges along the mouth
  line, or the mouth can't open. The check finds the lip points' edges. If they're
  joined, it offers to cut the seam with the Pen tool along the mapped lip points.
- **A mouth interior:** a drawn layer behind the mouth (dark cavity, teeth, tongue)
  that shows as the lips part. It's a separate bone, which the jaw and lower lip can
  drive (teeth follow the jaw; `TongueUp` moves the tongue).
- **Lid seams,** for blinks, the same way, with an eyeball or eye-white layer behind.
- **Enough vertices** around each feature: the check suggests densifying (Trace
  detail, or adding points) where a region has too few to bend smoothly.

#### Regenerating without losing work

Each generated shape stores **its generated deltas plus the artist's corrections**
separately. Regenerating, after remapping a landmark or changing a strength or the
style, rebuilds the generated part and keeps the corrections on top. Sculpting a
generated shape edits only its corrections. *Reset Corrections* returns to the pure
template.

#### The result

Map about 30 points (about 15 with mirroring), press *Generate Face*, and the
character has:
- the standard shapes;
- a working face rig (§6.5);
- viseme poses (§6.2) ready for lip sync.

Then the artist refines.

## 7. Auto lip sync

### 7.1 Pipeline

```
text + word timings  ← from the voice generator (§8), or from transcribing imported audio (§7.4)
   → text rules (per language): each word → its sounds, with relative lengths (§7.3)
   → sounds stretched over each word's time span → cues (time, sound)
   → mouth rig's viseme set (§6.1) → viseme track
   → keyer: anticipation, holds, smoothing, intensity
   → keys on the mouth rig's shapes or textures, in the Lip Sync layer
```

The transcript stays editable. Fixing a misheard word and pressing *Regenerate* is
the main way an artist corrects lip sync.

**Lip-sync tracks** (`LipSyncTrack { clip, mouth rig, words: Vec<(word, start, end)>, language, settings }`) are
stored with the animation:
- The keys they produce go in the track's own generated **Lip Sync layer** (TIMELINE.md §5.4).
  *Regenerate* (after changing the audio, text or settings) replaces only that
  layer's keys.
- *Bake* turns it into an ordinary layer the artist owns.
- The artist's other layers (brows, blinks, smiles, head turns) are never touched.

### 7.2 Keyer settings

| setting | default | why |
|---|---|---|
| Lead | 1 frame | Mouths read better slightly ahead of the sound. |
| Minimum hold | 2 frames (24 fps) | Prevents flicker on fast speech. |
| Smoothing | Medium | Shapes only: weight attack and release, and blending between neighbouring visemes (co-articulation). |
| Intensity | 100% | Mumbled to over-enunciated. |
| Rest between phrases | On, after 0.2 s of silence | |
| Key every frame vs only on changes | On changes | Fewer keys, easier to edit. |

Drawn mouths get stepped keys, while shapes get eased weight curves.

### 7.3 Engine: text rules + word timings (TalkingHead's technique)

The engine is the approach from [TalkingHead](https://github.com/met4citizen/TalkingHead)
(MIT), ported to Rust:
- A small **language module** turns each word into its sounds, each with a relative
  start and length. English uses spelling rules; TalkingHead reports about 80%
  accuracy against a pronunciation dictionary, and more on common words. Languages
  with consistent spelling, like Finnish, map letters straight to sounds.
- Those sounds are then **stretched over the word's time span**.
- TalkingHead has modules for English, German, French, Brazilian Portuguese, Finnish
  and Lithuanian. We port them with attribution, which MIT allows under our GPL-3.0.
- The port adds the `L` and `W` sounds (§6.1).
- Its philosophy is simplicity: *"It is often better to be small and fast than to
  aim for 100% accuracy."*

Why this rather than audio recognition:
- **It's plain code:** no external program, model or download for the core.
- **It works the same everywhere,** the web build included.
- **Mistakes are fixable:** they come from the words, so correcting the transcript
  corrects the mouth.

**Not chosen:**
- **Rhubarb Lip Sync** (MIT; audio in, timed shapes A–H/X out, 0.01 s, no weights).
  It's good for recordings with no script, but it's another external binary, and it's
  English-only in practice.
- **A wav2vec2 phoneme recogniser** in ONNX. It gives weighted, multilingual results,
  but needs a large model.

Either could become an optional engine later if transcripts prove too coarse.

**Loudness** stays as a no-download fallback: mouth openness from the audio envelope,
for creatures, robots, gibberish, and lines with no words.

### 7.4 Transcribing imported audio

For a recorded line, **Transcribe** runs **whisper.cpp** locally through `whisper-rs`.
It gives the text and word timestamps, which feed the same engine.
- **The model is a one-time download,** pinned and SHA-checked like FFmpeg: `tiny`
  ~75 MB, `base` ~142 MB, with multilingual and English-only variants.
- **The transcript shows in the Dialog panel** for checking. If the script is already
  known, the artist can paste it, and Whisper is only used to time it.
- **Timing precision:** Whisper's word times can be off by tens of milliseconds. The
  keyer's lead and holds absorb most of that. whisper.cpp's experimental token-level
  timestamps (DTW) tighten it, and if needed a forced-alignment pass can come later.
- **Desktop only.** On the web, the artist types or pastes the transcript, and the
  words are spread evenly over speech found by loudness.

## 8. Voice generation (dialog from text)

A **provider** interface:

```rust
fn synthesize(&self, text: &str, voice: &Voice, opts: &VoiceOpts)
    -> Result<(Wav, Option<Vec<WordTiming>>), String>
```

The text is known, so lip sync only needs **word timings**:
- Providers that give them (Kokoro's durations summed per word; ElevenLabs'
  character timestamps) feed the engine directly.
- For the others, Whisper times the known text (§7.4).

| provider | where | voices | timings | licence and notes |
|---|---|---|---|---|
| **Kokoro-82M** (first) | Local ONNX (`ort`), ~80–330 MB model download | 54 voices, 8 languages (v1.0) | Phonemes from its misaki G2P. Durations come from ONNX exports that output the duration tensor (the Rust Kokoros project reads them). | Apache-2.0 weights. Small and fast, CPU-friendly. No voice cloning. |
| **Chatterbox** (Resemble AI) | Local Python/PyTorch, as a helper process | **Voice cloning** from a reference clip. Emotion "exaggeration" control (original model). 23+ languages (Multilingual V3). | None, so Whisper times the known text | MIT. Every output carries an imperceptible Perth watermark. Heavier: 110M–500M parameters, and a GPU is best. |
| **ElevenLabs** (cloud, optional) | HTTPS with the artist's own API key | Large library and cloning | `/with-timestamps` returns per-character timing | Commercial service. Sends the text off the machine, so it's opt-in, with the key stored like the agent token. |
| Piper | — | — | — | Skipped for now: the original MIT repository was archived in October 2025, and development continued under GPL. Voice licences vary. |

**UI:** a **Dialog** panel on the animation:
1. Type the line and pick a provider and voice (and speed or emotion where supported).
2. *Generate* adds the clip at the playhead and runs lip sync.
3. Takes are kept, so you can audition and swap them.

**Consent:** voice cloning needs the voice owner's consent. The UI says so when a
reference clip is added, and Keyternity ships no celebrity or real-person voices.

## 9. Agent (MCP) tools

These sit alongside the skinning tools:
- `list_shapes`, `set_shape_weight`, `key_shapes`;
- `list_mouth_rigs`, `auto_fill_mouth_rig`;
- `add_dialog` (text, voice) and `import_audio`;
- `lip_sync` (animation, clip, transcript, settings).

The review page (`review_page`) can show a lip-synced animation as a frame strip with
the transcript. That lets an agent block out a scene's dialog, which the artist then
reviews and refines.

## 10. Phases

| phase | contents | notes |
|---|---|---|
| **0. Timeline pass and layers** | [TIMELINE.md](TIMELINE.md): navigation, compact rows and keys, then animation layers | Its own feature, done first |
| **1. Shapes** ✓ | Data, evaluation before skinning, ShapeWeight keys and timeline rows, Shapes panel, sculpt mode, mirror, capture from pose, remapping on topology edits, `.skf` and DragonBones (FFD) export, tests | Useful immediately: blinks, brows, squash. |
| **1b. Standard face and face rig** | The standard shape list and aliases, *Add Standard Face Shapes*, the mapping table, *Set Up Face*, control helper bones and the driver table, the default layout generated from `FaceBot.FBX`, the rig overlay panel, Eye Look, driven weights baked on export | The FaceBot structure in 2D (§6.4–6.6) |
| **1c. Generated faces** | Landmark map on the rig mask, *Set Up Face* (midline, one side, derived side: mirror for front views, cylinder model for three-quarter, profile mode), view suggestion, snapping to feature lines, mesh checks (lip and lid seams, mouth interior, density) with fixes, templates (JSON) with style presets and per-shape strength, helper-bone propagation with regions, *Generate Face*, generated + corrections and regenerate | After 1 and 1b. Makes the whole face a few minutes' setup. |
| **2. Dialog audio** | Clips embedded in `.skf`, Symphonia decoding, rodio playback in sync, waveform, audio scrub, video export with audio | Desktop first |
| **3. Mouth rigs + lip sync** | The 17 sounds and the viseme-set mappings, mouth rig panel and auto-fill, the English text rules (ported from TalkingHead), typed transcripts with words spread over speech, lip-sync tracks and keyer, regenerate and bake, loudness fallback, drawn and morph mouths | The Facebot loop, end to end. No downloads. |
| **3b. Transcription** | whisper.cpp through `whisper-rs`, model download, word timings, editable transcript; TalkingHead's other language modules | Imported recordings |
| **4. Voice generation** | Provider interface, Kokoro local with timings, Dialog panel with takes; then ElevenLabs (opt-in) and Chatterbox (helper process) | |
| **5. Agent** | MCP tools; optional audio-recognition engines (Rhubarb, ONNX phonemes) only if transcripts prove too coarse | |

## 11. Decisions to confirm

| # | question | recommendation |
|---|---|---|
| Q0 | Layers inside an animation, with Override and Additive modes, before Shapes? | Moved to [TIMELINE.md](TIMELINE.md) §7 |
| Q1 | Shapes after layers, with drawn-mouth (texture) lip sync also supported in phase 3? | Yes |
| Q2 | Viseme sets? | **Decided:** internally Oculus 15 + L + W (TalkingHead's output). Mouth rigs draw Preston Blair 10 (default), Oculus 15 or Simple 5 through mapping tables (§6.1). |
| Q3 | Audio embedded in the `.skf`, or linked by path? | Embedded (portable); a "linked" option later if files get large |
| Q4 | Voice generation: local only, or cloud allowed too? Voice cloning? | Local Kokoro first; cloud and cloning as opt-in later |
| Q5 | Languages that matter to you beyond English? | — |
| Q6 | Engine? | **Decided:** TalkingHead's technique (text rules + word timings), with Whisper transcription for imported audio. Rhubarb isn't used. Lip sync itself also works on the web; only transcription is desktop-only. |
| Q7 | Does Facebot's phoneme→viseme table, or its rig convention (shape names), exist anywhere we should match? | **Answered:** recovered from the FaceBot source (`REFERENCE/`). Its Mixamo shape set becomes the standard face (§6.4), its 13-control mask becomes the 2D face rig (§6.5), and its SAPI→Preston Blair table matches §6.1. |
| Q8 | Face-rig controls as hidden helper bones, with shape weights derived each frame (no bake round trip)? | Yes |
| Q9 | Canonical shape names shortened (`_L`/`_R`, jaw side and twist), with Mixamo's and FaceBot's spellings as aliases? | Yes |
| Q10 | Visemes built from rig values by default, with viseme shapes used when present? | Yes |
| Q11 | Generate the standard shapes from a ~30-point landmark map (10 for the mouth), mapping the midline and one side and deriving the other? | Yes (§6.7) |
| Q12 | Three-quarter mirroring by a cylinder model, solved from one far-side anchor or from measured asymmetry, with derived points always shown for confirmation? | Yes |
| Q13 | Profile as its own view, with a reduced landmark set, collapsed paired shapes and profile templates; several views of a character sharing shape and control names? | Yes |

## Sources

- TalkingHead (the lip-sync technique): [repository](https://github.com/met4citizen/TalkingHead)
- FaceBot (the structure for §6.4–6.6): `REFERENCE/FaceBot_Free`, with its conventions, morph and phoneme structure PDFs, rig FBX, and `source_recovered/` (decompiled from its Python 2.7 `.pyc` files)

- Rhubarb Lip Sync: [repository and README](https://github.com/DanielSWolf/rhubarb-lip-sync), [licence](https://github.com/DanielSWolf/rhubarb-lip-sync/blob/master/LICENSE.md), [releases](https://github.com/DanielSWolf/rhubarb-lip-sync/releases)
- Kokoro-82M: [model card](https://huggingface.co/hexgrad/Kokoro-82M); durations in the Rust port: [Kokoros](https://docsearch.algolia.com/mcp/docs/repo/lucasjinreal/kokoros)
- Chatterbox: [repository](https://github.com/resemble-ai/chatterbox)
- ElevenLabs timestamps: [API reference](https://elevenlabs.io/docs/api-reference/streaming-with-timestamps), [announcement](https://elevenlabs.io/blog/new-text-to-speech-endpoints-with-timestamps)
- Phoneme recognition in ONNX: [wav2vec2-espeak-ctc export](https://huggingface.co/sadda-speech/wav2vec2-espeak-ctc), [CTC aligner](https://huggingface.co/omote-ai/ctc-aligner), [torchaudio forced alignment](https://pytorch.org/audio/main/tutorials/forced_alignment_tutorial.html)
- Piper status: [overview](https://www.cekura.ai/discover/piper-tts)
- Audio in Rust: [rodio](https://docs.rs/crate/rodio/0.20.0), [Symphonia](https://www.mintlify.com/pdeljanov/Symphonia/resources/faq)
- whisper.cpp in Rust (transcription, word timings): [whisper-rs](https://docs.rs/crate/whisper-rs/latest)
- Rhubarb's output formats (considered, not chosen): [README](https://github.com/DanielSWolf/rhubarb-lip-sync/blob/master/README.adoc)
