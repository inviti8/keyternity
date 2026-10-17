//! Transcribing imported audio for lip sync (docs/LIP_SYNC.md §7.4): what was said, and
//! roughly when, from whisper.cpp's command-line tool.
//!
//! As with FFmpeg (docs/FFMPEG.md), the tool isn't built into Keyternity: on Windows
//! the official build is downloaded on first use, pinned and SHA-256 checked, and
//! elsewhere the system's `whisper-cli` is used (Homebrew's `whisper-cpp`, or a build
//! from source). Models are downloaded the same way, pinned to a revision and checked.
//! Both live in Keyternity's data folder.
//!
//! Whisper's word times are rough (measured: ~170 ms mean error on clean speech, some
//! words off by a second), so lip sync uses what it heard to split the words between
//! the stretches of speech, and spreads them within each (`lipsync::spread_heard`).

use crate::lipsync::{Language, TimedWord};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A Whisper model: its file, checksum and size.
pub struct Model {
    pub name: &'static str,
    pub file: &'static str,
    pub sha256: &'static str,
    pub mb: u32,
    /// English only (more accurate for English at the same size)
    pub english: bool,
}

/// The models offered, smallest first. `base.en` is the default.
pub const MODELS: &[Model] = &[
    Model { name: "tiny.en", file: "ggml-tiny.en.bin", sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f", mb: 78, english: true },
    Model { name: "base.en", file: "ggml-base.en.bin", sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002", mb: 148, english: true },
    Model { name: "small.en", file: "ggml-small.en.bin", sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d", mb: 488, english: true },
    Model { name: "tiny", file: "ggml-tiny.bin", sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21", mb: 78, english: false },
    Model { name: "base", file: "ggml-base.bin", sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe", mb: 148, english: false },
    Model { name: "small", file: "ggml-small.bin", sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b", mb: 488, english: false },
];

pub const DEFAULT_MODEL: usize = 1;

/// The model repository, pinned to a revision.
const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/";

/// whisper.cpp's official Windows build (release b5454, 2026-10-06).
#[cfg(target_os = "windows")]
const WHISPER_ZIP_URL: &str = "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip";
#[cfg(target_os = "windows")]
const WHISPER_ZIP_SHA256: &str = "6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb";
#[cfg(target_os = "windows")]
pub const WHISPER_ZIP_MB: u32 = 9;

/// Where the tool and models are kept.
pub fn data_dir() -> PathBuf {
    directories_next::ProjectDirs::from("com", "keyternity", "keyternity")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
        .join("whisper")
}

pub fn model_path(m: &Model) -> PathBuf {
    data_dir().join("models").join(m.file)
}

pub fn has_model(m: &Model) -> bool {
    model_path(m).exists()
}

/// The `whisper-cli` to run, if there is one.
pub fn whisper_cli() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let p = data_dir().join("bin").join("whisper-cli.exe");
        p.exists().then_some(p)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"].map(PathBuf::from));
        dirs.into_iter().map(|d| d.join("whisper-cli")).find(|p| p.exists())
    }
}

/// Whether the tool can be downloaded here (Windows), rather than installed by hand.
pub const CAN_DOWNLOAD_TOOL: bool = cfg!(target_os = "windows");

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Download `url` into memory, counting bytes into `progress`.
fn download(url: &str, progress: &AtomicU64) -> Result<Vec<u8>, String> {
    let resp = ureq::get(url).call().map_err(|e| format!("couldn't download: {e}"))?;
    let mut reader = resp.into_body().into_reader();
    let mut out = vec![];
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        progress.store(out.len() as u64, Ordering::Relaxed);
    }
    Ok(out)
}

/// Write `bytes` to `path` through a `.part` file, so a failed write leaves nothing.
fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    }
    let part = path.with_extension("part");
    std::fs::write(&part, bytes).and_then(|_| std::fs::rename(&part, path)).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("couldn't write {}: {e}", path.display())
    })
}

