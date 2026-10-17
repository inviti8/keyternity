//! Dialog lines and voice generation (docs/LIP_SYNC.md §8): providers' answers, takes
//! through the editor (adding, swapping, deleting, undo), exact timings into lip sync,
//! and saving every take.

use keyternity_lib::audio;
use keyternity_lib::dialog::{DialogLine, Provider, Take, Voice};
use keyternity_lib::editor;
use keyternity_lib::face;
use keyternity_lib::lipsync::{self, Language, TimedWord};
use keyternity_lib::shared::*;
use keyternity_lib::utils;
use keyternity_lib::voice;

fn run(s: &mut Shared) {
    while !s.events.events.is_empty() {
        #[rustfmt::skip]
        editor::iterate_events(
            &s.input, &mut s.config, &mut s.events, &mut s.camera, &mut s.edit_mode, &mut s.selections,
            &mut s.undo_states, &mut s.armature, &mut s.psd_armature, &mut s.copy_buffer, &mut s.ui, &mut s.renderer,
        );
    }
}

/// bursts of tone standing in for speech: `secs` long, silent where given
fn speech(secs: f32, silences: &[(f32, f32)]) -> Vec<u8> {
    let rate = 16000;
    let s: Vec<f32> = (0..(secs * rate as f32) as usize)
        .map(|i| {
            let t = i as f32 / rate as f32;
            if silences.iter().any(|(a, b)| t >= *a && t < *b) { 0. } else { 0.5 * (t * 220. * std::f32::consts::TAU).sin() }
        })
        .collect();
    audio::wav_bytes(&s, 1, rate)
}

/// a face rig and an animation with one line of dialog
fn with_line() -> Shared {
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
    s.edit_mode.anim_open = true;
    s.selections.anim = 0;
    let line = DialogLine {
        id: 1,
        text: "Hello there, friend.".into(),
        voice: Voice { provider: Provider::Kokoro, id: "af_heart".into(), name: "Heart".into() },
        start_frame: 12,
        ..Default::default()
    };
    s.events.set_dialog_line(&line, true);
    run(&mut s);
    s
}

/// a generated take, as the Dialog window adds it: its audio waits in the UI
fn add_take(s: &mut Shared, take_id: i32, audio_id: i32, words: Vec<TimedWord>) {
    let data = audio::decode(audio_id, "take", "wav", speech(1.6, &[(0., 0.1), (0.7, 0.9)])).unwrap();
    let take = Take { id: take_id, audio: audio_id, voice: "Heart".into(), speed: 1., secs: data.duration(), words };
    s.ui.pending_audio.push(data);
    s.events.add_take(1, &take);
    run(s);
}

#[test]
fn a_take_becomes_the_lines_lip_synced_clip() {
    let mut s = with_line();
    add_take(&mut s, 1, 5, vec![]);
    assert_eq!(s.ui.custom_error, "");
    assert!(s.ui.pending_audio.is_empty(), "its audio moved into the rig");
    let a = &s.armature.animations[0];
    let line = &a.dialog[0];
    assert_eq!((line.take, line.takes.len()), (1, 1));
    let clip = a.clips.iter().find(|c| c.id == line.clip).expect("a clip");
    assert_eq!((clip.audio, clip.start_frame, clip.name.as_str()), (5, 12, "Hello there, friend."));
    let track = a.lipsync.iter().find(|t| t.clip == clip.id).expect("lip synced");
    assert_eq!(track.text, "Hello there, friend.");
    assert!(!track.exact);
    assert!(a.layers.iter().any(|l| l.generated == Some(track.id)));
    assert!(s.armature.audio.iter().any(|d| d.id == 5));

    s.events.undo();
    run(&mut s);
    let a = &s.armature.animations[0];
    assert!(a.dialog[0].takes.is_empty() && a.clips.is_empty() && a.lipsync.is_empty());
}

