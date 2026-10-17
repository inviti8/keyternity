//! The skinning agent's tools (docs/AI_SKINNING.md §3–4, §7.1): what an AI agent can
//! see of the rig and do to its weights. Each tool is a name, a description, a JSON
//! schema and a handler on the open armature. `server` exposes them over MCP.
//!
//! Handlers run on the main thread between frames, like events. Every editing tool
//! leaves Pose Mode, pushes an undo step, and locks the vertices it weights.

pub mod dialog_tools;
pub mod render;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;

use crate::*;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Part of a tool's result.
#[derive(Clone, Debug)]
pub enum Content {
    Text(String),
    /// PNG bytes
    Image(Vec<u8>),
}

#[derive(Clone, Debug)]
pub struct ToolResult {
    pub content: Vec<Content>,
    pub is_error: bool,
}

impl ToolResult {
    fn text(s: impl Into<String>) -> Self {
        ToolResult { content: vec![Content::Text(s.into())], is_error: false }
    }
    fn json(v: Value) -> Self {
        ToolResult::text(v.to_string())
    }
    fn error(s: impl Into<String>) -> Self {
        ToolResult { content: vec![Content::Text(s.into())], is_error: true }
    }
}

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
    /// changes the rig (refused in read-only mode)
    pub edits: bool,
    /// the undo step an edit makes
    pub undo: Undo,
}

/// What an editing tool's undo step holds.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Undo {
    None,
    /// the bones (skinning, shapes, mouth rigs)
    Bones,
    /// the animation named by the `animation` argument (dialog, lip sync, keys)
    Anim,
}

/// State kept between calls.
#[derive(Default)]
pub struct AgentState {
    /// the MCP server, while it runs
    #[cfg(not(target_arch = "wasm32"))]
    pub server: Option<server::AgentServer>,
    /// why the server couldn't start (shown in Settings; cleared to retry)
    pub start_error: Option<String>,
    /// named snapshots of the bones (`checkpoint` / `revert`)
    pub checkpoints: HashMap<String, Vec<Bone>>,
    /// only the looking tools: get_rig, get_mesh, render, pose_test
    pub read_only: bool,
}

/// What the agent is told when it connects, and the `skin_character` prompt.
pub const INSTRUCTIONS: &str = "Keyternity is a 2D skeletal animation editor. These tools skin the open rig: they let you see the character art, meshes and bones, and set how strongly each bone moves each mesh vertex.\n\
Coordinates are world units at rest (the setup pose), y up. A vertex's weights are shares that always sum to 1 across bones; setting one bone's share scales the others.\n\
Work by regions, not single vertices: decide which drawn part follows which bone from the pictures, then call assign_parts with outlines. Check with pose_test, fix seams with blend_joint or smooth, and use set_weights only for touch-ups.\n\
They also do dialog and lip sync: speak lines with a voice (add_dialog), place audio (import_audio), lip-sync it on the rig's mouth (lip_sync), key shapes, and show the result as frame strips with the transcript (review_page with lipsync). The lip_sync_dialog prompt walks through it.\n\
You may change envelopes and bone lengths. Don't try to add bones or change meshes: suggest that in your report instead. If the art or the bone hierarchy isn't clear, ask the artist rather than guessing. Every edit can be undone; the artist reviews your work and saves the file.";

pub const WORKFLOW: &str = "Skin this character:\n\
1. get_rig, then render the whole character with bones.\n\
2. For each mesh that isn't skinned yet (bind_pose false), call bind_to_skeleton for a baseline.\n\
3. For each mesh, render it close up (crop to it) with bones, and work out which drawn parts follow which bone: limbs, and also hair, clothing, accessories and overlapping parts.\n\
4. Call assign_parts with an outline per part (world coordinates; read them off the render using its mapping), with a blend of about a fifth of the limb's width.\n\
5. pose_test each joint both ways (for example +-45 degrees on each limb bone, the spine and the neck). Look at the renders and the distortion numbers.\n\
6. Fix what the tests show (blend_joint at pinched joints, smooth on rough areas, set_region for parts on the wrong bone) and test again.\n\
7. Report what you did per mesh, what you're unsure about, which joints still distort, and anything the rig would need (bones, more mesh resolution). Finish with review_page: the poses that show the work (and any that still distort), the main bones' weights, and your report as notes, so the artist can review it in the browser.";

fn schema(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

fn outline_schema(desc: &str) -> Value {
    json!({
        "type": "array",
        "description": desc,
        "items": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 },
        "minItems": 3
    })
}

