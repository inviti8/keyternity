# Canvas toolbar and topology tools

**Goal:** add a toolbar above the canvas that holds the everyday tools: navigation
(Pan, Zoom), the transform tools (Move, Rotate, Scale) and two new mesh tools, a
**Pen** for creating topology and an **Eraser** for removing it. To make the Pen and
Eraser meaningful, the triangles a user draws become the mesh: the editor stops
re-triangulating the whole mesh on every vertex edit.

**Status:** steps 1–3 of §10 implemented (§0). Decisions are in §11.


## 0. As built

**Step 1 (model change)** is in `src/topology.rs`, with tests in `tests/topology.rs`.
- Click-to-add splits a triangle into 3, or the 1–2 triangles on an edge into 2 each.
  Right-click on a vertex dissolves it, and right-click on a triangle removes it.
  The blacklist and edit-time `sort_vertices` are gone. Re-triangulate is
  `Events::RetriangulateVerts`, behind a yes/no confirmation.
- **Departure from §5.5:** new vertices only get interpolated weights on
  **bind-posed** meshes. Classic binds store `Vertex.pos` in the bind bone's frame,
  so a new vertex added to a classic bind without that offset would jump. Those
  vertices stay unbound, as before.
- The DragonBones round-trip test from §9 isn't written yet: the import tests
  need external sample rigs and are `#[ignore]`d. The "untouched triangles
  survive a split" property is covered on a synthetic grid instead.

**Step 2 (toolbar shell)** is `toolbar()` in `src/ui.rs`, drawn as a
`TopBottomPanel` after the side panels.
- Move, Rotate and Scale work. The snap hints moved to the options area, and
  `edit_mode_bar` is gone.
- Trace, Center, Reset and Re-triangulate appear while editing a mesh, and the
  Trace gap and padding inputs moved to the options area. The bone panel keeps
  the Edit Vertices toggle and the bind UI.
- Pan, Zoom, Fit, Pen and Eraser are shown **disabled** ("Not available yet")
  until steps 3–5. Their shortcuts are added with them.
- `Tool` is on `EditMode`. Move, Rotate and Scale are `Tool::Transform` plus
  `EditMode.current`.
- Icons: `assets/toolbar_icons.svg` is the placeholder art. It's exported to
  `toolbar_icons.png` (64 px cells) and tinted with the theme's text color.

**Step 3 (navigation)** is in `src/navigation.rs`, with tests in
`tests/navigation.rs`. The tests check the maths against the renderer's own
`world_vert`, in all three layouts.
- Pan (H) and Zoom take the left button: `navigate` runs before the renderer,
  which then sees the left button as released, so these tools can't select or
  edit anything. Right-drag still pans in every tool. The cursor shows a
  grab/grabbing hand for Pan and zoom-in/zoom-out for Zoom.
- Zoom, scroll and `=`/`-` are multiplicative (×1.25 a step; scroll is
  `exp(-0.001 · delta)`). The Zoom tool and scroll are anchored on the cursor.
  This replaces the per-layout x nudge in `CamZoomScroll`. The layouts' fixed
  camera offset (`world_camera`) is a pure translation, so it cancels out of
  anchoring.
- Fit (Home) frames the selected bone, or every visible bone, inside
  `ctx.available_rect()` after all panels (stored as `Ui.canvas_rect`), with a
  20% margin. A bone with no texture is centred at the current zoom.
- The Zoom tool's options area shows the zoom as a percentage (100% is a new
  armature's 2000), and it can be edited.

> SkelForm facts below are from this repo's `src/` at `de5b6c31`. Blender behaviour
> is from the Knife tool (`K` in Edit Mode), which the Pen is modelled on.


## 1. Today

### 1.1 Mesh data

A mesh is `Bone.vertices` (`id`, `pos` in bone space, `uv` in 0..1 texture space)
plus `Bone.indices`, a flat triangle list. There are no stored edges: edges only
exist as sides of triangles.

### 1.2 Topology is derived, not authored

Every vertex add or delete throws the triangles away and rebuilds them:

