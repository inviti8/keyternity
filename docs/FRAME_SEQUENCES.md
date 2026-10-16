# Frame sequence and animation export

Status: **Phase 1 in progress.** Built so far:
- Render cameras: `src/render_camera.rs`, `src/camera_ui.rs`, tested in
  `tests/render_camera.rs` and `tests/camera_ui.rs`.
- The Frame Sequence tab (desktop): `src/export_sequence.rs`, rendered in `lib.rs`
  (`start_sequence_export`, `step_sequence_export`), tested in
  `tests/export_sequence.rs`. It covers:
  - camera, output size, step, skip-last and per-animation ranges;
  - folder or zip output, with naming;
  - the manifest, and the Phaser pack and animation files;
  - the overwrite prompt, batched rendering, and settings saved in `editor.json`.

- The Spritesheet tab (desktop; the web keeps the old Image tab), on the same options
  and pipeline. It covers:
  - grid sheets (Auto or fixed columns, padding, max sheet size, sheets per
    animation);
  - the atlas JSON: TexturePacker JSON Hash for one sheet, Phaser multiatlas for
    several;
  - Phaser animations and a pack that use the atlas.
- The out-of-frame check: before exporting through a chosen camera, frames that
  leave it are listed, and the artist can export anyway or go fit the camera.

- The export summary's engine values (`export_sequence::engine_notes`): the Phaser
  code (and `load.spritesheet` for a single grid sheet), the Godot AnimatedSprite2D
  offset, and Unity's custom pivot and grid slicing.

Phase 1 is complete (b9c57175).