/// Every tool, in the order an agent tends to use them.
pub fn tools() -> Vec<Tool> {
    let mesh = json!({ "type": "integer", "description": "mesh (textured bone) id, from get_rig" });
    let bone = json!({ "type": "integer", "description": "bone id, from get_rig" });
    vec![
        Tool {
            name: "get_rig",
            description: "The open rig: every bone (id, name, parent, head and tail at rest, length, envelope, whether it deforms or is an IK target) and every mesh (id, name, bounds at rest, vertex and triangle counts, whether it's skinned, its skin bones). Start here.",
            schema: schema(json!({}), &[]),
            edits: false,
            undo: Undo::None,
        },
        Tool {
            name: "get_mesh",
            description: "A mesh's vertices (id, rest position, shares per bone, locked) and triangles, paged.",
            schema: schema(json!({ "mesh": mesh, "offset": { "type": "integer" }, "limit": { "type": "integer" } }), &["mesh"]),
            edits: false,
            undo: Undo::None,
        },
        Tool {
            name: "render",
            description: "A picture of the art at rest. Bones are drawn as coloured blades labelled with their id (the result lists id, name and colour). Options: meshes to draw, a world crop rectangle or a mesh to frame, size (longest side, px), wireframe, vertex ids, and weights_bone to colour vertices by that bone's share (blue 0 to red 1). The result gives the pixel-to-world mapping, for reading outline coordinates off the picture.",
            schema: schema(
                json!({
                    "meshes": { "type": "array", "items": { "type": "integer" } },
                    "frame_mesh": { "type": "integer", "description": "crop to this mesh's bounds" },
                    "crop": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4, "description": "[x0, y0, x1, y1] world units" },
                    "size": { "type": "integer", "description": "longest side in px, 64-2048 (default 1024)" },
                    "bones": { "type": "boolean" },
                    "wireframe": { "type": "boolean" },
                    "vertex_ids": { "type": "boolean" },
                    "weights_bone": { "type": "integer" }
                }),
                &[],
            ),
            edits: false,
            undo: Undo::None,
        },
        Tool {
            name: "pose_test",
            description: "Pose a copy of the rig (bone rotations in degrees, added to the rest pose; moves in world units) and get a picture of the deformed art plus distortion numbers per mesh: triangles that flip (folds), triangles whose area changes by more than half, and the largest edge stretch, with the vertex ids of the worst triangles. Doesn't change the rig.",
            schema: schema(
                json!({
                    "rotations": { "type": "array", "items": schema(json!({ "bone": bone, "degrees": { "type": "number" } }), &["bone", "degrees"]) },
                    "moves": { "type": "array", "items": schema(json!({ "bone": bone, "dx": { "type": "number" }, "dy": { "type": "number" } }), &["bone", "dx", "dy"]) },
                    "meshes": { "type": "array", "items": { "type": "integer" } },
                    "frame_mesh": { "type": "integer" },
                    "size": { "type": "integer" },
                    "weights_bone": { "type": "integer" },
                    "use_ik": { "type": "boolean", "description": "keep IK constraints on (default off, so rotations of IK chain bones apply directly)" }
                }),
                &[],
            ),
            edits: false,
            undo: Undo::None,
        },
        Tool {
            name: "bind_to_skeleton",
            description: "Skin a mesh to the skeleton in one step from the bones' envelopes (turns on Bind Pose). Vertices weighted by hand keep their weights. Use it for a baseline, or again after changing envelopes or lengths.",
            schema: schema(json!({ "mesh": mesh }), &["mesh"]),
            edits: true,
            undo: Undo::Bones,
        },
        Tool {
            name: "set_envelope",
            description: "Change a bone's envelope: inner radius (full weight) and outer radius (fading to none) at its head and tail, in world units, the falloff curve, whether it's absolute (rigid), and whether the bone deforms meshes at all. Envelope-weighted vertices update live.",
            schema: schema(
                json!({
                    "bone": bone,
                    "head_inner": { "type": "number" }, "head_outer": { "type": "number" },
                    "tail_inner": { "type": "number" }, "tail_outer": { "type": "number" },
                    "falloff": { "type": "string", "enum": ["Linear", "Smooth", "FastOut", "SlowOut"] },
                    "absolute": { "type": "boolean" },
                    "deform": { "type": "boolean" }
                }),
                &["bone"],
            ),
            edits: true,
            undo: Undo::Bones,
        },
        Tool {
            name: "set_bone_length",
            description: "Set a bone's length (world units). Its tail, and so its envelope, follow.",
            schema: schema(json!({ "bone": bone, "length": { "type": "number" } }), &["bone", "length"]),
            edits: true,
            undo: Undo::Bones,
        },
        Tool {
            name: "assign_parts",
            description: "The main skinning tool. Give outlines of drawn parts (world coordinates at rest), each with the bone it follows. Vertices inside an outline go fully to its bone (the last part wins where outlines overlap); within `blend` world units of another part (measured along the mesh) the two blend, half and half at the boundary. Vertices in no outline keep their weights. Returns how many vertices were assigned.",
            schema: schema(
                json!({
                    "mesh": mesh,
                    "parts": { "type": "array", "items": schema(json!({ "bone": bone, "outline": outline_schema("[[x, y], ...] world units") }), &["bone", "outline"]) },
                    "blend": { "type": "number", "description": "blend width in world units (0: hard seams)" }
                }),
                &["mesh", "parts", "blend"],
            ),
            edits: true,
            undo: Undo::Bones,
        },
        Tool {
            name: "set_region",
            description: "Set one bone's share (0-1) for the vertices inside an outline, fading in over `feather` world units from the outline's edge. Other bones scale to make up the rest.",
            schema: schema(
                json!({ "mesh": mesh, "outline": outline_schema("[[x, y], ...] world units"), "bone": bone, "share": { "type": "number" }, "feather": { "type": "number" } }),
                &["mesh", "outline", "bone", "share"],
            ),
            edits: true,
            undo: Undo::Bones,
        },
        Tool {
            name: "smooth",
            description: "Smooth weights over the vertices inside an outline (or the whole mesh): one bone's share toward its neighbours', or with no bone, every bone's at once, which evens out a seam.",
            schema: schema(
                json!({ "mesh": mesh, "outline": outline_schema("[[x, y], ...] world units; omit for the whole mesh"), "bone": bone, "iterations": { "type": "integer", "description": "default 3" } }),
                &["mesh"],
            ),
            edits: true,
            undo: Undo::Bones,
        },
        Tool {
            name: "blend_joint",
            description: "Re-blend the seam between a parent bone and its child over `width` world units centred on the child's head, easing each vertex from the parent to the child along the child's direction. Fixes pinched or hard joints.",
            schema: schema(json!({ "mesh": mesh, "parent": bone, "child": bone, "width": { "type": "number" } }), &["mesh", "parent", "child", "width"]),
            edits: true,
            undo: Undo::Bones,
        },
        Tool {
            name: "set_weights",
            description: "Exact shares for single vertices: each entry sets one bone's share of one vertex; the vertex's other bones scale to make up the rest. For touch-ups.",
            schema: schema(
                json!({ "mesh": mesh, "weights": { "type": "array", "items": schema(json!({ "vertex": { "type": "integer" }, "bone": bone, "share": { "type": "number" } }), &["vertex", "bone", "share"]) } }),
                &["mesh", "weights"],
            ),
            edits: true,
            undo: Undo::Bones,
        },
        Tool {
            name: "unlock",
            description: "Return vertices (or the whole mesh) to their envelope weights, discarding hand and agent weighting.",
            schema: schema(json!({ "mesh": mesh, "vertices": { "type": "array", "items": { "type": "integer" } } }), &["mesh"]),
            edits: true,
            undo: Undo::Bones,
        },
        Tool {
            name: "review_page",
            description: "Make a web page for the artist to review the skinning: the rest pose, each test pose you list (named, with the same options as pose_test), optional weight views of chosen bones, and your notes. Written as one HTML file next to the rig (<rig>_review.html) and opened in the artist's browser. Returns the file's path. Use it at the end of a skinning pass, with the poses that show the work best (and any still-problematic ones).",
            schema: schema(
                json!({
                    "title": { "type": "string" },
                    "notes": { "type": "string", "description": "your report: what you did, what's uncertain, what still distorts (plain text; blank lines separate paragraphs)" },
                    "poses": { "type": "array", "items": schema(json!({
                        "name": { "type": "string" },
                        "rotations": { "type": "array", "items": schema(json!({ "bone": bone, "degrees": { "type": "number" } }), &["bone", "degrees"]) },
                        "moves": { "type": "array", "items": schema(json!({ "bone": bone, "dx": { "type": "number" }, "dy": { "type": "number" } }), &["bone", "dx", "dy"]) },
                        "frame_mesh": { "type": "integer" },
                        "use_ik": { "type": "boolean" },
                        "note": { "type": "string" }
                    }), &["name"]) },
                    "weights_bones": { "type": "array", "items": { "type": "integer" }, "description": "bones to show weight views of" },
                    "size": { "type": "integer", "description": "picture size, longest side px (default 700)" },
                    "lipsync": { "type": "array", "description": "lip-synced lines to show as frame strips with their transcript", "items": schema(json!({
                        "animation": { "description": "index or name" },
                        "clip": { "type": "integer", "description": "default: the animation's first lip-synced clip" },
                        "every": { "type": "integer", "description": "show every nth frame (default 2)" },
                        "from": { "type": "integer" },
                        "to": { "type": "integer" },
                        "frame_mesh": { "type": "integer", "description": "frame the strip on this mesh (the face)" }
                    }), &["animation"]) },
                    "open": { "type": "boolean", "description": "open the page in the browser (default true)" }
                }),
                &[],
            ),
            edits: false,
            undo: Undo::None,
        },
        Tool {
            name: "checkpoint",
            description: "Remember the rig's current state under a name, to try something and go back with revert.",
            schema: schema(json!({ "name": { "type": "string" } }), &["name"]),
            edits: false,
            undo: Undo::None,
        },
        Tool {
            name: "revert",
            description: "Go back to a checkpoint.",
            schema: schema(json!({ "name": { "type": "string" } }), &["name"]),
            edits: true,
            undo: Undo::Bones,
        },
    ]
    .into_iter()
    .chain(dialog_tools::tools())
    .collect()
}

