# Timeline pass

Status: **design, not built.** Decisions to confirm are in §7.

## 1. Why

The timeline is where animation gets read and compared, and it's hard to do either:

| problem | today (`keyframe_editor.rs`) |
|---|---|
| **Zoom is a single number**, stepped linearly by +/− (0.1–3) or modifier+scroll (0.1–10, a different limit), and not anchored to the cursor | At 24 fps frames are 5.5–166 px apart. A long animation never fits, and zooming loses your place. |
| **Scrollbars are hidden** | Nothing shows where you are in the animation, or how much is off screen. |
| **Keys are 20 px icons** (26 px hovered or selected), one picture per property | They overlap at low zoom. Their colour alternates per frame rather than meaning anything, so keys can't be compared down a column. |
| **One row per property** (Position X and Y apart; Tint R, G, B and A apart), each with a 🗑 button | Lots of tall rows for few keys, and room for only a handful of bones. |
| **Folded bones show nothing** | There's no way to see the timing of many bones at once. |
| **Every frame line is drawn** across the whole width | Wasted drawing, and visual noise at low zoom. |

This pass fixes reading and navigation, then adds animation layers (§5), which are
mostly timeline UI. Lip sync ([LIP_SYNC.md](LIP_SYNC.md)) builds on it.

## 2. Navigation

### 2.1 The view

The single `timeline_zoom` and the pixel offset become a **visible frame range**,
`view: (start, end)` in frames (fractional):
- Frame spacing = graph width / (end − start).
- Every draw and hit test uses one pair of pure functions, `frame_to_x` and
  `x_to_frame`, which are tested.
- **Limits:**
  - zoomed in: at least ~6 frames visible (so ~100 px or more per frame);
  - zoomed out: the whole animation plus some headroom, whatever its length;
  - the view never scrolls before frame 0.

### 2.2 Range bar (scroll handle with zoom handles)

The standard convention from After Effects, Premiere, DaVinci and Blender's sequencer:
a bar under the graph that stands for the **whole animation**, with a handle for the
visible part:

```
|----[<|=========visible==========|>]-------------------------------------|
0                                                                       end
```

- **Drag the middle** to scroll. **Drag either end** (the sub-handles) to zoom; the
  opposite end stays put.
- **Click the track** to jump the view there. **Double-click the bar** to fit the whole
  animation.
- The bar marks the last key and the current frame, so you can see where you are in a
  long animation at a glance.
- The handle never gets too small to grab: below ~24 px it keeps that size, and the end
  handles stay usable.

### 2.3 Mouse and keys

| input | action |
|---|---|
| wheel | scroll rows (as today) |
| Shift + wheel | scroll in time |
| zoom modifier (`timeline_zoom_mode`) + wheel, or wheel over the ruler or range bar | **zoom around the cursor**: the frame under the pointer stays under it, with each notch a fixed ratio (×1.25) |
| middle-drag | pan in time |
| `Home` (pointer over the timeline) | fit the whole animation (elsewhere, `Home` still fits the canvas) |
| `Shift+Home` (pointer over the timeline) | fit the selected keys, or all keys if none are selected (`F` is already Next Frame) |
| +/− buttons | zoom ×1.25 around the current frame |
| press or drag on the ruler | scrub the current frame |

### 2.4 Ruler

- **Adaptive labels:** the step is 1, 2, 5, 10, 20, 50… frames, whichever keeps the
  labels at least ~40 px apart. Minor ticks go between them.
- **Frames or seconds:** a toggle.
- The **current frame** line runs from the ruler down through every row.
- The **end of the animation** (its last key) is marked, and the area after it is
  shaded, as now.

### 2.5 Drawing only what's seen

- Frame lines, keys and labels are drawn only inside the view.
- When frames are under ~6 px apart, only every Nth frame line is drawn (N from the
  ruler step).
- Keys are found per row by binary search over their sorted frames, so long
  animations stay fast.

### 2.6 End (the animation's length)

Each animation has an **End** field in the bottom bar: its last frame (`end_frame` in
the `.skf`).
- Unset (0), it follows the last key, as before, and shows dimmed.
- Once set, playback loops there, D/F wrap there, exports (frame sequences,
  spritesheets, DragonBones) stop there, and frames after it are dimmed with a line
  marking the End. An End past the last key holds the final pose.
- Keys after the End are kept, but not played.
- ⟲ clears it. Setting it is undoable.

## 3. Rows and keys

### 3.1 Compact rows