- `Events::NewVertex` and `Events::DeleteVertex` (`src/editor.rs`) call
  `sort_vertices` (reorders the vertex array by angle around the centroid), then
  `triangulate` (`src/editor.rs:1925`): a full `spade` Delaunay over the vertex
  UVs, dropping triangles that cover no opaque pixel.
- Right-clicking a triangle (`Events::DeleteTriangle`) doesn't remove it. It adds
  the triangle's three vertex ids to `Bone.blacklist`, and `remove_blacklisted_tris`
  filters it out after every rebuild. The blacklist is saved in the editor sidecar
  (`EditorBone.blacklist`, `src/utils.rs:889`), not in the format. Trace and
  Reset clear it.

Consequences:
- **Imported topology doesn't survive editing.** A DragonBones mesh arrives with
  hand-made triangles. Adding one vertex re-Delaunays the whole mesh.
- **You can't shape the mesh.** You can't choose where an edge goes, only where
  vertices go, and Delaunay picks the edges.
- **The mesh can't grow outward.** New vertices are only created by clicking inside
  an existing triangle (`src/renderer.rs:218`) or on an existing edge
  (`vert_lines`). There's no way to place a vertex outside the current mesh.
- **Deleting a triangle is fragile.** The blacklist matches triangles by vertex
  id. If those three vertices end up in a different triangle after a rebuild, the
  deletion is lost.

### 1.3 Canvas UI

- Canvas controls are floating `egui::Window` bars: `edit_mode_bar` (Move,
  Rotate, Scale plus snap hints), `bone_pivot_bar`, `animate_bar`, the render bar
  and the camera bar. Each one is positioned by hand for the Split, Right and Left
  layouts (`src/ui.rs:290-360`).
- Mesh actions (Edit Vertices `V`, Trace with gap and padding, Center, Reset) are
  in the bone panel's Mesh Deformation section (`src/bone_panel.rs:700-830`).
- Navigation: scroll zooms (not anchored on the cursor), `=` and `-` zoom, right-drag
  pans (`src/renderer.rs:593`). The camera bar has numeric X, Y and zoom.
- Icons: the app has one icon strip, `assets/anim_icons.png`. It's cropped into
  200 px squares at load (`src/ui.rs:105-135`). That is the pattern to reuse.


## 2. Toolbar

### 2.1 Placement

An `egui::TopBottomPanel::top("toolbar")`, added **after** the side panels and the
armature panel so it only spans the canvas. It sits directly under the menu bar.
Because it is a real panel rather than a floating window:
- it is opaque and part of `camera.on_ui` hit-testing through
  `is_pointer_over_area`, with no extra work
- the floating bars that anchor to `top_panel_rect.bottom()` (anim bar, pivot bar)
  anchor to `toolbar_rect.bottom()` instead. That's one rectangle replacing another
  in the three layout branches.
- height is about 32 px. Groups wrap onto a second row on narrow windows
  (`horizontal_wrapped`).

### 2.2 Contents

```
[Move Q][Rotate W][Scale E] | [Pan H][Zoom] [Fit] | [Pen][Eraser] | [Trace][Center][Reset][Re-triangulate]    <tool options>
```

- **Transform group:** replaces `edit_mode_bar`. The snap and aspect-ratio hints
  that bar shows while dragging move into the tool options area (§2.4).
- **Navigation group:** §3.
- **Topology group:** Pen and Eraser (§5, §6). Enabled when the selected bone has a
  texture. Hover text explains why they are disabled otherwise. Picking either one
  turns on `editing_mesh`.
- **Mesh actions:** Trace, Center, Reset and the new Re-triangulate (§7), moved out
  of the bone panel. Shown only while `editing_mesh`. Bind and weight controls stay
  in the bone panel: they are per-bone data, not tools.

### 2.3 Tool state

```rust
pub enum Tool { Move, Rotate, Scale, Pan, Zoom, Pen, Eraser }   // on EditMode
```

- `Move`, `Rotate` and `Scale` keep driving the existing `EditModes` value, so
  `EditMode.current`, `temporary` and all the transform code are unchanged. The
  toolbar is just a new way to set them.
- Picking a tool goes through events (`events.set_tool(t)`), like everything else, so
  the change is recorded and keyboard and toolbar take the same path.