fn r1(x: f32) -> f32 {
    (x * 10.).round() / 10.
}

fn r3(x: f32) -> f32 {
    (x * 1000.).round() / 1000.
}

fn int(args: &Value, key: &str) -> Result<i32, String> {
    args.get(key).and_then(|v| v.as_i64()).map(|v| v as i32).ok_or(format!("`{key}` (an integer) is required"))
}

fn num(args: &Value, key: &str) -> Result<f32, String> {
    args.get(key).and_then(|v| v.as_f64()).map(|v| v as f32).ok_or(format!("`{key}` (a number) is required"))
}

fn opt_num(args: &Value, key: &str) -> Option<f32> {
    args.get(key).and_then(|v| v.as_f64()).map(|v| v as f32)
}

fn ints(v: Option<&Value>) -> Vec<i32> {
    v.and_then(|v| v.as_array()).map_or(vec![], |a| a.iter().filter_map(|x| x.as_i64()).map(|x| x as i32).collect())
}

fn outline(v: Option<&Value>) -> Result<Vec<Vec2>, String> {
    let points = v.and_then(|v| v.as_array()).ok_or("an outline ([[x, y], ...]) is required")?;
    let out: Vec<Vec2> = points
        .iter()
        .filter_map(|p| {
            let a = p.as_array()?;
            Some(Vec2::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))
        })
        .collect();
    if out.len() < 3 || out.len() != points.len() {
        return Err("an outline needs at least 3 [x, y] points".into());
    }
    Ok(out)
}