- **Row height** ~18 px, down from ~24.
- **Channels grouped** into one row per property: Position, Scale, Tint, Pivot, IK.
  - The row shows a key wherever any of its components has one.
  - A disclosure arrow opens the X/Y (or R/G/B/A) sub-rows when they're needed.
- **🗑 buttons removed** from every row. Deleting a row's keys moves to the row's
  right-click menu and to Delete with the row selected.
- **Resizable label column.** Bone headers stay pinned at the top while their rows
  scroll.

### 3.2 Keys

Keys become **small, uniform marks** (8 px, 12 px hit area) centred exactly on the
frame line, so keys on the same frame line up down the column:

| mark | meaning |
|---|---|
| ◆ diamond | an eased or bezier key (most keys) |
| ■ square | a **stepped** key: texture, visibility, z-index, IK flags |
| filled accent | selected |
| outline | hovered |
| faint | on another layer (§5.3) |

- The property's icon moves to the **row label**, next to its name, so each key no
  longer carries a picture.
- **Colour means something:**
  - each property group gets a muted hue (position, rotation, scale, tint, texture…)
    shown on the label and its keys;
  - frames alternate by background shading, not by key colour.

### 3.3 Comparing across many bones

- **Summary keys:** a bone's header row shows a mark on every frame where any of its
  properties has a key, and keeps showing them when folded. A **Summary** row at the
  top does the same for the whole animation.
  - Fold everything and you get one row per bone, so timing can be compared across
    the whole rig.
  - Dragging a summary key moves all the keys under it.
- **Column highlight:**
  - Hovering a key, or the current frame, lightly highlights that frame's column
    across all rows.
  - Every key on that frame is emphasised, so "what else is keyed here?" can be seen
    at a glance.
- **Key count tooltip:** hovering a summary key lists what's keyed on that frame.

### 3.4 Selection and editing

What exists stays the same, with clearer marks:
- click, Ctrl/Shift-click, drag to move;
- double-click to select related keys;
- the context menu.

Added:
- **Box select:** drag on empty graph space to select keys in a rectangle (Ctrl/Shift
  adds to the selection). Clicking empty space goes to that frame; scrubbing by
  dragging moved to the ruler (§2.3).
- **Select column:** click a summary key, or Alt-click a frame on the ruler, to
  select every key on that frame.

### 3.5 Scaling keys in time

Retiming a selection (making a motion faster or slower) without moving every key by
hand. The pivot is the **current frame**: keys on it stay put, and the rest move
toward or away from it.

- **Start:** press `S` over the timeline, or choose *Scale Keys* in the context menu,
  with keys selected. Move the playhead first to choose the pivot.
