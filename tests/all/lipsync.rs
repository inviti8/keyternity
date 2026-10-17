//! Lip sync (docs/LIP_SYNC.md §6.1–6.2, §7): the English rules against the original,
//! timing words over speech, cues to frames, mouth rigs, and tracks through the editor.

use keyternity_lib::audio::{self, AudioClip};
use keyternity_lib::editor;
use keyternity_lib::face;
use keyternity_lib::lipsync::{self, *};
use keyternity_lib::lipsync_en;
use keyternity_lib::shared::*;

fn run(s: &mut Shared) {
    while !s.events.events.is_empty() {
        #[rustfmt::skip]
        editor::iterate_events(
            &s.input, &mut s.config, &mut s.events, &mut s.camera, &mut s.edit_mode, &mut s.selections,
            &mut s.undo_states, &mut s.armature, &mut s.psd_armature, &mut s.copy_buffer, &mut s.ui, &mut s.renderer,
        );
    }
}

/// `tests/data/lipsync_en_reference.tsv` is the output of TalkingHead's own
/// `lipsync-en.mjs` (MIT, Mika Suominen) on these words: word, visemes, starts, lengths.
#[test]
fn english_rules_match_talkinghead() {
    let reference = include_str!("../data/lipsync_en_reference.tsv");
    let mut checked = 0;
    for line in reference.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        let (word, expected) = (cols[0], cols[1]);
        let ours = lipsync_en::word_sounds(word);
        // the port sounds L and W apart; the original folds them into nn and FF
        let mut folded: Vec<&str> = ours.iter().map(|s| match s.0 { "L" => "nn", "W" => "FF", x => x }).collect();
        folded.dedup();
        assert_eq!(folded.join(" "), expected, "{word}");
        // where nothing was folded, the timing matches too
        if !ours.iter().any(|s| s.0 == "L" || s.0 == "W") {
            let starts: Vec<String> = ours.iter().map(|s| format!("{:.3}", s.1)).collect();
            let lens: Vec<String> = ours.iter().map(|s| format!("{:.3}", s.2)).collect();
            assert_eq!(starts.join(" "), cols.get(2).copied().unwrap_or(""), "{word} starts");
            assert_eq!(lens.join(" "), cols.get(3).copied().unwrap_or(""), "{word} lengths");
        }
        checked += 1;
    }
    assert!(checked > 100);
}

#[test]
fn l_and_w_are_their_own_sounds() {
    let s = |w: &str| lipsync_en::word_sounds(w).iter().map(|s| s.0).collect::<Vec<_>>().join(" ");
    assert_eq!(s("will"), "W I L");
    assert_eq!(s("love"), "L aa FF");
    assert_eq!(s("queen"), "kk W I nn");
    assert_eq!(s("map"), "PP aa PP");
}

#[test]
fn numbers_and_symbols_are_spelled_out() {
    assert_eq!(lipsync_en::preprocess("I have 3 cats & 21 dogs"), "I have three cats and twenty one dogs");
    assert_eq!(lipsync_en::preprocess("in 1984, 50%"), "in nineteen eighty four , fifty percent");
    assert_eq!(lipsync_en::preprocess("the 2nd of 1,200"), "the second of one thousand two hundred");
    assert_eq!(lipsync_en::preprocess("pi is 3.14"), "pi is three point one four");
    assert_eq!(lipsync_en::preprocess("café"), "cafe");
}

#[test]
fn transcript_words_keep_their_pauses() {
    let w = words_of("Hello, world. Bye!", Language::English);
    assert_eq!(w, vec![("Hello".to_string(), 3.), ("world".to_string(), 4.), ("Bye".to_string(), 4.)]);
}

/// A tone of `secs` seconds at `rate`, with silences given as (start, end) seconds.
fn speech(secs: f32, rate: u32, silences: &[(f32, f32)]) -> Vec<f32> {
    (0..(secs * rate as f32) as usize)
        .map(|i| {
            let t = i as f32 / rate as f32;
            if silences.iter().any(|(a, b)| t >= *a && t < *b) { 0. } else { 0.5 * (t * 220. * std::f32::consts::TAU).sin() }
        })
        .collect()
}

#[test]
fn speech_is_found_between_silences() {
    let s = speech(3., 8000, &[(0., 0.5), (1.4, 1.9), (2.6, 3.)]);
    let spans = speech_spans(&s, 1, 8000);
    assert_eq!(spans.len(), 2, "{spans:?}");
    assert!((spans[0].0 - 0.5).abs() < 0.02 && (spans[0].1 - 1.4).abs() < 0.02, "{spans:?}");
    assert!((spans[1].0 - 1.9).abs() < 0.02 && (spans[1].1 - 2.6).abs() < 0.02, "{spans:?}");
    // a breath-length gap stays inside speech
    let s = speech(1., 8000, &[(0.5, 0.56)]);
    assert_eq!(speech_spans(&s, 1, 8000).len(), 1);
}

