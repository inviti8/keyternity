//! Transcription (docs/LIP_SYNC.md §7.4): reading whisper.cpp's output, the audio it's
//! given, and lining a transcript up with what was heard.
//!
//! `tests/data/whisper_tiny_en_sample.json` is whisper.cpp's output (b5454, tiny.en,
//! `-ojf -nfa`) for "Hello there. Peter Piper picked a peck of pickled peppers. Would
//! you like some water?".

use keyternity_lib::audio;
use keyternity_lib::lipsync::{self, Language, TimedWord};
use keyternity_lib::transcribe;

#[test]
fn whisper_tokens_become_words() {
    let words = transcribe::parse(include_str!("../data/whisper_tiny_en_sample.json")).unwrap();
    let text: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(text, vec!["Hello", "there.", "Peter", "Piper", "picked", "a", "peck", "of", "pickled", "peppers.", "Would", "you", "like", "some", "water?"]);
    assert!(words.windows(2).all(|p| p[0].start <= p[1].start));
    assert!(words.iter().all(|w| w.end >= w.start));
    // "peck" was two tokens, "pe" and "ck": one word spanning both
    let peck = &words[6];
    assert!((peck.start - 2.78).abs() < 1e-3 && (peck.end - 3.0).abs() < 1e-3, "{peck:?}");
    assert!(transcribe::parse("not json").is_err());
}

#[test]
fn audio_goes_to_whisper_as_16k_mono() {
    let rate = 44100;
    let stereo: Vec<f32> = (0..rate * 2).map(|i| if i % 2 == 0 { 0.5 } else { -0.5 }).collect();
    let d = audio::decode(1, "x", "wav", audio::wav_bytes(&stereo, 2, rate)).unwrap();
    let wav = transcribe::wav_16k(&d);
    let back = audio::decode(2, "y", "wav", wav).unwrap();
    assert_eq!((back.channels, back.sample_rate), (1, 16000));
    assert!((back.duration() - 1.).abs() < 0.01);
    assert!(back.samples.iter().all(|v| v.abs() < 1e-3), "the channels mix down");
}

#[test]
fn a_typed_line_lines_up_with_what_was_heard() {
    let heard: Vec<TimedWord> = ["Hello", "there.", "Peter", "Piper", "picked", "a", "pec", "of", "pickled"]
        .iter()
        .enumerate()
        .map(|(i, w)| TimedWord { text: w.to_string(), start: i as f32, end: i as f32 + 0.5 })
        .collect();
    let typed = lipsync::words_of("Hello there! Peter Piper picked a peck of pickled peppers.", Language::English);
    let m = lipsync::align_words(&typed, &heard);
    assert_eq!(m, vec![Some(0), Some(1), Some(2), Some(3), Some(4), Some(5), Some(6), Some(7), Some(8), None], "a one-letter slip still matches");
}

#[test]
fn what_was_heard_decides_close_splits() {
    // two stretches of speech; by length alone "four" could go either way
    let words = lipsync::words_of("one two three four five six", Language::English);
    let spans = [(0., 1.2), (1.6, 2.6)];
    let heard: Vec<TimedWord> = [("one", 0.1), ("two", 0.4), ("three", 0.8), ("four", 1.7), ("five", 2.), ("six", 2.3)]
        .iter()
        .map(|(w, t)| TimedWord { text: w.to_string(), start: *t, end: t + 0.2 })
        .collect();
    let timed = lipsync::spread_heard(&words, &spans, &heard, Language::English);
    assert_eq!(timed.len(), 6);
    assert!(timed[3].start >= 1.6 - 1e-4, "four was heard in the second stretch: {:?}", timed[3]);
    assert!(timed[2].end <= 1.2 + 1e-4);
}

/// Opt-in (downloads ~87 MB): fetch whisper.cpp and the tiny.en model the way the app
/// does, checking their checksums, then transcribe a line end to end.
/// `cargo test --test all -- transcribe:: --ignored`
#[test]
#[ignore]
fn download_and_transcribe() {
    use std::sync::atomic::AtomicU64;
    if transcribe::whisper_cli().is_none() {
        transcribe::fetch_tool(&AtomicU64::new(0)).unwrap();
    }
    let tiny = &transcribe::MODELS[0];
    if !transcribe::has_model(tiny) {
        transcribe::fetch_model(tiny, &AtomicU64::new(0)).unwrap();
    }
    // a tone isn't speech: Whisper should run and return without failing
    let rate = 16000;
    let tone: Vec<f32> = (0..rate * 2).map(|i| 0.3 * (i as f32 / rate as f32 * 300. * std::f32::consts::TAU).sin()).collect();
    let d = audio::decode(1, "tone", "wav", audio::wav_bytes(&tone, 1, rate as u32)).unwrap();
    let words = transcribe::transcribe(&d, tiny, Language::English);
    assert!(words.is_ok(), "{words:?}");
    // and real speech, if a WAV is given: WHISPER_TEST_WAV=<file> WHISPER_TEST_TEXT=<words>
    if let (Ok(path), Ok(text)) = (std::env::var("WHISPER_TEST_WAV"), std::env::var("WHISPER_TEST_TEXT")) {
        let d = audio::decode(2, "line", "wav", std::fs::read(path).unwrap()).unwrap();
        let heard = transcribe::transcribe(&d, tiny, Language::English).unwrap();
        let typed = lipsync::words_of(&text, Language::English);
        let matched = lipsync::align_words(&typed, &heard).iter().filter(|m| m.is_some()).count();
        println!("heard {:?}", heard.iter().map(|w| &w.text).collect::<Vec<_>>());
        assert!(matched * 10 >= typed.len() * 8, "{matched} of {} words matched", typed.len());
    }
}
