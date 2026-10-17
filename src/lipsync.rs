//! Auto lip sync (docs/LIP_SYNC.md §6.1–6.2, §7): text and audio in, keys out.
//!
//! ```text
//! transcript → words spread over the speech found in the clip (or the clip's loudness)
//!   → each word's sounds (lipsync_en) stretched over its time → cues (time, sound)
//!   → the mouth rig's viseme set → frames (lead, holds) → keys on the rig's controls,
//!     shapes or textures, in the track's own locked Lip Sync layer
//! ```
//!
//! A mouth rig lives on the bone that hosts the mouth (a face rig's root, a mesh with
//! viseme shapes, or a bone with viseme textures), so it's saved and undone with it.
//! Without one, the rig is filled in automatically.

use crate::lipsync_en;
use crate::shared::*;

/// The sounds lip sync speaks internally: Oculus's 15 visemes plus `L` and `W` (§6.1).
pub const SOUNDS: [&str; 17] = ["sil", "PP", "FF", "TH", "DD", "kk", "CH", "SS", "nn", "L", "RR", "aa", "E", "I", "O", "U", "W"];

/// The visemes a mouth rig draws.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum VisemeSet {
    #[default]
    PrestonBlair10,
    Oculus15,
    Simple5,
}

impl VisemeSet {
    pub const ALL: [VisemeSet; 3] = [VisemeSet::PrestonBlair10, VisemeSet::Oculus15, VisemeSet::Simple5];

    pub fn visemes(&self) -> &'static [&'static str] {
        match self {
            VisemeSet::PrestonBlair10 => &["rest", "MBP", "FV", "etc", "L", "AI", "E", "O", "U", "WQ"],
            VisemeSet::Oculus15 => &["sil", "PP", "FF", "TH", "DD", "kk", "CH", "SS", "nn", "RR", "aa", "E", "I", "O", "U"],
            VisemeSet::Simple5 => &["closed", "mid", "open", "wide", "round"],
        }
    }

    /// The set's viseme for a sound (the table in §6.1).
    pub fn viseme_of(&self, sound: &str) -> &'static str {
        match self {
            VisemeSet::PrestonBlair10 => match sound {
                "PP" => "MBP",
                "FF" => "FV",
                "L" => "L",
                "aa" => "AI",
                "E" | "I" => "E",
                "O" => "O",
                "U" => "U",
                "W" => "WQ",
                "sil" => "rest",
                _ => "etc",
            },
            VisemeSet::Oculus15 => match sound {
                "L" => "nn",
                "W" => "U",
                s => SOUNDS.iter().copied().find(|x| *x == s && *x != "L" && *x != "W").unwrap_or("sil"),
            },
            VisemeSet::Simple5 => match sound {
                "sil" | "PP" => "closed",
                "aa" => "open",
                "E" | "I" => "wide",
                "O" | "U" | "W" => "round",
                _ => "mid",
            },
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            VisemeSet::PrestonBlair10 => "Preston Blair (10)",
            VisemeSet::Oculus15 => "Oculus (15)",
            VisemeSet::Simple5 => "Simple (5)",
        }
    }
}

/// Which viseme each sound shows, and how each viseme is posed (§6.2).
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct MouthRig {
    pub set: VisemeSet,
    /// (sound, viseme): this character's changes to the set's table
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<(String, String)>,
    pub poses: Vec<MouthPose>,
    /// the face rig (its root bone) whose controls the poses name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<i32>,
}

impl MouthRig {
    pub fn viseme_of(&self, sound: &str) -> String {
        match self.overrides.iter().find(|(s, _)| s == sound) {
            Some((_, v)) => v.clone(),
            None => self.set.viseme_of(sound).to_string(),
        }
    }
    pub fn pose(&self, viseme: &str) -> Option<&MouthPose> {
        self.poses.iter().find(|p| p.viseme == viseme)
    }
}

/// A viseme's mouth: face-rig values, shapes, a texture, or a mix.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct MouthPose {
    pub viseme: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<PoseControl>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<PoseShape>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<PoseTexture>,
}

/// A face-rig control's values in a pose, as fractions of its limits (−1…1, as the
/// panel's travel), so a pose fits any size of rig.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct PoseControl {
    pub control: String,
    pub x: f32,
    pub y: f32,
    pub twist: f32,
    pub depth: f32,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Default, Debug)]
pub struct PoseShape {
    pub mesh: i32,
    pub shape: i32,
    pub weight: f32,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Default, Debug)]
pub struct PoseTexture {
    pub bone: i32,
    pub tex: String,
}

/// How smoothly the mouth moves between visemes (§7.2): Off steps; Low, Medium and High
/// ease over 1, 2 and the whole time to the next viseme.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Smoothing {
    Off,
    Low,
    #[default]
    Medium,
    High,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct KeyerSettings {
    /// frames the mouth leads the sound by
    pub lead: i32,
    /// the shortest a viseme shows, in frames
    pub min_hold: i32,
    pub smoothing: Smoothing,
    /// 1 = as posed; less mumbles, more over-enunciates
    pub intensity: f32,
    /// silences at least this long (seconds) rest the mouth
    pub rest_gap: f32,
}

impl Default for KeyerSettings {
    fn default() -> Self {
        KeyerSettings { lead: 1, min_hold: 2, smoothing: Smoothing::Medium, intensity: 1., rest_gap: 0.2 }
    }
}

/// The language a line is spoken in: its text rules (TalkingHead's modules, §7.3) and
/// the language transcription listens for.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Language {
    #[default]
    English,
    German,
    French,
    PortugueseBr,
    Finnish,
    Lithuanian,
}