#[test]
fn words_spread_over_the_spans_without_crossing_gaps() {
    let words = words_of("one two three four", Language::English);
    let timed = spread(&words, &[(0.5, 1.4), (1.9, 2.6)], Language::English);
    assert_eq!(timed.len(), 4);
    for w in &timed {
        let inside = |a: f32, b: f32| w.start >= a - 1e-4 && w.end <= b + 1e-4;
        assert!(inside(0.5, 1.4) || inside(1.9, 2.6), "{w:?} crosses a gap");
    }
    assert!(timed.windows(2).all(|p| p[0].start < p[1].start));
    assert!((timed[0].start - 0.5).abs() < 1e-4 && (timed[3].end - 2.6).abs() < 1e-4);
}

#[test]
fn cues_rest_in_silences_only() {
    let words = vec![
        TimedWord { text: "map".into(), start: 0.2, end: 0.5 },
        TimedWord { text: "map".into(), start: 0.55, end: 0.8 },
        TimedWord { text: "map".into(), start: 1.5, end: 1.8 },
    ];
    let cues = cues_from_words(&words, 0.2, Language::English);
    let sils: Vec<f32> = cues.iter().filter(|c| c.1 == "sil").map(|c| c.0).collect();
    assert_eq!(sils, vec![0., 0.8, 1.8], "at the start, the long gap and the end");
    let sounds: Vec<&str> = cues.iter().take(4).map(|c| c.1).collect();
    assert_eq!(sounds, vec!["sil", "PP", "aa", "PP"]);
}

#[test]
fn frames_lead_hold_and_keep_closures() {
    let rig = MouthRig::default(); // Preston Blair
    let s = KeyerSettings { lead: 1, min_hold: 2, ..Default::default() };
    // at 24 fps: a closure, a brief "E" squeezed out before the "aa", an "O" later
    let cues = vec![(0., "sil"), (0.5, "PP"), (0.54, "E"), (0.58, "aa"), (1.0, "O"), (1.5, "sil")];
    let f = frame_cues(&cues, &rig, 24, 10, &s);
    let names: Vec<&str> = f.iter().map(|c| c.1.as_str()).collect();
    assert_eq!(names, vec!["rest", "MBP", "AI", "O", "rest"], "{f:?}");
    assert_eq!(f[1].0, 10 + 12 - 1, "led by a frame from the clip's start");
    assert_eq!(f[4].0, 10 + 36, "rest isn't led");
    assert!(f.windows(2).all(|p| p[1].0 - p[0].0 >= 2), "{f:?}");
}

#[test]
fn a_brief_sound_between_closures_still_shows() {
    // "Piper picked" at speed: PP, a quick "er", PP; dropping the "er" for the hold
    // would leave the mouth shut through both
    let rig = MouthRig::default();
    let s = KeyerSettings { lead: 0, min_hold: 2, ..Default::default() };
    let cues = vec![(0., "sil"), (0.5, "PP"), (0.6, "E"), (0.64, "PP"), (0.75, "I"), (1.0, "sil")];
    let f = frame_cues(&cues, &rig, 24, 0, &s);
    let names: Vec<&str> = f.iter().map(|c| c.1.as_str()).collect();
    assert_eq!(names, vec!["rest", "MBP", "E", "MBP", "E", "rest"], "{f:?}");
}

#[test]
fn viseme_sets_map_every_sound() {
    for set in VisemeSet::ALL {
        for sound in SOUNDS {
            assert!(set.visemes().contains(&set.viseme_of(sound)), "{set:?} {sound}");
        }
    }
    assert_eq!(VisemeSet::Oculus15.viseme_of("L"), "nn");
    assert_eq!(VisemeSet::PrestonBlair10.viseme_of("W"), "WQ");
    let rig = MouthRig { overrides: vec![("TH".into(), "L".into())], ..Default::default() };
    assert_eq!(rig.viseme_of("TH"), "L");
}

/// a face mesh with the standard shapes, a face rig over it, and a talking animation
/// with a clip of three bursts of "speech"
fn talking() -> Shared {
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
    let samples = speech(2., 16000, &[(0., 0.3), (1.0, 1.3), (1.8, 2.)]);
    let mut data = audio::decode(1, "line", "wav", audio::wav_bytes(&samples, 1, 16000)).unwrap();
    data.id = 1;
    s.armature.audio = vec![data];
    s.armature.animations[0].clips = vec![AudioClip { id: 1, audio: 1, name: "line".into(), start_frame: 6, ..Default::default() }];
    s.edit_mode.anim_open = true;
    s.selections.anim = 0;
    s
}