#[test]
fn takes_swap_and_delete() {
    let mut s = with_line();
    add_take(&mut s, 1, 5, vec![]);
    add_take(&mut s, 2, 6, vec![]);
    let clip_of = |s: &Shared| s.armature.animations[0].clips.iter().find(|c| c.id == s.armature.animations[0].dialog[0].clip).map(|c| c.audio);
    assert_eq!(clip_of(&s), Some(6), "a new take is used");
    assert_eq!(s.armature.animations[0].clips.len(), 1, "one clip per line");
    s.events.use_take(1, 1);
    run(&mut s);
    assert_eq!(clip_of(&s), Some(5));
    // deleting the take in use falls back to the newest left
    s.events.delete_take(1, 1);
    run(&mut s);
    assert_eq!((s.armature.animations[0].dialog[0].take, clip_of(&s)), (2, Some(6)));
    // and the last one takes the clip and its lip sync with it
    s.events.delete_take(1, 2);
    run(&mut s);
    let a = &s.armature.animations[0];
    assert!(a.clips.is_empty() && a.lipsync.is_empty() && a.layers.is_empty());
}

#[test]
fn every_take_is_saved() {
    let mut s = with_line();
    add_take(&mut s, 1, 5, vec![]);
    add_take(&mut s, 2, 6, vec![]);
    let used: Vec<i32> = utils::used_audio(&s.armature).iter().map(|a| a.id).collect();
    assert!(used.contains(&5) && used.contains(&6), "takes not in use are kept too: {used:?}");
    let (json, _) = utils::prepare_files(&s.armature, Camera::default(), vec![], &EditMode::default(), None);
    let root: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(root["animations"][0]["dialog"][0]["takes"].as_array().unwrap().len(), 2);
}

#[test]
fn removing_a_line_removes_its_clip_and_lip_sync() {
    let mut s = with_line();
    add_take(&mut s, 1, 5, vec![]);
    s.events.delete_dialog_line(1);
    run(&mut s);
    let a = &s.armature.animations[0];
    assert!(a.dialog.is_empty() && a.clips.is_empty() && a.lipsync.is_empty());
}

#[test]
fn a_voices_word_times_are_used_exactly() {
    let mut s = with_line();
    let spoken = vec![
        TimedWord { text: "Hello".into(), start: 0.1, end: 0.4 },
        TimedWord { text: "there,".into(), start: 0.45, end: 0.7 },
        TimedWord { text: "friend.".into(), start: 0.9, end: 1.5 },
    ];
    add_take(&mut s, 1, 5, spoken.clone());
    let t = &s.armature.animations[0].lipsync[0];
    assert!(t.exact);
    let starts: Vec<f32> = t.words.iter().map(|w| w.start).collect();
    assert_eq!(starts, vec![0.1, 0.45, 0.9]);

    // an edited line: matched words keep their times, new ones share the gaps
    let words = lipsync::words_of("Hello there, my friend.", Language::English);
    let timed = lipsync::exact_timing(&words, &[(0.1, 1.5)], &spoken, Language::English);
    assert_eq!(timed.len(), 4);
    assert_eq!((timed[0].start, timed[1].start, timed[3].start), (0.1, 0.45, 0.9));
    assert!(timed[2].start >= 0.7 - 1e-4 && timed[2].end <= 0.9 + 1e-4, "{:?}", timed[2]);
}

