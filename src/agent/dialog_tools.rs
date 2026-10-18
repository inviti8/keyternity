//! The agent's shape, mouth, dialog and lip-sync tools (docs/LIP_SYNC.md §9): what an
//! agent needs to block out a scene's dialog for the artist to review and refine.
//! Like the skinning tools, they run on the main thread between frames, and each edit
//! is one undo step.

#[cfg_attr(target_arch = "wasm32", allow(unused_imports))]
use super::{int, num, opt_num, Tool, ToolResult, Undo};
#[cfg_attr(target_arch = "wasm32", allow(unused_imports))]
use crate::dialog::{DialogLine, Provider, Take, Voice};
use crate::lipsync::{self, KeyerSettings, Language, LipSyncTrack, Smoothing, TrackSource, VisemeSet};
use crate::*;
use serde_json::{json, Value};

/// What the agent is told about the dialog tools, and the `lip_sync_dialog` prompt.
pub const WORKFLOW: &str = "Block out this scene's dialog:\n\
1. get_dialog for the animations, their clips, dialog lines and lip sync, and the voices on offer; list_mouth_rigs for the rig's mouths (a face rig, viseme shapes, or drawn mouths).\n\
2. If a mouth has no saved rig, auto_fill_mouth_rig shows what lip sync will use; that's usually fine as it is.\n\
3. For each line: add_dialog with the words and a voice that suits the character (it's spoken, placed at start_frame, and lip-synced). For recorded lines, import_audio, then lip_sync with the transcript (or transcribe: true to have Whisper listen).\n\
4. Adjust with lip_sync's settings (lead, min_hold, smoothing, intensity) if the mouth reads too busy or too slack; key_shapes for expressions (brows, smiles) on top.\n\
5. Finish with review_page with lipsync set for each line, and notes on voices, timing and anything to check, so the artist can review the frame strips with the transcript.";

fn anim_schema() -> Value {
    json!({ "description": "animation: its index (from get_dialog) or name" })
}