**Phase 2**, built:
- **Formats:** PNG, lossless WebP, or JPG (with a quality setting, filled with the
  camera's background or white), for frames and sheets.
- **Trim and Pack** (Spritesheet tab): each frame is cropped to its visible pixels.
  The writer keeps the cropped frames, then packs them onto shelves, tallest first,
  with each sheet cropped to its contents (`pack_trimmed`). The atlas records each
  frame's offset (`trimmed`, `spriteSourceSize`, `sourceSize`), and so does the
  manifest (`offset`, `trimmed_size`). Frame sequences don't trim, because a cropped
  image on its own loses its offset.
- **Godot 4:** `<rig>.tres`, a SpriteFrames resource (`godot_tres`). For a sequence it
  has one ext_resource per image; for sheets, AtlasTexture regions, with a `margin`
  restoring trimmed offsets. Each animation has its speed (fps / step) and relative
  frame durations. A Godot Folder option sets the `res://` path prefix.
- **Follow bone** (camera panel): the camera follows a bone's movement on X and/or Y
  (`render_camera::follow_offset`). Moving animations export in place, the manifest
  records `root_motion` in output pixels, and Fit and the out-of-frame check follow
  too.
- **Export Again** (Ctrl+Shift+E, and in the File menu once there's been an
  export): re-runs the last frame sequence or spritesheet export to the same folder or
  zip, overwriting without asking. The destination is saved with the other options
  in `editor.json`.
- The writer gets the format (`Encoding`), the layout mode (`WriteMode`: files, grid,
  or packed) and a `Finisher` that builds the data files from the final `Layout`
  (sheets, trims, root motion).

Still to check in the engines: Phaser (pack paths, atlas `anchor`, trimmed frames)
and Godot (opening the `.tres`, margins).

The decisions are in §9.

## 1. Goal

Export animations as rendered frames that game engines, compositors and video editors
can use directly:
- **Phaser** comes first.
- **Godot**, **Unity** and video tools (After Effects, Premiere, DaVinci, Blender)
  should also work, wherever that stays simple.

Three exports render frames:
- frame sequence
- spritesheet
- video

All three share one set of animation options: which frames to export, and which
**render camera** frames them.

## 2. What exists today

Export (`Ctrl+E`, `src/export_modal.rs`) has four tabs: Armature, Image, Video and
DragonBones. The Image tab has an **Export type** dropdown with two choices:
Spritesheets and **Sequences**.

**How rendering works** (`utils::render_spritesheets`, `lib.rs` `check_export`):
1. For each chosen animation, it samples whole frames `0..=last keyframe` at the
   animation's own FPS, times `anim_cycles`.
2. It fits a camera to that animation's bounds, or to every animation's bounds with
   "global bounds".
3. It renders each frame on the GPU (`take_screenshot`) into one mapped buffer per
   frame, held in `rendered_spritesheets`.
4. After the buffers map, `encode_sequence` or `encode_spritesheets` turns them into
   PNGs.

**Sequences** are written into a zip as `<anim>/0.png`, `<anim>/1.png`, and so on.
**Video** pipes the same frames into FFmpeg to make MP4 or GIF.

**Gaps:**

| gap | effect |
|---|---|
| Zip only | Engines and editors want a folder. You have to unzip every time. |
| Names `0.png … 10.png`, unpadded, with no animation prefix | They sort as 0, 1, 10, 2. Frames from several animations can't share a folder. |
| Always the whole animation | You can't export a range. |
| Camera zooms to fit each animation | **The character's pixel size changes between animations**, so walk and idle don't line up in-game. |
| The camera fit ignores the output's aspect ratio | Frames are scaled into whatever frame size you typed. |
| No pivot, origin or timing data | Engines can't place the frames relative to the character, so you set offsets by hand. |
| No engine data files | Phaser, Godot and Unity setup is all manual. |
| Every frame stays in GPU-mapped memory until the end | A 1920×1080 frame takes 8 MB, so 300 frames use about 2.4 GB. |
| Video export runs `remove_dir_all` on the chosen path | That's dangerous, and must not be copied. |
| Choices are lost when the dialog closes | You redo every option on each re-export. |

## 3. Proposal overview

1. **Render cameras (D2).** A new canvas helper: a rectangle you place over the
   character that sets exactly what gets rendered and at what resolution (§4). Every
   animation exported through one camera shares its size, scale and pivot.
2. **Frame Sequence gets its own tab (D1).** The tabs become Armature, Spritesheet,
   Frame Sequence, Video and DragonBones.
3. **Shared Animation options** across Spritesheet, Frame Sequence and Video (§5).
4. **Folder output** by default (D3). Zip stays as an option, and is the only choice
   on the web.
5. **Engine targets (D7).** The exported files work in each engine as they are,
   starting with Phaser (§7).
6. **Streaming render** (§8.3), and **settings remembered per file** (§8.4).

```
Export
├ Armature         (unchanged)
├ Spritesheet      Animation options + sheet layout + engine files
├ Frame Sequence   Animation options + naming + format + engine files
├ Video            Animation options + MP4/GIF (+ WebM later)
└ DragonBones      (unchanged)
```

## 4. Render camera

### 4.1 What it is

A render camera is a named rectangle in world space. Its properties:

| property | meaning | default |
|---|---|---|
| Name | Shown on the canvas and in Export | "Camera", "Camera 2"… |
| Position | The rectangle's centre in world units | The bounds' centre (§4.4) |
| Resolution | Output width × height in pixels | From the bounds (§4.4) |
| Scale | Output pixels per world unit | 1.0 |
| Pivot | A point inside the frame where the character's origin sits, shown as a crosshair | The world origin (the root's rest position) |
| Background | Transparent, or a colour | Transparent |

The rectangle's size in world units is `resolution / scale`. The pixel size of the
character comes only from **scale**, so it's the same in every animation exported
through the camera.

A rig can have several cameras, for example "Full body" and "Portrait" at different
scales. Export picks one.

**Storage:** cameras are saved in the `.skf`. Runtimes don't need them, so they go in
`editor.json`. Every edit to a camera is undoable.

### 4.2 On the canvas

- **Show Cameras** (View menu, and the camera button on the toolbar, which adds the
  first camera if there are none) draws each camera with:
  - a dashed rectangle;
  - a label with its name and `W×H @ scale`;
  - the pivot crosshair.
  Content outside the active camera is dimmed.
