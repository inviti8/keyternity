//! The agent's shape, mouth, dialog and lip-sync tools (docs/LIP_SYNC.md §9), called
//! as an MCP client calls them.

use keyternity_lib::agent::{self, Content};
use keyternity_lib::audio;
use keyternity_lib::editor;
use keyternity_lib::face;
use keyternity_lib::shared::*;
use serde_json::{json, Value};

fn text(r: &agent::ToolResult) -> String {
    r.content.iter().filter_map(|c| if let Content::Text(t) = c { Some(t.clone()) } else { None }).collect::<Vec<_>>().join("\n")
}

fn ok(s: &mut Shared, name: &str, args: Value) -> Value {
    let r = agent::call(s, name, &args);
    assert!(!r.is_error, "{name}: {}", text(&r));
    serde_json::from_str(&text(&r)).unwrap_or(Value::String(text(&r)))
}

fn undo(s: &mut Shared) {
    s.events.undo();
    while !s.events.events.is_empty() {
        #[rustfmt::skip]
        editor::iterate_events(
            &s.input, &mut s.config, &mut s.events, &mut s.camera, &mut s.edit_mode, &mut s.selections,
            &mut s.undo_states, &mut s.armature, &mut s.psd_armature, &mut s.copy_buffer, &mut s.ui, &mut s.renderer,
        );
    }
}

/// a face mesh with the standard shapes and a face rig, and an empty animation
fn rig() -> Shared {
    let mut s = Shared::default();
    let v = |id: u32, x: f32, y: f32| Vertex { id, pos: Vec2::new(x, y), init_pos: Vec2::new(x, y), ..Default::default() };
    let mut mesh = Bone {
        id: 1,
        parent_id: -1,
        name: "face".into(),
        scale: Vec2::new(1., 1.),
        pivot_scale: Vec2::new(1., 1.),
        vertices: vec![v(0, -10., -15.), v(1, 10., -15.), v(2, 10., 10.), v(3, -10., 10.)],
        indices: vec![0, 1, 2, 0, 2, 3],
        ..Default::default()
    };
    face::add_standard_shapes(&mut mesh, false);
    s.armature.bones = vec![mesh];
    face::set_up(&mut s.armature, 1, "Face");
    s.armature.animations = vec![Animation { id: 0, name: "talk".into(), fps: 24, ..Default::default() }];
    s.selections.anim = 0;
    s
}

/// a WAV of tone bursts standing in for speech, in a temp file
fn speech_file(name: &str) -> std::path::PathBuf {
    let rate = 16000;
    let s: Vec<f32> = (0..rate * 2)
        .map(|i| {
            let t = i as f32 / rate as f32;
            if t < 0.3 || (1.0..1.3).contains(&t) || t > 1.8 { 0. } else { 0.5 * (t * 220. * std::f32::consts::TAU).sin() }
        })
        .collect();
    let p = std::env::temp_dir().join(format!("keyternity_agent_{name}_{}.wav", std::process::id()));
    std::fs::write(&p, audio::wav_bytes(&s, 1, rate as u32)).unwrap();
    p
}

#[test]
fn the_dialog_tools_are_offered() {
    let names: Vec<&str> = agent::tools().iter().map(|t| t.name).collect();
    for n in ["list_shapes", "set_shape_weight", "key_shapes", "list_mouth_rigs", "auto_fill_mouth_rig", "get_dialog", "add_dialog", "import_audio", "lip_sync"] {
        assert!(names.contains(&n), "{n}");
    }
    for t in agent::tools() {
        assert_eq!(t.schema["type"], "object", "{}", t.name);
    }
}

#[test]
fn shapes_are_listed_set_and_keyed() {
    let mut s = rig();
    let shapes = ok(&mut s, "list_shapes", json!({}));
    let blink = shapes["meshes"][0]["shapes"].as_array().unwrap().iter().find(|x| x["name"] == "Blink_L").unwrap()["id"].as_i64().unwrap();
    assert_eq!(shapes["face_rigs"][0]["controls"].as_array().unwrap().len(), 13);

    ok(&mut s, "set_shape_weight", json!({ "mesh": 1, "shape": blink, "weight": 0.4 }));
    assert_eq!(s.armature.bones[0].shapes.iter().find(|x| x.id == blink as i32).unwrap().weight, 0.4);
    undo(&mut s);
    assert_eq!(s.armature.bones[0].shapes.iter().find(|x| x.id == blink as i32).unwrap().weight, 0.);

    ok(&mut s, "key_shapes", json!({ "animation": "talk", "frame": 6, "keys": [{ "mesh": 1, "shape": blink, "weight": 1 }] }));
    let a = &s.armature.animations[0];
    assert!(a.keyframes.iter().any(|k| k.frame == 6 && k.element == AnimElement::ShapeWeight && k.shape == blink as i32 && k.value == 1.));
    undo(&mut s);
    assert!(s.armature.animations[0].keyframes.is_empty());
}

