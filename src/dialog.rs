//! Dialog lines (docs/LIP_SYNC.md §8): typed lines spoken by a voice, each generation
//! kept as a take. The chosen take is the line's audio clip on the timeline, lip-synced
//! on the line's words. Lines live on their animation, so they undo with it.

use crate::lipsync::{self, Language, LipSyncTrack, TimedWord};
use crate::shared::*;

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Provider {
    #[default]
    Kokoro,
    System,
    ElevenLabs,
    Chatterbox,
}

impl Provider {
    pub const ALL: [Provider; 4] = [Provider::Kokoro, Provider::System, Provider::ElevenLabs, Provider::Chatterbox];
    pub fn label(&self) -> &'static str {
        match self {
            Provider::Kokoro => "Kokoro",
            Provider::System => "System",
            Provider::ElevenLabs => "ElevenLabs",
            Provider::Chatterbox => "Chatterbox",
        }
    }
}

/// A voice: its provider, the provider's id for it, and a name to show.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct Voice {
    pub provider: Provider,
    pub id: String,
    pub name: String,
}

/// How a line is spoken.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct VoiceOpts {
    /// 1 = normal
    pub speed: f32,
    pub language: Language,
    /// Chatterbox's emotion exaggeration, 0.25–2 (0.5 neutral)
    pub exaggeration: f32,
    /// Chatterbox: the reference clip to clone (a WAV of the voice's owner)
    pub reference: String,
}

impl Default for VoiceOpts {
    fn default() -> Self {
        VoiceOpts { speed: 1., language: Language::English, exaggeration: 0.5, reference: String::new() }
    }
}


/// A line of dialog.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct DialogLine {
    pub id: i32,
    pub text: String,
    pub voice: Voice,
    pub opts: VoiceOpts,
    pub takes: Vec<Take>,
    /// the take in use (`Take::id`; 0: none)
    pub take: i32,
    /// its clip (`AudioClip::id`; 0: none yet)
    pub clip: i32,
    /// where its clip starts
    pub start_frame: i32,
}

/// One generation of a line.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct Take {
    pub id: i32,
    /// `AudioData::id`
    pub audio: i32,
    /// the voice's name and speed, as generated
    pub voice: String,
    pub speed: f32,
    pub secs: f32,
    /// the words' times, when the provider gave them (exact)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<TimedWord>,
}

/// A short name for a line's clip: its first few words.
pub fn clip_name(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().take(5).collect();
    let mut s = words.join(" ");
    if text.split_whitespace().count() > 5 {
        s += "…";
    }
    if s.is_empty() { "Dialog".into() } else { s }
}

/// Use take `take_id` for line `line_id`: it becomes the line's clip (made at the
/// line's start frame if there's none yet), lip-synced on the line's words when the
/// rig has a mouth. Returns a lip-sync problem to show, if any.
pub fn use_take(armature: &mut Armature, anim: usize, line_id: i32, take_id: i32) -> Option<String> {
    let a = armature.animations.get_mut(anim)?;
    let line = a.dialog.iter().find(|l| l.id == line_id)?.clone();
    let take = line.takes.iter().find(|t| t.id == take_id)?.clone();
    let clip_id = match a.clips.iter_mut().find(|c| c.id == line.clip) {
        Some(c) => {
            c.audio = take.audio;
            c.name = clip_name(&line.text);
            c.id
        }
        None => {
            let id = crate::audio::next_id(a.clips.iter().map(|c| c.id));
            a.clips.push(crate::audio::AudioClip { id, audio: take.audio, name: clip_name(&line.text), start_frame: line.start_frame, ..Default::default() });
            id
        }
    };
    if let Some(l) = a.dialog.iter_mut().find(|l| l.id == line_id) {
        l.take = take_id;
        l.clip = clip_id;
    }

    // lip sync: the clip's track, or a new one on the rig's mouth
    let host = lipsync::mouth_hosts(armature).first().copied();
    let a = &mut armature.animations[anim];
    let track_id = match a.lipsync.iter_mut().find(|t| t.clip == clip_id) {
        Some(t) => {
            t.text = line.text.clone();
            t.language = line.opts.language;
            t.heard = take.words.clone();
            t.exact = !take.words.is_empty();
            t.source = lipsync::TrackSource::Text;
            t.id
        }
        None => {
            let host = host?;
            let id = crate::audio::next_id(a.lipsync.iter().map(|t| t.id));
            a.lipsync.push(LipSyncTrack {
                id,
                clip: clip_id,
                mouth: host,
                text: line.text.clone(),
                language: line.opts.language,
                heard: take.words.clone(),
                exact: !take.words.is_empty(),
                ..Default::default()
            });
            id
        }
    };
    lipsync::generate(armature, anim, track_id).err()
}

/// Apply a dialog event to the selected animation. Audio for a new take comes from
/// `pending` (made off the main thread). Returns a problem to show, if any.
pub fn dialog_event(event: &Events, value: f32, str_value: &str, armature: &mut Armature, anim: usize, pending: &mut Vec<crate::audio::AudioData>) -> Option<String> {
    match event {
        Events::SetDialogLine => {
            let Ok(line) = serde_json::from_str::<DialogLine>(str_value) else { return None };
            let a = armature.animations.get_mut(anim)?;
            match a.dialog.iter_mut().find(|l| l.id == line.id) {
                Some(l) => *l = line,
                None => a.dialog.push(line),
            }
            None
        }
        Events::DeleteDialogLine => {
            let a = armature.animations.get_mut(anim)?;
            let line = a.dialog.iter().find(|l| l.id == value as i32)?.clone();
            a.dialog.retain(|l| l.id != line.id);
            // the line's clip and its lip sync go with it
            a.clips.retain(|c| c.id != line.clip);
            let tracks: Vec<i32> = a.lipsync.iter().filter(|t| t.clip == line.clip).map(|t| t.id).collect();
            for t in tracks {
                lipsync::delete(a, t);
            }
            None
        }
        Events::AddTake => {
            let Ok((line_id, take)) = serde_json::from_str::<(i32, Take)>(str_value) else { return None };
            if let Some(i) = pending.iter().position(|d| d.id == take.audio) {
                armature.audio.push(pending.remove(i));
            }
            let a = armature.animations.get_mut(anim)?;
            let line = a.dialog.iter_mut().find(|l| l.id == line_id)?;
            let take_id = take.id;
            line.takes.push(take);
            use_take(armature, anim, line_id, take_id)
        }
        Events::UseTake => {
            let (l, t) = str_value.split_once('|')?;
            use_take(armature, anim, l.parse().ok()?, t.parse().ok()?)
        }
        Events::DeleteTake => {
            let (l, t) = str_value.split_once('|')?;
            let (line_id, take_id): (i32, i32) = (l.parse().ok()?, t.parse().ok()?);
            let a = armature.animations.get_mut(anim)?;
            let line = a.dialog.iter_mut().find(|l| l.id == line_id)?;
            line.takes.retain(|t| t.id != take_id);
            if line.take != take_id {
                return None;
            }
            // the take in use: the newest left takes its place, or the clip goes
            match line.takes.last().map(|t| t.id) {
                Some(next) => use_take(armature, anim, line_id, next),
                None => {
                    line.take = 0;
                    let clip = std::mem::take(&mut line.clip);
                    a.clips.retain(|c| c.id != clip);
                    let tracks: Vec<i32> = a.lipsync.iter().filter(|t| t.clip == clip).map(|t| t.id).collect();
                    for t in tracks {
                        lipsync::delete(a, t);
                    }
                    None
                }
            }
        }
        _ => None,
    }
}