fn face_root(s: &Shared) -> i32 {
    keyternity_lib::face_panel::rigs(&s.armature)[0].0
}

#[test]
fn a_face_rig_hosts_a_mouth_with_built_in_poses() {
    let s = talking();
    let root = face_root(&s);
    assert!(mouth_hosts(&s.armature).contains(&root));
    let rig = auto_fill(&s.armature, root, VisemeSet::PrestonBlair10);
    assert_eq!(rig.face, Some(root));
    assert_eq!(rig.poses.len(), 10);
    let ai = rig.pose("AI").unwrap();
    assert!(ai.controls.iter().any(|c| c.control == "Jaw" && c.y < -0.5), "AI opens the jaw");
    assert!(rig.pose("rest").unwrap().controls.is_empty());
}

#[test]
fn viseme_shapes_are_used_when_the_face_has_them() {
    let mut s = talking();
    face::add_standard_shapes(&mut s.armature.bones[0], true);
    let root = face_root(&s);
    let rig = auto_fill(&s.armature, root, VisemeSet::PrestonBlair10);
    let ai = rig.pose("AI").unwrap();
    assert_eq!(ai.shapes.len(), 1);
    assert!(ai.controls.is_empty(), "the shape replaces the rig pose");
    assert!(names_viseme("viseme_PP", "PP") && names_viseme("Mouth_AI", "ai") && !names_viseme("Blink_L", "L"));
}

#[test]
fn generating_keys_the_rig_in_its_own_layer_and_undoes() {
    let mut s = talking();
    let root = face_root(&s);
    let track = LipSyncTrack { id: 1, clip: 1, mouth: root, text: "Hello there. How are you? I am fine.".into(), ..Default::default() };
    s.events.generate_lipsync(&track);
    run(&mut s);
    assert_eq!(s.ui.custom_error, "");
    let a = &s.armature.animations[0];
    assert_eq!(a.lipsync.len(), 1);
    assert_eq!(a.lipsync[0].words.len(), 8);
    let li = a.layers.iter().position(|l| l.generated == Some(1)).expect("a Lip Sync layer");
    let layer = &a.layers[li];
    assert!(layer.locked && layer.mode == LayerMode::Additive);
    let keys = a.layer_keys(li);
    assert!(!keys.is_empty());
    let jaw = keyternity_lib::face_panel::rigs(&s.armature)[0].1.iter().find(|(n, _)| n == "Jaw").unwrap().1;
    assert!(keys.iter().any(|k| k.bone_id == jaw && k.element == AnimElement::PositionY));
    assert!(keys.iter().all(|k| k.frame >= 0));
    // the words sit in the speech, which starts 0.3 s into the clip
    assert!(a.lipsync[0].words[0].start >= 0.29);
    // and the mouth opens while speaking
    let at = |f: i32| keyternity_lib::layers::pose(&s.armature.animations[0], &s.armature.bones, f).iter().find(|b| b.id == jaw).unwrap().pos.y;
    let rest = s.armature.bones.iter().find(|b| b.id == jaw).unwrap().pos.y;
    assert!((at(0) - rest).abs() < 1e-4, "closed before the line");
    assert!((6..30).any(|f| at(f) < rest - 0.1), "open somewhere in the first phrase");
    assert!(s.armature.animations[0].has_keys());

    s.events.undo();
    run(&mut s);
    assert!(s.armature.animations[0].lipsync.is_empty());
    assert!(s.armature.animations[0].layers.is_empty());
}

#[test]
fn regenerating_replaces_only_the_tracks_keys() {
    let mut s = talking();
    let root = face_root(&s);
    let mut track = LipSyncTrack { id: 1, clip: 1, mouth: root, text: "one two".into(), ..Default::default() };
    s.events.generate_lipsync(&track);
    run(&mut s);
    track.text = "one two three four five six".into();
    s.events.generate_lipsync(&track);
    run(&mut s);
    let a = &s.armature.animations[0];
    assert_eq!(a.layers.iter().filter(|l| l.generated.is_some()).count(), 1);
    assert_eq!(a.lipsync[0].words.len(), 6);
}

#[test]
fn baking_hands_the_layer_over_and_deleting_removes_it() {
    let mut s = talking();
    let root = face_root(&s);
    s.events.generate_lipsync(&LipSyncTrack { id: 1, clip: 1, mouth: root, text: "hello".into(), ..Default::default() });
    run(&mut s);
    s.events.bake_lipsync(1);
    run(&mut s);
    let a = &s.armature.animations[0];
    assert!(a.lipsync.is_empty());
    let l = a.layers.iter().find(|l| l.name.starts_with("Lip Sync")).unwrap();
    assert!(!l.locked && l.generated.is_none() && !l.keyframes.is_empty());

    let mut s = talking();
    s.events.generate_lipsync(&LipSyncTrack { id: 1, clip: 1, mouth: root, text: "hello".into(), ..Default::default() });
    run(&mut s);
    s.events.delete_lipsync(1);
    run(&mut s);
    assert!(s.armature.animations[0].layers.is_empty());
}