fn bone_exists(armature: &Armature, id: i32) -> Result<&Bone, String> {
    armature
        .bones
        .iter()
        .find(|b| b.id == id && b.bind_owner.is_none())
        .ok_or(format!("no bone {id}; get_rig lists the bones"))
}

/// A mesh the region tools can weight: textured, with vertices, bind-posed.
fn skinned_mesh(armature: &Armature, id: i32) -> Result<&Bone, String> {
    let mesh = bone_exists(armature, id)?;
    if armature.tex_of(id).is_none() || mesh.vertices.is_empty() {
        return Err(format!("bone {id} isn't a mesh"));
    }
    if !mesh.bind_pose {
        return Err(format!("mesh {id} isn't skinned yet: call bind_to_skeleton first"));
    }
    Ok(mesh)
}

fn is_ik_target(bones: &[Bone], id: i32) -> bool {
    bones.iter().any(|b| b.ik.as_ref().map_or(false, |c| c.target_id == id))
}

/// Run tool `name` on the open armature.
pub fn call(shared: &mut Shared, name: &str, args: &Value) -> ToolResult {
    let Some(tool) = tools().into_iter().find(|t| t.name == name) else {
        return ToolResult::error(format!("unknown tool `{name}`"));
    };
    if tool.edits && shared.agent.read_only {
        return ToolResult::error("Keyternity's agent access is read-only (Settings); this tool would change the rig.");
    }
    if shared.armature.bones.is_empty() {
        return ToolResult::error("no rig is open in Keyternity");
    }
    if tool.edits {
        // skinning is set up at rest: leave Pose Mode, and make this one undo step
        editor::exit_pose_mode(&mut shared.armature, &mut shared.edit_mode, &mut shared.undo_states);
        match tool.undo {
            Undo::Bones => shared.undo_states.new_undo_bones(&shared.armature.bones),
            Undo::Anim => match dialog_tools::anim_index(&shared.armature, args) {
                Ok(i) => shared.undo_states.new_undo_anim(&shared.armature.animations[i]),
                Err(e) => return ToolResult::error(e),
            },
            Undo::None => {}
        }
    }
    let result = run(shared, name, args);
    match result {
        Ok(r) => r,
        Err(e) => {
            // nothing changed: drop the undo step
            if tool.edits && tool.undo != Undo::None {
                shared.undo_states.undo_actions.pop();
            }
            ToolResult::error(e)
        }
    }
}

