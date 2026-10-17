# AI skinning assistant

**Goal:** let an artist hand the tedious part of skinning to an AI agent. The
agent looks at the character art, the mesh and the skeleton, weights the meshes,
checks its work by posing the rig, and leaves the result for the artist to review
and touch up with the paint tools.

**Status:** steps 1–3 of the work plan (§9) built: the region tools, the
pictures and test poses, and the MCP server with every tool (§0). The workflow
prompt is written; evals, the built-in button and the how-to are next.

**Builds on:** envelope skinning and normalized shares (`ENVELOPES.md`), weight
painting (`ENVELOPES.md` §0 step 5), Bind Pose (`BIND_POSE.md`).


## 0. As built

**Connecting:** *Settings → Agent* shows the server's address and a command to
copy, run once in a terminal:
`claude mcp add --transport http keyternity http://127.0.0.1:47870/mcp --header
"Authorization: Bearer <token>"`. Then, in Claude Code, with a rig open in
Keyternity: "skin the character", or the `skin_character` prompt.

**Region maths** (`weights.rs`, `skin.rs`; `tests/all/regions.rs`, 5):
- `in_polygon`, `polygon_edge_distance`, `geodesic` (Dijkstra along mesh edges,
  so distances go round gaps), `part_shares` (labelled vertices to shares: full
  share inside a part, `(1 − d/blend)²` toward neighbouring parts, normalized),
  `joint_share` (smoothstep across a joint), `blur` (every bone's shares averaged
  with edge neighbours).
- `skin::assign_parts`, `set_region`, `smooth_vertices`, `blend_joint`,
  `set_shares`, `vertices_in`, `rest_positions`. Outlines are tested against the
  vertices' rest positions (world units, as drawn in the setup pose).
- **Lasso** in Paint Weights (a toggle in the tool's row): draw an outline; on
  release the vertices inside go fully to the bone (Add), leave it (Subtract), or
  are smoothed 3 times (Smooth). Shift and Ctrl change the mode as for the brush.
  One undo step.

**Pictures** (`agent/render.rs`): on the CPU, not the GPU pipeline the design
suggested, so a picture is made inside the tool call on the main thread and in
tests. Textured triangles (nearest sample, z order), then wireframe, vertex ids
(a 3×5 pixel digit font), a weight view (art dimmed, vertices blue→red), and
bones as blades or rings in a 12-colour palette labelled with their id. The
result text gives the pixel→world mapping and the bones' id, name and colour.
`distortion` compares a mesh's triangles at rest and posed: flips, area changes
over 50%, the largest edge stretch.

**Tools** (`agent/mod.rs`; `tests/all/agent.rs`, 7): `get_rig`, `get_mesh`,
`render`, `pose_test`, `bind_to_skeleton`, `set_envelope`, `set_bone_length`,
`assign_parts`, `set_region`, `smooth`, `blend_joint`, `set_weights`, `unlock`,
`checkpoint`, `revert`. JSON schemas are strict (every object closed). Editing
tools leave Pose Mode and push one undo step each (dropped again if the call
fails); read-only mode refuses them. `pose_test` turns IK off in its copy unless
`use_ik` is set, since IK would override the rotations being tested; Bind Pose
helpers stay at rest so the meshes deform. Checkpoints hold copies of the bones
for the session.

**Server** (`agent/server.rs`): MCP Streamable HTTP, plain JSON responses (GET
gets 405), protocol versions 2025-11-25, 2025-06-18 and 2025-03-26. Methods:
`initialize` (with the instructions text), `ping`, `tools/list`, `tools/call`,
`prompts/list`, `prompts/get` (`skin_character`, with optional artist notes);
notifications get 202. HTTP/1.1 on `std::net` (a thread per connection,
keep-alive, Content-Length or chunked bodies, 16 MB cap), bound to 127.0.0.1,
port 47870 or the next free of nine. `tiny_http` was tried first and dropped: a
process that had served a request and then shut the server down never exited.
Bearer token required; requests with a non-local `Origin` are refused. Tool calls
go to the main thread through a channel, drained once a frame (`poll`), and wait
up to 2 minutes. Settings: on (default), read-only, port, token (made on first
run, *Regenerate Token*), status and the copyable command.

**Not done yet:** the eval set (§8), the built-in button (§7.3), docs for
artists beyond Settings' text, mesh refinement suggestions as a tool.


## 1. What makes skinning hard for an agent

Skinning is two problems:

1. **Semantic:** which part of the drawing belongs to which bone. "This is the
   sleeve, it follows the upper arm; this is the cuff, it follows the forearm; the
   hair on the shoulder follows the head, not the arm." This is visual judgement,
   which a vision model is good at and envelopes are bad at (an envelope only knows
   distance to the bone).
2. **Numeric:** smooth, normalized weights over hundreds of vertices, with
   gradual blends at joints and no vertex left behind. This is what code is good at
   and a language model is bad at: asking it for 400 per-vertex numbers is slow,
   expensive and error-prone.

So the design splits the work: **the agent decides regions, the app computes
weights.** The agent's main tool takes regions (polygons drawn on the art, one
bone each), and the app turns them into weights with blends at the boundaries.
Per-vertex tools exist for touch-ups, not as the main path.

