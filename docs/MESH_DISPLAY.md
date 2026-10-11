# Mesh display: thin edges and weight-colored bind vertices

**Goal:** make meshes easier to read and work with on top of the art.
1. Edges draw as thin dotted lines by default, instead of thick solid green ones.
2. While a bind is selected, each of its vertices gets its own hue. The same
   hue marks its row in the bind panel, and the vertex's weight sets how
   saturated the color is.

**Status:** implemented (§0). Auto-mesh tracing is a separate design, in
[AUTO_MESH.md](AUTO_MESH.md). Decisions are in §6.


## 0. As built

**Edges (§2)** are `renderer::vert_lines`, which now returns `EdgeGeometry`:
- patterned quads (outlines first, then lines), drawn with the pattern bind
  group from `edge_bindgroup`
- solid quads for the hovered or selected edge, drawn in the generic batch
  with the vertices

Details:
- Picking is `mesh_tools::pick_edge` (nearest edge within `EDGE_PICK_PX`, plus
  where along it the cursor projects). Click-to-add uses that projection
  instead of the old distance ratio.
- The geometry is `mesh_tools::edge_quad`. Pattern textures come from
  `renderer::edge_pattern_pixels` and `create_pattern_bind_group` (repeat
  sampler, one texel per point). Edges are deduplicated with
  `topology::edges`.
- Config: `mesh_edge_style`, `mesh_edge_width`, `mesh_edge_outline`, and the
  theme color `mesh_edge`. They're under Settings › Rendering and in the
  Colors list.
- **Departure from §2.1:** widths, dash lengths and the pick tolerance are in
  **UI points**, not physical pixels. On a high-DPI display, 1 physical px was
  barely visible. `Camera.px_scale` (UI scale × display scale, set each frame)
  converts them.
- **Fix found on the way:** the edit overlay and the wireframe view both wrote
  `meshframe_buffer` in one frame, so with the wireframe view on, the second
  write replaced the first. The wireframe now has its own buffers
  (`wireframe_buffer`, `wireframe_edge_buffer`), and the edges have
  `edge_buffer`. These mesh buffers hold 60k vertices, up from 5k, so dense
  traced meshes fit.

**Vertex size** (added after review, relaxing D2): Settings › Rendering ›
*Mesh vertex size*, a multiplier (0.1–3, default 1) on `center_point_radius`
for mesh vertex markers only (`Config.mesh_vert_size`). Dense traced meshes
crowd at low zoom. Bone center points and the Pen and Eraser highlights keep
their size.

**Bind colors (§3)** are in `renderer::bone_vertices`:
- `mesh_tools::bind_vert_color` and `bind_hue` hold the 12 `BIND_HUES`, which
  stay more than 35° away from green.
- A selected weight-colored vertex is a white point at 1.45× drawn under it.
- Setting: `bind_weight_colors`.
- `add_point` now offsets indices by the vertices already added, rather than
  assuming 4 per mesh vertex.

The panel swatches, row highlight and scroll-to-row are in
`bone_panel::mesh_deformation`. Scroll happens once per hovered vertex, and
not while the pointer is over the panel.

**Checked in the app:** dotted edges and outline, hover (solid, brighter),
click-to-add on an edge, a 5-vertex bind with weights 0, 0.45 and 1, the
canvas-to-row highlight, and the selection outline.


## 1. Today

### 1.1 Edges
`renderer::vert_lines` (`src/renderer.rs`) draws each edge of the mesh being
edited as a solid quad:
- width `2 · (zoom / 500) / zoom`, a constant 0.004 in renderer space, which is
  about 2–4 px depending on the window
- color `mesh_base − (125, 125, 125)`, so dark green, at alpha 200 (100 when not
  editable)
- de-duplicated with a linear `lines.contains` search per edge, which is
  O(edges²). `topology::edges` already does this properly.

The same quad is the **hit target**. Hover, edge select (press) and
click-to-add-a-vertex (`VertSite::Edge`) all test the mouse against the
quad's two triangles, so a thinner line would also be harder to click.

The read-only mesh wireframe view (`renderer.render_mesh_wf`) calls the same
function and recolors the result with the bone's group color.

### 1.2 Vertices
`renderer::bone_vertices` draws each vertex with `draw_point`:
- `mesh_base` green, rotated 45° (a diamond)
- `bound_vert` (orange by default), as an unrotated square, when it's in the
  selected bind
- white when selected
- 1.5× size when hovered

The vertex markers **stay as they are** (shape, size, hover), apart from the
bind colors in §3.

### 1.3 Bind panel
`bone_panel::mesh_deformation` lists the selected bind's vertices as `#id:`
followed by a 0–1 weight slider. Hovering a row sets `hovering_vert_id`, which
enlarges that vertex on the canvas. Nothing goes the other way: hovering a
vertex on the canvas doesn't point to its row.


## 2. Edges

### 2.1 Look
New settings under **Settings › Rendering › Mesh edges**:

| Setting | Default | Notes |
|---|---|---|
| Style | Dotted | Dotted, Dashed or Solid |
| Width | 1 px | 1–4 px |
| Color | `mesh_edge`, a new theme color | Light green with alpha. No longer derived from `mesh_base` |
| Outline | On, low alpha (D5) | A 1 px dark halo either side, so edges read on both light and dark art |

Widths and dash lengths are in **screen pixels**, so lines and the pattern stay
the same on screen at any zoom. Pixels convert to renderer space with the
window size, as `draw_point` already does for the vertex radius.