/// The animation an argument names (index or name).
pub fn anim_index(armature: &Armature, args: &Value) -> Result<usize, String> {
    let v = args.get("animation").ok_or("`animation` (index or name) is required")?;
    let i = match v {
        Value::Number(n) => n.as_u64().map(|n| n as usize),
        Value::String(s) => armature.animations.iter().position(|a| a.name == *s),
        _ => None,
    };
    i.filter(|i| *i < armature.animations.len()).ok_or(format!("no animation {v}; get_dialog lists them"))
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

pub fn tools() -> Vec<Tool> {
    let settings = json!({
        "type": "object",
        "description": "keyer settings (all optional)",
        "properties": {
            "lead": { "type": "integer", "description": "frames the mouth leads the sound (default 1)" },
            "min_hold": { "type": "integer", "description": "fewest frames a mouth shape shows (default 2)" },
            "smoothing": { "type": "string", "enum": ["Off", "Low", "Medium", "High"] },
            "intensity": { "type": "number", "description": "1 = as posed (0-1.5)" },
            "rest_gap": { "type": "number", "description": "silences at least this long (s) close the mouth (default 0.2)" }
        }
    });
    let language = json!({ "type": "string", "enum": ["en", "de", "fr", "pt", "fi", "lt"], "description": "the line's language (default en)" });
    vec![
        Tool {
            name: "list_shapes",
            description: "The rig's shapes (morph targets): each mesh with shapes, and each shape's id, name, group and setup weight; and each face rig (root bone and its controls).",
            schema: schema(json!({}), &[]),
            edits: false,
            undo: Undo::None,
        },
        Tool {
            name: "set_shape_weight",
            description: "Set a shape's setup weight (0-1.5): how much of it shows when nothing keys it.",
            schema: schema(json!({ "mesh": { "type": "integer" }, "shape": { "type": "integer" }, "weight": { "type": "number" } }), &["mesh", "shape", "weight"]),
            edits: true,
            undo: Undo::Bones,
        },
        Tool {
            name: "key_shapes",
            description: "Key shape weights on a frame of an animation (on the layer being edited), e.g. a smile or raised brows over the lip sync.",
            schema: schema(
                json!({
                    "animation": anim_schema(),
                    "frame": { "type": "integer" },
                    "keys": { "type": "array", "items": schema(json!({ "mesh": { "type": "integer" }, "shape": { "type": "integer" }, "weight": { "type": "number" } }), &["mesh", "shape", "weight"]) }
                }),
                &["animation", "frame", "keys"],
            ),
            edits: true,
            undo: Undo::Anim,
        },
        Tool {
            name: "list_mouth_rigs",
            description: "The bones that can host a mouth (face rigs' roots, meshes with viseme shapes, bones with viseme textures) and each one's mouth rig: its viseme set, and what each viseme uses (face-rig controls, shapes, a texture). 'saved' false means it's filled in automatically.",
            schema: schema(json!({}), &[]),
            edits: false,
            undo: Undo::None,
        },
        Tool {
            name: "auto_fill_mouth_rig",
            description: "Fill a mouth's rig automatically (viseme shapes, viseme textures, or built-in face-rig poses) and save it on the mouth's bone. set: PrestonBlair10 (default), Oculus15 or Simple5.",
            schema: schema(json!({ "mouth": { "type": "integer", "description": "the mouth's bone, from list_mouth_rigs" }, "set": { "type": "string", "enum": ["PrestonBlair10", "Oculus15", "Simple5"] } }), &["mouth"]),
            edits: true,
            undo: Undo::Bones,
        },
        Tool {
            name: "get_dialog",
            description: "Every animation (index, name, fps, length) with its audio clips, dialog lines (text, voice, takes) and lip-sync tracks (words and their times); and the voices available for add_dialog.",
            schema: schema(json!({}), &[]),
            edits: false,
            undo: Undo::None,
        },
        Tool {
            name: "add_dialog",
            description: "Speak a line with a voice and place it on an animation at start_frame (default 0), lip-synced on the rig's mouth if it has one. voice: {provider: Kokoro|System|Hosted, id} (Hosted: Chatterbox on the hosted service, id chatterbox) from get_dialog's voices (default Kokoro af_heart). Takes a few seconds. Returns the clip, its length, and the words' times.",
            schema: schema(
                json!({
                    "animation": anim_schema(),
                    "text": { "type": "string" },
                    "voice": schema(json!({ "provider": { "type": "string", "enum": ["Kokoro", "System", "Hosted"] }, "id": { "type": "string" } }), &["provider", "id"]),
                    "language": language,
                    "speed": { "type": "number", "description": "1 = normal (0.5-2)" },
                    "start_frame": { "type": "integer" }
                }),
                &["animation", "text"],
            ),
            edits: true,
            undo: Undo::Anim,
        },
        Tool {
            name: "import_audio",
            description: "Place an audio file (wav, mp3, ogg, flac, m4a; a path on this computer) on an animation at start_frame (default 0). It's saved inside the rig file. Then lip_sync it.",
            schema: schema(json!({ "animation": anim_schema(), "path": { "type": "string" }, "start_frame": { "type": "integer" } }), &["animation", "path"]),
            edits: true,
            undo: Undo::Anim,
        },
        Tool {
            name: "lip_sync",
            description: "Lip-sync an audio clip: its words (transcript) spread over the speech in it, or the mouth opening with its loudness (source: loudness), keyed in the clip's own Lip Sync layer (regenerated if it exists). transcribe: true has Whisper listen first (it must have been installed from the Lip Sync window); the transcript, if given, is still the words used. mouth defaults to the rig's first.",
            schema: schema(
                json!({
                    "animation": anim_schema(),
                    "clip": { "type": "integer" },
                    "transcript": { "type": "string" },
                    "mouth": { "type": "integer" },
                    "language": language,
                    "source": { "type": "string", "enum": ["words", "loudness"] },
                    "transcribe": { "type": "boolean" },
                    "settings": settings
                }),
                &["animation", "clip"],
            ),
            edits: true,
            undo: Undo::Anim,
        },
    ]
}

fn language_of(args: &Value) -> Result<Language, String> {
    match args.get("language").and_then(|v| v.as_str()).unwrap_or("en") {
        "en" => Ok(Language::English),
        "de" => Ok(Language::German),
        "fr" => Ok(Language::French),
        "pt" => Ok(Language::PortugueseBr),
        "fi" => Ok(Language::Finnish),
        "lt" => Ok(Language::Lithuanian),
        l => Err(format!("unknown language `{l}`")),
    }
}

fn r2(x: f32) -> f32 {
    (x * 100.).round() / 100.
}

fn words_json(words: &[lipsync::TimedWord]) -> Value {
    Value::Array(words.iter().map(|w| json!([w.text, r2(w.start), r2(w.end)])).collect())
}

/// A pose's parts, briefly.
fn pose_json(armature: &Armature, p: &lipsync::MouthPose) -> Value {
    let shape = |s: &lipsync::PoseShape| armature.bones.iter().find(|b| b.id == s.mesh).and_then(|b| b.shapes.iter().find(|x| x.id == s.shape)).map_or("?".into(), |x| x.name.clone());
    json!({
        "viseme": p.viseme,
        "controls": p.controls.iter().map(|c| json!({ "control": c.control, "x": c.x, "y": c.y, "twist": c.twist, "depth": c.depth })).collect::<Vec<_>>(),
        "shapes": p.shapes.iter().map(shape).collect::<Vec<_>>(),
        "texture": p.texture.as_ref().map(|t| t.tex.clone()),
    })
}

/// Run one of these tools, if `name` is one.
pub fn run(shared: &mut Shared, name: &str, args: &Value) -> Option<Result<ToolResult, String>> {
    Some(match name {
        "list_shapes" => Ok(list_shapes(&shared.armature)),
        "set_shape_weight" => set_shape_weight(&mut shared.armature, args),
        "key_shapes" => key_shapes(&mut shared.armature, args),
        "list_mouth_rigs" => Ok(list_mouth_rigs(&shared.armature)),
        "auto_fill_mouth_rig" => auto_fill_mouth_rig(&mut shared.armature, args),
        "get_dialog" => Ok(get_dialog(shared)),
        "add_dialog" => add_dialog(shared, args),
        "import_audio" => import_audio(&mut shared.armature, args),
        "lip_sync" => lip_sync(&mut shared.armature, args),
        _ => return None,
    })
}

fn list_shapes(armature: &Armature) -> ToolResult {
    let meshes: Vec<Value> = armature
        .bones
        .iter()
        .filter(|b| !b.shapes.is_empty())
        .map(|b| {
            json!({
                "mesh": b.id, "name": b.name,
                "shapes": b.shapes.iter().map(|s| json!({ "id": s.id, "name": s.name, "group": s.group, "weight": s.weight })).collect::<Vec<_>>()
            })
        })
        .collect();
    let rigs: Vec<Value> = crate::face_panel::rigs(armature)
        .into_iter()
        .map(|(root, cs)| json!({ "root": root, "controls": cs.iter().map(|(n, id)| json!({ "name": n, "bone": id })).collect::<Vec<_>>() }))
        .collect();
    ToolResult::json(json!({ "meshes": meshes, "face_rigs": rigs, "note": "face rigs drive shapes from their controls; lip sync keys the controls" }))
}

fn set_shape_weight(armature: &mut Armature, args: &Value) -> Result<ToolResult, String> {
    let (mesh, shape, weight) = (int(args, "mesh")?, int(args, "shape")?, num(args, "weight")?.clamp(0., 1.5));
    let b = armature.bones.iter_mut().find(|b| b.id == mesh).ok_or(format!("no mesh {mesh}"))?;
    let s = b.shapes.iter_mut().find(|s| s.id == shape).ok_or(format!("mesh {mesh} has no shape {shape}; list_shapes lists them"))?;
    s.weight = weight;
    Ok(ToolResult::text(format!("{} = {weight}", s.name)))
}

fn key_shapes(armature: &mut Armature, args: &Value) -> Result<ToolResult, String> {
    let a = anim_index(armature, args)?;
    let frame = int(args, "frame")?.max(0);
    let keys = args.get("keys").and_then(|v| v.as_array()).ok_or("`keys` is required")?.clone();
    let anim = &armature.animations[a];
    if anim.layers.get(anim.active_layer).is_some_and(|l| l.locked) {
        return Err("the layer being edited is locked (a Lip Sync layer?); select another layer in the timeline".into());
    }
    let (ease_in, ease_out) = utils::interp_preset(HandlePreset::SineInOut);
    let mut n = 0;
    for k in &keys {
        let (mesh, shape, weight) = (int(k, "mesh")?, int(k, "shape")?, num(k, "weight")?.clamp(0., 1.5));
        let ok = armature.bones.iter().any(|b| b.id == mesh && b.shapes.iter().any(|s| s.id == shape));
        if !ok {
            return Err(format!("mesh {mesh} has no shape {shape}"));
        }
        let anim = &mut armature.animations[a];
        anim.keyframes.retain(|kf| !(kf.bone_id == mesh && kf.element == AnimElement::ShapeWeight && kf.shape == shape && kf.frame == frame));
        anim.keyframes.push(Keyframe {
            frame,
            bone_id: mesh,
            element: AnimElement::ShapeWeight,
            shape,
            value: weight,
            start_handle: ease_in,
            end_handle: ease_out,
            handle_preset: HandlePreset::SineInOut,
            ..Default::default()
        });
        n += 1;
    }
    armature.animations[a].sort_keyframes();
    Ok(ToolResult::text(format!("{n} shape keys on frame {frame}")))
}

fn list_mouth_rigs(armature: &Armature) -> ToolResult {
    let faces = crate::face_panel::rigs(armature);
    let mouths: Vec<Value> = lipsync::mouth_hosts(armature)
        .into_iter()
        .map(|id| {
            let b = armature.bones.iter().find(|b| b.id == id).unwrap();
            let kind = if faces.iter().any(|(r, _)| *r == id) {
                "face rig"
            } else if !b.shapes.is_empty() {
                "viseme shapes"
            } else {
                "drawn (textures)"
            };
            let rig = lipsync::rig_for(armature, id, None);
            json!({
                "bone": id, "name": b.name, "kind": kind, "saved": b.mouth_rig.is_some(),
                "set": format!("{:?}", rig.set), "overrides": rig.overrides,
                "visemes": rig.poses.iter().map(|p| pose_json(armature, p)).collect::<Vec<_>>()
            })
        })
        .collect();
    ToolResult::json(json!({ "mouths": mouths, "note": "control values are fractions of each control's travel (-1..1); depth is the lips' and jaw's in/out" }))
}

fn auto_fill_mouth_rig(armature: &mut Armature, args: &Value) -> Result<ToolResult, String> {
    let mouth = int(args, "mouth")?;
    if !lipsync::mouth_hosts(armature).contains(&mouth) {
        return Err(format!("bone {mouth} can't host a mouth; list_mouth_rigs lists the ones that can"));
    }
    let set = match args.get("set").and_then(|v| v.as_str()).unwrap_or("PrestonBlair10") {
        "PrestonBlair10" => VisemeSet::PrestonBlair10,
        "Oculus15" => VisemeSet::Oculus15,
        "Simple5" => VisemeSet::Simple5,
        s => return Err(format!("unknown set `{s}`")),
    };
    let rig = lipsync::auto_fill(armature, mouth, set);
    let poses: Vec<Value> = rig.poses.iter().map(|p| pose_json(armature, p)).collect();
    armature.bones.iter_mut().find(|b| b.id == mouth).unwrap().mouth_rig = Some(rig);
    Ok(ToolResult::json(json!({ "mouth": mouth, "saved": true, "visemes": poses })))
}

fn get_dialog(shared: &Shared) -> ToolResult {
    let armature = &shared.armature;
    let anims: Vec<Value> = armature
        .animations
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let secs = |audio: i32| armature.audio.iter().find(|d| d.id == audio).map_or(0., |d| r2(d.duration()));
            json!({
                "index": i, "name": a.name, "fps": a.fps, "last_frame": a.last_frame(),
                "clips": a.clips.iter().map(|c| json!({ "id": c.id, "name": c.name, "start_frame": c.start_frame, "seconds": secs(c.audio), "muted": c.muted })).collect::<Vec<_>>(),
                "dialog": a.dialog.iter().map(|l| json!({
                    "id": l.id, "text": l.text, "voice": format!("{} {}", l.voice.provider.label(), l.voice.id), "clip": l.clip, "take": l.take,
                    "takes": l.takes.iter().map(|t| json!({ "id": t.id, "voice": t.voice, "seconds": r2(t.secs) })).collect::<Vec<_>>()
                })).collect::<Vec<_>>(),
                "lip_sync": a.lipsync.iter().map(|t| json!({
                    "clip": t.clip, "mouth": t.mouth, "language": t.language.code(), "source": format!("{:?}", t.source), "words": words_json(&t.words)
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    #[cfg(not(target_arch = "wasm32"))]
    let voices = {
        let kokoro: Vec<Value> = crate::voice::KOKORO_VOICES
            .iter()
            .map(|(_, id)| {
                let (lang, name) = crate::voice::kokoro_voice_info(id);
                json!({ "provider": "Kokoro", "id": id, "name": name, "language": lang.code() })
            })
            .collect();
        let m = &crate::voice::KOKORO_MODELS[shared.config.kokoro_model.min(crate::voice::KOKORO_MODELS.len() - 1)];
        let installed = crate::voice::kokoro_tool().is_some() && crate::voice::has_kokoro_model(m);
        let system: Vec<Value> = crate::voice::system_voices().iter().map(|v| json!({ "provider": "System", "id": v.id, "name": v.name })).collect();
        json!({
            "kokoro": { "installed": installed, "voices": kokoro, "note": if installed { "" } else { "not installed: the artist installs it by generating one line in the Dialog window" } },
            "system": system,
        })
    };
    #[cfg(target_arch = "wasm32")]
    let voices = json!("voice generation is desktop only");
    ToolResult::json(json!({ "animations": anims, "voices": voices, "mouths": lipsync::mouth_hosts(armature) }))
}

#[cfg(not(target_arch = "wasm32"))]
fn add_dialog(shared: &mut Shared, args: &Value) -> Result<ToolResult, String> {
    let a = anim_index(&shared.armature, args)?;
    let text = args.get("text").and_then(|v| v.as_str()).ok_or("`text` is required")?.trim().to_string();
    if text.is_empty() {
        return Err("`text` is empty".into());
    }
    let voice = match args.get("voice") {
        Some(v) => {
            let provider = match v.get("provider").and_then(|p| p.as_str()).unwrap_or("") {
                "Kokoro" => Provider::Kokoro,
                "System" => Provider::System,
                "Hosted" => Provider::Hosted,
                p => return Err(format!("unknown provider `{p}` (Kokoro, System or Hosted)")),
            };
            let id = v.get("id").and_then(|i| i.as_str()).ok_or("voice `id` is required")?.to_string();
            let name = match provider {
                Provider::Kokoro => crate::voice::kokoro_voice_info(&id).1,
                _ => id.clone(),
            };
            Voice { provider, id, name }
        }
        None => Voice { provider: Provider::Kokoro, id: "af_heart".into(), name: crate::voice::kokoro_voice_info("af_heart").1 },
    };
    if voice.provider == Provider::Kokoro {
        let m = &crate::voice::KOKORO_MODELS[shared.config.kokoro_model.min(crate::voice::KOKORO_MODELS.len() - 1)];
        if crate::voice::kokoro_tool().is_none() || !crate::voice::has_kokoro_model(m) {
            return Err("Kokoro isn't installed yet: ask the artist to generate one line in the Dialog window (it downloads it), or use a System voice".into());
        }
    }
    let opts = crate::dialog::VoiceOpts { speed: opt_num(args, "speed").unwrap_or(1.).clamp(0.5, 2.), language: language_of(args)?, ..Default::default() };
    let cfg = crate::voice::VoiceConfig {
        kokoro_model: shared.config.kokoro_model,
        chatterbox_runner: String::new(),
        hosted_endpoint: shared.config.hosted_endpoint.clone(),
        hosted_key: shared.config.hosted_key.clone(),
    };
    let speech = crate::voice::synthesize(&text, &voice, &opts, &cfg)?;

    let anim = &shared.armature.animations[a];
    let line_id = crate::audio::next_id(anim.dialog.iter().map(|l| l.id));
    let audio_id = crate::audio::next_id(shared.armature.audio.iter().map(|d| d.id));
    let data = crate::audio::decode(audio_id, &crate::dialog::clip_name(&text), &speech.ext, speech.bytes)?;
    let take = Take { id: 1, audio: audio_id, voice: voice.name.clone(), speed: opts.speed, secs: data.duration(), words: speech.words.unwrap_or_default() };
    let secs = data.duration();
    let line = DialogLine { id: line_id, text, voice, opts, start_frame: args.get("start_frame").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as i32, ..Default::default() };
    let mut pending = vec![data];
    let arm = &mut shared.armature;
    crate::dialog::dialog_event(&Events::SetDialogLine, 1., &serde_json::to_string(&line).unwrap(), arm, a, &mut pending);
    let problem = crate::dialog::dialog_event(&Events::AddTake, line_id as f32, &serde_json::to_string(&(line_id, &take)).unwrap(), arm, a, &mut pending);
    let anim = &arm.animations[a];
    let l = anim.dialog.iter().find(|l| l.id == line_id).unwrap();
    let track = anim.lipsync.iter().find(|t| t.clip == l.clip);
    Ok(ToolResult::json(json!({
        "line": line_id, "clip": l.clip, "seconds": r2(secs), "start_frame": l.start_frame,
        "end_frame": l.start_frame + (secs * anim.fps as f32).ceil() as i32,
        "lip_synced": track.is_some(), "words": track.map(|t| words_json(&t.words)),
        "problem": problem,
    })))
}

#[cfg(target_arch = "wasm32")]
fn add_dialog(_: &mut Shared, _: &Value) -> Result<ToolResult, String> {
    Err("voice generation is desktop only".into())
}

#[cfg(not(target_arch = "wasm32"))]
fn import_audio(armature: &mut Armature, args: &Value) -> Result<ToolResult, String> {
    let a = anim_index(armature, args)?;
    let path = std::path::PathBuf::from(args.get("path").and_then(|v| v.as_str()).ok_or("`path` is required")?);
    let bytes = std::fs::read(&path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("audio").to_string();
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("wav").to_string();
    let id = crate::audio::next_id(armature.audio.iter().map(|d| d.id));
    let data = crate::audio::decode(id, &name, &ext, bytes)?;
    let secs = data.duration();
    armature.audio.push(data);
    let anim = &mut armature.animations[a];
    let clip = crate::audio::next_id(anim.clips.iter().map(|c| c.id));
    let start_frame = args.get("start_frame").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as i32;
    anim.clips.push(crate::audio::AudioClip { id: clip, audio: id, name, start_frame, ..Default::default() });
    Ok(ToolResult::json(json!({ "clip": clip, "seconds": r2(secs), "start_frame": start_frame })))
}

#[cfg(target_arch = "wasm32")]
fn import_audio(_: &mut Armature, _: &Value) -> Result<ToolResult, String> {
    Err("audio import is desktop only".into())
}

fn lip_sync(armature: &mut Armature, args: &Value) -> Result<ToolResult, String> {
    let a = anim_index(armature, args)?;
    let clip_id = int(args, "clip")?;
    let anim = &armature.animations[a];
    let clip = anim.clips.iter().find(|c| c.id == clip_id).ok_or(format!("animation {a} has no clip {clip_id}; get_dialog lists them"))?.clone();
    let existing = anim.lipsync.iter().find(|t| t.clip == clip_id).cloned();
    let hosts = lipsync::mouth_hosts(armature);
    let mouth = match args.get("mouth").and_then(|v| v.as_i64()) {
        Some(m) if hosts.contains(&(m as i32)) => m as i32,
        Some(m) => return Err(format!("bone {m} can't host a mouth; list_mouth_rigs lists the ones that can")),
        None => existing.as_ref().map(|t| t.mouth).filter(|m| hosts.contains(m)).or(hosts.first().copied()).ok_or("the rig has no mouth: set up a face rig, or add viseme shapes or textures")?,
    };
    let mut track = existing.unwrap_or(LipSyncTrack { id: crate::audio::next_id(anim.lipsync.iter().map(|t| t.id)), clip: clip_id, ..Default::default() });
    track.mouth = mouth;
    if args.get("language").is_some() {
        track.language = language_of(args)?;
    }
    if let Some(t) = args.get("transcript").and_then(|v| v.as_str()) {
        track.text = t.to_string();
        track.exact = false;
    }
    track.source = match args.get("source").and_then(|v| v.as_str()) {
        Some("loudness") => TrackSource::Loudness,
        Some("words") | None => TrackSource::Text,
        Some(s) => return Err(format!("unknown source `{s}`")),
    };
    if let Some(s) = args.get("settings") {
        let d = KeyerSettings::default();
        let get_i = |k: &str, def: i32| s.get(k).and_then(|v| v.as_i64()).map_or(def, |v| v as i32);
        let get_f = |k: &str, def: f32| s.get(k).and_then(|v| v.as_f64()).map_or(def, |v| v as f32);
        track.settings = KeyerSettings {
            lead: get_i("lead", d.lead).clamp(0, 6),
            min_hold: get_i("min_hold", d.min_hold).clamp(1, 8),
            smoothing: match s.get("smoothing").and_then(|v| v.as_str()) {
                Some("Off") => Smoothing::Off,
                Some("Low") => Smoothing::Low,
                Some("High") => Smoothing::High,
                Some("Medium") | None => Smoothing::Medium,
                Some(x) => return Err(format!("unknown smoothing `{x}`")),
            },
            intensity: get_f("intensity", d.intensity).clamp(0., 1.5),
            rest_gap: get_f("rest_gap", d.rest_gap).clamp(0.05, 1.),
        };
    }
    #[cfg(not(target_arch = "wasm32"))]
    if args.get("transcribe").and_then(|v| v.as_bool()).unwrap_or(false) {
        let data = armature.audio.iter().find(|d| d.id == clip.audio).ok_or("the clip's audio isn't loaded")?;
        let model = crate::transcribe::MODELS.iter().find(|m| crate::transcribe::has_model(m)).ok_or("no Whisper model is installed: the artist installs one with Transcribe in the Lip Sync window")?;
        let heard = crate::transcribe::transcribe(data, model, track.language)?;
        if track.text.trim().is_empty() {
            track.text = heard.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        }
        track.heard = heard;
        track.exact = false;
    }
    if track.source == TrackSource::Text && track.text.trim().is_empty() {
        return Err("give the clip's transcript (or transcribe: true, or source: loudness)".into());
    }
    let id = track.id;
    let anim = &mut armature.animations[a];
    match anim.lipsync.iter_mut().find(|t| t.id == id) {
        Some(t) => *t = track,
        None => anim.lipsync.push(track),
    }
    let keys = lipsync::generate(armature, a, id)?;
    let t = armature.animations[a].lipsync.iter().find(|t| t.id == id).unwrap();
    Ok(ToolResult::json(json!({ "clip": clip_id, "mouth": mouth, "keys": keys, "words": words_json(&t.words), "layer": format!("Lip Sync: {}", clip.name) })))
}

// ---------------------------------------------------------------- the review page's lip sync

/// The review page's lip-sync section: frame strips of each requested clip, each frame
/// labelled with its time, viseme and word, under the transcript.
#[cfg(not(target_arch = "wasm32"))]
pub fn review_section(armature: &Armature, specs: &[Value], size: u32) -> Result<String, String> {
    use base64::Engine;
    let b64 = |png: &[u8]| base64::engine::general_purpose::STANDARD.encode(png);
    let mut html = String::new();
    for spec in specs {
        let a = anim_index(armature, spec)?;
        let anim = &armature.animations[a];
        let clip_id = spec.get("clip").and_then(|v| v.as_i64()).map(|c| c as i32);
        let track = anim.lipsync.iter().find(|t| clip_id.is_none_or(|c| t.clip == c)).ok_or(format!("animation {a} has no lip sync{}", clip_id.map_or(String::new(), |c| format!(" for clip {c}"))))?;
        let clip = anim.clips.iter().find(|c| c.id == track.clip).ok_or("the lip sync's clip is gone")?;
        let fps = anim.fps.max(1);
        let rig = lipsync::rig_for(armature, track.mouth, None);
        let cues = lipsync::frame_cues(
            &match track.source {
                TrackSource::Text => lipsync::cues_from_words(&track.words, track.settings.rest_gap, track.language),
                TrackSource::Loudness => {
                    let d = armature.audio.iter().find(|d| d.id == clip.audio).ok_or("the clip's audio isn't loaded")?;
                    lipsync::cues_from_loudness(&d.samples, d.channels, d.sample_rate, fps)
                }
            },
            &rig,
            fps,
            clip.start_frame,
            &track.settings,
        );
        let secs = armature.audio.iter().find(|d| d.id == clip.audio).map_or(1., |d| d.duration());
        let first = spec.get("from").and_then(|v| v.as_i64()).map_or(clip.start_frame.max(0), |v| v as i32);
        let last = spec.get("to").and_then(|v| v.as_i64()).map_or(clip.start_frame + (secs * fps as f32).ceil() as i32, |v| v as i32);
        let every = spec.get("every").and_then(|v| v.as_i64()).unwrap_or(2).clamp(1, 24) as i32;
        let frame_mesh = spec.get("frame_mesh").and_then(|v| v.as_i64()).map(|m| m as i32);

        // a fixed crop for the whole strip, so the face doesn't jump between frames
        let rest_world = skin::setup_world(armature);
        let crop = frame_mesh.and_then(|m| {
            let mesh = rest_world.iter().find(|b| b.id == m)?;
            let pts = bind_pose::drawn_vertices(armature, &rest_world, mesh.id);
            let (mut lo, mut hi) = (Vec2::new(f32::MAX, f32::MAX), Vec2::new(f32::MIN, f32::MIN));
            for p in pts {
                lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
            }
            let pad = (hi.x - lo.x).max(hi.y - lo.y) * 0.1;
            Some((lo.x - pad, lo.y - pad, hi.x + pad, hi.y + pad))
        });
        let mut tiles = String::new();
        let mut f = first;
        while f <= last {
            let mut world = vec![];
            renderer::construction(&mut world, &layers::pose(anim, &armature.bones, f));
            let opts = agent::render::RenderOptions { size, bones: false, crop, ..Default::default() };
            let pic = agent::render::render(armature, &world, &opts);
            let viseme = cues.iter().rev().find(|c| c.0 <= f).map_or("rest".to_string(), |c| c.1.clone());
            let t = (f - clip.start_frame) as f32 / fps as f32;
            let word = track.words.iter().find(|w| t >= w.start && t < w.end).map_or(String::new(), |w| w.text.clone());
            tiles += &format!(
                "<figure class=tile><img src=\"data:image/png;base64,{}\"><figcaption><b>{f}</b> {:.2}s · {} <i>{}</i></figcaption></figure>",
                b64(&agent::render::png(&pic.image)),
                f as f32 / fps as f32,
                super::esc(&viseme),
                super::esc(&word)
            );
            f += every;
        }
        let words: String = track.words.iter().map(|w| format!("<span title=\"{:.2}–{:.2} s\">{}</span> ", w.start, w.end, super::esc(&w.text))).collect();
        html += &format!(
            "<section class=lipsync><h2>{} · {}</h2><p class=sub>Frames {first}–{last}, every {every}; each labelled frame, time, viseme and word.</p><p class=transcript>{words}</p><div class=strip>{tiles}</div></section>",
            super::esc(&anim.name),
            super::esc(&clip.name)
        );
    }
    Ok(html)
}