fn run(shared: &mut Shared, name: &str, args: &Value) -> Result<ToolResult, String> {
    if let Some(r) = dialog_tools::run(shared, name, args) {
        return r;
    }
    let armature = &mut shared.armature;
    match name {
        "get_rig" => Ok(ToolResult::json(rig_json(armature))),
        "get_mesh" => {
            let id = int(args, "mesh")?;
            let mesh = bone_exists(armature, id)?.clone();
            let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(500).clamp(1, 2000) as usize;
            let pos = skin::rest_positions(armature, id);
            let shares = skin::vertex_shares(&armature.bones, &mesh);
            let verts: Vec<Value> = mesh
                .vertices
                .iter()
                .zip(&pos)
                .skip(offset)
                .take(limit)
                .map(|(v, p)| {
                    let s: Vec<Value> = shares.get(&v.id).map_or(vec![], |s| s.iter().map(|(b, x)| json!([b, r3(*x)])).collect());
                    json!({ "id": v.id, "x": r1(p.x), "y": r1(p.y), "shares": s, "locked": mesh.locked_verts.contains(&v.id) })
                })
                .collect();
            let tris: Vec<[u32; 3]> = if offset == 0 {
                mesh.indices.chunks_exact(3).map(|t| [mesh.vertices[t[0] as usize].id, mesh.vertices[t[1] as usize].id, mesh.vertices[t[2] as usize].id]).collect()
            } else {
                vec![]
            };
            Ok(ToolResult::json(json!({
                "mesh": id, "vertex_count": mesh.vertices.len(), "offset": offset, "vertices": verts,
                "triangles": tris, "note": "shares are [bone id, share]; the mesh's own id is its owner (unskinned) share"
            })))
        }
        "render" => {
            let world = skin::setup_world(armature);
            let opts = render_options(armature, &world, args)?;
            let pic = render::render(armature, &world, &opts);
            Ok(picture_result(&pic, String::new()))
        }
        "pose_test" => pose_test(armature, args),
        "review_page" => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let rig_path = shared.ui.save_path.clone().or_else(|| {
                    shared.ui.file_path.lock().ok().and_then(|p| p.first().cloned()).filter(|p| {
                        p.extension().map_or(false, |e| e == "skf" || e == "skfe")
                    })
                });
                review_page(&shared.armature, args, rig_path)
            }
            #[cfg(target_arch = "wasm32")]
            Err("review pages aren't available on the web".into())
        }
        "bind_to_skeleton" => {
            let id = int(args, "mesh")?;
            let name = bone_exists(armature, id)?.name.clone();
            let outside = skin::bind_to_skeleton(armature, id).map_err(|e| format!("{name}: {e}"))?;
            let bones = skin::skin_bones(&armature.bones, armature.bones.iter().find(|b| b.id == id).unwrap());
            Ok(ToolResult::json(json!({ "mesh": id, "skin_bones": bones, "outside_every_envelope": outside })))
        }
        "set_envelope" => {
            let id = int(args, "bone")?;
            bone_exists(armature, id)?;
            type F = EnvelopeField;
            for (key, field) in [("head_inner", F::HeadInner), ("head_outer", F::HeadOuter), ("tail_inner", F::TailInner), ("tail_outer", F::TailOuter)] {
                if let Some(v) = opt_num(args, key) {
                    skin::set_envelope_field(armature, id, field, v);
                }
            }
            if let Some(f) = args.get("falloff").and_then(|v| v.as_str()) {
                let value = match f {
                    "Linear" => envelope::Falloff::Linear,
                    "Smooth" => envelope::Falloff::Smooth,
                    "FastOut" => envelope::Falloff::FastOut,
                    "SlowOut" => envelope::Falloff::SlowOut,
                    _ => return Err(format!("unknown falloff `{f}`")),
                };
                skin::set_envelope_field(armature, id, F::Falloff, value as i32 as f32);
            }
            for (key, field) in [("absolute", F::Absolute), ("deform", F::Deform)] {
                if let Some(v) = args.get(key).and_then(|v| v.as_bool()) {
                    skin::set_envelope_field(armature, id, field, if v { 1. } else { 0. });
                }
            }
            let e = skin::envelope_of(&skin::setup_world(armature), id);
            Ok(ToolResult::json(envelope_json(&e)))
        }
        "set_bone_length" => {
            let id = int(args, "bone")?;
            bone_exists(armature, id)?;
            let length = num(args, "length")?.max(0.);
            armature.find_bone_mut(id).unwrap().length = length;
            Ok(ToolResult::text(format!("bone {id} length {length}")))
        }
        "assign_parts" => {
            let id = int(args, "mesh")?;
            skinned_mesh(armature, id)?;
            let blend = num(args, "blend")?.max(0.);
            let mut parts = vec![];
            for p in args.get("parts").and_then(|v| v.as_array()).ok_or("`parts` is required")? {
                let b = int(p, "bone")?;
                bone_exists(armature, b)?;
                parts.push((b, outline(p.get("outline"))?));
            }
            let n = skin::assign_parts(armature, id, &parts, blend);
            let total = armature.bones.iter().find(|b| b.id == id).unwrap().vertices.len();
            Ok(ToolResult::text(format!("assigned {n} of {total} vertices")))
        }
        "set_region" => {
            let id = int(args, "mesh")?;
            skinned_mesh(armature, id)?;
            let b = int(args, "bone")?;
            bone_exists(armature, b)?;
            let o = outline(args.get("outline"))?;
            let n = skin::set_region(armature, id, &o, b, num(args, "share")?.clamp(0., 1.), opt_num(args, "feather").unwrap_or(0.));
            Ok(ToolResult::text(format!("{n} vertices changed")))
        }
        "smooth" => {
            let id = int(args, "mesh")?;
            skinned_mesh(armature, id)?;
            let o = match args.get("outline") {
                Some(v) => Some(outline(Some(v))?),
                None => None,
            };
            let b = args.get("bone").and_then(|v| v.as_i64()).map(|v| v as i32);
            if let Some(b) = b {
                bone_exists(armature, b)?;
            }
            let iterations = args.get("iterations").and_then(|v| v.as_u64()).unwrap_or(3).clamp(1, 50) as u32;
            let ids = skin::vertices_in(armature, id, o.as_deref());
            let n = skin::smooth_vertices(armature, id, &ids, b, iterations);
            Ok(ToolResult::text(format!("{n} vertices smoothed")))
        }
        "blend_joint" => {
            let id = int(args, "mesh")?;
            skinned_mesh(armature, id)?;
            let (p, c) = (int(args, "parent")?, int(args, "child")?);
            bone_exists(armature, p)?;
            bone_exists(armature, c)?;
            let n = skin::blend_joint(armature, id, p, c, num(args, "width")?.max(0.));
            Ok(ToolResult::text(format!("{n} vertices re-blended")))
        }
        "set_weights" => {
            let id = int(args, "mesh")?;
            skinned_mesh(armature, id)?;
            let entries = args.get("weights").and_then(|v| v.as_array()).ok_or("`weights` is required")?;
            let mut by_bone: HashMap<i32, Vec<(u32, f32)>> = HashMap::new();
            for e in entries {
                let b = int(e, "bone")?;
                bone_exists(armature, b)?;
                by_bone.entry(b).or_default().push((int(e, "vertex")? as u32, num(e, "share")?.clamp(0., 1.)));
            }
            let mut bones: Vec<i32> = by_bone.keys().copied().collect();
            bones.sort();
            for b in bones {
                skin::edit_shares(armature, id, b, &by_bone[&b], false);
            }
            Ok(ToolResult::text(format!("{} weights set", entries.len())))
        }
        "unlock" => {
            let id = int(args, "mesh")?;
            skinned_mesh(armature, id)?;
            let which = ints(args.get("vertices"));
            let mesh = armature.find_bone_mut(id).unwrap();
            let before = mesh.locked_verts.len();
            if which.is_empty() {
                mesh.locked_verts.clear();
            } else {
                mesh.locked_verts.retain(|v| !which.contains(&(*v as i32)));
            }
            mesh.skin_sig = 0;
            Ok(ToolResult::text(format!("{} vertices unlocked", before - mesh.locked_verts.len())))
        }
        "checkpoint" => {
            let name = args.get("name").and_then(|v| v.as_str()).ok_or("`name` is required")?.to_string();
            shared.agent.checkpoints.insert(name.clone(), shared.armature.bones.clone());
            Ok(ToolResult::text(format!("checkpoint `{name}` saved")))
        }
        "revert" => {
            let name = args.get("name").and_then(|v| v.as_str()).ok_or("`name` is required")?;
            let bones = shared.agent.checkpoints.get(name).ok_or(format!("no checkpoint `{name}`"))?.clone();
            shared.armature.bones = bones;
            Ok(ToolResult::text(format!("reverted to `{name}`")))
        }
        _ => Err(format!("unknown tool `{name}`")),
    }
}