#[test]
fn elevenlabs_answers_are_read() {
    use base64::Engine;
    let wav = speech(0.3, &[]);
    let json = serde_json::json!({
        "audio_base64": base64::engine::general_purpose::STANDARD.encode(&wav),
        "alignment": {
            "characters": ["H", "i", " ", "t", "h", "e", "r", "e", "!"],
            "character_start_times_seconds": [0.0, 0.08, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45],
            "character_end_times_seconds": [0.08, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5]
        }
    });
    let (bytes, words) = voice::parse_elevenlabs_speech(&json.to_string()).unwrap();
    assert_eq!(bytes, wav);
    let w: Vec<(&str, f32, f32)> = words.iter().map(|w| (w.text.as_str(), w.start, w.end)).collect();
    assert_eq!(w, vec![("Hi", 0.0, 0.15), ("there!", 0.2, 0.5)]);
    let voices = voice::parse_elevenlabs_voices(r#"{"voices":[{"voice_id":"abc","name":"Rachel"}]}"#).unwrap();
    assert_eq!((voices[0].id.as_str(), voices[0].name.as_str(), voices[0].provider), ("abc", "Rachel", Provider::ElevenLabs));
    assert!(voice::parse_elevenlabs_speech("{}").is_err());
}

#[test]
fn sapi_word_positions_become_words() {
    // events can arrive out of order; the character position orders them
    let tsv = "\u{feff}100\t0\tHello\n1650\t13\tPeter\n465\t6\tthere\n";
    let w = voice::sapi_words(tsv, 2.5);
    let w: Vec<(&str, f32, f32)> = w.iter().map(|w| (w.text.as_str(), w.start, w.end)).collect();
    assert_eq!(w, vec![("Hello", 0.1, 0.465), ("there", 0.465, 1.65), ("Peter", 1.65, 2.5)]);
}

#[test]
fn kokoro_voices_have_readable_names() {
    assert_eq!(voice::kokoro_voice_info("af_heart"), (Language::English, "Heart (US, female)".to_string()));
    assert_eq!(voice::kokoro_voice_info("bm_george"), (Language::English, "George (UK, male)".to_string()));
    assert_eq!(voice::kokoro_voice_info("pf_dora").0, Language::PortugueseBr);
    assert!(voice::KOKORO_VOICES.iter().any(|(id, n)| *id == 3 && *n == "af_heart"));
}

/// Opt-in: speak with this computer's voice (Windows, macOS, or espeak-ng).
/// `cargo test --test all -- dialog::system_voice_speaks --ignored`
#[test]
#[ignore]
fn system_voice_speaks() {
    let voices = voice::system_voices();
    assert!(!voices.is_empty(), "no system voices");
    let s = voice::synthesize("Hello there, how are you?", &voices[0], &Default::default(), &Default::default()).unwrap();
    let d = audio::decode(1, "x", &s.ext, s.bytes).unwrap();
    assert!(d.duration() > 0.5);
    if cfg!(target_os = "windows") {
        let words = s.words.expect("SAPI gives word times");
        let text: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(text, vec!["Hello", "there", "how", "are", "you"]);
        assert!(words.windows(2).all(|p| p[0].start < p[1].start) && words.last().unwrap().end <= d.duration() + 0.01);
    }
}

/// Opt-in (downloads ~350 MB): fetch sherpa-onnx and Kokoro the way the app does,
/// checking their checksums, and speak a line.
/// `cargo test --test all -- dialog::kokoro_speaks --ignored`
#[test]
#[ignore]
fn kokoro_speaks() {
    use std::sync::atomic::AtomicU64;
    if voice::kokoro_tool().is_none() {
        voice::fetch_kokoro_tool(&AtomicU64::new(0)).unwrap();
    }
    let m = &voice::KOKORO_MODELS[0];
    if !voice::has_kokoro_model(m) {
        voice::fetch_kokoro_model(m, &AtomicU64::new(0)).unwrap();
    }
    let v = Voice { provider: Provider::Kokoro, id: "af_heart".into(), name: "Heart".into() };
    let s = voice::synthesize("Hello there. Would you like some water?", &v, &Default::default(), &Default::default()).unwrap();
    let d = audio::decode(1, "x", &s.ext, s.bytes).unwrap();
    assert!(d.duration() > 1.5, "{} s", d.duration());
    let spans = lipsync::speech_spans(&d.samples, d.channels, d.sample_rate);
    assert!(!spans.is_empty());
}