- Leaving Pen with pending cuts **discards** them, the same as Esc. Only Enter
  applies.
- Pen and Eraser are unavailable in Pose Mode (`edit_mode.pose_mode`), because
  topology is rest-pose data. Picking another bone while one of them is active
  switches back to Move if the new bone has no texture.

### 2.4 Tool options area

Right-aligned in the toolbar, it shows the active tool's settings:

| Tool | Options |
|---|---|
| Move / Rotate / Scale | snap and ratio hints while dragging (what `edit_mode_bar` shows now) |
| Zoom | zoom % (editable), Fit |
| Pen | Snap ☐, Angle constraint ☐, live hint ("Enter: apply · Esc: cancel · RMB: new cut") |
| Eraser | Dissolve ◉ / Delete ○ |
| Trace (while active) | Gap, Padding (move from the bone panel) |

### 2.5 Icons

- Placeholder art for now: simple line icons in the style of `skf_icons.svg`,
  drawn as part of this work and meant to be replaced later.
- A new strip, `assets/toolbar_icons.svg`, exported to `toolbar_icons.png`. It's
  loaded like `anim_icons`: cropped into fixed squares at startup and kept as
  `shared_ui.toolbar_icons: Vec<TextureHandle>`.
- Icons are white on transparent, tinted with `config.colors.text`, so every theme
  works without extra art.
- Drawn at 20 px in a 28 px button. The selected tool gets the `selection_button`
  look already used by `edit_mode_bar`. Hover text gives the name and shortcut.
- 13 icons: move, rotate, scale, pan, zoom, fit, pen, eraser, trace, center, reset,
  re-triangulate, plus snap for the options area.

### 2.6 Shortcuts

All defaults go in `KeyboardConfig` and are rebindable in Settings › Keyboard.
These are the free keys today:

| Tool | Default | Note |
|---|---|---|
| Move / Rotate / Scale | Q / W / E | unchanged |
| Pan | H | "hand", as in Photoshop and Figma |
| Zoom | (none) | `=` / `-` already zoom |
| Fit | Home | |
| Pen | K | Blender's knife key |
| Eraser | Shift+K | |

`Space` is Play and `Z`/`X` are next/previous bone, so the usual Space-to-pan and
Z-for-zoom aren't available. Right-drag pan stays in every tool.


## 3. Navigation tools

- **Pan (H):** left-drag pans. It reuses `events.edit_camera` exactly as right-drag
  does today.
- **Zoom:** left-click zooms in, Alt+click zooms out, left-drag left/right scrubs
  zoom. All of them are **anchored on the cursor**: the world point under the mouse
  stays put. Scroll zoom changes to cursor-anchored as well, replacing the per-layout
  x nudge in `Events::CamZoomScroll`. The zoom step becomes multiplicative (×1.25),
  so it feels the same at every zoom level. Right now it's ±10 on a value of about
  2000.
- **Fit (Home):** frames the selected bone, or the whole armature if nothing is
  selected. It uses the bounds of `world_verts` (textured bones) and bone
  positions, with a 10% margin.


## 4. Topology model change (decided: option A)

**Rule: `indices` is the mesh.** Editor operations change it locally and never
rebuild it from scratch implicitly.

| Operation | Today | After |
|---|---|---|
| Click inside a triangle (Move tool, editing mesh) | add vertex, sort, full Delaunay | add vertex, **split that triangle into 3** |
| Click on an edge | add vertex, sort, full Delaunay | add vertex, **split the 1–2 triangles on that edge into 2 each** |
| Right-click a vertex | delete, sort, full Delaunay (fills the hole) | **dissolve** the vertex (§6.3): remove it and fill its hole, touching only its neighbours |
| Right-click a triangle | blacklist it | **remove it from `indices`** |
| Drag vertices | move | unchanged |
| Trace / Reset | generate a mesh | unchanged (they generate a new mesh on purpose) |
| Re-triangulate (new) | — | the old behaviour as an explicit button (§7) |

Supporting changes:
- **No more `sort_vertices` on edits.** It reorders the vertex array, which only
  worked because indices were always rebuilt afterwards. Vertex order is now stable
  and new vertices are appended. (`create_tex_rect` and Trace can keep sorting,
  since they build fresh meshes.)