/// Download and check whisper.cpp's official build, and unpack its command-line tool.
#[cfg(target_os = "windows")]
pub fn fetch_tool(progress: &AtomicU64) -> Result<(), String> {
    let zip_bytes = download(WHISPER_ZIP_URL, progress)?;
    if sha256_hex(&zip_bytes) != WHISPER_ZIP_SHA256 {
        return Err("the whisper.cpp download doesn't match the official build (SHA-256 mismatch); nothing was installed".into());
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).map_err(|e| format!("couldn't open the archive: {e}"))?;
    let names: Vec<String> = zip.file_names().filter(|n| n.ends_with("whisper-cli.exe") || n.ends_with(".dll")).map(String::from).collect();
    if !names.iter().any(|n| n.ends_with("whisper-cli.exe")) {
        return Err("whisper-cli.exe isn't in the archive".into());
    }
    let dir = data_dir().join("bin");
    for name in names {
        let mut bytes = vec![];
        zip.by_name(&name).and_then(|mut e| e.read_to_end(&mut bytes).map_err(Into::into)).map_err(|e| format!("couldn't extract {name}: {e}"))?;
        let file = name.rsplit('/').next().unwrap_or(&name);
        write_atomic(&dir.join(file), &bytes)?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn fetch_tool(_progress: &AtomicU64) -> Result<(), String> {
    Err("install whisper.cpp's whisper-cli (macOS: brew install whisper-cpp)".into())
}

/// Download and check a model.
pub fn fetch_model(m: &Model, progress: &AtomicU64) -> Result<(), String> {
    let bytes = download(&format!("{MODEL_URL}{}", m.file), progress)?;
    if sha256_hex(&bytes) != m.sha256 {
        return Err(format!("the {} model doesn't match its published checksum; nothing was saved", m.name));
    }
    write_atomic(&model_path(m), &bytes)
}

/// Audio as 16 kHz mono 16-bit WAV, as Whisper wants it.
pub fn wav_16k(data: &crate::audio::AudioData) -> Vec<u8> {
    let ch = data.channels.max(1) as usize;
    let frames = data.samples.len() / ch;
    let mono = |f: usize| data.samples[f * ch..f * ch + ch].iter().sum::<f32>() / ch as f32;
    let n = (frames as f64 * 16000. / data.sample_rate.max(1) as f64) as usize;
    let step = data.sample_rate as f64 / 16000.;
    let out: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f64 * step;
            let f0 = (t as usize).min(frames.saturating_sub(1));
            let f1 = (f0 + 1).min(frames.saturating_sub(1));
            let k = (t - f0 as f64) as f32;
            mono(f0) * (1. - k) + mono(f1) * k
        })
        .collect();
    crate::audio::wav_bytes(&out, 1, 16000)
}

/// Words and their times from whisper.cpp's full JSON output (`-ojf`): its tokens
/// joined into words (a token starting with a space starts a word; punctuation stays
/// on the word before), from the first token's start to the last's end.
pub fn parse(json: &str) -> Result<Vec<TimedWord>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("couldn't read Whisper's output: {e}"))?;
    let mut words: Vec<TimedWord> = vec![];
    for seg in v["transcription"].as_array().ok_or("Whisper's output has no transcription")? {
        for t in seg["tokens"].as_array().into_iter().flatten() {
            let text = t["text"].as_str().unwrap_or("");
            // special tokens: [_BEG_], [_TT_123]…
            if text.starts_with("[_") || text.is_empty() {
                continue;
            }
            let ms = |k: &str| t["offsets"][k].as_f64().unwrap_or(0.) as f32 / 1000.;
            let (from, to) = (ms("from"), ms("to"));
            let word_char = text.trim().chars().next().is_some_and(char::is_alphanumeric);
            match words.last_mut() {
                Some(w) if !text.starts_with(' ') || !word_char => {
                    w.text += text.trim();
                    w.end = w.end.max(to);
                }
                _ => words.push(TimedWord { text: text.trim().to_string(), start: from, end: to.max(from) }),
            }
        }
    }
    words.retain(|w| w.text.chars().any(char::is_alphanumeric));
    Ok(words)
}