/// Escape text for HTML.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The review page (`review_page`): pictures as data URIs, so it's one file.
#[cfg(not(target_arch = "wasm32"))]
fn review_page(armature: &Armature, args: &Value, rig_path: Option<std::path::PathBuf>) -> Result<ToolResult, String> {
    use base64::Engine;
    let b64 = |png: &[u8]| base64::engine::general_purpose::STANDARD.encode(png);
    let size = args.get("size").and_then(|v| v.as_u64()).unwrap_or(700).clamp(200, 1600);
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("Skinning review").to_string();
    let bone_name = |id: i32| armature.bones.iter().find(|b| b.id == id).map_or(id.to_string(), |b| b.name.clone());

    let mut cards = String::new();
    let mut card = |name: &str, note: &str, png: &[u8], issues: &[Value]| {
        let mut issue_html = String::new();
        for m in issues {
            issue_html += &format!(
                "<li><b>{}</b>: {} folded, {} squashed or stretched triangles, max stretch {}×</li>",
                esc(m["name"].as_str().unwrap_or("")),
                m["flipped"],
                m["area_changed"],
                m["max_stretch"]
            );
        }
        let status = if issues.is_empty() { "<p class=ok>No distortion worth noting.</p>".to_string() } else { format!("<ul class=issues>{issue_html}</ul>") };
        cards += &format!(
            "<figure><img src=\"data:image/png;base64,{}\" alt=\"{}\"><figcaption><h2>{}</h2>{}{}</figcaption></figure>\n",
            b64(png),
            esc(name),
            esc(name),
            if note.is_empty() { String::new() } else { format!("<p>{}</p>", esc(note)) },
            status
        );
    };

    // rest pose, then each pose
    let rest = posed_picture(armature, &json!({ "size": size }))?;
    card("Rest pose", "", &render::png(&rest.0.image), &[]);
    let lipsync = match args.get("lipsync").and_then(|v| v.as_array()) {
        Some(specs) => dialog_tools::review_section(armature, specs, (size as u32 / 3).clamp(120, 400))?,
        None => String::new(),
    };
    let poses = args.get("poses").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    for pose in &poses {
        let mut a = pose.clone();
        a["size"] = json!(size);
        let name = pose.get("name").and_then(|v| v.as_str()).unwrap_or("Pose").to_string();
        let note = pose.get("note").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let (pic, issues) = posed_picture(armature, &a)?;
        card(&name, &note, &render::png(&pic.image), &issues);
    }
    // weight views
    let world = skin::setup_world(armature);
    for b in ints(args.get("weights_bones")) {
        bone_exists(armature, b)?;
        let opts = render::RenderOptions { size: size as u32, weights_bone: Some(b), ..Default::default() };
        let pic = render::render(armature, &world, &opts);
        let name = format!("Weights: {}", bone_name(b));
        card(&name, "Blue: no influence, red: full.", &render::png(&pic.image), &[]);
    }

    let notes: String = args
        .get("notes")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .map(|p| format!("<p>{}</p>", esc(p.trim()).replace('\n', "<br>")))
        .collect();
    let rig = rig_path.as_ref().and_then(|p| p.file_name()).map_or(String::new(), |n| n.to_string_lossy().to_string());
    let html = format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content=\"width=device-width, initial-scale=1\"><title>{t}</title><style>\
        :root{{color-scheme:dark;--bg:#16181d;--card:#22252c;--text:#e6e6e6;--dim:#9aa0a6;--warn:#f0a35e;--ok:#7cc48a}}\
        body{{margin:0;padding:24px 16px;background:var(--bg);color:var(--text);font:15px/1.5 system-ui,sans-serif}}\
        main{{max-width:1400px;margin:0 auto}}h1{{margin:0 0 4px}}.sub{{color:var(--dim);margin:0 0 20px}}\
        .notes{{background:var(--card);border-radius:8px;padding:12px 18px;margin-bottom:24px;max-width:900px}}\
        .grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(320px,1fr));gap:18px}}\
        figure{{margin:0;background:var(--card);border-radius:8px;overflow:hidden}}\
        img{{display:block;width:100%;height:auto;background:#2e3036;cursor:zoom-in}}img.big{{position:fixed;inset:0;margin:auto;width:auto;max-width:96vw;max-height:96vh;z-index:9;cursor:zoom-out;box-shadow:0 0 0 100vmax rgba(0,0,0,.8)}}\
        figcaption{{padding:10px 14px}}h2{{font-size:16px;margin:0 0 4px}}.issues{{color:var(--warn);margin:4px 0;padding-left:18px}}.ok{{color:var(--ok);margin:4px 0}}\
        .lipsync{{background:var(--card);border-radius:8px;padding:12px 14px;margin-bottom:24px}}.transcript span{{border-bottom:1px dotted var(--dim)}}\
        .strip{{display:flex;flex-wrap:wrap;gap:6px}}.tile{{width:150px;background:#1b1d22}}.tile figcaption{{padding:4px 6px;font-size:12px}}\
        </style></head><body><main><h1>{t}</h1><p class=sub>{rig}{count} poses, made by an agent in Keyternity. Click a picture to enlarge it.</p>{n}{lipsync}<div class=grid>{cards}</div></main>\
        <script>document.querySelectorAll('img').forEach(i=>i.onclick=()=>i.classList.toggle('big'))</script></body></html>",
        t = esc(&title),
        rig = if rig.is_empty() { String::new() } else { format!("{} · ", esc(&rig)) },
        count = poses.len(),
        n = if notes.is_empty() { String::new() } else { format!("<section class=notes>{notes}</section>") },
        lipsync = lipsync,
        cards = cards
    );

    let path = match &rig_path {
        Some(p) => p.with_file_name(format!("{}_review.html", p.file_stem().map_or("rig".into(), |s| s.to_string_lossy()))),
        None => utils::crashlog_file().with_file_name("review.html"),
    };
    std::fs::write(&path, html).map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    if args.get("open").and_then(|v| v.as_bool()).unwrap_or(true) {
        let _ = open::that(&path);
    }
    Ok(ToolResult::text(format!("Review page written to {} ({} poses){}", path.display(), poses.len(),
        if args.get("open").and_then(|v| v.as_bool()).unwrap_or(true) { ", opened in the browser" } else { "" })))
}