impl Language {
    pub const ALL: [Language; 6] = [Language::English, Language::German, Language::French, Language::PortugueseBr, Language::Finnish, Language::Lithuanian];

    pub fn label(&self) -> &'static str {
        match self {
            Language::English => "English",
            Language::German => "Deutsch",
            Language::French => "Français",
            Language::PortugueseBr => "Português (Brasil)",
            Language::Finnish => "Suomi",
            Language::Lithuanian => "Lietuvių",
        }
    }

    /// Its ISO 639-1 code, as Whisper takes it.
    pub fn code(&self) -> &'static str {
        match self {
            Language::English => "en",
            Language::German => "de",
            Language::French => "fr",
            Language::PortugueseBr => "pt",
            Language::Finnish => "fi",
            Language::Lithuanian => "lt",
        }
    }

    /// Text with numbers and symbols spelled out, ready for `sounds`.
    pub fn preprocess(&self, text: &str) -> String {
        match self {
            Language::English => lipsync_en::preprocess(text),
            Language::German => crate::lipsync_de::preprocess(text),
            Language::French => crate::lipsync_fr::preprocess(text),
            Language::PortugueseBr => crate::lipsync_pt::preprocess(text),
            Language::Finnish => crate::lipsync_fi::fi_preprocess(text),
            Language::Lithuanian => crate::lipsync_fi::lt_preprocess(text),
        }
    }

    /// A word's sounds, with relative starts and lengths.
    pub fn sounds(&self, word: &str) -> Vec<crate::lipsync_rules::Sound> {
        match self {
            Language::English => lipsync_en::word_sounds(word),
            Language::German => crate::lipsync_de::word_sounds(word),
            Language::French => crate::lipsync_fr::word_sounds(word),
            Language::PortugueseBr => crate::lipsync_pt::word_sounds(word),
            Language::Finnish => crate::lipsync_fi::fi_word_sounds(word),
            Language::Lithuanian => crate::lipsync_fi::lt_word_sounds(word),
        }
    }
}

/// Where a track's timing comes from.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum TrackSource {
    /// the transcript, spread over the speech in the clip
    #[default]
    Text,
    /// the clip's loudness alone (creatures, gibberish, lines without words)
    Loudness,
}

/// A word and its time in the clip (seconds from the clip's start).
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Default, Debug)]
pub struct TimedWord {
    pub text: String,
    pub start: f32,
    pub end: f32,
}

/// Lip sync for one audio clip of an animation (§7.1). Its keys are in the layer
/// whose `generated` is this track's id.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct LipSyncTrack {
    pub id: i32,
    /// `AudioClip::id`
    pub clip: i32,
    /// the bone hosting the mouth rig
    pub mouth: i32,
    pub text: String,
    pub source: TrackSource,
    pub language: Language,
    /// what transcription heard (words and times), guiding the timing of `text`
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub heard: Vec<TimedWord>,
    /// as last generated
    pub words: Vec<TimedWord>,
    pub settings: KeyerSettings,
}

// ---------------------------------------------------------------- text and timing

/// A transcript's words, each with the pause after it (relative units: a comma 3, the
/// end of a sentence 4).
pub fn words_of(text: &str, lang: Language) -> Vec<(String, f32)> {
    let text = lang.preprocess(text);
    let mut out: Vec<(String, f32)> = vec![];
    for raw in text.split_whitespace() {
        let word: String = raw.chars().filter(|c| c.is_alphabetic() || *c == '\'').collect();
        let pause = if raw.ends_with([',', ';', ':', '-']) {
            3.
        } else if raw.ends_with(['.', '!', '?']) {
            4.
        } else {
            0.
        };
        if word.trim_matches('\'').is_empty() {
            if let Some(last) = out.last_mut() {
                last.1 = last.1.max(pause);
            }
            continue;
        }
        out.push((word, pause));
    }
    out
}

/// A word's length in relative units.
fn weight(word: &str, lang: Language) -> f32 {
    lang.sounds(word).iter().map(|s| s.2).sum::<f32>().max(0.6)
}

/// Where speech is in audio (interleaved samples): spans in seconds, from a 10 ms
/// loudness envelope against a threshold between the noise floor and the peaks.
pub fn speech_spans(samples: &[f32], channels: u16, rate: u32) -> Vec<(f32, f32)> {
    let env = envelope(samples, channels, rate, 0.01);
    if env.is_empty() {
        return vec![];
    }
    let mut sorted = env.clone();
    sorted.sort_by(f32::total_cmp);
    let noise = sorted[sorted.len() / 10];
    let peak = sorted[(sorted.len() * 98 / 100).min(sorted.len() - 1)];
    if peak < 1e-4 {
        return vec![];
    }
    let thr = noise + 0.12 * (peak - noise);
    let mut spans: Vec<(f32, f32)> = vec![];
    for (i, v) in env.iter().enumerate() {
        if *v < thr {
            continue;
        }
        let (a, b) = (i as f32 * 0.01, (i + 1) as f32 * 0.01);
        match spans.last_mut() {
            // gaps shorter than 120 ms are within speech
            Some(last) if a - last.1 < 0.12 => last.1 = b,
            _ => spans.push((a, b)),
        }
    }
    spans.retain(|(a, b)| b - a >= 0.06);
    spans
}

/// RMS loudness of the channels' mix per `window` seconds.
pub fn envelope(samples: &[f32], channels: u16, rate: u32, window: f32) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    let per = ((rate as f32 * window) as usize).max(1) * ch;
    samples
        .chunks(per)
        .map(|c| {
            let frames = c.chunks(ch).map(|f| f.iter().sum::<f32>() / f.len() as f32);
            let (sum, n) = frames.fold((0., 0), |(s, n), v| (s + v * v, n + 1));
            (sum / n.max(1) as f32).sqrt()
        })
        .collect()
}