The other half is **checking**. A human skinner bends each joint and looks. The
agent gets the same loop: pose the rig, get a picture of the deformed art and
numeric distortion measures, fix, repeat.


## 2. Two ways in

| | A. MCP server in Keyternity | B. Built-in "Auto-skin" |
|---|---|---|
| Who runs the agent | The artist's own agent: Claude Code, Claude Desktop, or any MCP client | Keyternity, calling the Claude API |
| Setup | Turn on *Agent access*, add the server to the agent once | An API key in Settings |
| Cost | The artist's existing Claude plan | API usage, billed to the key |
| Control | Conversational: "skin the arms, keep the hair rigid" | One button, optional instructions box |
| Work in Keyternity | Tool server + render service | The same, plus the agent loop and a progress UI |

**Decided: A first (D1).** It needs the same tools as B and the render
service, but no agent loop, no key handling and no billing UI. It's also the most
flexible: the artist can talk the agent through a tricky rig. B reuses everything
from A, adding a loop on top (§7).

Both share one **tool registry** (§4): each tool is defined once (name, JSON
schema, handler), and the MCP server and the built-in loop both expose it.


## 3. What the agent sees

### 3.1 Rig data: `get_rig`
JSON, in **world units at rest** (the setup pose, as Bind Pose captures it), y up:

- `bones`: id, name, parent id, head, tail, length, deform flag, envelope (radii,
  falloff), whether it's an IK target or a Bind Pose helper (helpers are hidden;
  the agent works with real bones only).
- `meshes`: id, name, texture size, bounding box, whether it's bind-posed and
  envelope-weighted, the skin bones, and per-vertex: id, rest position, UV,
  current shares (bone id → share), locked flag. Triangles as vertex-id triples.
- `frame`: how image pixels map to world units for each render (§3.2), so the
  agent can turn a point it sees into a position.

Large meshes: vertex data is paged (`get_mesh(mesh_id, offset, limit)`), and
`get_rig` returns counts only.

### 3.2 Pictures: `render`
PNG images from an offscreen pass of the normal renderer, so they look like the
canvas. Options:

| Option | Effect |
|---|---|
| `meshes` | which meshes to draw (default all) |
| `crop` | a world rectangle, or a mesh id to frame, with padding |
| `size` | longest side in px (default 1024: detailed enough to read the art, ~1.4k input tokens) |
| `bones` | draw bones as labelled blades (name + id), each bone a distinct colour from a fixed palette, so the agent can tell them apart |
| `wireframe` | the mesh edges; with `vertex_ids`, vertex ids next to the vertices (only legible on small meshes or tight crops) |
| `weights` | colour vertices by one bone's share (blue 0 → red 1), as the weight view does |
| `grid` | a labelled world-unit grid, to read coordinates off the picture |
| `pose` | bone rotations (§3.3) to render a test pose instead of the rest pose |

Every render's result includes, as text, the world rectangle it shows and the
pixel → world transform, so a point the agent picks in the image converts exactly.