fn envelope_json(e: &envelope::Envelope) -> Value {
    json!({
        "deform": e.deform, "head_inner": r1(e.head_inner), "head_outer": r1(e.head_outer),
        "tail_inner": r1(e.tail_inner), "tail_outer": r1(e.tail_outer),
        "falloff": format!("{:?}", e.falloff), "absolute": e.absolute
    })
}

fn rig_json(armature: &Armature) -> Value {
    let world = skin::setup_world(armature);
    let bones: Vec<Value> = world
        .iter()
        .filter(|b| b.bind_owner.is_none() && armature.tex_of(b.id).is_none())
        .map(|b| {
            let tail = renderer::tail_pos(b);
            json!({
                "id": b.id, "name": b.name, "parent": b.parent_id,
                "head": [r1(b.pos.x), r1(b.pos.y)], "tail": [r1(tail.x), r1(tail.y)], "length": r1(b.length),
                "envelope": envelope_json(&skin::envelope_of(&world, b.id)),
                "ik_target": is_ik_target(&armature.bones, b.id),
            })
        })
        .collect();
    let meshes: Vec<Value> = world
        .iter()
        .filter(|b| b.bind_owner.is_none() && armature.tex_of(b.id).is_some())
        .map(|b| {
            let mesh = armature.bones.iter().find(|m| m.id == b.id).unwrap();
            let pos = if mesh.vertices.is_empty() { vec![b.pos] } else { bind_pose::drawn_vertices(armature, &world, b.id) };
            let bounds = pos.iter().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |a, p| (a.0.min(p.x), a.1.min(p.y), a.2.max(p.x), a.3.max(p.y)));
            json!({
                "id": b.id, "name": b.name, "parent": b.parent_id,
                "bounds": [r1(bounds.0), r1(bounds.1), r1(bounds.2), r1(bounds.3)],
                "vertices": mesh.vertices.len(), "triangles": mesh.indices.len() / 3,
                "has_mesh": !mesh.vertices.is_empty(),
                "skinned": mesh.bind_pose, "envelope_weights": mesh.envelope_weights,
                "skin_bones": skin::skin_bones(&armature.bones, mesh),
                "locked_vertices": mesh.locked_verts.len(), "outside_every_envelope": mesh.skin_outside.len(),
                "zindex": b.zindex,
            })
        })
        .collect();
    json!({
        "units": "world units at rest, y up",
        "bones": bones,
        "meshes": meshes,
        "note": "Meshes are textured bones. Bones with ik_target true are IK controls, not skeleton; skin meshes to the skeleton bones. A mesh without a mesh (has_mesh false) can't be weighted."
    })
}

