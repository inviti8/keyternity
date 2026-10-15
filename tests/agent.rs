//! The skinning agent's tools and MCP server (docs/AI_SKINNING.md §4, §7).

use keyternity_lib::agent::{self, Content};
use keyternity_lib::shared::*;
use keyternity_lib::skin;
use keyternity_lib::weights;
use serde_json::{json, Value};

fn bone(id: i32, parent_id: i32, name: &str, pos: Vec2) -> Bone {
    Bone {
        id,
        parent_id,
        name: name.to_string(),
        pos,
        scale: Vec2::new(1., 1.),
        pivot_scale: Vec2::new(1., 1.),
        tint: TintColor::new(1., 1., 1., 1.),
        physics_id: -1,
        visuals_id: -1,
        ..Default::default()
    }
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

const UPPER: i32 = 0;
const LOWER: i32 = 1;
const MESH: i32 = 2;

/// An arm skinned with Bind to Skeleton: upper (0..100) and lower (100..200) bones
/// along +x under a strip mesh of 21 columns of 2 vertices at y = ±10.
fn skinned_arm() -> Armature {
    let mut upper = bone(UPPER, -1, "upper", v(0., 0.));
    upper.length = 100.;
    let mut lower = bone(LOWER, UPPER, "lower", v(100., 0.));
    lower.length = 100.;
    let mut mesh = bone(MESH, -1, "arm art", v(0., 0.));
    mesh.tex = "arm".into();
    mesh.verts_edited = true;
    for i in 0..21 {
        for (j, y) in [-10., 10.].into_iter().enumerate() {
            let id = (i * 2 + j) as u32;
            mesh.vertices.push(Vertex { id, pos: v(i as f32 * 10., y), uv: v(0., 0.), ..Default::default() });
        }
    }
    for i in 0..20u32 {
        let (a, b, c, d) = (i * 2, i * 2 + 1, i * 2 + 2, i * 2 + 3);
        mesh.indices.extend([a, b, c, b, d, c]);
    }
    let mut a = Armature { bones: vec![upper, lower, mesh], ..Default::default() };
    // a 200x20 texture, so the mesh counts as textured and renders
    let size = Vec2::new(200., 20.);
    let tex = Texture { name: "arm".into(), size, data_id: 0, ..Default::default() };
    a.styles.push(Style { id: 0, name: "Default".into(), active: true, textures: vec![tex] });
    let img = image::RgbaImage::from_pixel(200, 20, image::Rgba([200, 120, 60, 255]));
    a.tex_data.push(TextureData { id: 0, image: image::DynamicImage::ImageRgba8(img), bind_group: None, ui_img: None });
    for (i, vert) in a.bones[2].vertices.iter_mut().enumerate() {
        vert.uv = v((i / 2) as f32 / 20., (i % 2) as f32);
    }
    skin::bind_to_skeleton(&mut a, MESH).unwrap();
    a
}

fn shared() -> Shared {
    let mut s = Shared::default();
    s.armature = skinned_arm();
    s
}

fn text(r: &agent::ToolResult) -> String {
    r.content.iter().filter_map(|c| if let Content::Text(t) = c { Some(t.clone()) } else { None }).collect::<Vec<_>>().join("
")
}

fn share(s: &Shared, vert: u32, bone: i32) -> f32 {
    let mesh = s.armature.bones.iter().find(|b| b.id == MESH).unwrap();
    weights::share_of(&skin::vertex_shares(&s.armature.bones, mesh)[&vert], bone)
}

#[test]
fn every_tool_has_a_strict_object_schema() {
    for t in agent::tools() {
        assert_eq!(t.schema["type"], "object", "{}", t.name);
        assert_eq!(t.schema["additionalProperties"], false, "{}", t.name);
        assert!(!t.description.is_empty());
    }
}

#[test]
fn get_rig_lists_bones_and_meshes() {
    let mut s = shared();
    let r = agent::call(&mut s, "get_rig", &json!({}));
    assert!(!r.is_error, "{}", text(&r));
    let rig: Value = serde_json::from_str(&text(&r)).unwrap();
    assert_eq!(rig["bones"].as_array().unwrap().len(), 2);
    let mesh = &rig["meshes"][0];
    assert_eq!(mesh["id"], MESH);
    assert_eq!(mesh["skinned"], true);
    assert_eq!(mesh["skin_bones"], json!([UPPER, LOWER]));
}

#[test]
fn assign_parts_through_the_tool_is_one_undo_step() {
    let mut s = shared();
    let undo = s.undo_states.undo_actions.len();
    let args = json!({ "mesh": MESH, "blend": 0, "parts": [
        { "bone": LOWER, "outline": [[-5, -50], [95, -50], [95, 50], [-5, 50]] },
        { "bone": UPPER, "outline": [[95, -50], [205, -50], [205, 50], [95, 50]] }
    ]});
    let r = agent::call(&mut s, "assign_parts", &args);
    assert!(!r.is_error, "{}", text(&r));
    assert_eq!(text(&r), "assigned 42 of 42 vertices");
    assert!((share(&s, 0, LOWER) - 1.).abs() < 1e-4);
    assert_eq!(s.undo_states.undo_actions.len(), undo + 1);

    // a failing call changes nothing and leaves no undo step
    let r = agent::call(&mut s, "assign_parts", &json!({ "mesh": 99, "blend": 0, "parts": [] }));
    assert!(r.is_error);
    assert_eq!(s.undo_states.undo_actions.len(), undo + 1);
}

#[test]
fn read_only_refuses_edits() {
    let mut s = shared();
    s.agent.read_only = true;
    assert!(agent::call(&mut s, "set_bone_length", &json!({ "bone": UPPER, "length": 50 })).is_error);
    assert!(!agent::call(&mut s, "get_rig", &json!({})).is_error);
}

#[test]
fn render_and_pose_test_return_pictures() {
    let mut s = shared();
    let r = agent::call(&mut s, "render", &json!({ "size": 400, "weights_bone": UPPER }));
    assert!(!r.is_error, "{}", text(&r));
    let Content::Image(png) = &r.content[0] else { panic!("no image") };
    let img = image::load_from_memory(png).unwrap();
    assert_eq!(img.width().max(img.height()), 400);
    assert!(text(&r).contains("world_x ="), "{}", text(&r));
    assert!(text(&r).contains("0 upper"), "{}", text(&r));

    let r = agent::call(&mut s, "pose_test", &json!({ "rotations": [{ "bone": LOWER, "degrees": 90 }], "size": 300 }));
    assert!(!r.is_error, "{}", text(&r));
    assert!(text(&r).contains("Distortion"), "{}", text(&r));
    // posing a copy leaves the rig alone
    assert_eq!(s.armature.bones.iter().find(|b| b.id == LOWER).unwrap().rot, 0.);
}

#[test]
fn checkpoints_revert_the_rig() {
    let mut s = shared();
    agent::call(&mut s, "checkpoint", &json!({ "name": "start" }));
    agent::call(&mut s, "set_weights", &json!({ "mesh": MESH, "weights": [{ "vertex": 0, "bone": LOWER, "share": 1 }] }));
    assert!((share(&s, 0, LOWER) - 1.).abs() < 1e-4);
    agent::call(&mut s, "revert", &json!({ "name": "start" }));
    assert!(share(&s, 0, LOWER) < 0.5);
}

/// The MCP server end to end: a client thread speaks JSON-RPC over HTTP while this
/// thread plays the app's main loop.
#[test]
fn mcp_server_answers_over_http() {
    let mut s = shared();
    s.config.agent_access = true;
    s.config.agent_port = 47990;
    s.config.agent_token = "test-token".into();
    agent::server::poll(&mut s);
    let url = s.agent.server.as_ref().expect("server didn't start").url.clone();

    let client = std::thread::spawn(move || {
        let post = |token: &str, body: Value| {
            ureq::post(&url)
                .header("Authorization", &format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .send(body.to_string())
                .map(|mut r| r.body_mut().read_to_string().unwrap_or_default())
        };
        assert!(post("wrong", json!({})).is_err(), "accepted a wrong token");
        let init = post("test-token", json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } } })).unwrap();
        let init: Value = serde_json::from_str(&init).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        post("test-token", json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).unwrap();
        let list: Value = serde_json::from_str(&post("test-token", json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap()).unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "assign_parts"));
        let call: Value = serde_json::from_str(&post("test-token", json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "render", "arguments": { "size": 128 } } })).unwrap()).unwrap();
        let content = call["result"]["content"].as_array().unwrap().clone();
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["mimeType"], "image/png");
        call["result"]["isError"].clone()
    });

    // the main loop: run queued calls until the client is done
    let start = std::time::Instant::now();
    while !client.is_finished() {
        agent::server::poll(&mut s);
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(start.elapsed().as_secs() < 30, "timed out");
    }
    assert_eq!(client.join().unwrap(), json!(false));
}