/// Spread words over speech spans (§7.4's fallback for typed transcripts): the words
/// are split between the spans (`partition`), then spread over each by their lengths.
pub fn spread(words: &[(String, f32)], spans: &[(f32, f32)], lang: Language) -> Vec<TimedWord> {
    spread_parts(words, spans, &partition(words, spans, None, lang), lang)
}

/// Spread each span's words (`assigned[k]` is word k's span) over it.
fn spread_parts(words: &[(String, f32)], spans: &[(f32, f32)], assigned: &[usize], lang: Language) -> Vec<TimedWord> {
    let mut out = vec![];
    for (s, span) in spans.iter().enumerate() {
        let group: Vec<(String, f32)> = words.iter().zip(assigned).filter(|(_, a)| **a == s).map(|(w, _)| w.clone()).collect();
        out.extend(spread_even(&group, &[*span], lang));
    }
    out
}

/// Split words between speech spans, in order (each word's span): the split that keeps
/// the speaking rate most even, prefers sentence ends at gaps, leaves no long span
/// wordless, and, given `heard` (each word's span as a recogniser placed it), agrees
/// with it where it can. A dynamic program over where each span's words end.
pub fn partition(words: &[(String, f32)], spans: &[(f32, f32)], heard: Option<&[Option<usize>]>, lang: Language) -> Vec<usize> {
    let (n, m) = (words.len(), spans.len());
    if m <= 1 || n == 0 {
        return vec![0; n];
    }
    let w: Vec<f32> = words.iter().map(|(x, _)| weight(x, lang)).collect();
    let rate = w.iter().sum::<f32>() / spans.iter().map(|(a, b)| b - a).sum::<f32>().max(1e-3);
    // the cost of span `s` holding words i..j
    let cost = |s: usize, i: usize, j: usize| -> f32 {
        let d = spans[s].1 - spans[s].0;
        if i == j {
            return 4. * d;
        }
        let g: f32 = w[i..j].iter().sum();
        let mut c = (d * rate / g).ln().powi(2);
        // a sentence or clause ending where the speech stops
        if words[j - 1].1 > 0. {
            c -= 0.3;
        }
        c += 0.15 * words[i..j - 1].iter().filter(|(_, p)| *p >= 3.).count() as f32;
        if let Some(h) = heard {
            c += 0.12 * h[i..j].iter().filter(|a| a.is_some_and(|a| a != s)).count() as f32;
        }
        c
    };
    // best[s][j]: the least cost of spans 0..s holding words 0..j
    let mut best = vec![vec![f32::INFINITY; n + 1]; m + 1];
    let mut from = vec![vec![0usize; n + 1]; m + 1];
    best[0][0] = 0.;
    for s in 0..m {
        for i in 0..=n {
            if !best[s][i].is_finite() {
                continue;
            }
            for j in i..=n {
                let c = best[s][i] + cost(s, i, j);
                if c < best[s + 1][j] {
                    best[s + 1][j] = c;
                    from[s + 1][j] = i;
                }
            }
        }
    }
    let mut out = vec![0; n];
    let mut j = n;
    for s in (1..=m).rev() {
        let i = from[s][j];
        for k in i..j {
            out[k] = s - 1;
        }
        j = i;
    }
    out
}

/// Spread words evenly by their lengths over spans laid end to end: a word falling
/// across a gap goes to the side holding most of it. Pauses count only when there's
/// one span to spread over.
fn spread_even(words: &[(String, f32)], spans: &[(f32, f32)], lang: Language) -> Vec<TimedWord> {
    if words.is_empty() || spans.is_empty() {
        return vec![];
    }
    let use_pauses = spans.len() == 1;
    let weights: Vec<(f32, f32)> = words.iter().map(|(w, p)| (weight(w, lang), if use_pauses { *p } else { 0. })).collect();
    // the last word's pause doesn't take time
    let total_w: f32 = weights.iter().enumerate().map(|(i, (w, p))| w + if i + 1 < weights.len() { *p } else { 0. }).sum();
    let total_t: f32 = spans.iter().map(|(a, b)| b - a).sum();
    // speech time (spans laid end to end) to clip time, and the span holding it
    let starts: Vec<f32> = spans.iter().scan(0., |acc, (a, b)| {
        let s = *acc;
        *acc += b - a;
        Some(s)
    }).collect();
    let span_of = |st: f32| starts.iter().rposition(|s| *s <= st).unwrap_or(0);
    let real = |st: f32, i: usize| spans[i].0 + (st - starts[i]).clamp(0., spans[i].1 - spans[i].0);

    let mut out = vec![];
    let mut acc = 0.;
    for (k, (w, p)) in weights.iter().enumerate() {
        let a = acc / total_w * total_t;
        let b = (acc + w) / total_w * total_t;
        acc += w + p;
        let (i, j) = (span_of(a + 1e-5), span_of((b - 1e-5).max(a)));
        let (start, end) = if i == j {
            (real(a, i), real(b, i))
        } else if starts[i] + (spans[i].1 - spans[i].0) - a >= b - starts[j] {
            (real(a, i), spans[i].1)
        } else {
            (spans[j].0, real(b, j))
        };
        out.push(TimedWord { text: words[k].0.clone(), start, end: end.max(start + 0.02) });
    }
    out
}