#[test]
fn mouths_are_listed_and_filled() {
    let mut s = rig();
    let m = ok(&mut s, "list_mouth_rigs", json!({}));
    let mouth = &m["mouths"][0];
    assert_eq!((mouth["kind"].as_str(), mouth["saved"].as_bool()), (Some("face rig"), Some(false)));
    let root = mouth["bone"].as_i64().unwrap();
    let ai = mouth["visemes"].as_array().unwrap().iter().find(|p| p["viseme"] == "AI").unwrap();
    assert!(ai["controls"].as_array().unwrap().iter().any(|c| c["control"] == "Jaw"));

    ok(&mut s, "auto_fill_mouth_rig", json!({ "mouth": root, "set": "Simple5" }));
    assert!(s.armature.bones.iter().find(|b| b.id == root as i32).unwrap().mouth_rig.is_some());
    let m = ok(&mut s, "list_mouth_rigs", json!({}));
    assert_eq!((m["mouths"][0]["saved"].as_bool(), m["mouths"][0]["set"].as_str()), (Some(true), Some("Simple5")));
    undo(&mut s);
    assert!(s.armature.bones.iter().find(|b| b.id == root as i32).unwrap().mouth_rig.is_none());

    let r = agent::call(&mut s, "auto_fill_mouth_rig", &json!({ "mouth": 9999 }));
    assert!(r.is_error);
}

#[test]
fn audio_is_placed_and_lip_synced() {
    let mut s = rig();
    let path = speech_file("line");
    let clip = ok(&mut s, "import_audio", json!({ "animation": 0, "path": path.to_string_lossy(), "start_frame": 4 }));
    let clip_id = clip["clip"].as_i64().unwrap();
    assert!((clip["seconds"].as_f64().unwrap() - 2.).abs() < 0.01);

    // no words, no loudness: refused, and no undo step left behind
    let steps = s.undo_states.undo_actions.len();
    let r = agent::call(&mut s, "lip_sync", &json!({ "animation": 0, "clip": clip_id }));
    assert!(r.is_error && text(&r).contains("transcript"));
    assert_eq!(s.undo_states.undo_actions.len(), steps);

    let r = ok(&mut s, "lip_sync", json!({ "animation": 0, "clip": clip_id, "transcript": "Hello there. How are you?", "settings": { "lead": 2, "smoothing": "Low" } }));
    assert!(r["keys"].as_i64().unwrap() > 0);
    assert_eq!(r["words"].as_array().unwrap().len(), 5);
    let t = &s.armature.animations[0].lipsync[0];
    assert_eq!((t.settings.lead, t.settings.smoothing), (2, keyternity_lib::lipsync::Smoothing::Low));

    let d = ok(&mut s, "get_dialog", json!({}));
    let a = &d["animations"][0];
    assert_eq!(a["clips"][0]["start_frame"], 4);
    assert_eq!(a["lip_sync"][0]["words"].as_array().unwrap().len(), 5);

    // lip_sync is one undo step
    undo(&mut s);
    assert!(s.armature.animations[0].lipsync.is_empty() && s.armature.animations[0].layers.is_empty());
    assert_eq!(s.armature.animations[0].clips.len(), 1);
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_review_page_shows_lip_sync_strips() {
    let mut s = rig();
    let path = speech_file("review");
    let clip = ok(&mut s, "import_audio", json!({ "animation": 0, "path": path.to_string_lossy() }));
    ok(&mut s, "lip_sync", json!({ "animation": 0, "clip": clip["clip"], "transcript": "Hello there. How are you?" }));
    let dir = std::env::temp_dir().join(format!("keyternity_lipsync_review_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    s.ui.save_path = Some(dir.join("talk.skf"));
    let r = agent::call(&mut s, "review_page", &json!({ "title": "Dialog", "lipsync": [{ "animation": 0, "every": 4 }], "size": 240, "open": false }));
    assert!(!r.is_error, "{}", text(&r));
    let html = std::fs::read_to_string(dir.join("talk_review.html")).unwrap();
    assert!(html.contains("class=lipsync") && html.contains("class=transcript"));
    // 2 s at 24 fps, every 4th frame
    assert_eq!(html.matches("<figure class=tile>").count(), 13);
    assert!(html.contains("<span title=") && html.contains("Hello"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(path);
}

#[test]
fn read_only_refuses_the_editing_tools() {
    let mut s = rig();
    s.agent.read_only = true;
    assert!(agent::call(&mut s, "lip_sync", &json!({ "animation": 0, "clip": 1 })).is_error);
    assert!(!agent::call(&mut s, "list_mouth_rigs", &json!({})).is_error);
}

/// Opt-in: speak a line with this computer's voice through `add_dialog`.
/// `cargo test --test all -- agent_dialog:: --ignored`
#[test]
#[ignore]
fn add_dialog_speaks_and_lip_syncs() {
    let mut s = rig();
    let voices = keyternity_lib::voice::system_voices();
    let r = ok(&mut s, "add_dialog", json!({ "animation": 0, "text": "Hello there, how are you?", "voice": { "provider": "System", "id": voices[0].id }, "start_frame": 6 }));
    assert_eq!(r["lip_synced"], true, "{r}");
    assert_eq!(r["start_frame"], 6);
    assert_eq!(r["words"].as_array().unwrap().len(), 5);
    let a = &s.armature.animations[0];
    assert_eq!((a.dialog.len(), a.clips.len(), a.lipsync.len()), (1, 1, 1));
    assert!(a.lipsync[0].exact, "the system voice's word times are used");
    undo(&mut s);
    assert!(s.armature.animations[0].dialog.is_empty());
}