Patterns (on/off, in px): dotted 2/3, dashed 6/4.

Edge states:
- **Normal:** the chosen style.
- **Hovered or selected:** solid, 1 px wider, and brightened by the same
  `add_color` as today. Hover and selection read clearly against the dotted
  default.
- **Not editable** (the wireframe view): same style, in the group color, at
  today's lower alpha.

### 2.2 Drawing the pattern
Each edge stays **one quad**. The pattern comes from a small repeating
texture in a second bind group that uses a repeat sampler. Each quad's `uv.x`
runs from 0 to (edge length in px ÷ pattern length), so the texture repeats
along the edge. The pattern starts at the edge's lower vertex index, so edges
stay stable from frame to frame.

Fallback, if the extra bind group turns out awkward: split each edge into
dash quads on the CPU. That's simpler, but the quad count grows with zoom, and
a large mesh at high zoom could reach tens of thousands of quads per frame.

### 2.3 Hit-testing, separate from the look
Hover, select and click-to-add test the **distance from the mouse to the edge
segment in screen pixels**, with a tolerance of 6 px, however thin the line
is drawn. Vertices keep priority over edges, as today (`hovering_vert`).

The add-vertex position along the edge stays as it is: the mouse projected
onto the segment, and the uv interpolated.

### 2.4 Code shape
`vert_lines` splits into:
- `mesh_edges(bone)`: the unique edges, via `topology::edges`
- `pick_edge(edges, world_verts, mouse, camera)`: the nearest edge within the
  tolerance. It can live in `mesh_tools.rs`, next to `pick`, so it can be
  unit-tested without a renderer.
- `edge_quads(...)`: the geometry, with style, width and state

The hover, select and add-vertex behavior moves into the caller unchanged.


## 3. Bind vertices colored by weight

### 3.1 Color
While a bind is selected, vertex *i* of that bind (its row position in the
panel) gets hue `palette[i mod N]`:
- The palette is a fixed list of about 12 hues, ordered so neighbouring rows
  contrast.
- It leaves out hues close to `mesh_base` green, so unbound vertices still
  read as unbound.

The marker color comes from that hue, with the weight setting saturation:

```
saturation = 0.25 + 0.75 · weight     (weight clamped to 0..1)
value      = 1.0
```

So a weight-0 vertex is a 25% tint of its hue, and a weight-1 vertex is the
full hue. Dragging a weight slider updates the marker live.

- Vertices **not** in the selected bind keep the green diamond.
- Bound vertices keep the unrotated square shape, so shape still says "bound"
  and color says "which one, and how much".
- With no bind selected, nothing changes.
- **Rendering › Color bind vertices by weight**, on by default. Off falls back
  to today's single `bound_vert` color.

### 3.2 Panel
Each row gets a swatch before `#id`:
- a small square filled with the same weight color as the marker
- a 1 px border in the **full** hue, so the row's hue still shows at weight 0

### 3.3 Linking both ways
- Hovering a row enlarges the vertex, as today.
- **New:** hovering a vertex on the canvas highlights its row and scrolls it
  into view (`scroll_to_me`). `hovering_vert_id` already holds what's needed.

### 3.4 Limits
- A bind with more than N vertices reuses hues. The hover link (§3.3) tells
  them apart.
- Pale low-weight tints can look alike. The hover link tells them apart.

### 3.5 Selection
Selected vertices turn white today, which is close to a weight-0 tint. For
vertices **in the selected bind**, selection instead draws a white outline
around the colored marker (D6). Every other vertex still turns white.


## 4. Tests

- **`tests/mesh_tools.rs`:**
  - `pick_edge` returns the nearest edge within the tolerance and nothing
    beyond it
  - vertices win over edges
  - the tolerance is in pixels, so it holds at different zoom levels
- **Color mapping:**
  - weight 0 gives saturation 0.25 and weight 1 gives 1.0
  - out-of-range weights are clamped
  - palette index wraps
  - no palette hue is within the green exclusion band
- **Manual pass:**
  - edit and wireframe views on light and dark art, at 25%, 100% and 800% zoom
  - click-to-add on an edge at each zoom
  - a bind with more than 12 vertices


## 5. Work plan

1. **Edge picking (§2.3–2.4).** `pick_edge` plus tests. Rewire hover, select and
   add-vertex onto it, still drawing today's quads. *No visible change, and
   shippable on its own.*
2. **Edge look (§2.1–2.2).** Settings, `mesh_edge` color, repeat-sampler pattern,
   edge states, wireframe view. Defaults per D1.
3. **Bind colors (§3).** Palette and weight mapping, markers, panel swatches,
   canvas-to-row link, setting.
4. **Docs.** README entry and an as-built section here.


## 6. Decisions

| # | Decision |
|---|---|
| D1 | Edges default to thin (1 px) dotted lines, in both mesh editing and the wireframe view. |
| D2 | Vertex markers keep their shape, size and hover behavior. |
| D3 | Bind vertices: hue from the row position, saturation from the weight (25% at weight 0, 100% at weight 1). |
| D4 | Edge clicking uses a screen-pixel tolerance, separate from the drawn width. |
| D5 | The dark edge outline is on by default, at low alpha. |
| D6 | Selected vertices in the selected bind show a white outline around their colored marker, instead of turning white. |


## 7. Open questions

None at the moment.