- **Remove `blacklist`.** Delete `Bone.blacklist`, `remove_blacklisted_tris` and
  the blacklist id/index remapping in `utils.rs` save. `EditorBone.blacklist`
  stays as `#[serde(skip_serializing)]` so old files still load, and is ignored.
  On load, the saved `indices` already exclude blacklisted triangles, so nothing
  is lost.
- **Minimum mesh is one triangle.** The current `vert_limit` (more than 4 vertices) and
  `indices_limit` (more than 2 triangles) guards come from the rebuild needing a
  quad. Afterwards, an operation is refused (same modals) only if it would leave
  no triangles.
- `cleanup_vertices` (drop vertices no triangle uses, and drop them from binds)
  already does what the new operations need, and runs after each one.

**Format:** unchanged. `.skf` already stores `vertices` and `indices`. Runtimes,
DragonBones export and bind pose read those and nothing else.


## 5. Pen tool (modelled on Blender's Knife)

### 5.1 Interaction

| Input | Effect |
|---|---|
| LMB | place a point. The first click starts a cut. Each later click adds a point and an edge from the previous point. |
| LMB on the cut's first point | close the loop and end this cut |
| double-click | end this cut where it is |
| RMB | end this cut and keep it pending. The next LMB starts a new cut (Blender: RMB / `E`). |
| Backspace | remove the last point |
| Enter | **apply** all pending cuts as one undo step |
| Esc | discard all pending cuts |
| Ctrl (held) | snap to the midpoint of the hovered edge |
| Shift (held) | turn snapping off (free placement) |
| C (toggle) | constrain the segment angle to 45° steps |

While cutting, the canvas shows the pending polyline, a rubber-band segment to the
cursor, the snap target (vertex or edge highlighted), and a **live preview** of the
result: new faces tinted, cut edges drawn. Segments that won't produce any faces
are drawn dashed (§5.4), so the user can see why before pressing Enter.

### 5.2 Placing a point

Each point resolves to a UV (texture space) and a `pos` (bone space):

1. **Snap to vertex** (within the existing vertex hit radius): reuse that vertex.
2. **Snap to edge** (within the line hit width, as in `vert_lines`): a point on
   that edge. `uv` and `pos` are interpolated along it.
3. **Inside a triangle:** barycentric `uv` and `pos`. This is the existing
   `renderer.rs:205-216` maths.
4. **Outside the mesh:** `uv` from the texture's rest mapping, the inverse of
   `create_tex_rect`: `uv = (pos.x / w + 0.5, 0.5 - pos.y / h)`, clamped to
   0..1, and `pos` from that UV. If vertices have been dragged away from their
   rest positions, a free point outside the mesh lands where the *texture* is,
   not where nearby moved vertices are drawn. Snapping avoids this, and it is
   the honest mapping for new texture coverage.

Topology is computed in **UV space**, like `triangulate`. UV is the stable
authored layout: `pos` may have been dragged so that faces overlap.

### 5.3 Applying cuts: the algorithm

One pass, using `spade::ConstrainedDelaunayTriangulation` (already a dependency):

1. Insert the UV of every mesh vertex and every new free point.
2. Add **every existing triangle edge** as a constraint. A CDT keeps all its
   constraints, so every existing triangle that no cut crosses comes back
   exactly as it was.
3. Add each pending segment with `add_constraint_and_split`. Where a segment
   crosses an existing edge, spade inserts a vertex at the crossing. These are
   the knife cuts.
4. Classify the CDT's faces:
   - inside an original triangle (centroid test) → **keep**. This is the
     original surface, now subdivided where it was cut.
   - otherwise, group the remaining faces into regions: connected sets that don't
     cross a constraint edge. A region is **filled** only if it is enclosed by
     the cuts. It must not be connected to the outer (infinite) face, it must
     touch at least one pen segment, and, before the cut, its area must have been
     *outside the mesh*, not a hole in it. The last condition is checked by
     running the same region labelling on the CDT from steps 1–2.
   - everything else → discard.