- **Selecting a camera:** click its edge or label. Camera picking only runs while
  cameras are shown, and bones get priority inside the rectangle, so cameras don't get
  in the way of rigging.
- **Moving and resizing** (with the Transform tool, so Pan, Zoom and the mesh tools
  are unaffected):
  - Drag the edge to move it.
  - Drag a corner or side handle to resize. **Lock** decides what changes:
    - *Scale* (default): resizing changes the resolution, and the character keeps its
      pixel size.
    - *Resolution*: resizing changes the scale, so the frame stays a fixed size, such
      as 512×512.
  - Drag the crosshair to move the pivot.
- **Preview:** while a camera is selected, everything outside it is dimmed, so you can
  play or scrub an animation and see what leaves the frame. A letterboxed full preview
  can come later if it's wanted.
- **Warnings:** if any frame of the chosen animations goes outside the frame, Export
  warns ("walk: frames 8–11 go outside 'Full body'") and offers **Fit to animations**.

### 4.3 Properties panel

Selecting a camera shows its fields in the properties panel:
- name
- position x and y
- resolution w and h
- scale
- pivot x and y
- lock (Scale or Resolution)
- background
- "Round to even" (or a multiple of N): video encoders need even sizes, and some
  engines want powers of two
- buttons: **Fit to animations**, **Fit to rest pose**, **Duplicate**, **Delete**

### 4.4 Creating one

- **Add Render Camera** (View menu, or the toolbar's camera button when there are
  none) creates one fitted to the rest pose, with 8 px padding, at scale 1.
- **Fit to animations** grows it to the union of every frame of every animation, plus
  padding (`render_camera::animation_bounds`). It measures what
  `render_screenshot` draws, texture pivots included.
- If Export opens and the rig has no camera, the camera dropdown shows **Auto (fit
  animations)**. That makes a temporary fitted camera for the export only, which keeps
  today's one-click flow working. The dialog then offers **Save as camera**.

### 4.5 Later: follow and animate

- **Follow bone** (Phase 2): the camera's position tracks a bone's x and/or y, so
  root-motion walks stay in frame.
  - The pivot moves inside the frame, so the manifest records the pivot per frame.
  - For in-place game sprites, the usual choice is to leave follow off and let the
    pivot move.
- **Keyframed cameras** (Phase 3): only if cutscene-style renders are wanted.

## 5. Shared animation options

| option | values | default |
|---|---|---|
| Camera | The rig's cameras · Auto (fit animations) | The last used, else the first, else Auto |
| Animations | A checkbox per animation, with its FPS and frame count | The current animation |
| Range | Per animation, `start–end` | The whole animation |
| Step | Every frame · every 2nd ("on twos") · every Nth | Every frame |
| Skip last frame | For seamless loops, when the last frame equals the first | Off |
| Output size | Percent of the camera (25 · 50 · 75 · 100 · custom) · Width (px) · Height (px) | 100% |
| Cycles | Video only | 1 |

- **Frame rate is always the animation's own FPS (D6).**
- With a step of N, each image lasts N frames. The manifest and the engine files
  record that, so playback speed stays right.
- **Background:** the camera's, overridable per export. JPG and MP4 always fill with
  the colour.
- **Premultiplied alpha:** off by default.

### 5.1 Output size (D8)

Exports can be resized without touching the camera, which stays the master framing.
The camera's proportions are always kept:
- **Percent** scales both sides, e.g. 50% of a 412×588 camera gives 206×294.
- **Width** or **Height** sets one side in pixels, and the other follows the camera's
  aspect ratio.
- Sizes round to whole pixels. That can shift the aspect by under a pixel. Video
  rounds to even sizes.
- The dialog shows the result: `Output: 206 × 294 px (camera 412 × 588 @ 1.0)`.