/// Transcribe audio with `model`, in `lang` (English-only models always listen for
/// English). Blocks; run it on a thread.
pub fn transcribe(data: &crate::audio::AudioData, model: &Model, lang: Language) -> Result<Vec<TimedWord>, String> {
    let cli = whisper_cli().ok_or("whisper.cpp isn't installed")?;
    let base = std::env::temp_dir().join(format!("keyternity_whisper_{}_{}", std::process::id(), data.id));
    let wav = base.with_extension("wav");
    std::fs::write(&wav, wav_16k(data)).map_err(|e| format!("couldn't write the audio for Whisper: {e}"))?;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let mut cmd = std::process::Command::new(&cli);
    cmd.arg("-m")
        .arg(model_path(model))
        .arg("-f")
        .arg(&wav)
        .args(["-ojf", "-np", "-nfa", "-t", &threads.to_string(), "-l", if model.english { "en" } else { lang.code() }])
        .arg("-of")
        .arg(&base)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        // no console window flashing up
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|e| format!("couldn't run whisper-cli: {e}"));
    let _ = std::fs::remove_file(&wav);
    let out = out?;
    let json_path = base.with_extension("json");
    let json = std::fs::read_to_string(&json_path);
    let _ = std::fs::remove_file(&json_path);
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
        return Err(format!("whisper-cli failed: {last}"));
    }
    parse(&json.map_err(|e| format!("Whisper wrote no output: {e}"))?)
}

// ---------------------------------------------------------------- background job

/// The one transcription job at a time: downloading what it needs, then running.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    Idle,
    /// what's downloading, bytes so far, megabytes in all
    Downloading(String, u64, u32),
    Transcribing,
    /// the clip it was for, and the words heard (or why not)
    Finished(i32, Result<Vec<TimedWord>, String>),
}

static JOB: Mutex<Job> = Mutex::new(Job::Idle);

pub fn job() -> Job {
    JOB.lock().unwrap().clone()
}

/// The finished result for `clip`, once (the job goes back to idle).
pub fn take_result(clip: i32) -> Option<Result<Vec<TimedWord>, String>> {
    let mut j = JOB.lock().unwrap();
    match &*j {
        Job::Finished(c, _) if *c == clip => match std::mem::replace(&mut *j, Job::Idle) {
            Job::Finished(_, r) => Some(r),
            _ => None,
        },
        _ => None,
    }
}

pub fn busy() -> bool {
    matches!(job(), Job::Downloading(..) | Job::Transcribing)
}

/// Start transcribing a clip's audio on a thread, downloading the tool and model first
/// if needed. `repaint` is called as it progresses.
pub fn start(clip: i32, data: crate::audio::AudioData, model: usize, lang: Language, repaint: impl Fn() + Send + 'static) {
    if busy() {
        return;
    }
    *JOB.lock().unwrap() = Job::Transcribing;
    std::thread::spawn(move || {
        let set = |j: Job| *JOB.lock().unwrap() = j;
        let m = &MODELS[model.min(MODELS.len() - 1)];
        // a download, reporting progress while it runs
        let fetch = |what: String, mb: u32, f: &dyn Fn(&AtomicU64) -> Result<(), String>| -> Result<(), String> {
            let progress = Arc::new(AtomicU64::new(0));
            let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (p, d) = (progress.clone(), done.clone());
            let what2 = what.clone();
            let ticker = std::thread::spawn(move || {
                while !d.load(Ordering::Relaxed) {
                    *JOB.lock().unwrap() = Job::Downloading(what2.clone(), p.load(Ordering::Relaxed), mb);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            });
            let r = f(&progress);
            done.store(true, Ordering::Relaxed);
            let _ = ticker.join();
            r
        };
        let result = (|| {
            if whisper_cli().is_none() {
                #[cfg(target_os = "windows")]
                fetch("whisper.cpp".into(), WHISPER_ZIP_MB, &fetch_tool)?;
                #[cfg(not(target_os = "windows"))]
                fetch_tool(&AtomicU64::new(0))?;
                repaint();
            }
            if !has_model(m) {
                fetch(format!("the {} model", m.name), m.mb, &|p| fetch_model(m, p))?;
            }
            set(Job::Transcribing);
            repaint();
            transcribe(&data, m, lang)
        })();
        set(Job::Finished(clip, result));
        repaint();
    });
}