- **Drag:** the factor is the pointer's distance from the pivot, divided by its
  distance when scaling started. The factor and the resulting span ("×0.50, 12 → 6
  frames") show beside the pointer, and the keys preview live. Ctrl snaps the factor
  to 0.25 steps. Click or Enter applies; right-click or Esc cancels.
- **Typed:** *Scale Keys…* in the context menu takes an exact factor or a target
  length in frames.
- **Rules:**
  - New frames are `pivot + (frame − pivot) × factor`, rounded to whole frames.
  - Dragging past the pivot gives a negative factor, which **reverses** the keys.
  - Keys can't go below frame 0. The drag stops at the factor that would put one
    there.
  - Two keys of one channel landing on the same frame merge. The key that moved
    farther wins (the outer one, so the motion's extremes survive), and the status
    line says how many keys merged.
  - The current keys' easing is kept, so only the timing changes.
- One scale is one undo step. With layers (§5), only keys on the active layer are
  scaled.
- The same pivot rule applies to dragging keys with Alt held, so a selection can be
  stretched straight from the graph.

## 4. Layout

```
┌ animation list ┬──────────────────────────────────────────────┐
│                │ ruler (adaptive) ·········▼current···········│
│ layers (§5.3)  ├──────────────────────────────────────────────┤
│ ▸ Base     ◉   │ Summary        ◆     ◆        ◆   ◆         │
│ ▸ Face  ●  ◉   │ ▾ head         ◆     ◆            ◆         │
│                │     Rotation   ◆     ◆            ◆         │
│ rows:          │     Position   ◆                  ◆         │
│ label column   │ ▸ jaw          ◆  ◆  ◆  ◆  ◆  ◆  ◆          │
│ (resizable)    │ ▸ arm_L        ◆           ◆                │
│                ├──────────────────────────────────────────────┤
│                │ [<|====== range bar ======|>]···············│
├────────────────┴──────────────────────────────────────────────┤
│ play · frame · fps · onion · set key · key all · breakdown    │
└────────────────────────────────────────────────────────────────┘
```

### 4.1 Where layers sit

The layer stack is a short list at the top of the label column. It's collapsible and
hidden while an animation has only Base. Selecting a layer makes it the active one
(§5.3).

## 5. Animation layers

Layers let separate kinds of animation live in separate places that combine: a
breathing cycle over a walk, a blink layer, or generated lip sync that a smile animates
over without the two fighting for keys (MotionBuilder's layers did this for Facebot;
see [LIP_SYNC.md](LIP_SYNC.md)). Keyternity has no layers yet:
- An `Animation` is one flat list of keys per (bone, property). Absolute values are
  interpolated over the setup pose (`Armature::interpolate_keyframes`).
- One animation plays at a time.

### 5.1 Model

Layers live **inside an animation** (like MotionBuilder's layers within a take, and
Blender's NLA tracks on one action):

```rust
pub struct AnimLayer {
    pub id: i32,
    pub name: String,          // "Base", "Lip Sync", "Face", "Breathing"
    pub mode: LayerMode,       // Override | Additive
    pub weight: f32,           // 0..1; keyable later (§5.6)
    pub muted: bool,
    pub solo: bool,
    pub locked: bool,          // no editing (e.g. a generated layer)
    pub generated: Option<i32>,// owned by a lip-sync track (LIP_SYNC.md §7.1): regenerate replaces its keys
}
// Animation gains `layers: Vec<AnimLayer>` (bottom to top); Keyframe gains `layer: i32`.
```

- **Compatibility:** layer 0 is **Base**. Every existing key belongs to it
  (`Keyframe.layer` defaults to 0), so current files and code paths are unchanged.
- **Channels:** a layer only affects the channels (bone, property, shape) it has keys
  for. Everything else passes through from below, so layers need no bone masks to
  start with.

### 5.2 Evaluation

For each channel, bottom to top:

```
v = setup value, then the Base layer's keys as today
for each upper layer L with keys on this channel (not muted; only soloed ones if any solo):
    k = L's interpolated value
    Override:  v = lerp(v, k, L.weight)
    Additive:  v = v + L.weight * (k - rest)          // rest = the channel's setup value
```

What "additive" means depends on the property:

| property | additive | override |
|---|---|---|
| position, rotation | adds the offset from the setup pose | blends to the layer's value |
| scale | multiplies by (layer ÷ setup), weighted | blends |
| shape weights ([LIP_SYNC.md](LIP_SYNC.md) §4) | **adds the weight** (0 = no change), clamped per exclusive group (LIP_SYNC.md §6.3) | blends |
| tint | blends toward the layer's colour | blends |
| texture, hidden, z-index, IK flags (discrete) | treated as override | the top layer with a key wins, if its weight ≥ 0.5 |

This runs in `Armature::animate`, before IK and physics, which see the combined pose
just as they see today's. Evaluating several layers costs only a little more than
today, because channels without keys on a layer are skipped quickly.

### 5.3 Editing

The timeline gets a **layer stack** above the rows (§4.1):
- Each layer has its name, mode, a weight slider, and mute, solo and lock toggles.
- One layer is **active**. New keys (from transforms, the bone panel, key buttons,
  paste) go there.
- The rows show the active layer's keys. Other layers' keys show as faint ticks, so
  you can see what's happening above and below.
- Actions: *Add Layer*, *Duplicate*, *Merge Down* (bake a layer into the one below),
  *Bake All* (flatten), *Delete*. All are undoable.
- **Generated layers** (lip sync) are locked. Unlocking one, or editing its keys,
  first asks whether to detach it from regeneration.

**As built:**
- **Storage:** the active layer's keys live in `Animation.keyframes`, and the other
  layers keep theirs in `AnimLayer.keyframes`. Every existing tool (keying, the rows,
  the keyframe panel, copy and paste, breakdown) therefore works on the active layer
  unchanged. Saving puts Base back in `keyframes` (`active_layer` 0), so files read as
  before.
- **Keying through a layer:** editing a bone, or Set Key, writes the key the active
  layer needs for the pose to show what was set. The other playing layers give the
  value below it, and the layer's mode and weight are inverted over that
  (`layers::key_on_active`). On an additive layer, the automatic first key is the
  setup value, which is a zero offset. An override layer gets no automatic first key,
  so it doesn't hold the setup pose over Base.
- **The stack:** the header above the labels holds *+ Layer* (additive by default) and
  folds the stack. Each layer shows its name (click to edit that layer, double-click
  to rename), weight (drag; double-click for 100%), mode (`+` additive, `=` override),
  and **M**ute, **S**olo and **L**ock. Its keyed frames show as ticks across the graph.
  Right-click a layer for Rename, Duplicate, Merge Down, Delete, or Flatten All Layers.
- **Merging and flattening** sample every frame. That's exact, because the editor and
  runtimes play whole frames. Straight runs are reduced back to two keys.

### 5.4 Lip sync on layers

- The lip-sync track writes into its own **Lip Sync** layer:
  - additive for morph mouths (so expression shapes on other layers add to it);
  - override for drawn mouths (textures).
- *Regenerate* replaces only that layer's keys. *Bake* detaches it into an ordinary
  layer.
- The artist animates the face and body on other layers. Lowering the Lip Sync
  layer's weight mumbles; raising it over 1 isn't allowed.

### 5.5 Saving and export

| target | layers |
|---|---|
| `.skf` (editor file) | Kept as they are: `layers` on animations and `layer` on keys, documented in `skf_readme.md`. Runtimes that predate layers would mix all keys, so `.skfe` exports default to **Flatten layers**. |
| DragonBones | Flattened. Each channel is sampled at the union of its key frames across layers (plus a set rate where curves combine), so the result matches the editor. |
| Frame sequence, spritesheet, video | Rendered from the combined pose as they are. |

Flattening is a pure function (`layers::flatten(animation, armature) -> Animation`),
tested against the evaluator frame by frame.

### 5.6 Later

- **Keyable layer weight:** fade a layer in and out, e.g. a sneer layer at 0 → 1.
- **Bone masks:** limit an override layer to an arm, for "wave while walking".
- **Blending whole animations at runtime:** play idle + talk together. Free-running
  loops (§5.7) are the first case of this.

### 5.7 Play mode: Linear or Looped

A breathing cycle lasts 3 seconds. It should run under a 1 s jump, a 10 s idle and a
walk alike: looping smoothly, never stretched, and never restarting because the
animation under it changed or ended. So every layer gets a **play mode**:

- **Linear** (the default, and always the mode for Base): the layer follows the
  animation's time, frame for frame, as layers do today.
- **Looped:** the layer has its own **length** (say 3 s), and its keys repeat for as
  long as the animation runs. It also has:
  - **speed** (×1 by default);
  - **phase**, an offset in seconds, so two characters don't breathe in step;
  - **clock:** *Animation* (the default) restarts with the animation. *Free-running*
    keeps one clock going while you switch animations or scrub, to preview how it
    will look in a game.

  Its frame is `(t × speed + phase) mod length`, where `t` is in seconds, so its
  timing doesn't depend on the host animation's length or fps.
- **Source:** a Looped layer holds its own keys, or **links another animation**
  (*Add Linked Layer…*). Link a "Breathing" animation into walk, idle and talk, edit
  it once, and all three update. A linked layer can be Linear too, which plays the
  linked animation once from the start.
- **Seams:** the loop's end shows on the layer's rows. *Close Loop* copies the first
  frame's keys to the end, so it loops without a pop.
- **The timeline** shows the repeats past the loop length as faint copies
  (`|— breath —|— breath —|…`). Only the first one is editable. Double-click a linked
  layer to open the animation it links.

```rust
pub enum PlayMode {
    Linear,
    Looped { length: f32, speed: f32, phase: f32, free_running: bool }, // seconds
}
// AnimLayer gains `play: PlayMode` and `link: Option<i32>` (the animation it plays).
```

**Runtimes and export.** The point of a free-running loop is that it keeps running
when a game switches animations. That needs the runtime to play it on its own track:
- `.skf`/`.skfe` keep the layer's play mode and link. A runtime with tracks plays a
  free-running loop on one, with its own clock.
- *Flatten layers* (§5.5), and runtimes without tracks, bake the loop into each host
  animation instead. It then loops correctly within that animation, but restarts when
  the animation does, and shows a seam when the host's length isn't a whole number of
  loops. Export warns about such seams.
- DragonBones gets a free-running loop as its own animation, which the game plays on
  a separate layer (DragonBones calls these animation groups).
- Frame sequences and spritesheets render the combined pose, using each host's own
  time.

### 5.8 Procedural channels (generators)

Some motion is easier to describe than to key: breathing is roughly a sine wave, idle
sway is slow noise. A **generator** drives one channel on a layer, added to whatever
that layer's keys give:

| generator | settings | for |
|---|---|---|
| **Wave** | shape (sine, triangle, square), amplitude, period (s), phase, offset | breathing, bobbing, pulsing lights (tint), wings |
| **Noise** | amplitude, frequency, octaves, seed | idle sway, head jitter, flicker |

- **Adding one:** right-click a channel row, then *Add Generator*. It shows as a
  wavy band on the row, previewed live, with its settings in a small popover.
- **Weight:** a generator's amplitude can be keyed, so a sway can grow and fade.
- **Seamless in a loop:** on a Looped layer, a Wave's period snaps to a whole
  fraction of the loop length (3 s, 1.5 s, 1 s…), and Noise loops over that length.
  The baked result therefore always loops.
- **Bake to Keys:** converts a generator into keys, sampled every N frames (2 by
  default), for hand-editing or for runtimes that don't support generators.
- **Saving:** generators are saved in the `.skf`. Exports bake them to keys, since
  runtimes don't evaluate them; that keeps runtimes simple. Baking is the same pure
  sampling the evaluator uses, so the result matches the editor.

Breathing, then, is a Looped 3 s layer, either keyed by hand or made of two Wave
generators: chest scale Y at 3 s, and a slight shoulder rise half a cycle behind. Put
it in a "Breathing" animation, and it links into every other animation as a Looped,
free-running layer.

Later: an **Expression** generator (a small formula over `t`), and a **Random event**
generator for blinks (play a short clip at random intervals between a min and max).

## 6. Phases

| phase | contents |
|---|---|
| **1. Navigation** ✓ | Visible-range view and `frame_to_x`/`x_to_frame` (tested), range bar with zoom handles, cursor-anchored zoom, Shift-scroll and middle-drag pan, Home/F fit, adaptive ruler, drawing only what's in view |
| **2. Rows and keys** ✓ | Compact rows, grouped channels with disclosure, small marks (diamond/square), colour per property group, summary rows (folded bones show keys), column highlight, box select, scaling keys around the playhead (§3.5), 🗑 moved to the context menu |
| *2, later* | Pinned bone headers while scrolling; the typed *Scale Keys…* factor (`S`-drag with Ctrl snapping covers most uses); alternating frame shading |
| **3. Layers** ✓ | §5.1–5.5: data, evaluation, the layer stack, Merge Down/Bake, flattening for export (`.skfe` and DragonBones exports flatten; `.skf` keeps layers) |
| **4. Play modes and generators** | §5.7–5.8: Linear/Looped layers (length, speed, phase, free-running clock), linked layers, Close Loop, Wave and Noise generators, Bake to Keys, seam warnings on export |

Phases 1 and 2 change only the timeline UI: no file format changes, and nothing for
runtimes. Phase 3 adds layers to the `.skf` (§5.5). Phase 4 adds play modes, links and
generators to it. Runtimes need track support only to keep free-running loops going
across animation switches; without it they get baked keys.

## 7. Decisions to confirm

| # | question | recommendation |
|---|---|---|
| T1 | Range bar under the graph (time) only, or also one for rows? | Time only; rows keep the wheel and a thin scrollbar |
| T2 | Group channels into one row per property, with disclosure for X/Y and R/G/B/A? | Yes |
| T3 | Small diamond/square marks, with the property icon moved to the row label? | Yes |
| T4 | Summary rows (per bone, and for the whole animation) that stay visible when folded? | Yes |
| T5 | A frames/seconds toggle on the ruler? | Yes, with frames as the default |
| T6 | Layers inside an animation, with Override and Additive modes, as phase 3? | Yes. Bone masks and keyable weight later (§5.6). |
| T7 | Scale keys around the current frame, by `S`-drag, Alt-drag and a typed factor; reverse past the pivot; colliding keys merge, keeping the outer one? | Yes |
| T8 | A play mode per layer, Linear or Looped (length, speed, phase, free-running clock), with layers that can link another animation? | Yes |
| T9 | Generators: start with Wave and Noise, saved in the `.skf` and baked to keys on export? | Yes. Expression and Random event (blinks) later. |
| T10 | Free-running loops at runtime: keep the link in `.skf`/`.skfe` for runtimes with tracks, and bake into each animation otherwise? | Yes, and add a track API to our runtimes as a follow-up |