**Quality:**
- **Smaller** sizes render at twice the output size (or the camera's full size, if
  that's smaller) and then downsample with a Lanczos3 filter, so thin lines and
  texture detail don't alias. Always rendering at full size made large cameras very
  slow at small output sizes.
- Downsampling happens in **premultiplied alpha** (premultiply, resize, then
  unpremultiply), so transparent edges don't pick up dark fringes.
- **Larger** sizes (up to 400%) render straight at the target size, rather than
  enlarging an image.

**Everything downstream uses the output size:**
- the manifest's `size` and `pivot`;
- its `scale`, as camera scale × output factor;
- spritesheet cells and the atlas JSON rects;
- the Phaser, Godot and Unity values;
- the video resolution.

The renders come out the same as a camera whose scale is multiplied by the factor, so
the pixel art can't drift between sizes.

A **multiple sizes** export (e.g. `@1x` and `@0.5x` side by side, as some engines
and Phaser setups use) is a later option. It would use the same resize step once per
size.

## 6. Per-tab output options

### 6.1 Frame Sequence

| option | values | default |
|---|---|---|
| Output | Folder · Zip | **Folder** (D3); Zip on the web |
| Layout | One folder per animation · All in one folder | One folder per animation |
| Name pattern | Tokens `{anim}` `{frame}` `{index}` | `{anim}_{index}` (D4) |
| Digits | 1–6, zero-padded | 4 |
| Start number | 0 · 1 | **0** (D4) |
| Format | PNG · WebP (lossless) · JPG (quality) | PNG |
| Engine files | Manifest JSON (always) · Phaser · Godot | Manifest + Phaser |

- `{frame}` is the animation frame the image came from, so it isn't contiguous when
  stepping. `{index}` is the image's position in the output.
- If the target already has files matching the pattern, Export asks before
  overwriting. It never removes the folder.

With the defaults, an export into `Exports/` looks like this:
```
Exports/
  walk/walk_0000.png … walk_0023.png
  idle/idle_0000.png … idle_0047.png
  alice2_frames.json          manifest (D5)
  alice2_phaser_anims.json    Phaser animations (§7.1)
```

### 6.2 Spritesheet

- **Layout:**
  - Grid: sprites per row (today), or **Auto**, which picks the most square sheet.
    Every cell is the camera's resolution.
  - **Packed:** one sheet, or several up to 2048 or 4096 px, holding every chosen
    animation.
  - Padding and 1 px extrude between sprites.
- **Engine files:** the manifest, plus the Phaser atlas and animations (§7.1), the
  Godot SpriteFrames (§7.2), and the Unity grid setup (§7.3).

### 6.3 Video

- Unchanged apart from using the shared options and the camera's resolution.
- **WebM with alpha** (VP9) comes later.
- The disabled top-bar "Export Video" item stays disabled.

## 7. Engine targets (D7)

Every export writes the **manifest** (D5), which is Keyternity's own small JSON. Each
engine target then adds the files that engine reads directly.

**Manifest** (`<rig>_frames.json`):
```json
{
  "rig": "alice2.skf",
  "camera": "Full body",
  "scale": 1.0,
  "size": [412, 588],
  "pivot": [206, 540],
  "animations": [
    {
      "name": "walk",
      "fps": 24,
      "loop": true,
      "frames": [
        { "file": "walk/walk_0000.png", "frame": 0, "duration": 0.0417 },
        { "file": "walk/walk_0001.png", "frame": 1, "duration": 0.0417 }
      ]
    }
  ]
}
```
- `pivot` is in pixels from the top left.
- Trimmed frames (Phase 2) add `"offset"` and `"trimmed_size"`.
- With Follow bone, each frame gets its own `pivot`.

### 7.1 Phaser (first)

**Spritesheet** (recommended for Phaser): writes the atlas image(s) plus
`<rig>_atlas.json` in TexturePacker **JSON Hash** format, which Phaser 3 loads with
`this.load.atlas(key, png, json)`.
- Frame names follow the name pattern, e.g. `walk_0000`.
- Each frame has `frame`, `rotated: false`, `trimmed`, `spriteSourceSize` and
  `sourceSize`. The pivot is written as `anchor` (Phaser's custom pivot) and as
  `pivot` (TexturePacker's), both normalised.
- **Packed** uses Phaser's `multiatlas` layout (`textures: [...]`), loaded with
  `this.load.multiatlas`.

**Animations** (`<rig>_phaser_anims.json`): the format `this.anims.fromJSON(...)`
reads.
```json
{ "anims": [ { "key": "walk", "frameRate": 24, "repeat": -1,
  "frames": [ { "key": "alice2", "frame": "walk_0000" }, … ] } ] }
```
- With a step of N, each frame gets `"duration"` so speed is kept.
- `repeat` is -1 for looping animations and 0 otherwise. Animations loop unless
  marked otherwise.

**Frame Sequence + Phaser:** writes the same animations file with one image key per
frame, plus a small `preload` snippet in the export summary. The atlas is still better
for Phaser, so the dialog says so.

A **Grid** spritesheet also works with `this.load.spritesheet(key, png, { frameWidth,
frameHeight })` and no JSON. Every cell is the camera's resolution, so that works.

### 7.2 Godot 4

Writes `<rig>.tres`, a **SpriteFrames** resource that an AnimatedSprite2D uses
directly:
- one animation per exported animation, with `speed` set to its FPS, `loop`, and each
  frame's `duration` (relative; 2.0 for "on twos");
- textures as `ExtResource`s pointing at the frame PNGs, or at `AtlasTexture`
  sub-resources for spritesheets.
- Paths need `res://`. The export asks for the project folder the files will sit in,
  and remembers it per file.
- The pivot can't go in SpriteFrames (it belongs to the node), so the summary and
  manifest give the AnimatedSprite2D `offset`, which is `size/2 − pivot` in Godot's
  coordinates.

### 7.3 Unity

Unity's import setup is in `.meta` files, which are fiddly to write, so Keyternity
doesn't write them:
- **Sequence:** Unity users select the frames and drag them into the scene, which
  creates the animation. The summary gives the Pixels Per Unit (the scale) and the
  custom pivot, normalised.
- **Grid spritesheet:** Sprite Mode = Multiple, Slice → Grid By Cell Size with the
  camera's resolution, and the custom pivot. The summary lists those values.
- Writing `.meta` files can come later, if Unity becomes a real target.

### 7.4 Video and compositing tools

A zero-padded PNG sequence with transparency starting at 0 imports as an image
sequence in After Effects, Premiere, DaVinci and Blender. They need no extra files.

### 7.5 Export summary

After an export, the modal lists:
- the files written;
- for each engine target, the few values to type in (pivot, pixels per unit, offset);
- a copyable Phaser `preload` and `create` snippet.

## 8. Implementation

### 8.1 Data

- `RenderCamera { id, name, pos: Vec2, resolution: Vec2, scale: f32, pivot: Vec2,
  lock, background: Option<Color>, round_to: u32, follow_bone: Option<i32> }`, kept
  in `editor.json`.
- `ExportAnimOptions` (§5) and `SequenceOptions` (§6.1), held in `shared::Ui` while
  the dialog is open, and saved per file (§8.4).
- `exporting_anims`, `sprite_size`, `anim_cycles`, `export_global_bounds` and
  `image_sequences` fold into these.

### 8.2 Frame planning

Add pure functions, tested in `tests/`:

```rust
fn plan_frames(armature, &ExportAnimOptions) -> Vec<AnimPlan>
// AnimPlan { anim_idx, samples: Vec<Sample { frame: i32, duration: f32 }>, loop }
fn camera_view(&RenderCamera) -> (Camera, Vec2 /*px size*/)
// exact mapping, keeps the aspect ratio
fn fit_camera(armature, &[AnimPlan], padding) -> RenderCamera
```

- `plan_frames` handles range, step, cycles and skip-last.
- `camera_view` turns the rectangle into the `Camera` that `take_screenshot` uses, so
  the rectangle fills the output exactly. This fixes today's aspect-ratio bug.
- Engine writers (`export_engines.rs`) take the plan, camera and file names and return
  file contents, so they're testable without a GPU:
  - the Phaser atlas and animation JSON must parse and match the frame names;
  - the Godot `.tres` must be well formed.

### 8.3 Streaming render

- Built for Frame Sequence. Each app frame renders and reads back frames for up to
  about 60 ms. A background `Writer` thread shrinks, encodes and writes them (to a
  folder or zip), then writes the extra files.
- Rendering pauses while the writer has 3 frames queued, so memory stays at a few
  frames and the app stays responsive.
- `check_export`'s 250 ms timer becomes a per-batch poll of `mapped_frames`.
- The exporting modal shows progress, such as `walk 12/24`.
- Peak memory is about one batch, plus the sheet being built.

### 8.4 Remembering settings

- The last export settings go in `editor.json`: tab, camera, animations, options, the
  output path and the Godot `res://` folder.
- **Export Again** (`Ctrl+Shift+E`, Phase 2) re-runs the last export without the
  dialog. Consume it before `Ctrl+E`, because egui's shortcut matching ignores extra
  modifiers.

### 8.5 Canvas

- Cameras are drawn with egui on a background layer clipped to the canvas (dashed
  rectangles, labels, handles, the pivot crosshair, and the dimming), so they need no
  `RenderBuffer`.
- `camera_ui::overlay` hit-tests the pointer itself and reports it as over the UI, so
  the canvas leaves those clicks alone. Drags send `SetCamera` events: the first one
  of a drag makes the undo step.

### 8.6 Agent (later)

- An MCP tool `export_frames` would take the same options as JSON, including the
  camera.
- It could render through the CPU renderer (`agent/render.rs`) when no GPU frame is
  available. The same path enables headless and CLI export.

## 9. Decisions

| # | decision |
|---|---|
| D1 | Frame Sequence gets its own Export tab. Spritesheet takes over the rest of the Image tab. |
| D2 | Framing comes from a new **render camera** canvas helper that sets the render bounds (§4). If the rig has none, Export offers "Auto (fit animations)". |
| D3 | Frame Sequence writes to a folder by default (zip on the web). |
| D4 | File names are `{anim}/{anim}_{index}` with 4 digits, starting at 0. |
| D5 | Every export writes the manifest JSON. |
| D6 | Frames use the animation's own FPS. There's no custom FPS option. Stepping ("on twos") stays, and it records frame durations. |
| D7 | Engines: Phaser first (atlas JSON Hash/multiatlas plus `anims.fromJSON` file). Godot SpriteFrames `.tres`. Unity through a grid sheet plus summary values. Video tools through padded PNG sequences. |
| D8 | Exports can be resized (percent, or a width or height in pixels), always keeping the camera's proportions. Smaller sizes render at full resolution and downsample (Lanczos3, premultiplied alpha). Every output and engine file uses the resized values (§5.1). |

## 10. Phases

| phase | contents |
|---|---|
| **1** | Render cameras: data, canvas drawing and editing, properties panel, Fit, Preview. Frame Sequence tab, and shared options: camera, range, step, skip-last, output size. Folder or zip output, naming, PNG. Manifest. Phaser atlas and animation files, with Grid and Auto spritesheet layout. Overwrite confirmation. Streaming render. Settings saved per file. Export summary. |
| 2 | Godot `.tres`. Packed and multiatlas sheets. Follow bone. Trim. WebP and JPG frames. Export Again. |
| 3 | WebM with alpha. The `export_frames` MCP tool. CLI/headless export. Keyframed cameras, if wanted. Unity `.meta` files, if wanted. |

**To check during Phase 1:**
- Load the atlas in a real Phaser 3 scene to confirm the `anchor` pivot and
  `anims.fromJSON` frame durations.
- Open the Godot `.tres` in Godot 4.
