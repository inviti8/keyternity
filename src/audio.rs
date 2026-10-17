//! Dialog audio in animations (docs/LIP_SYNC.md §5): clips placed on an animation's
//! timeline, embedded in the `.skf`, with a waveform, playback in sync, and scrubbing.
//!
//! Audio is stored once per file (`Armature::audio`: the original bytes, kept so the
//! file is saved as it came, and its decoded samples), and animations place clips of
//! it. Decoding (Symphonia, through rodio: wav, mp3, ogg, flac, m4a) and playback are
//! desktop-only for now.

use crate::shared::*;
use std::sync::Arc;

/// Audio files the rig carries, decoded.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct AudioData {
    pub id: i32,
    pub name: String,
    /// the file's extension ("wav", "mp3"…), for saving it as it came
    pub ext: String,
    /// the file as imported, saved into the `.skf` unchanged
    pub bytes: Arc<Vec<u8>>,
    pub channels: u16,
    pub sample_rate: u32,
    /// decoded, interleaved
    pub samples: Arc<Vec<f32>>,
    /// the waveform: (min, max) of the channels' mix per 1/`PEAKS_PER_SEC` second
    pub peaks: Arc<Vec<(f32, f32)>>,
}

/// File types that can be imported.
pub const EXTENSIONS: &[&str] = &["wav", "mp3", "ogg", "flac", "m4a"];

/// Waveform resolution.
pub const PEAKS_PER_SEC: f32 = 200.;

impl AudioData {
    /// Length in seconds.
    pub fn duration(&self) -> f32 {
        let frames = self.samples.len() as f32 / self.channels.max(1) as f32;
        frames / self.sample_rate.max(1) as f32
    }
}

/// A clip: some audio placed on an animation's timeline.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct AudioClip {
    pub id: i32,
    /// `AudioData::id`
    pub audio: i32,
    pub name: String,
    /// the animation frame the clip starts on (may be negative: starts before it)
    pub start_frame: i32,
    pub gain: f32,
    pub muted: bool,
}

impl Default for AudioClip {
    fn default() -> Self {
        AudioClip { id: 0, audio: -1, name: String::new(), start_frame: 0, gain: 1., muted: false }
    }
}

/// An audio file's entry in the saved armature: which zip file holds it.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct AudioFile {
    pub id: i32,
    pub name: String,
    pub file: String,
}

/// The zip entry an audio file is saved as.
pub fn file_name(a: &AudioData) -> String {
    let safe: String = a.name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("audio/{}_{}.{}", a.id, safe, if a.ext.is_empty() { "wav" } else { &a.ext })
}

/// The waveform of interleaved samples.
pub fn peaks(samples: &[f32], channels: u16, sample_rate: u32) -> Vec<(f32, f32)> {
    let ch = channels.max(1) as usize;
    let per = ((sample_rate as f32 / PEAKS_PER_SEC).round() as usize).max(1) * ch;
    samples
        .chunks(per)
        .map(|chunk| {
            let mut lo = 0f32;
            let mut hi = 0f32;
            for frame in chunk.chunks(ch) {
                let v = frame.iter().sum::<f32>() / frame.len() as f32;
                lo = lo.min(v);
                hi = hi.max(v);
            }
            (lo, hi)
        })
        .collect()
}

/// Decode an audio file's bytes into an `AudioData` (desktop).
#[cfg(not(target_arch = "wasm32"))]
pub fn decode(id: i32, name: &str, ext: &str, bytes: Vec<u8>) -> Result<AudioData, String> {
    use rodio::Source;
    let decoder = rodio::Decoder::try_from(std::io::Cursor::new(bytes.clone())).map_err(|e| e.to_string())?;
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    let samples: Vec<f32> = decoder.collect();
    if samples.is_empty() {
        return Err("the file has no audio".to_string());
    }
    let peaks = peaks(&samples, channels, sample_rate);
    Ok(AudioData {
        id,
        name: name.to_string(),
        ext: ext.to_lowercase(),
        bytes: Arc::new(bytes),
        channels,
        sample_rate,
        samples: Arc::new(samples),
        peaks: Arc::new(peaks),
    })
}

/// Where a clip's audio is at `frame` of an animation running at `fps`: seconds into
/// the audio (negative: it hasn't started yet).
pub fn clip_time(clip: &AudioClip, frame: f32, fps: i32) -> f32 {
    (frame - clip.start_frame as f32) / fps.max(1) as f32
}

