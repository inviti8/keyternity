//! Dialog audio (docs/LIP_SYNC.md §5): decoding, clips through the editor with undo,
//! the mixdown for video, and saving into and loading from the `.skf`.

use keyternity_lib::audio::{self, AudioClip};
use keyternity_lib::editor;
use keyternity_lib::shared::*;
use keyternity_lib::utils;
use std::io::Write;

fn run(s: &mut Shared) {
    while !s.events.events.is_empty() {
        #[rustfmt::skip]
        editor::iterate_events(
            &s.input, &mut s.config, &mut s.events, &mut s.camera, &mut s.edit_mode, &mut s.selections,
            &mut s.undo_states, &mut s.armature, &mut s.psd_armature, &mut s.copy_buffer, &mut s.ui, &mut s.renderer,
        );
    }
}

/// `secs` of a 440 Hz tone at half volume, as a mono 16-bit WAV
fn tone(secs: f32, rate: u32) -> Vec<u8> {
    let n = (secs * rate as f32) as usize;
    let samples: Vec<f32> = (0..n).map(|i| 0.5 * (i as f32 / rate as f32 * 440. * std::f32::consts::TAU).sin()).collect();
    audio::wav_bytes(&samples, 1, rate)
}

fn shared_with_clip() -> Shared {
    let mut s = Shared::default();
    s.armature.bones = vec![Bone { id: 0, parent_id: -1, name: "root".into(), scale: Vec2::new(1., 1.), ..Default::default() }];
    s.armature.animations = vec![Animation { id: 0, name: "talk".into(), fps: 24, ..Default::default() }];
    s.armature.audio = vec![audio::decode(1, "hello", "wav", tone(1., 22050)).unwrap()];
    s.armature.animations[0].clips = vec![AudioClip { id: 1, audio: 1, name: "hello".into(), start_frame: 12, ..Default::default() }];
    s.edit_mode.anim_open = true;
    s.selections.anim = 0;
    s
}

#[test]
fn decoding_a_wav_gives_its_samples_and_waveform() {
    let d = audio::decode(3, "hello", "WAV", tone(1., 22050)).unwrap();
    assert_eq!((d.channels, d.sample_rate, d.ext.as_str()), (1, 22050, "wav"));
    assert!((d.duration() - 1.).abs() < 0.01, "{}", d.duration());
    assert!(d.peaks.len().abs_diff(audio::PEAKS_PER_SEC as usize) <= 1, "{}", d.peaks.len());
    let (lo, hi) = audio::peak_span(&d.peaks, 0, d.peaks.len());
    assert!((hi - 0.5).abs() < 0.02 && (lo + 0.5).abs() < 0.02, "{lo} {hi}");
    assert_eq!(audio::file_name(&d), "audio/3_hello.wav");
}

#[test]
fn junk_doesnt_decode() {
    assert!(audio::decode(1, "x", "wav", b"not audio at all".to_vec()).is_err());
}

#[test]
fn the_mix_places_clips_by_frame_with_their_volume() {
    let mut s = shared_with_clip();
    let rate = audio::MIX_RATE as usize;
    let loudest = |m: &[f32], from: f32, to: f32| {
        m[(from * rate as f32) as usize * 2..(to * rate as f32) as usize * 2].iter().fold(0f32, |a, v| a.max(v.abs()))
    };
    let m = audio::mix(&s.armature, 0, 2.).unwrap();
    assert_eq!(m.len(), rate * 2 * 2, "stereo, two seconds");
    assert_eq!(loudest(&m, 0., 0.49), 0., "frame 12 at 24 fps: silent for half a second");
    assert!((loudest(&m, 0.55, 1.45) - 0.5).abs() < 0.02);
    assert_eq!(loudest(&m, 1.52, 2.), 0., "and done a second later");

    s.armature.animations[0].clips[0].gain = 2.;
    s.armature.animations[0].clips[0].start_frame = -12;
    let m = audio::mix(&s.armature, 0, 1.).unwrap();
    assert!((loudest(&m, 0., 0.45) - 1.).abs() < 0.03, "started half a second in, twice as loud");
    assert_eq!(loudest(&m, 0.52, 1.), 0.);

    s.armature.animations[0].clips[0].muted = true;
    assert!(audio::mix(&s.armature, 0, 1.).is_none(), "nothing to mix");
}

#[test]
fn moving_a_clip_is_one_undo_step() {
    let mut s = shared_with_clip();
    let clip = s.armature.animations[0].clips[0].clone();
    // a drag: the first change makes the step, the rest follow it
    for (i, f) in [14, 18, 20].into_iter().enumerate() {
        s.events.set_clip(&AudioClip { start_frame: f, ..clip.clone() }, i == 0);
        run(&mut s);
    }
    assert_eq!(s.armature.animations[0].clips[0].start_frame, 20);
    s.events.undo();
    run(&mut s);
    assert_eq!(s.armature.animations[0].clips[0].start_frame, 12);
}

#[test]
fn removing_a_clip_undoes() {
    let mut s = shared_with_clip();
    s.events.delete_clip(1);
    run(&mut s);
    assert!(s.armature.animations[0].clips.is_empty());
    s.events.undo();
    run(&mut s);
    assert_eq!(s.armature.animations[0].clips.len(), 1);
}

#[test]
fn saving_lists_only_audio_that_clips_use() {
    let mut s = shared_with_clip();
    s.armature.audio.push(audio::decode(2, "unused", "wav", tone(0.2, 8000)).unwrap());
    let (json, _) = utils::prepare_files(&s.armature, Camera::default(), vec![], &EditMode::default(), None);
    let root: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(root["audio"].as_array().unwrap().len(), 1);
    assert_eq!(root["audio"][0]["file"], "audio/1_hello.wav");
    assert_eq!(root["animations"][0]["clips"][0]["start_frame"], 12);

    // and none at all without clips
    s.armature.animations[0].clips.clear();
    let (json, _) = utils::prepare_files(&s.armature, Camera::default(), vec![], &EditMode::default(), None);
    let root: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(root.get("audio").is_none());
}

#[test]
fn a_saved_file_loads_its_audio_back() {
    let s = shared_with_clip();
    let (json, editor_json) = utils::prepare_files(&s.armature, Camera::default(), vec![], &EditMode::default(), None);
    let mut buf = std::io::Cursor::new(vec![]);
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("armature.json", options).unwrap();
        zip.write_all(json.as_bytes()).unwrap();
        zip.start_file("editor.json", options).unwrap();
        zip.write_all(editor_json.as_bytes()).unwrap();
        for a in utils::used_audio(&s.armature) {
            zip.start_file(audio::file_name(a), options).unwrap();
            zip.write_all(&a.bytes).unwrap();
        }
        zip.finish().unwrap();
    }
    buf.set_position(0);

    let mut loaded = Shared::default();
    utils::import(buf, &mut loaded, None, None, None, None);
    let arm = &loaded.armature;
    assert_eq!(arm.animations[0].clips, s.armature.animations[0].clips);
    assert_eq!(arm.audio.len(), 1);
    assert_eq!(arm.audio[0].samples, s.armature.audio[0].samples);
    assert_eq!(*arm.audio[0].bytes, *s.armature.audio[0].bytes, "saved as it came");
}