fn render_options(armature: &Armature, world: &Vec<Bone>, args: &Value) -> Result<render::RenderOptions, String> {
    let mut opts = render::RenderOptions::default();
    opts.meshes = ints(args.get("meshes"));
    if let Some(size) = args.get("size").and_then(|v| v.as_u64()) {
        opts.size = size as u32;
    }
    for (key, field) in [("bones", &mut opts.bones), ("wireframe", &mut opts.wireframe), ("vertex_ids", &mut opts.vertex_ids)] {
        if let Some(v) = args.get(key).and_then(|v| v.as_bool()) {
            *field = v;
        }
    }
    opts.weights_bone = args.get("weights_bone").and_then(|v| v.as_i64()).map(|v| v as i32);
    if let Some(c) = args.get("crop").and_then(|v| v.as_array()) {
        let c: Vec<f32> = c.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
        if c.len() != 4 || c[2] <= c[0] || c[3] <= c[1] {
            return Err("crop is [x0, y0, x1, y1] with x1 > x0 and y1 > y0".into());
        }
        opts.crop = Some((c[0], c[1], c[2], c[3]));
    } else if let Some(m) = args.get("frame_mesh").and_then(|v| v.as_i64()).map(|v| v as i32) {
        bone_exists(armature, m)?;
        let pos = bind_pose::drawn_vertices(armature, world, m);
        if !pos.is_empty() {
            let b = pos.iter().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |a, p| (a.0.min(p.x), a.1.min(p.y), a.2.max(p.x), a.3.max(p.y)));
            let pad = ((b.2 - b.0).max(b.3 - b.1) * 0.08).max(10.);
            opts.crop = Some((b.0 - pad, b.1 - pad, b.2 + pad, b.3 + pad));
        }
    }
    Ok(opts)
}

fn picture_result(pic: &render::Picture, extra: String) -> ToolResult {
    let legend: Vec<String> =
        pic.legend.iter().map(|(id, name, c)| format!("{id} {name} #{:02x}{:02x}{:02x}", c[0], c[1], c[2])).collect();
    let mut text = pic.mapping();
    if !legend.is_empty() {
        text += &format!("\nBones (id name colour): {}", legend.join("; "));
    }
    text += &extra;
    ToolResult { content: vec![Content::Image(render::png(&pic.image)), Content::Text(text)], is_error: false }
}

fn pose_test(armature: &Armature, args: &Value) -> Result<ToolResult, String> {
    let (pic, report) = posed_picture(armature, args)?;
    let summary = if report.is_empty() {
        "\nDistortion: none worth noting (no folds, no triangle area change over 50%, stretch under 1.3).".to_string()
    } else {
        format!("\nDistortion (meshes with problems): {}", Value::Array(report))
    };
    Ok(picture_result(&pic, summary))
}

/// A test pose (`pose_test`'s arguments) rendered, and the meshes it distorts.
fn posed_picture(armature: &Armature, args: &Value) -> Result<(render::Picture, Vec<Value>), String> {
    let snapshot = bind_pose::pose_snapshot(armature);
    let mut posed = Armature { bones: armature.bones.clone(), styles: armature.styles.clone(), ..Default::default() };
    // IK would override the rotations being tested, so it's off unless asked for
    if !args.get("use_ik").and_then(|v| v.as_bool()).unwrap_or(false) {
        for b in &mut posed.bones {
            if let Some(ik) = &mut b.ik {
                ik.enabled = false;
            }
        }
    }
    let rotations = args.get("rotations").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let moves = args.get("moves").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    for r in &rotations {
        let id = int(r, "bone")?;
        bone_exists(armature, id)?;
        let deg = num(r, "degrees")?;
        posed.bones.iter_mut().find(|b| b.id == id).unwrap().rot += deg.to_radians();
    }
    for m in &moves {
        let id = int(m, "bone")?;
        bone_exists(armature, id)?;
        let b = posed.bones.iter_mut().find(|b| b.id == id).unwrap();
        b.pos += Vec2::new(num(m, "dx")?, num(m, "dy")?);
    }
    // helpers stay bound to the rest pose, so the meshes deform
    bind_pose::maintain_with_rest(&mut posed, Some(&snapshot));
    let mut world = posed.bones.clone();
    renderer::construction(&mut world, &posed.bones);
    let rest = skin::setup_world(armature);

    let opts = render_options(armature, &rest, args)?;
    let pic = render::render(armature, &world, &opts);

    let mut report = vec![];
    for b in armature.bones.iter().filter(|b| armature.tex_of(b.id).is_some() && !b.vertices.is_empty() && b.bind_owner.is_none()) {
        if !opts.meshes.is_empty() && !opts.meshes.contains(&b.id) {
            continue;
        }
        let d = render::distortion(b, &bind_pose::drawn_vertices(armature, &rest, b.id), &bind_pose::drawn_vertices(armature, &world, b.id));
        if d.flipped.is_empty() && d.area_changed.is_empty() && d.max_stretch < 1.3 {
            continue;
        }
        report.push(json!({
            "mesh": b.id, "name": b.name, "triangles": d.triangles,
            "flipped": d.flipped.len(), "area_changed": d.area_changed.len(), "max_stretch": r3(d.max_stretch),
            "worst_flipped": d.flipped.iter().take(8).collect::<Vec<_>>(),
            "worst_area_changed": d.area_changed.iter().take(8).collect::<Vec<_>>(),
        }));
    }
    Ok((pic, report))
}