5. Write back: existing vertices keep their ids and order, new vertices are
   appended with `generate_id`, and `indices` is rebuilt from the kept faces with
   consistent winding.

What this gives:
- **A cut across the mesh** subdivides the triangles it crosses (knife).
- **A closed loop outside the mesh** becomes a new filled region attached to
  nothing (a separate island is allowed).
- **A cut from the mesh boundary, out, and back to the boundary** fills the area
  between the cut and the boundary. This is the "grow the mesh" case.
- **A closed loop inside a hole** fills it. **A line across a hole** doesn't: the
  hole stays a hole, as with Blender's knife, which never creates faces in empty
  space without a closed boundary.

### 5.4 Segments that produce nothing

A dangling open path outside the mesh encloses nothing. With option A it has
nowhere to live: the mesh stores no loose edges (D8). The preview draws it dashed.
On Enter its points are dropped and a toast says so. Nothing else is applied
partially.

### 5.5 New vertices: positions, binds, ids

- **On an existing edge or inside a triangle** (snapped points and knife
  crossings): `pos` is interpolated from the corners. For each **weight** bind,
  the weight is interpolated the same way, and the vertex is added to that bind if
  the result is > 0. This mirrors Blender, which interpolates vertex groups on
  knife cuts.
- **Outside the mesh:** no binds. The user binds them as usual.
- **Path binds** (`is_path`) are ordered vertex lists, so new vertices are never
  inserted into them automatically.
- **Bind-posed meshes** (`docs/BIND_POSE.md`): `pos` is already in the mesh's
  rest frame, so interpolation is correct. There is a test for this (§9).

### 5.6 Rendering

A new `pen_overlay` pass inside the existing `editing_mesh` block of
`renderer.rs`. It reuses `vert_lines`-style quads for segments and `draw_point`
for points, with no new GPU pipeline. The live preview runs §5.3 on a copy of
the bone each time a point is added or the cursor moves to a different snap
target. Meshes are small (tens to hundreds of vertices), so this is well under
1 ms. It's cached between points.


## 6. Eraser

### 6.1 Interaction

- Hover highlights what will be erased. **Vertex has priority over edge**, using
  the same hit tests as today.
- **Click** erases the highlighted element. **Drag** is a stroke that erases
  everything the cursor passes over. One stroke is one undo step.
- The mode is set in the tool options: **Dissolve** (default) or **Delete**.
  **Ctrl** held switches to the other mode for this click or stroke.

### 6.2 Delete (removes surface, like Blender's Delete Vertices / Edges)

- **Vertex:** remove every triangle that uses it, then `cleanup_vertices`.
- **Edge:** remove the 1–2 triangles on it, then `cleanup_vertices`.

### 6.3 Dissolve (removes topology, keeps surface)

- **Vertex:** take the ordered ring of neighbours. Remove the vertex and its
  triangles. Fill the ring polygon with a CDT whose constraints are the ring edges,
  keeping only the faces inside the ring. On a boundary vertex the fan is open: the
  polygon is the fan path closed by a chord between its two ends, and the chord is
  only accepted if it stays outside existing faces. If it can't, fall back to
  Delete.
- **Interior edge:** merge its two triangles into a quad and re-split it along
  the other diagonal (an edge flip). If the quad is concave the flip is invalid:
  the edge is refused with a brief red flash.
- **Boundary edge:** nothing to merge with, so it acts as Delete.

Binds need no work: removed vertices leave binds through `cleanup_vertices`.
Dissolve creates no vertices.

### 6.4 Right-click in the Move tool

Today, right-click on a vertex deletes it and Delaunay fills the hole. Under §4,
right-click becomes **Dissolve vertex**, the closest match to that behaviour.
Right-click on a triangle removes it. The Eraser is the full tool and right-click
is the shortcut.


## 7. Mesh actions in the toolbar

- **Trace:** unchanged behaviour. Gap and Padding move to the tool options area
  while tracing is active.
- **Center / Reset:** unchanged, moved from the bone panel.
- **Re-triangulate (new):** the old automatic behaviour on demand. It keeps the
  vertices, runs `triangulate` (Delaunay plus the opaque-pixel filter) and
  replaces `indices`. A confirmation modal warns that hand-made topology will be
  replaced.

