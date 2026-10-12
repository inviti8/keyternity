# Auto-mesh: tracing topology from the image

**Goal:** replace **Trace** (the auto-mesh in the mesh toolbar) with a method
that:
- follows any outline, including concave ones, separate pieces and holes
- never puts triangles over transparent areas
- fills the inside with evenly sized, well-shaped triangles that deform cleanly

**Status:** steps 1, 2 and 4 of §6 implemented (§0); the preview (step 3)
isn't built yet. Mesh display is in [MESH_DISPLAY.md](MESH_DISPLAY.md).
Decisions are in §7.


## 0. As built

The pipeline is `src/auto_mesh.rs` (`trace`, `trace_alpha`, `TraceOptions`),
with tests in `tests/auto_mesh.rs`. The old `editor::trace_mesh`,
`winding_sort` and `line_of_sight` are gone.

**Departures from §2:**
- **Outlines:** they follow pixel edges (crack following), not interpolated
  marching squares. Simplifying them gives the same result, with no
  interpolation to get wrong. Pixels that touch only at a corner trace as
  separate loops, not connected (§2.2).
- **Constraints:** they're added with `add_constraint_and_split`, so outlines
  that cross after simplifying get split rather than panicking.
- **Refinement doesn't keep the constraint edges.** Splitting outline edges
  gives better triangles near the outline. Faces are kept by an even-odd
  test of each face's centroid against the simplified loops, not by
  `excluded_faces`. That works the same with and without refinement.
- **Refinement caps:** at most `MAX_ADDED_VERTS` (4000) new vertices, and a
  minimum triangle area of (max area ÷ 64).

**Departures from §3 (defaults):** fixed 2 px detail crowded large textures
(154 vertices on the 530×849 sample dress), so the defaults scale with the
texture's shorter side:
- detail = 1% (at least 1 px)
- density = ⅙
- padding = max(2 px, detail), because simplifying can cut up to `detail`
  into the art

The dress now gets 80 vertices, and a 400×832 character 244.

**Outline only** (added after review): a toolbar toggle that traces just the
outline, with no interior vertices, so an artist can cut a shape out and draw
its topology by hand (faces, for example) with the Pen tool. The mesh still
needs triangles (TOPOLOGY_TOOLS.md D8), so the inside is the constrained
triangulation of the outline vertices alone. It's density 0 under the hood,
and Density hides while it's on. `Ui.tracing_outline_only` persists across
Trace sessions.

**Toolbar:** Outline, Detail, Padding, Density and Alpha. The labels drop the
ℹ marker so the row fits beside the mesh actions; the hover text is
unchanged. Each Trace session starts
from the texture's defaults (`Ui.tracing_*` set to `None`), and the toolbar
then shows the values used. Editing a value re-traces, which gives a live
preview of sorts. The minimum piece size isn't exposed and stays at 0.1%.

**Binds (§4):** turning Trace on for a mesh that has binds opens
`PolarId::TraceWithBinds` first.

**Compared on real art:** a 400 px character from the Alice PSD. The old
Trace bridged between the legs and between arm and body, and missed most of
the bag, flower and feather. The new one follows them all.

**Observation:** a traced mesh has many more vertices than the old 4-corner
default. At low zoom, the unchanged vertex markers (MESH_DISPLAY.md D2)
overlap into a solid mass, so the marker size may be worth revisiting.


## 1. Today

`editor::trace_mesh(texture, gap, padding)` (`src/editor.rs`), run by
`Events::TraceBoneVerts`:

1. Steps a grid every `gap` px over the texture (plus `padding`) and keeps the
   points whose pixel is fully transparent.
2. Keeps the points that have at least one non-transparent neighbour, which
   gives a ring of points just outside the art. The neighbour test uses
   `poi_clone.contains`, which is O(n²), so small gaps are slow.
3. Orders the ring with `winding_sort`, **by angle around the centroid**.
4. Thins the ring with forward and reverse line-of-sight passes. The wrap index
   is `(p + 1) % (poi.len() - 1)`, which never reaches the last point.
5. Triangulates the remaining points with an **unconstrained** Delaunay
   triangulation, then keeps the triangles where at least one sampled pixel
   has alpha > 125.

Trace then replaces the mesh, **clears all binds**, and runs
`cleanup_vertices`.

### Why results are mixed
- **Angle sorting only works for star-shaped outlines**, roughly convex ones
  seen from the centroid. Arms, legs, hair and any deep inward curve get their
  points jumbled, which zig-zags the outline. This is the main cause.
- **One outline only.** Separate pieces end up in one jumbled ring, and holes
  (such as the gap between an arm and the body) aren't represented.
- **No boundary constraint.** Delaunay triangles can bridge inward curves.
  The alpha test then either keeps the bridge (mesh over transparency) or
  drops it (missing chunks), depending on what the triangle happens to cover.
- **Outline vertices only.** With nothing inside, triangles are long slivers
  that bend badly when bound.
- The alpha thresholds are fixed, and inconsistent with each other: 0 for the
  outline and 125 for keeping triangles.


## 2. Method

This is the pipeline Godot (`BitMap.opaque_to_polygons`), Unity (sprite
outlines) and Spine (Trace) broadly share, plus interior refinement. It uses
only crates we already have: `image` and `spade` 2.15.