### 3.3 Test poses: `pose_test`
The checking loop. Input: a list of `{bone, rotation_degrees}` (and optionally
translations for IK targets). Output:

- a render of the deformed art (and optionally the rest pose beside it)
- **distortion metrics** per mesh, so problems are numbers, not just pictures:
  - triangles that flip (negative area): a fold or tear
  - triangles whose area changes by more than 50%: collapse or ballooning
  - the largest edge stretch
  - vertices that moved more than they should for the rotation applied (a vertex
    far from the rotated bone moving a lot means it's wrongly weighted)
  - each flagged triangle's vertex ids and location, so the agent can aim a fix

`pose_test` never changes the rig: it poses a copy.


## 4. What the agent does

Every editing tool works on **normalized shares** (`weights.rs`), locks the vertices
it touches (so the live envelope recompute keeps them, `ENVELOPES.md` §3.6), and is
undoable. An agent session is grouped (§6).

### 4.1 Region tools (the main path)
- **`assign_parts`** `(mesh, parts: [{bone, polygon}], blend_px)`: the semantic
  segmentation in one call. Each part is a polygon in world (or render pixel)
  coordinates and the bone it follows. Vertices inside a part go fully to its bone.
  Near a boundary between two parts, weights blend over `blend_px`, measured along
  the mesh (geodesic: across triangles, not straight through gaps, so a hand
  resting on a hip doesn't blend into the hip). Vertices in no part keep their
  current weights. This is the tool that does most of the work.
- **`set_region`** `(mesh, polygon, bone, share, feather_px)`: set one bone's
  share in a region, with a soft edge.
- **`smooth_region`** `(mesh, polygon | all, bone | all, iterations)`: the paint
  tool's Smooth (and Blur, for every bone) over a region.
- **`blend_joint`** `(mesh, bone_a, bone_b, width)`: re-blend the seam between two
  bones over a width centred on their joint: a common fix after a test pose shows
  a pinch.

### 4.2 Touch-ups
- **`set_weights`** `(mesh, [{vertex, bone, share}])`: exact per-vertex shares, as
  the bind panel's sliders do.
- **`unlock`** `(mesh, vertices | all)`: back to envelope weights.

### 4.3 Setup levers
Often the fastest fix isn't weights at all:
- **`set_envelope`** `(bone, radii, falloff, deform)` then **`bind_to_skeleton`**
  `(mesh)`: the cheap global first pass.
- **`set_bone_length`** `(bone, length)`: envelopes and weights depend on it.
- The agent may **not** add, delete, move or reparent bones, or change the mesh
  (D2, D3): it suggests those in its report.


## 5. The agent's workflow

The system prompt (B) or the tool descriptions and an MCP prompt template (A) set
out this plan; the agent adapts it.

1. `get_rig`, then `render` the whole character with bones and labels.
2. For each mesh: if it isn't skinned, `bind_to_skeleton` for a baseline.
3. `render` the mesh close up, with bones. Work out the parts: which drawn
   regions follow which bone, including the non-obvious ones (hair, clothing that
   hangs, accessories, overlapping limbs).
4. `assign_parts` with those polygons.
5. `pose_test` each joint both ways (for example ±45° on each limb bone, and the
   spine and neck), look at the renders and the metrics.
6. Fix what the tests show (`blend_joint`, `smooth_region`, `set_region`), and
   re-test.
7. Report: what it did per mesh, what it's unsure about, which joints still
   distort (with the numbers), and the test renders. The artist reviews, keeps or
   undoes the session, and touches up with Paint Weights.


## 6. Safety and review

- **Review is the file (D5):** the agent just does the work. The artist reviews
  the result and saves the file, or rejects it by not saving (reopening the file).
  Agent edits are ordinary undoable edits, so the artist can also undo back
  through them; there's no separate keep/undo step per mesh or per session. The
  agent has `checkpoint(name)` and `revert(name)` for its own experiments.
- Agent-weighted vertices are locked like hand-weighted ones, and shown with the
  same white ring, so they're easy to review in the weight view.
- The agent can't save, export, open files, or touch anything but skinning data
  (§4.3). The MCP server has no file or shell tools.
- **Read-only mode** (A, a setting): `get_rig`, `render` and `pose_test` only. The
  agent can review a rig and suggest fixes without editing.
- **Asking for guidance:** when the art or hierarchy is ambiguous (an unnamed bone,
  a part it can't place, two bones it can't tell apart), the agent asks the
  artist rather than guessing: in A that's just the conversation; the tool
  results and the workflow prompt tell it to. `get_rig` includes bone names, and
  `render` labels, so the artist can answer by name.
- While an agent is editing, the canvas shows a banner ("Agent editing:
  *Alice*… Stop"), and the artist's own edits pause the agent's queue (an edit
  from the agent arriving after an artist edit is applied to the new state; tools
  are idempotent where they can be, and every result reports the state it saw).


## 7. Architecture

### 7.1 Tool registry and execution (shared)
- `src/agent/` with a registry: `name`, description, JSON schema (with
  `additionalProperties: false` and `required`, so it can be used with strict tool
  use), and a handler `fn(&mut Shared, Value) -> Result<ToolOutput>`, where
  `ToolOutput` is text plus optional images (PNG bytes).
- Handlers run **on the main thread between frames**, like events, so they see
  and change the same state the UI does and land in undo. The server or loop runs
  on a background thread and sends `(call, reply_channel)` through a channel the
  main loop drains each frame.
- **Render service:** `render` and `pose_test` render offscreen with the existing
  wgpu pipeline into a texture, then read it back to a PNG. It needs the GPU, so it
  runs on the main thread too; a render request completes on a later frame and the
  reply is sent then. Useful beyond the agent: thumbnails and documentation
  screenshots.
- New editing maths in `weights.rs` / `skin.rs`: polygon membership, geodesic
  distance over the mesh (Dijkstra on edges), boundary blending, joint re-blend.
  Unit-tested like the rest.
- `assign_parts` and `set_region` are worth having as **artist tools** too: a
  lasso "assign to bone" in Paint Weights is the manual version of the agent's
  main tool.

### 7.2 A: MCP server
- **Built in and always running (D1):** Keyternity starts a local server using MCP's
  Streamable HTTP transport whenever it runs, bound to `127.0.0.1` only, on a
  fixed default port (configurable, with the next free port if it's taken).
  *Settings → Agent access* shows the URL and a one-line command to add it to
  Claude Code (`claude mcp add --transport http keyternity <url> --header
  "Authorization: Bearer <token>"`) or the Claude Desktop config. The token is
  generated once and kept in the config, so the agent is set up once and works
  every time the app is open; *Regenerate token* revokes old setups, and a switch
  turns the server off for anyone who doesn't want it.
- The server only works on the armature open in the app. With no file open, the
  tools say so.
- Rust: the official Rust MCP SDK (`rmcp`) if its transport fits; the protocol is
  small enough (JSON-RPC: `initialize`, `tools/list`, `tools/call`, plus
  `prompts/list`) to implement on the HTTP server crate otherwise.
- Exposes the tools, and a **prompt** (`skin_character`) with the §5 workflow, so
  the artist can start with one command and their own notes ("keep the bag rigid").
- Images go back as MCP image content (base64 PNG).

### 7.3 B: built-in Auto-skin
A manual tool-use loop in Rust, calling the Messages API over HTTPS with `ureq`
(already a dependency). There's no official Anthropic SDK for Rust, so this is raw
HTTP against `POST /v1/messages`.

Request settings:
- **Model `claude-opus-5-5`**: strong vision and spatial reasoning, long agentic
  runs. Thinking is always on for this model: send `thinking: {type: "adaptive"}`
  (or omit it). `output_config.effort: "high"`: this is intelligence-sensitive
  work, and the default on this model is `medium`, so set it explicitly.
- **Tools:** the registry, each with `strict: true`. `tool_choice` stays `auto`:
  forcing a tool (`any` / `tool`) is rejected on this model, so the system prompt
  directs the workflow instead.
- **Streaming**, with `max_tokens` around 64000, so long turns don't hit HTTP
  timeouts and the UI can show progress. With streaming and client tools, set
  `eager_input_streaming: true` on each tool and validate each tool input against
  its schema before running it (a truncated input is possible); check
  `stop_reason` for `max_tokens` and `refusal` before running tools.
- **Refusals:** always check `stop_reason == "refusal"` before reading content.
  Opt into server-side fallbacks (`fallbacks: "default"` with the
  `server-side-fallback-2026-07-01` beta header) so a declined turn re-runs on a
  fallback model instead of stopping; tell the artist if a run ended in a
  refusal.
- **Prompt caching:** tools and the system prompt never change during a run, and the
  first user message (the rig JSON and the overview render) doesn't either; put a
  `cache_control` breakpoint after it. The history is **append-only**: never edit
  or delete earlier turns (that would invalidate the cache and earlier thinking
  blocks). Verify with `usage.cache_read_input_tokens`.
- **Images pile up.** Renders are the bulk of the input. Keep them at 1024 px or
  smaller and crop to what's being checked. For long runs, use server-side
  context editing to clear old tool results (`clear_tool_uses`), rather than
  editing history in the app.
- **Key:** from Settings, stored in the OS keychain where available, never in
  `.skf` files or logs. Log each response's request id for support.
- **Budget:** a per-run token cap (stop and report when reached), and the run's
  token usage and estimated cost shown live and at the end.
- **UI:** an *Auto-skin* button in the Skin section (whole rig, or the selected
  meshes), an optional instructions box, a progress panel with the agent's
  renders and notes as they arrive, Stop, and at the end the report with *Keep*
  and *Undo*.

Rough cost, to be measured: a mid-size character (a few meshes, ~20 bones, a few
hundred vertices per mesh) might take 20–40 tool calls and 15–30 renders. With
caching, that's on the order of a few hundred thousand input tokens and tens of
thousands of output tokens per run, so roughly a few dollars on Opus. The first
eval runs (§8) should replace this guess with numbers.


## 8. Evaluating it

- **Test rigs:** the sample rigs (`samples/`, the Alice rig) and a few with
  hard cases: overlapping limbs, hair over shoulders, a skirt over both legs, a
  prop held in a hand. Each with reference weights painted by hand.
- **Scores:** (1) the `pose_test` distortion metrics over a fixed set of test
  poses, against the reference's; (2) agreement with the reference shares
  (mean absolute difference per vertex); (3) a human rating of a few test-pose
  renders; (4) tokens and wall time per run.
- Run it whenever the prompt, the tools or the model change, to see whether
  results improved.


## 9. Work plan

1. **Region weighting maths and artist lasso** (§4.1, §7.1): `assign_parts`,
   `set_region`, `smooth_region`, `blend_joint` in `weights.rs` / `skin.rs`, with
   tests, plus a lasso mode in Paint Weights. Useful without any agent.
2. **Render service and test poses** (§3.2–3.3): offscreen render to PNG,
   distortion metrics. Tests for the metrics.
3. **Tool registry and MCP server** (§7.1–7.2), read-only mode first, then
   editing with session undo groups.
4. **Agent workflow prompt** and a first round of evals (§5, §8) through Claude
   Code.
5. **Built-in Auto-skin** (§7.3).
6. **Docs:** README, a how-to for connecting an agent, and an as-built section here.


## 10. Decisions

| # | Question | Leaning |
|---|---|---|
Answered by the artist:

| # | Decision |
|---|---|
| D1 | **MCP server (A) first**, built into Keyternity and running whenever the app is (§7.2). The built-in button (B) comes later, if at all. |
| D2 | The agent may change **envelopes and bone lengths** (§4.3). New bones (for hair, clothing) are only suggested in its report. |
| D3 | It doesn't edit meshes yet. Adding vertices where a joint needs more resolution is a later feature; until then it suggests it. |
| D4 | **The agent handles every skinning task** on the open rig: binding, envelopes, lengths, weights, checking. By default the whole rig, or what the artist asks for. When the art or hierarchy isn't clear, it asks the artist (§6). |
| D5 | **No review step in the app.** The agent does the work; the artist reviews, and saves or rejects (doesn't save) the file (§6). |