// ---------------------------------------------------------------- playback (desktop)

#[cfg(not(target_arch = "wasm32"))]
mod player {
    use super::*;
    use rodio::Source;
    use std::cell::RefCell;
    use std::time::Duration;

    struct Player {
        stream: rodio::OutputStream,
        sinks: Vec<rodio::Sink>,
    }

    thread_local! {
        // the output device, opened on first use; playback is driven from the main thread
        static PLAYER: RefCell<Option<Player>> = const { RefCell::new(None) };
    }

    fn with_player(f: impl FnOnce(&mut Player)) {
        PLAYER.with(|p| {
            let mut p = p.borrow_mut();
            if p.is_none() {
                match rodio::OutputStreamBuilder::open_default_stream() {
                    Ok(mut stream) => {
                        stream.log_on_drop(false);
                        *p = Some(Player { stream, sinks: vec![] });
                    }
                    Err(e) => {
                        eprintln!("audio: no output device ({e})");
                        return;
                    }
                }
            }
            if let Some(p) = p.as_mut() {
                f(p);
            }
        });
    }

    /// Play an animation's clips from `frame`, for up to `length` seconds (all of them
    /// without a length), replacing whatever was playing.
    /// The shared samples from `start` to `end` (sample indexes), without copying them.
    struct Slice {
        data: Arc<Vec<f32>>,
        pos: usize,
        end: usize,
        channels: u16,
        rate: u32,
    }

    impl Iterator for Slice {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            if self.pos >= self.end {
                return None;
            }
            self.pos += 1;
            self.data.get(self.pos - 1).copied()
        }
    }

    impl Source for Slice {
        fn current_span_len(&self) -> Option<usize> {
            None
        }
        fn channels(&self) -> rodio::ChannelCount {
            self.channels
        }
        fn sample_rate(&self) -> rodio::SampleRate {
            self.rate
        }
        fn total_duration(&self) -> Option<Duration> {
            let frames = (self.end - self.pos.min(self.end)) as f32 / self.channels.max(1) as f32;
            Some(Duration::from_secs_f32(frames / self.rate.max(1) as f32))
        }
    }

    /// The part of `data` from `t` seconds, for `len` seconds (to the end without).
    fn slice(data: &AudioData, t: f32, len: Option<f32>) -> Slice {
        let ch = data.channels.max(1) as usize;
        let at = |secs: f32| ((secs.max(0.) * data.sample_rate as f32) as usize * ch).min(data.samples.len());
        let start = at(t);
        let end = len.map_or(data.samples.len(), |l| at(t + l));
        Slice { data: data.samples.clone(), pos: start, end, channels: data.channels, rate: data.sample_rate }
    }

    pub fn play_from(armature: &Armature, anim: usize, frame: f32, length: Option<f32>) {
        let Some(a) = armature.animations.get(anim) else { return };
        let clips: Vec<&AudioClip> = a.clips.iter().filter(|c| !c.muted).collect();
        with_player(|p| {
            for s in p.sinks.drain(..) {
                s.stop();
            }
            for clip in clips {
                let Some(data) = armature.audio.iter().find(|d| d.id == clip.audio) else { continue };
                let t = clip_time(clip, frame, a.fps);
                if t >= data.duration() {
                    continue;
                }
                // before the clip starts: wait for it (within the slice, when scrubbing)
                let (from, wait, len) = if t >= 0. {
                    (t, 0., length)
                } else {
                    match length {
                        Some(l) if -t >= l => continue,
                        Some(l) => (0., -t, Some(l + t)),
                        None => (0., -t, None),
                    }
                };
                let source = slice(data, from, len).amplify(clip.gain).delay(Duration::from_secs_f32(wait));
                let sink = rodio::Sink::connect_new(p.stream.mixer());
                sink.append(source);
                p.sinks.push(sink);
            }
        });
    }

    /// Play one piece of audio from its start, alone (auditioning a take).
    pub fn play_data(data: &AudioData) {
        let source = slice(data, 0., None);
        with_player(|p| {
            for s in p.sinks.drain(..) {
                s.stop();
            }
            let sink = rodio::Sink::connect_new(p.stream.mixer());
            sink.append(source);
            p.sinks.push(sink);
        });
    }

    pub fn stop() {
        PLAYER.with(|p| {
            if let Some(p) = p.borrow_mut().as_mut() {
                for s in p.sinks.drain(..) {
                    s.stop();
                }
            }
        });
    }
}