The bone panel's Mesh Deformation section keeps the Edit Vertices toggle and the
bind and weight UI.


## 8. Undo

Every topology change is one `new_undo_bone` snapshot, taken **before** the change
(the same mechanism as `NewVertex` today): one pen apply (Enter), one eraser click
or stroke, one dissolve. Pending pen points are tool state, not armature state.
Backspace removes them and undo doesn't see them.

The existing `undo_actions.pop()` workaround in `NewVertex` ("remove drag vertex
action, since it's always triggered") becomes unnecessary for the Pen, because the
Pen never starts a vertex drag. It stays for click-to-add in the Move tool.


## 9. Tests

The topology operations are pure functions on `Bone`, in a new `src/topology.rs`.
They are tested in `tests/topology.rs` with no UI:

- `split_triangle`, `split_edge`: triangle count, vertices appended, ids
  stable, interpolated binds.
- `apply_cuts`:
  - a cut across a quad gives the expected faces, with untouched triangles
    byte-identical
  - a closed loop outside the mesh gives a filled island
  - a boundary-to-boundary loop grows the mesh
  - a line across a hole leaves the hole
  - a dangling segment changes nothing
- `delete_vertex`, `delete_edge`, `dissolve_vertex` (interior and boundary),
  `dissolve_edge` (convex flip, concave refusal); the at-least-one-triangle guard.
- Bind integrity: after every operation, every bind vertex id exists and weights
  are unchanged for vertices that existed before.
- Round trip: an imported DragonBones sample keeps its exact `indices` after a
  split, then save and reload (the regression this design exists to fix).
- Bind pose: split an edge on a bind-posed mesh. The new vertex renders at the
  interpolated rest position (reuses `tests/bind_pose.rs` helpers).
- The existing `tests/dragonbones_export.rs` and `tests/bind_pose.rs` stay green.


## 10. Work plan

1. **Model change (§4).** `src/topology.rs` with split, delete, dissolve and cleanup.
   Rewire `NewVertex`, `DeleteVertex` and `DeleteTriangle`. Remove the blacklist
   and edit-time `sort_vertices`. Add the Re-triangulate event. Tests. *Shippable
   on its own: fixes imported topology being overwritten.*
2. **Toolbar shell (§2).** Panel, `Tool` enum and events, placeholder icon art and strip, shortcuts,
   i18n strings (`assets/i18n/en.json`), transform group replacing `edit_mode_bar`,
   re-anchored bars for all three layouts, mesh actions moved from the bone panel.
3. **Navigation (§3).** Pan, Zoom (cursor-anchored, multiplicative), Fit.
4. **Eraser (§6).** Hover, click, drag stroke, Delete and Dissolve.
5. **Pen (§5).** Point placement and snapping, overlay, `apply_cuts` with live
   preview, knife key handling.
6. **Docs.** Update `readme` / user docs with the toolbar and tool shortcuts, and an
   as-built section here.

Steps 1 and 2 are independent and can be built in parallel. 4 and 5 need both.


## 11. Decisions

| # | Decision |
|---|---|
| D1 | Authored `indices` are the mesh (option A). Automatic Delaunay only on explicit Re-triangulate, Trace or Reset. |
| D2 | The Pen works like Blender's Knife: each click adds a vertex with an edge from the previous one, and cuts apply on Enter. |
| D3 | The Eraser removes vertices and edges. |
| D4 | The toolbar holds navigation (Pan, Zoom), Move, Rotate, Scale and the mesh actions as well as the two new tools. |
| D5 | Toolbar buttons use icons. Placeholder art is drawn as part of this work. |
| D6 | The Eraser defaults to Dissolve. Ctrl switches to Delete. |
| D7 | Leaving the Pen with pending cuts discards them. Only Enter applies. |
| D8 | No loose edges: the mesh stays triangles only, and dangling pen lines are dropped. |
| D9 | Shortcuts: Pen K, Eraser Shift+K, Pan H, Fit Home. |
| D10 | The Edit Vertices toggle (`V`) stays in the bone panel for now. |


## 12. Open questions

None at the moment.