### 2.1 Mask
- **Opaque** means alpha ≥ the alpha threshold (default 16, out of 255).
- **Dilate** the mask by `padding` px, so the outline sits just outside soft,
  anti-aliased edges. The texture is clamped to its bounds as today, so
  padding past the texture edge is cut off at the edge.
- Pad the mask with a 1 px transparent border, so every outline closes.

### 2.2 Outlines (marching squares)
Trace every boundary between opaque and transparent with **marching squares**.
This gives ordered, closed outlines, one per separate piece and one per hole,
by construction.
- Resolve the saddle cases (two diagonal pixels) consistently, as connected.
- Throw away pieces and holes smaller than the minimum area (default 0.1% of
  the texture area), so specks and tiny gaps don't become geometry.

### 2.3 Simplify (Ramer–Douglas–Peucker)
Simplify each outline with RDP at a tolerance of `detail` px (default 2):
lower is more faithful, higher means fewer vertices. For closed loops, split
each loop at its two farthest-apart points first. Then drop a loop that ends
up with fewer than 3 points.

### 2.4 Constrained triangulation
Insert all outline points and edges into a spade
`ConstrainedDelaunayTriangulation`, the same type the Pen tool uses
(`src/pen.rs`). Outline edges are constraints, so no triangle can cross a
boundary.

### 2.5 Interior refinement
Run spade's `refine` with:
- `exclude_outer_faces(true)`: faces outside the art, **and holes** (by
  odd/even winding), are excluded, and returned as `excluded_faces`.
- `with_angle_limit(AngleLimit::from_deg(25.))`: no slivers.
- `with_max_allowed_area(density²)`, where `density` is a target edge length
  in px. It defaults to an eighth of the texture's shorter side. A density of
  0 skips refinement, which gives an outline-only mesh like today's.
- `keep_constraint_edges()`: the simplified outline stays exactly as it is.

Emit every face that isn't in `excluded_faces`.

### 2.6 Output
Build vertices and indices:
- uv = point ÷ texture size
- pos = (x − w/2, h/2 − y), the same space as the default quad
  (`renderer::create_tex_rect`), so the art stays where it was. The old
  `trace_mesh` called `center_verts` here, which centered the mesh on the
  traced outline and moved any art that wasn't centered in its texture.

`trace_mesh` keeps its signature, apart from taking a `TraceOptions` struct
instead of `gap` and `padding`, so `Events::TraceBoneVerts` barely changes.


## 3. Options

These go in the mesh toolbar's options area, where Trace's gap and padding
inputs are today.

| Option | Default | Replaces |
|---|---|---|
| Detail (px) | 2 | Gap (today 25) |
| Padding (px) | 2 | Padding (today 50). Same name, new meaning: today it only extends the sampling grid past the texture edge; now it's how far the outline sits outside the art |
| Density (px, 0 = outline only) | ⅛ of the shorter side | — |
| Alpha threshold | 16 | the fixed 0 and 125 |
| Minimum piece size | 0.1% of the texture | — |

The values stay in `Ui`, like `tracing_gap` and `tracing_padding` today, so
they aren't saved per file.

**Preview** (step 3 in §6): while the Trace options are open, show the
resulting mesh as an overlay in the edge style from MESH_DISPLAY.md. Trace
applies it. The pipeline runs fast enough on typical textures (≤ 2048 px) to
recompute when an option changes, debounced.


## 4. Binds

Trace replaces the topology, so binds can't carry over. Today it clears them
silently. New: if the mesh has binds, Trace asks first ("This replaces the
mesh and removes its binds"), using the same yes/no confirmation as
Re-triangulate.


## 5. Tests (`tests/auto_mesh.rs`)

Synthetic RGBA masks, built in the test:

| Shape | Checks |
|---|---|
| Square | 1 outline; outline-only mode gives 2 triangles |
| L shape (concave) | No triangle centroid falls in the cut-out corner |
| Ring (hole) | No triangle centroid falls in the hole; the hole's outline is kept |
| Two separate squares | Both covered; no triangle spans the gap |
| Single-pixel specks | Dropped by the minimum piece size |
| Fully transparent | Empty mesh, so Trace shows its existing error modal |

For every shape:
- every triangle's centroid is on the dilated mask
- the smallest angle is at least the limit (except triangles at outline
  corners sharper than it)
- indices are in range
- no degenerate triangles

Manual: compare old and new Trace on real art (the Alice PSD layers, and the
samples) at the default options.


## 6. Work plan

1. **Pipeline (§2.1–2.6) with tests (§5).** A new `src/auto_mesh.rs`. Trace
   switches to it with fixed defaults. *Shippable on its own.*
2. **Options (§3)** in the toolbar's options area, and the bind confirmation
   (§4).
3. **Preview** overlay.
4. **Docs.** README entry and an as-built section here.

Possible follow-up, not in this scope: **bone-aware density**, with tighter
triangles where bones meet so joints bend better, using the bind bones'
positions as extra refinement hints.


## 7. Decisions

| # | Decision |
|---|---|
| D1 | Marching squares + RDP + constrained Delaunay + spade `refine`. No new crates. |
| D2 | Separate pieces and holes are supported. Pieces below the minimum size are dropped. |
| D3 | Trace keeps replacing the mesh and clearing binds, but asks first when there are binds. |
| D4 | Several separate pieces in one texture all go into the one mesh. |


## 8. Open questions

None at the moment.