/// Start an animation's audio at `frame` (playback starting or looping).
pub fn play(armature: &Armature, anim: usize, frame: f32) {
    #[cfg(not(target_arch = "wasm32"))]
    player::play_from(armature, anim, frame, None);
    #[cfg(target_arch = "wasm32")]
    let _ = (armature, anim, frame);
}

/// Scrubbing: a short slice of the audio at `frame`, to check sync by ear.
pub fn scrub(armature: &Armature, anim: usize, frame: f32) {
    let fps = armature.animations.get(anim).map_or(24, |a| a.fps.max(1));
    let len = (2. / fps as f32).max(0.07);
    #[cfg(not(target_arch = "wasm32"))]
    player::play_from(armature, anim, frame, Some(len));
    #[cfg(target_arch = "wasm32")]
    let _ = (armature, anim, frame, len);
}

pub fn stop() {
    #[cfg(not(target_arch = "wasm32"))]
    player::stop();
}

/// Audition one piece of audio on its own.
pub fn play_data(data: &AudioData) {
    #[cfg(not(target_arch = "wasm32"))]
    player::play_data(data);
    #[cfg(target_arch = "wasm32")]
    let _ = data;
}

/// Apply a clip event (`Events::SetClip`, `Events::DeleteClip`) to `anim`.
pub fn clip_event(event: &Events, value: f32, str_value: &str, anim: &mut Animation) {
    match event {
        Events::SetClip => {
            let Ok(mut clip) = serde_json::from_str::<AudioClip>(str_value) else { return };
            clip.gain = clip.gain.clamp(0., 4.);
            match anim.clips.iter_mut().find(|c| c.id == clip.id) {
                Some(c) => *c = clip,
                None => anim.clips.push(clip),
            }
        }
        Events::DeleteClip => anim.clips.retain(|c| c.id != value as i32),
        _ => {}
    }
}

/// The waveform's (min, max) over peaks `a..b` (clamped to the waveform).
pub fn peak_span(peaks: &[(f32, f32)], a: usize, b: usize) -> (f32, f32) {
    let a = a.min(peaks.len());
    let b = b.max(a + 1).min(peaks.len());
    peaks[a..b].iter().fold((0f32, 0f32), |(l, h), p| (l.min(p.0), h.max(p.1)))
}

/// The rate mixes are made at.
pub const MIX_RATE: u32 = 48000;

/// An animation's unmuted clips mixed down to stereo at `MIX_RATE`, from frame 0 for
/// `secs` seconds (for video export). `None` without any audio to mix.
pub fn mix(armature: &Armature, anim: usize, secs: f32) -> Option<Vec<f32>> {
    let a = armature.animations.get(anim)?;
    let len = (secs.max(0.) * MIX_RATE as f32).round() as usize;
    let mut out = vec![0f32; len * 2];
    let mut any = false;
    for clip in a.clips.iter().filter(|c| !c.muted) {
        let Some(d) = armature.audio.iter().find(|d| d.id == clip.audio) else { continue };
        any = true;
        let ch = d.channels.max(1) as usize;
        let frames = d.samples.len() / ch;
        let start = clip.start_frame as f64 / a.fps.max(1) as f64;
        for i in 0..len {
            // where this output sample is in the clip's audio, in its frames
            let t = (i as f64 / MIX_RATE as f64 - start) * d.sample_rate as f64;
            if t < 0. {
                continue;
            }
            let f0 = t.floor() as usize;
            if f0 >= frames {
                break;
            }
            let f1 = (f0 + 1).min(frames - 1);
            let k = (t - f0 as f64) as f32;
            let at = |f: usize, c: usize| d.samples[f * ch + c.min(ch - 1)];
            for c in 0..2 {
                let v = at(f0, c) * (1. - k) + at(f1, c) * k;
                out[i * 2 + c] += v * clip.gain;
            }
        }
    }
    any.then(|| out.into_iter().map(|v| v.clamp(-1., 1.)).collect())
}

/// 16-bit PCM WAV bytes of interleaved samples.
pub fn wav_bytes(samples: &[f32], channels: u16, rate: u32) -> Vec<u8> {
    let data_len = samples.len() as u32 * 2;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    b.extend_from_slice(&(channels * 2).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&((s.clamp(-1., 1.) * 32767.) as i16).to_le_bytes());
    }
    b
}

/// The next free id among `ids`.
pub fn next_id(ids: impl Iterator<Item = i32>) -> i32 {
    ids.max().unwrap_or(0) + 1
}