#[test]
fn review_page_writes_one_html_file_next_to_the_rig() {
    let mut s = shared();
    let dir = std::env::temp_dir().join("keyternity_review_test");
    std::fs::create_dir_all(&dir).unwrap();
    s.ui.save_path = Some(dir.join("arm.skf"));
    let args = json!({
        "title": "Arm <test>",
        "notes": "Bent the elbow.\n\nLooks fine.",
        "poses": [{ "name": "Elbow 90", "rotations": [{ "bone": LOWER, "degrees": 90 }], "note": "the bend" }],
        "weights_bones": [UPPER],
        "size": 240,
        "open": false
    });
    let r = agent::call(&mut s, "review_page", &args);
    assert!(!r.is_error, "{}", text(&r));
    let html = std::fs::read_to_string(dir.join("arm_review.html")).unwrap();
    assert!(html.contains("Arm &lt;test&gt;"), "title not escaped");
    assert_eq!(html.matches("<figure>").count(), 3, "rest, one pose, one weight view");
    assert!(html.contains("Elbow 90") && html.contains("Weights: upper"));
    assert!(html.contains("<p>Bent the elbow.</p><p>Looks fine.</p>"));
    assert!(html.contains("data:image/png;base64,"));
    let _ = std::fs::remove_dir_all(&dir);
}