#[test]
fn loudness_alone_opens_the_mouth_with_the_sound() {
    let mut s = talking();
    let root = face_root(&s);
    s.events.generate_lipsync(&LipSyncTrack { id: 1, clip: 1, mouth: root, source: TrackSource::Loudness, ..Default::default() });
    run(&mut s);
    assert_eq!(s.ui.custom_error, "");
    let a = &s.armature.animations[0];
    let li = a.layers.iter().position(|l| l.generated == Some(1)).unwrap();
    assert!(!a.layer_keys(li).is_empty());
}

#[test]
fn a_drawn_mouth_swaps_textures_stepped() {
    let mut s = Shared::default();
    let names = ["rest", "MBP", "AI", "E", "O"];
    s.armature.styles = vec![Style { id: 0, name: "mouths".into(), textures: names.iter().map(|n| Texture { name: n.to_string(), ..Default::default() }).collect(), ..Default::default() }];
    s.armature.bones = vec![Bone { id: 3, parent_id: -1, name: "mouth".into(), tex: "rest".into(), scale: Vec2::new(1., 1.), ..Default::default() }];
    s.armature.animations = vec![Animation { id: 0, name: "talk".into(), fps: 24, ..Default::default() }];
    let samples = speech(1., 16000, &[(0., 0.2)]);
    s.armature.audio = vec![audio::decode(1, "line", "wav", audio::wav_bytes(&samples, 1, 16000)).unwrap()];
    s.armature.animations[0].clips = vec![AudioClip { id: 1, audio: 1, name: "line".into(), ..Default::default() }];
    s.selections.anim = 0;
    assert_eq!(mouth_hosts(&s.armature), vec![3]);
    let rig = auto_fill(&s.armature, 3, VisemeSet::PrestonBlair10);
    assert_eq!(rig.pose("AI").unwrap().texture.as_ref().unwrap().tex, "AI");
    assert_eq!(rig.pose("FV").unwrap().texture.as_ref().unwrap().tex, "rest", "missing visemes show rest");

    s.events.generate_lipsync(&LipSyncTrack { id: 1, clip: 1, mouth: 3, text: "map".into(), ..Default::default() });
    run(&mut s);
    assert_eq!(s.ui.custom_error, "");
    let a = &s.armature.animations[0];
    let keys = a.layer_keys(a.layers.iter().position(|l| l.generated == Some(1)).unwrap());
    let texs: Vec<&str> = keys.iter().map(|k| k.value_str.as_str()).collect();
    assert_eq!(texs, vec!["rest", "MBP", "AI", "MBP", "rest"]);
    assert!(keys.iter().all(|k| k.element == AnimElement::Texture));
}

#[test]
fn a_mouth_rig_saves_with_its_bone_and_undoes() {
    let mut s = talking();
    let root = face_root(&s);
    let mut rig = auto_fill(&s.armature, root, VisemeSet::Simple5);
    rig.overrides.push(("TH".into(), "open".into()));
    s.events.set_mouth_rig(root, Some(&rig));
    run(&mut s);
    assert_eq!(lipsync::rig_for(&s.armature, root, None), rig);
    let (json, _) = keyternity_lib::utils::prepare_files(&s.armature, Camera::default(), vec![], &EditMode::default(), None);
    assert!(json.contains("\"mouth_rig\""));
    s.events.undo();
    run(&mut s);
    assert!(s.armature.bones.iter().find(|b| b.id == root).unwrap().mouth_rig.is_none());
}

#[test]
fn capturing_reads_the_posed_rig() {
    let s = talking();
    let root = face_root(&s);
    let rigs = keyternity_lib::face_panel::rigs(&s.armature);
    let jaw = rigs[0].1.iter().find(|(n, _)| n == "Jaw").unwrap().1;
    let mut posed = s.armature.bones.clone();
    let b = posed.iter_mut().find(|b| b.id == jaw).unwrap();
    let lim = b.face_control.as_ref().unwrap().limits.unwrap();
    b.pos.y += lim.y.0 * 0.5;
    let pose = capture_pose(&s.armature, &posed, root);
    assert_eq!(pose.len(), 1);
    assert_eq!((pose[0].control.as_str(), pose[0].y), ("Jaw", -0.5));
}