/// A word as compared between a transcript and what was heard: letters only, lower case.
fn plain(w: &str) -> String {
    w.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Whether two plain words are the same word, allowing a slip of one letter in longer
/// ones ("pec" for "peck").
fn same_word(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.chars().count().min(b.chars().count()) < 3 {
        return false;
    }
    // edit distance ≤ 1
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (s, l) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
    if l.len() - s.len() > 1 {
        return false;
    }
    let (mut i, mut j, mut edits) = (0, 0, 0);
    while i < s.len() && j < l.len() {
        if s[i] == l[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        if s.len() == l.len() {
            i += 1;
        }
        j += 1;
    }
    edits + (l.len() - j) + (s.len() - i) <= 1
}

/// Match a transcript's words to heard words, in order (the longest common run of
/// same words): for each transcript word, the heard word it is, if any.
pub fn align_words(typed: &[(String, f32)], heard: &[TimedWord]) -> Vec<Option<usize>> {
    let a: Vec<String> = typed.iter().map(|(w, _)| plain(w)).collect();
    let b: Vec<String> = heard.iter().map(|w| plain(&w.text)).collect();
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if same_word(&a[i], &b[j]) { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }
    let mut out = vec![None; n];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if same_word(&a[i], &b[j]) && dp[i][j] == dp[i + 1][j + 1] + 1 {
            out[i] = Some(j);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// Spread words over speech spans with what was heard as a guide (§7.4): where a
/// recogniser heard each word decides, with the speaking rate and punctuation, which
/// span it's in; then each span's words are spread over it by their lengths.
/// Recognisers place words roughly, while a span's edges are exact, so this keeps the
/// best of each.
pub fn spread_heard(words: &[(String, f32)], spans: &[(f32, f32)], heard: &[TimedWord], lang: Language) -> Vec<TimedWord> {
    if words.is_empty() {
        return vec![];
    }
    let matched = align_words(words, heard);
    if spans.is_empty() || matched.iter().all(Option::is_none) {
        return spread(words, spans, lang);
    }
    // the span each heard word is in, or nearest
    let span_at = |t: f32| -> usize {
        spans
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let d = |s: &(f32, f32)| if t < s.0 { s.0 - t } else if t > s.1 { t - s.1 } else { 0. };
                d(a).total_cmp(&d(b))
            })
            .map_or(0, |(i, _)| i)
    };
    let anchored: Vec<Option<usize>> = matched.iter().map(|m| m.map(|j| span_at((heard[j].start + heard[j].end) / 2.))).collect();
    spread_parts(words, spans, &partition(words, spans, Some(&anchored), lang), lang)
}

/// Cues (seconds, sound) from timed words: each word's sounds stretched over it, and
/// the mouth at rest before, after, and in silences of at least `rest_gap`.
pub fn cues_from_words(words: &[TimedWord], rest_gap: f32, lang: Language) -> Vec<(f32, &'static str)> {
    let mut cues: Vec<(f32, &'static str)> = vec![(0., "sil")];
    for (k, w) in words.iter().enumerate() {
        let sounds = lang.sounds(&w.text);
        let len = sounds.last().map_or(0., |s| s.1 + s.2).max(1e-3);
        for s in &sounds {
            cues.push((w.start + s.1 / len * (w.end - w.start), s.0));
        }
        let gap = words.get(k + 1).map_or(f32::MAX, |n| n.start - w.end);
        if gap >= rest_gap {
            cues.push((w.end, "sil"));
        }
    }
    cues
}

/// Cues from loudness alone, a frame at a time: closed, a little open, open, wide open.
pub fn cues_from_loudness(samples: &[f32], channels: u16, rate: u32, fps: i32) -> Vec<(f32, &'static str)> {
    let frame = 1. / fps.max(1) as f32;
    let env = envelope(samples, channels, rate, frame);
    let mut sorted = env.clone();
    sorted.sort_by(f32::total_cmp);
    let peak = sorted.get(sorted.len() * 95 / 100).copied().unwrap_or(0.).max(1e-4);
    let mut cues = vec![(0., "sil")];
    for (i, v) in env.iter().enumerate() {
        let o = v / peak;
        let sound = if o < 0.12 {
            "sil"
        } else if o < 0.35 {
            "nn"
        } else if o < 0.7 {
            "E"
        } else {
            "aa"
        };
        cues.push((i as f32 * frame, sound));
    }
    cues
}

/// Cues as visemes on frames (§7.2): led, merged where they repeat, and held at least
/// `min_hold` frames. Closures (MBP) are kept over brief sounds, as they read most.
pub fn frame_cues(cues: &[(f32, &str)], rig: &MouthRig, fps: i32, start_frame: i32, s: &KeyerSettings) -> Vec<(i32, String)> {
    let closure = rig.viseme_of("PP");
    let mut timed: Vec<(i32, String)> = cues
        .iter()
        .map(|(t, sound)| {
            let lead = if *sound == "sil" { 0 } else { s.lead };
            ((start_frame + (t * fps as f32).round() as i32 - lead).max(0), rig.viseme_of(sound))
        })
        .collect();
    timed.sort_by_key(|c| c.0);
    // one viseme per frame: a closure wins, otherwise the later
    let mut one: Vec<(i32, String)> = vec![];
    for c in timed {
        match one.last_mut() {
            Some(last) if last.0 == c.0 => {
                if last.1 != closure {
                    *last = c;
                }
            }
            _ => one.push(c),
        }
    }
    one.dedup_by(|b, a| a.1 == b.1);
    let hold = s.min_hold.max(1);
    let mut out: Vec<(i32, String)> = vec![];
    for (i, c) in one.iter().enumerate() {
        let Some(last) = out.last() else {
            out.push(c.clone());
            continue;
        };
        let mut f = c.0.max(last.0 + hold);
        let next = one.get(i + 1);
        let squeezed = next.is_some_and(|n| n.0 - f < hold);
        if squeezed && c.1 != closure {
            // dropping it would join the visemes either side into one long hold (the
            // mouth shut through "Piper picked"): show it for a frame instead
            let short = c.0.max(last.0 + 1);
            match next {
                Some(n) if n.1 == last.1 && short < n.0 => f = short,
                _ => continue,
            }
        }
        if last.1 != c.1 {
            out.push((f, c.1.clone()));
        }
    }
    out
}

// ---------------------------------------------------------------- mouth rigs

/// Shape or texture names that mean a viseme: the name itself, or with a `viseme_` or
/// `mouth_` prefix ("viseme_PP", "Mouth_AI"), ignoring case.
pub fn names_viseme(name: &str, viseme: &str) -> bool {
    let n = name.trim().to_lowercase();
    let v = viseme.to_lowercase();
    ["", "viseme_", "viseme", "mouth_", "mouth-", "mouth"].iter().any(|p| n.strip_prefix(p) == Some(v.as_str()))
}

/// Bones that can host a mouth: face rigs' roots, meshes with viseme shapes, bones
/// with viseme textures, and any with a mouth rig already.
pub fn mouth_hosts(armature: &Armature) -> Vec<i32> {
    let mut out: Vec<i32> = crate::face_panel::rigs(armature).into_iter().map(|(root, _)| root).collect();
    let textures = all_textures(armature);
    for b in &armature.bones {
        let shapes = VisemeSet::ALL.iter().any(|set| set.visemes().iter().filter(|v| b.shapes.iter().any(|s| names_viseme(&s.name, v))).count() >= 3);
        let texs = b.tex != "" && VisemeSet::ALL.iter().any(|set| set.visemes().iter().filter(|v| textures.iter().any(|t| names_viseme(t, v))).count() >= 3)
            && textures.iter().any(|t| *t == b.tex && VisemeSet::ALL.iter().any(|set| set.visemes().iter().any(|v| names_viseme(t, v))));
        if (shapes || texs || b.mouth_rig.is_some()) && !out.contains(&b.id) {
            out.push(b.id);
        }
    }
    out
}

fn all_textures(armature: &Armature) -> Vec<String> {
    armature.styles.iter().flat_map(|s| s.textures.iter().map(|t| t.name.clone())).collect()
}

/// A rig's control names → bone ids.
fn controls_of(armature: &Armature, root: i32) -> Vec<(String, i32)> {
    crate::face_panel::rigs(armature).into_iter().find(|(r, _)| *r == root).map_or(vec![], |(_, c)| c)
}

/// The mouth rig to use for `host`: its own, or one filled in automatically.
pub fn rig_for(armature: &Armature, host: i32, set: Option<VisemeSet>) -> MouthRig {
    match armature.bones.iter().find(|b| b.id == host).and_then(|b| b.mouth_rig.clone()) {
        Some(r) if set.is_none_or(|s| s == r.set) => r,
        _ => auto_fill(armature, host, set.unwrap_or_default()),
    }
}

/// Fill a mouth rig for `host` (§6.2): viseme shapes where the face has them, viseme
/// textures for a drawn mouth, and otherwise the built-in face-rig poses.
pub fn auto_fill(armature: &Armature, host: i32, set: VisemeSet) -> MouthRig {
    let rigs = crate::face_panel::rigs(armature);
    let host_bone = armature.bones.iter().find(|b| b.id == host);
    // the face rig: the host itself, or one set up for this mesh
    let face = rigs.iter().find(|(r, _)| *r == host).map(|(r, _)| *r).or_else(|| {
        rigs.iter()
            .find(|(_, cs)| cs.iter().any(|(_, id)| armature.bones.iter().find(|b| b.id == *id).and_then(|b| b.face_control.as_ref()).and_then(|f| f.mesh) == Some(host)))
            .map(|(r, _)| *r)
    });
    // meshes to look in for viseme shapes: the host, and the face rig's mesh
    let mut meshes: Vec<&Bone> = host_bone.into_iter().filter(|b| !b.shapes.is_empty()).collect();
    if let Some(root) = face {
        for (_, id) in controls_of(armature, root) {
            let mesh = armature.bones.iter().find(|b| b.id == id).and_then(|b| b.face_control.as_ref()).and_then(|f| f.mesh);
            if let Some(m) = mesh.and_then(|m| armature.bones.iter().find(|b| b.id == m)) {
                if !meshes.iter().any(|x| x.id == m.id) {
                    meshes.push(m);
                }
            }
        }
    }
    let textures = all_textures(armature);
    let drawn = host_bone.filter(|b| b.tex != "").map(|b| b.id);

    let poses = set
        .visemes()
        .iter()
        .map(|v| {
            let mut pose = MouthPose { viseme: v.to_string(), ..Default::default() };
            for m in &meshes {
                if let Some(s) = m.shapes.iter().find(|s| names_viseme(&s.name, v)) {
                    pose.shapes.push(PoseShape { mesh: m.id, shape: s.id, weight: 1. });
                }
            }
            if let (Some(bone), Some(t)) = (drawn, textures.iter().find(|t| names_viseme(t, v))) {
                pose.texture = Some(PoseTexture { bone, tex: t.clone() });
            }
            if pose.shapes.is_empty() && pose.texture.is_none() && face.is_some() {
                pose.controls = builtin_pose(v);
            }
            pose
        })
        .collect();
    let mut rig = MouthRig { set, overrides: vec![], poses, face };
    // a drawn mouth missing some visemes shows the nearest it has
    if drawn.is_some() && rig.poses.iter().any(|p| p.texture.is_some()) {
        let fallback = rig.poses.iter().find(|p| p.texture.is_some()).and_then(|p| p.texture.clone());
        let rest = rig.pose(set.visemes()[0]).and_then(|p| p.texture.clone()).or(fallback);
        for p in rig.poses.iter_mut().filter(|p| p.texture.is_none() && p.shapes.is_empty() && p.controls.is_empty()) {
            p.texture = rest.clone();
        }
    }
    rig
}

/// The built-in face-rig pose of a viseme (any set's name), as FaceBot recommended:
/// jaw, corners and lips (§6.5).
pub fn builtin_pose(viseme: &str) -> Vec<PoseControl> {
    let c = |control: &str, x: f32, y: f32, depth: f32| PoseControl { control: control.into(), x, y, twist: 0., depth };
    // corners: x is inward (Corner_L's limit is to the left, Corner_R's to the right)
    let corners = |inward: f32, up: f32| vec![c("Corner_L", -inward, up, 0.), c("Corner_R", inward, up, 0.)];
    let lips = |up: f32, down: f32, depth_u: f32, depth_l: f32| vec![c("UpperLip", 0., up, depth_u), c("LowerLip", 0., -down, depth_l)];
    let jaw = |open: f32| vec![c("Jaw", 0., -open, 0.)];
    let all = |parts: Vec<Vec<PoseControl>>| parts.into_iter().flatten().filter(|p| p.x != 0. || p.y != 0. || p.depth != 0.).collect();
    match viseme {
        "rest" | "sil" | "closed" => vec![],
        "MBP" | "PP" => all(vec![lips(0., 0., -0.25, -0.3)]),
        "FV" | "FF" => all(vec![jaw(0.12), lips(0.25, 0., 0., -0.8)]),
        "L" => all(vec![jaw(0.4), lips(0.25, 0.2, 0., 0.), vec![c("Tongue", 0., 1., 0.)]]),
        "TH" => all(vec![jaw(0.25), lips(0.2, 0.15, 0., 0.), vec![c("Tongue", 0., 0.6, 0.)]]),
        "AI" | "aa" | "open" => all(vec![jaw(0.75), corners(0., 0.1), lips(0.25, 0.35, 0., 0.)]),
        "E" | "wide" => all(vec![jaw(0.35), corners(0., 0.35), lips(0.35, 0.45, 0., 0.)]),
        "I" => all(vec![jaw(0.2), corners(0., 0.4), lips(0.3, 0.35, 0., 0.)]),
        "O" | "round" => all(vec![jaw(0.6), corners(0.7, 0.), lips(0., 0., 0.4, 0.4)]),
        "U" => all(vec![jaw(0.25), corners(0.8, 0.), lips(0., 0., 0.7, 0.7)]),
        "WQ" => all(vec![jaw(0.12), corners(1., 0.), lips(0., 0., 0.8, 0.8)]),
        "CH" => all(vec![jaw(0.2), corners(0.4, 0.), lips(0.2, 0.2, 0.5, 0.5)]),
        "SS" => all(vec![jaw(0.1), corners(0., 0.2), lips(0.3, 0.3, 0., 0.)]),
        "RR" => all(vec![jaw(0.25), corners(0.5, 0.), lips(0., 0., 0.3, 0.3)]),
        "kk" => all(vec![jaw(0.35), lips(0.1, 0.15, 0., 0.)]),
        "nn" => all(vec![jaw(0.2), lips(0.1, 0.1, 0., 0.)]),
        // etc, DD, mid
        _ => all(vec![jaw(0.3), corners(0., 0.05), lips(0.15, 0.2, 0., 0.)]),
    }
}

/// A fraction of a limit (−1…1) as an offset: toward the limit's low end when negative.
fn of_limit(v: f32, lim: (f32, f32)) -> f32 {
    if v >= 0. { v * lim.1 } else { -v * lim.0 }
}

/// An offset as a fraction of a limit (the inverse of `of_limit`).
fn to_limit(off: f32, lim: (f32, f32)) -> f32 {
    let frac = |v: f32, l: f32| if l.abs() < 1e-6 { 0. } else { v / l };
    if off >= 0. { frac(off, lim.1) } else { -frac(off, lim.0) }
}

/// A control's offsets for a pose: (position, rotation, X scale ratio).
fn control_offsets(fc: &FaceControl, p: &PoseControl) -> (Vec2, f32, f32) {
    match &fc.limits {
        Some(l) => (Vec2::new(of_limit(p.x, l.x), of_limit(p.y, l.y)), of_limit(p.twist, l.twist), of_limit(p.depth, l.depth)),
        None => (Vec2::new(p.x, p.y) * fc.range, p.twist * std::f32::consts::FRAC_PI_4, 0.),
    }
}

/// The face rig's current pose as a viseme's controls (from `posed`, the bones as shown):
/// the mouth's controls that are away from rest.
pub fn capture_pose(armature: &Armature, posed: &[Bone], face: i32) -> Vec<PoseControl> {
    let mouth = ["Mouth", "Corner_L", "Corner_R", "UpperLip", "Jaw", "LowerLip", "Tongue"];
    let mut out = vec![];
    for (name, id) in controls_of(armature, face) {
        if !mouth.contains(&name.as_str()) {
            continue;
        }
        let (Some(b), Some(fc)) = (posed.iter().find(|b| b.id == id), armature.bones.iter().find(|b| b.id == id).and_then(|b| b.face_control.as_ref())) else { continue };
        let off = b.pos - fc.rest;
        let p = match &fc.limits {
            Some(l) => PoseControl { control: name, x: to_limit(off.x, l.x), y: to_limit(off.y, l.y), twist: to_limit(b.rot - fc.rest_rot, l.twist), depth: to_limit(b.scale.x - 1., l.depth) },
            None => {
                let r = fc.range.max(1e-4);
                PoseControl { control: name, x: off.x / r, y: off.y / r, twist: (b.rot - fc.rest_rot) / std::f32::consts::FRAC_PI_4, depth: 0. }
            }
        };
        let round = |v: f32| (v * 100.).round() / 100.;
        let p = PoseControl { x: round(p.x), y: round(p.y), twist: round(p.twist), depth: round(p.depth), ..p };
        if p.x != 0. || p.y != 0. || p.twist != 0. || p.depth != 0. {
            out.push(p);
        }
    }
    out
}

// ---------------------------------------------------------------- keys

/// A keyed channel and its value in each viseme.
struct Channel {
    bone: i32,
    el: AnimElement,
    shape: i32,
    /// the setup value (numbers), or texture
    rest: f32,
    rest_tex: String,
}

/// The keys for frame cues on a mouth rig (§7.2): every channel any pose moves is keyed
/// on each viseme, held until it eases into the next.
pub fn keys(armature: &Armature, rig: &MouthRig, cues: &[(i32, String)], s: &KeyerSettings) -> Vec<Keyframe> {
    let controls = rig.face.map_or(vec![], |f| controls_of(armature, f));
    let bone = |id: i32| armature.bones.iter().find(|b| b.id == id);
    let mut channels: Vec<Channel> = vec![];
    let mut add = |c: Channel| {
        if !channels.iter().any(|x| x.bone == c.bone && x.el == c.el && x.shape == c.shape) {
            channels.push(c);
        }
    };
    for p in &rig.poses {
        for pc in &p.controls {
            let Some((_, id)) = controls.iter().find(|(n, _)| *n == pc.control) else { continue };
            let Some(b) = bone(*id) else { continue };
            let mut ch = |el: AnimElement, rest: f32| add(Channel { bone: b.id, el, shape: 0, rest, rest_tex: String::new() });
            if pc.x != 0. {
                ch(AnimElement::PositionX, b.pos.x);
            }
            if pc.y != 0. {
                ch(AnimElement::PositionY, b.pos.y);
            }
            if pc.twist != 0. {
                ch(AnimElement::Rotation, b.rot);
            }
            if pc.depth != 0. {
                ch(AnimElement::ScaleX, b.scale.x);
            }
        }
        for ps in &p.shapes {
            let rest = bone(ps.mesh).and_then(|b| b.shapes.iter().find(|x| x.id == ps.shape)).map_or(0., |x| x.weight);
            add(Channel { bone: ps.mesh, el: AnimElement::ShapeWeight, shape: ps.shape, rest, rest_tex: String::new() });
        }
        if let Some(t) = &p.texture {
            let rest_tex = bone(t.bone).map_or(String::new(), |b| b.tex.clone());
            add(Channel { bone: t.bone, el: AnimElement::Texture, shape: 0, rest: 0., rest_tex });
        }
    }

    let k = s.intensity.max(0.);
    let value = |c: &Channel, viseme: &str| -> (f32, String) {
        let Some(p) = rig.pose(viseme) else { return (c.rest, c.rest_tex.clone()) };
        match c.el {
            AnimElement::ShapeWeight => {
                let w = p.shapes.iter().find(|x| x.mesh == c.bone && x.shape == c.shape).map_or(0., |x| x.weight);
                (c.rest + w * k, String::new())
            }
            AnimElement::Texture => (0., p.texture.as_ref().filter(|t| t.bone == c.bone).map_or(c.rest_tex.clone(), |t| t.tex.clone())),
            _ => {
                let name = controls.iter().find(|(_, id)| *id == c.bone).map(|(n, _)| n.as_str()).unwrap_or("");
                let fc = bone(c.bone).and_then(|b| b.face_control.as_ref());
                let pc = p.controls.iter().find(|x| x.control == name);
                let (off, rot, depth) = match (fc, pc) {
                    (Some(fc), Some(pc)) => control_offsets(fc, pc),
                    _ => (Vec2::ZERO, 0., 0.),
                };
                let v = match c.el {
                    AnimElement::PositionX => c.rest + off.x * k,
                    AnimElement::PositionY => c.rest + off.y * k,
                    AnimElement::Rotation => c.rest + rot * k,
                    _ => c.rest * (1. + depth * k),
                };
                (v, String::new())
            }
        }
    };

    let (ease_in, ease_out) = crate::utils::interp_preset(HandlePreset::SineInOut);
    let (snap_in, snap_out) = crate::utils::interp_preset(HandlePreset::Snap);
    let blend = match s.smoothing {
        Smoothing::Off => Some(0),
        Smoothing::Low => Some(1),
        Smoothing::Medium => Some(2),
        Smoothing::High => None,
    };
    let mut out = vec![];
    for c in &channels {
        let stepped = c.el == AnimElement::Texture || blend == Some(0);
        let (sh, eh, preset) = if stepped { (snap_in, snap_out, HandlePreset::Snap) } else { (ease_in, ease_out, HandlePreset::SineInOut) };
        let mut ks: Vec<(i32, (f32, String))> = vec![];
        for (i, (f, v)) in cues.iter().enumerate() {
            let val = value(c, v);
            ks.push((*f, val.clone()));
            // hold, then ease into the next viseme over `t` frames
            if let (Some(t), Some((g, _))) = (blend.filter(|_| !stepped), cues.get(i + 1)) {
                if t > 0 && g - t > *f {
                    ks.push((g - t, val));
                }
            }
        }
        // only where the value changes: drop keys inside a flat run
        let mut keep: Vec<(i32, (f32, String))> = vec![];
        for (i, kf) in ks.iter().enumerate() {
            let same = |o: Option<&(i32, (f32, String))>| o.is_some_and(|o| (o.1 .0 - kf.1 .0).abs() < 1e-5 && o.1 .1 == kf.1 .1);
            if i > 0 && i + 1 < ks.len() && same(ks.get(i - 1)) && same(ks.get(i + 1)) {
                continue;
            }
            keep.push(kf.clone());
        }
        for (f, (v, tex)) in keep {
            out.push(Keyframe {
                frame: f,
                bone_id: c.bone,
                element: c.el.clone(),
                shape: c.shape,
                value: v,
                value_str: tex,
                start_handle: sh,
                end_handle: eh,
                handle_preset: preset.clone(),
                ..Default::default()
            });
        }
    }
    out.sort_by_key(|k| k.frame);
    out
}

// ---------------------------------------------------------------- tracks

/// Make (or remake) a track's keys in its Lip Sync layer: the words timed afresh from
/// its transcript and clip. Returns how many keys, or why there are none.
pub fn generate(armature: &mut Armature, anim: usize, track_id: i32) -> Result<usize, String> {
    let a = armature.animations.get(anim).ok_or("no animation")?;
    let track = a.lipsync.iter().find(|t| t.id == track_id).cloned().ok_or("no track")?;
    let clip = a.clips.iter().find(|c| c.id == track.clip).cloned().ok_or("the track's audio clip is gone")?;
    let fps = a.fps.max(1);
    let data = armature.audio.iter().find(|d| d.id == clip.audio);
    if !armature.bones.iter().any(|b| b.id == track.mouth) {
        return Err("the mouth's bone is gone".into());
    }
    let rig = rig_for(armature, track.mouth, None);
    if rig.poses.iter().all(|p| p.controls.is_empty() && p.shapes.is_empty() && p.texture.is_none()) {
        return Err("the mouth has nothing to pose: set up a face rig, or add viseme shapes or textures".into());
    }

    let (cues, words) = match track.source {
        TrackSource::Text => {
            let words = words_of(&track.text, track.language);
            if words.is_empty() {
                return Err("type the line's words first".into());
            }
            let spans = match data {
                Some(d) => {
                    let s = speech_spans(&d.samples, d.channels, d.sample_rate);
                    if s.is_empty() { vec![(0., d.duration())] } else { s }
                }
                // no audio to listen to: an even pace
                None => vec![(0., words.len() as f32 * 0.35)],
            };
            let timed = if track.heard.is_empty() { spread(&words, &spans, track.language) } else { spread_heard(&words, &spans, &track.heard, track.language) };
            (cues_from_words(&timed, track.settings.rest_gap, track.language), timed)
        }
        TrackSource::Loudness => {
            let d = data.ok_or("the clip's audio isn't loaded")?;
            (cues_from_loudness(&d.samples, d.channels, d.sample_rate, fps), vec![])
        }
    };
    let fcues = frame_cues(&cues, &rig, fps, clip.start_frame, &track.settings);
    let keys = keys(armature, &rig, &fcues, &track.settings);
    let n = keys.len();

    let a = &mut armature.animations[anim];
    if let Some(t) = a.lipsync.iter_mut().find(|t| t.id == track_id) {
        t.words = words;
    }
    let li = match a.layers.iter().position(|l| l.generated == Some(track_id)) {
        Some(i) => i,
        None => {
            let i = a.add_layer(&format!("Lip Sync: {}", clip.name), LayerMode::Additive);
            a.layers[i].generated = Some(track_id);
            a.layers[i].locked = true;
            i
        }
    };
    if li == a.active_layer {
        a.keyframes = keys;
    } else {
        a.layers[li].keyframes = keys;
    }
    Ok(n)
}

/// Remove a track and its layer.
pub fn delete(anim: &mut Animation, track_id: i32) {
    anim.lipsync.retain(|t| t.id != track_id);
    if let Some(i) = anim.layers.iter().position(|l| l.generated == Some(track_id)) {
        if anim.active_layer == i {
            anim.set_active_layer(0);
        } else if anim.active_layer > i {
            anim.active_layer -= 1;
        }
        anim.layers.remove(i);
        if anim.layers.len() == 1 {
            // just Base again: back to no layers
            anim.layers.clear();
            anim.active_layer = 0;
        }
    }
}

/// Bake a track: its layer becomes an ordinary one, the artist's to edit.
pub fn bake(anim: &mut Animation, track_id: i32) {
    if let Some(l) = anim.layers.iter_mut().find(|l| l.generated == Some(track_id)) {
        l.generated = None;
        l.locked = false;
    }
    anim.lipsync.retain(|t| t.id != track_id);
}

/// Apply a lip-sync event to the selected animation, or a mouth rig to its bone.
/// Returns an error to show, if any.
pub fn lipsync_event(event: &Events, value: f32, str_value: &str, armature: &mut Armature, anim: usize) -> Option<String> {
    match event {
        Events::GenerateLipSync => {
            let Ok(track) = serde_json::from_str::<LipSyncTrack>(str_value) else { return None };
            let a = armature.animations.get_mut(anim)?;
            let id = track.id;
            match a.lipsync.iter_mut().find(|t| t.id == id) {
                Some(t) => *t = track,
                None => a.lipsync.push(track),
            }
            generate(armature, anim, id).err()
        }
        Events::DeleteLipSync => {
            delete(armature.animations.get_mut(anim)?, value as i32);
            None
        }
        Events::BakeLipSync => {
            bake(armature.animations.get_mut(anim)?, value as i32);
            None
        }
        Events::SetMouthRig => {
            let rig = serde_json::from_str::<Option<MouthRig>>(str_value).ok()?;
            armature.bones.iter_mut().find(|b| b.id == value as i32)?.mouth_rig = rig;
            None
        }
        _ => None,
    }
}
