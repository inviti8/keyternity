//! Voice generation: dialog from text (docs/LIP_SYNC.md §8).
//!
//! Providers, behind one `synthesize`:
//! - **Kokoro** (Apache-2.0), on this computer through sherpa-onnx's official
//!   `sherpa-onnx-offline-tts` (Apache-2.0). The tool and the model are downloaded on
//!   first use, pinned and SHA-256 checked, like FFmpeg and Whisper. No word timings:
//!   generated speech is clean, so lip sync's speech detection times it well.
//! - **System**: the computer's own voices (Windows SAPI, with exact word timings;
//!   macOS `say`; `espeak-ng` on Linux). Nothing to download.
//! - **Chatterbox** (MIT, Resemble AI): voice cloning from a reference clip, run as a
//!   helper script through `uv` (it brings PyTorch; a GPU helps). Its output carries
//!   Resemble's imperceptible watermark.
//! - **Chatterbox (hosted)**: the same model on our own GPU service, hvym-img-tools'
//!   `tts` tool (docs/LIP_SYNC.md §8.2), called as Inkternity calls its tools: an
//!   API key, one long request, and retries on 502/504.

use crate::lipsync::{Language, TimedWord};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub use crate::dialog::{Provider, Voice, VoiceOpts};

/// What a provider made: an audio file, and the words' times if it knows them.
#[derive(Debug)]
pub struct Speech {
    pub bytes: Vec<u8>,
    pub ext: String,
    pub words: Option<Vec<TimedWord>>,
}

/// What providers need from the settings.
#[derive(Clone, Default)]
pub struct VoiceConfig {
    /// `KOKORO_MODELS` index
    pub kokoro_model: usize,
    /// what runs the Chatterbox helper: `uv` (default) or a Python with chatterbox-tts
    pub chatterbox_runner: String,
    /// the hosted service (hvym-img-tools): its base URL (blank: `HVYM_TOOLS_ENDPOINT`,
    /// then `HOSTED_ENDPOINT`) and API key (blank: `HVYM_TOOLS_KEY`)
    pub hosted_endpoint: String,
    pub hosted_key: String,
}

fn data_dir() -> PathBuf {
    directories_next::ProjectDirs::from("com", "keyternity", "keyternity")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
        .join("voice")
}

fn temp_base(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!("keyternity_voice_{}_{}_{tag}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
}

fn hidden(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// Run a command; its last error line if it fails.
fn run(cmd: &mut std::process::Command, what: &str) -> Result<(), String> {
    let out = hidden(cmd).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped()).output().map_err(|e| format!("couldn't run {what}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
    Err(format!("{what} failed: {last}"))
}

// ---------------------------------------------------------------- Kokoro

/// sherpa-onnx's official build of its tools (release v1.13.8): (archive, SHA-256, MB).
#[cfg(target_os = "windows")]
const SHERPA: (&str, &str, u32) = ("sherpa-onnx-v1.13.8-win-x64-shared-MD-MinSizeRel.tar.bz2", "416011eabb9a1e26fd4433b41871d67c7a00b1f0a11b75fe66ff8182b8224f95", 17);
#[cfg(target_os = "macos")]
const SHERPA: (&str, &str, u32) = ("sherpa-onnx-v1.13.8-osx-universal2-shared.tar.bz2", "2249f97f10df7d828af1b0d10e3d1eeaa2198b60b9a999a7ed853c150181ccfd", 44);
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
const SHERPA: (&str, &str, u32) = ("sherpa-onnx-v1.13.8-linux-x64-shared.tar.bz2", "c0bdb7907d3a74bba1d55d22bf4d9fa75586cf1530614ebe88a27b9118e015c4", 28);
const SHERPA_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/";

/// A Kokoro v1.0 model package from sherpa-onnx's model releases.
pub struct KokoroModel {
    pub name: &'static str,
    pub archive: &'static str,
    pub sha256: &'static str,
    pub mb: u32,
    pub onnx: &'static str,
}

/// Full precision (faster on desktop CPUs: measured ~0.65× real time on 8 threads,
/// against ~3× for int8) and int8 (a smaller download).
pub const KOKORO_MODELS: &[KokoroModel] = &[
    KokoroModel { name: "Kokoro v1.0", archive: "kokoro-multi-lang-v1_0.tar.bz2", sha256: "c5f7e2d2caf082bc1d20fb70334a61d99d20b484500aad32e7cf84c128ea3298", mb: 334, onnx: "model.onnx" },
    KokoroModel { name: "Kokoro v1.0 int8", archive: "kokoro-int8-multi-lang-v1_0.tar.bz2", sha256: "4c3052abaa60943a341f193888cf6abd68787dae6ab8ae5c925a706caa247e4e", mb: 126, onnx: "model.int8.onnx" },
];
const KOKORO_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/";

/// Kokoro v1.0's voices (speaker ids from the model's metadata) for the languages lip
/// sync reads: American and British English, French, Brazilian Portuguese.
pub const KOKORO_VOICES: &[(u32, &str)] = &[
    (3, "af_heart"), (0, "af_alloy"), (1, "af_aoede"), (2, "af_bella"), (4, "af_jessica"), (5, "af_kore"), (6, "af_nicole"), (7, "af_nova"),
    (8, "af_river"), (9, "af_sarah"), (10, "af_sky"), (11, "am_adam"), (12, "am_echo"), (13, "am_eric"), (14, "am_fenrir"), (15, "am_liam"),
    (16, "am_michael"), (17, "am_onyx"), (18, "am_puck"), (19, "am_santa"), (20, "bf_alice"), (21, "bf_emma"), (22, "bf_isabella"), (23, "bf_lily"),
    (24, "bm_daniel"), (25, "bm_fable"), (26, "bm_george"), (27, "bm_lewis"), (30, "ff_siwis"), (42, "pf_dora"), (43, "pm_alex"), (44, "pm_santa"),
];

/// A Kokoro voice's language and a readable name ("Heart (US, female)").
pub fn kokoro_voice_info(name: &str) -> (Language, String) {
    let (accent, lang) = match name.chars().next() {
        Some('a') => ("US", Language::English),
        Some('b') => ("UK", Language::English),
        Some('f') => ("FR", Language::French),
        Some('p') => ("BR", Language::PortugueseBr),
        _ => ("", Language::English),
    };
    let sex = if name.chars().nth(1) == Some('m') { "male" } else { "female" };
    let base = name.split('_').nth(1).unwrap_or(name);
    let mut cap = base.chars();
    let pretty = cap.next().map(|c| c.to_uppercase().collect::<String>() + cap.as_str()).unwrap_or_default();
    (lang, format!("{pretty} ({accent}, {sex})"))
}

fn sherpa_dir() -> PathBuf {
    data_dir().join("sherpa-onnx")
}

pub fn kokoro_tool() -> Option<PathBuf> {
    let exe = if cfg!(target_os = "windows") { "sherpa-onnx-offline-tts.exe" } else { "sherpa-onnx-offline-tts" };
    let p = sherpa_dir().join("bin").join(exe);
    p.exists().then_some(p)
}

pub fn kokoro_dir(m: &KokoroModel) -> PathBuf {
    data_dir().join(m.archive.trim_end_matches(".tar.bz2"))
}

pub fn has_kokoro_model(m: &KokoroModel) -> bool {
    kokoro_dir(m).join(m.onnx).exists()
}

/// Download `url` to a file, hashing as it goes; the SHA-256 must match.
fn download_checked(url: &str, sha256: &str, to: &Path, progress: &AtomicU64) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let resp = ureq::get(url).call().map_err(|e| format!("couldn't download: {e}"))?;
    let mut reader = resp.into_body().into_reader();
    if let Some(d) = to.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("couldn't create {}: {e}", d.display()))?;
    }
    let mut file = std::fs::File::create(to).map_err(|e| format!("couldn't write {}: {e}", to.display()))?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| format!("couldn't write {}: {e}", to.display()))?;
        total += n as u64;
        progress.store(total, Ordering::Relaxed);
    }
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != sha256 {
        let _ = std::fs::remove_file(to);
        return Err("the download doesn't match its published checksum (SHA-256 mismatch); nothing was installed".into());
    }
    Ok(())
}

/// Unpack a `.tar.bz2` into `dir`, keeping only entries `keep` accepts (paths with the
/// archive's top folder removed).
fn untar_bz2(archive: &Path, dir: &Path, keep: &dyn Fn(&Path) -> bool) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(std::io::BufReader::new(file)));
    let part = dir.with_extension("part");
    let _ = std::fs::remove_dir_all(&part);
    for entry in tar.entries().map_err(|e| format!("couldn't read the archive: {e}"))? {
        let mut entry = entry.map_err(|e| format!("couldn't read the archive: {e}"))?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        // drop the top folder; refuse anything that climbs out
        let rel: PathBuf = path.components().skip(1).collect();
        if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) || !keep(&rel) {
            continue;
        }
        let to = part.join(&rel);
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        entry.unpack(&to).map_err(|e| format!("couldn't unpack {}: {e}", rel.display()))?;
    }
    let _ = std::fs::remove_dir_all(dir);
    std::fs::rename(&part, dir).map_err(|e| format!("couldn't install into {}: {e}", dir.display()))
}

/// Download and unpack sherpa-onnx's TTS tool.
pub fn fetch_kokoro_tool(progress: &AtomicU64) -> Result<(), String> {
    let tmp = data_dir().join(SHERPA.0);
    download_checked(&format!("{SHERPA_URL}{}", SHERPA.0), SHERPA.1, &tmp, progress)?;
    // the TTS tool and its libraries, not the package's other tools
    let keep = |p: &Path| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let library = name.ends_with(".dll") || name.contains(".so") || name.ends_with(".dylib");
        (p.starts_with("bin") && (name.starts_with("sherpa-onnx-offline-tts") || library)) || (p.starts_with("lib") && library)
    };
    let r = untar_bz2(&tmp, &sherpa_dir(), &keep);
    let _ = std::fs::remove_file(&tmp);
    r
}

pub fn kokoro_tool_mb() -> u32 {
    SHERPA.2
}

/// Download and unpack a Kokoro model.
pub fn fetch_kokoro_model(m: &KokoroModel, progress: &AtomicU64) -> Result<(), String> {
    let tmp = data_dir().join(m.archive);
    download_checked(&format!("{KOKORO_URL}{}", m.archive), m.sha256, &tmp, progress)?;
    let r = untar_bz2(&tmp, &kokoro_dir(m), &|_| true);
    let _ = std::fs::remove_file(&tmp);
    r
}

fn kokoro(text: &str, voice: &Voice, opts: &VoiceOpts, cfg: &VoiceConfig) -> Result<Speech, String> {
    let tool = kokoro_tool().ok_or("Kokoro isn't installed")?;
    let m = &KOKORO_MODELS[cfg.kokoro_model.min(KOKORO_MODELS.len() - 1)];
    let dir = kokoro_dir(m);
    let sid = KOKORO_VOICES.iter().find(|(_, n)| *n == voice.id).map_or(3, |(s, _)| *s);
    let out = temp_base("kokoro").with_extension("wav");
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let mut cmd = std::process::Command::new(&tool);
    cmd.arg(format!("--kokoro-model={}", dir.join(m.onnx).display()))
        .arg(format!("--kokoro-voices={}", dir.join("voices.bin").display()))
        .arg(format!("--kokoro-tokens={}", dir.join("tokens.txt").display()))
        .arg(format!("--kokoro-data-dir={}", dir.join("espeak-ng-data").display()))
        .arg(format!("--num-threads={threads}"))
        .arg(format!("--sid={sid}"))
        .arg(format!("--kokoro-length-scale={}", 1. / opts.speed.clamp(0.5, 2.)))
        .arg(format!("--output-filename={}", out.display()));
    // English through its lexicons (US or UK), other languages through espeak-ng
    match voice.id.chars().next() {
        Some('a') => cmd.arg(format!("--kokoro-lexicon={}", dir.join("lexicon-us-en.txt").display())),
        Some('b') => cmd.arg(format!("--kokoro-lexicon={}", dir.join("lexicon-gb-en.txt").display())),
        Some('f') => cmd.arg("--kokoro-lang=fr"),
        Some('p') => cmd.arg("--kokoro-lang=pt-br"),
        _ => cmd.arg("--kokoro-lang=en"),
    };
    cmd.arg(text);
    let r = run(&mut cmd, "Kokoro");
    let bytes = std::fs::read(&out);
    let _ = std::fs::remove_file(&out);
    r?;
    Ok(Speech { bytes: bytes.map_err(|e| format!("Kokoro wrote no audio: {e}"))?, ext: "wav".into(), words: None })
}

// ---------------------------------------------------------------- the computer's voices

/// The computer's voices (cached after the first look).
pub fn system_voices() -> Vec<Voice> {
    static V: Mutex<Option<Vec<Voice>>> = Mutex::new(None);
    let mut v = V.lock().unwrap();
    if v.is_none() {
        *v = Some(list_system_voices());
    }
    v.clone().unwrap()
}

fn list_system_voices() -> Vec<Voice> {
    let mk = |n: &str| Voice { provider: Provider::System, id: n.to_string(), name: n.to_string() };
    #[cfg(target_os = "windows")]
    {
        let script = "Add-Type -AssemblyName System.Speech; (New-Object System.Speech.Synthesis.SpeechSynthesizer).GetInstalledVoices() | Where-Object Enabled | ForEach-Object { $_.VoiceInfo.Name }";
        let out = hidden(std::process::Command::new("powershell").args(["-NoProfile", "-Command", script])).output();
        out.map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(mk).collect()).unwrap_or_default()
    }
    #[cfg(target_os = "macos")]
    {
        // "Alex                en_US    # Most people recognize me by my voice."
        let out = std::process::Command::new("say").args(["-v", "?"]).output();
        out.map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.split("  ").next().map(str::trim))
                .filter(|n| !n.is_empty())
                .map(mk)
                .collect()
        })
        .unwrap_or_default()
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let found = std::process::Command::new("espeak-ng").arg("--version").output().is_ok();
        if found { vec![mk("espeak-ng")] } else { vec![] }
    }
}

/// Windows SAPI: speak to a WAV, with each word's position (exact).
#[cfg(target_os = "windows")]
fn system(text: &str, voice: &Voice, opts: &VoiceOpts) -> Result<Speech, String> {
    let base = temp_base("sapi");
    let (txt, wav, times, ps1) = (base.with_extension("txt"), base.with_extension("wav"), base.with_extension("tsv"), base.with_extension("ps1"));
    std::fs::write(&txt, text).map_err(|e| e.to_string())?;
    // SAPI's rate is -10..10; roughly ±10 per doubling of speed
    let rate = (opts.speed.clamp(0.5, 2.).log2() * 10.).round() as i32;
    let script = r#"param([string]$Text, [string]$Voice, [int]$Rate, [string]$Wav, [string]$Times)
Add-Type -AssemblyName System.Speech
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
if ($Voice) { $s.SelectVoice($Voice) }
$s.Rate = $Rate
$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo 22050, ([System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen), ([System.Speech.AudioFormat.AudioChannel]::Mono)
$s.SetOutputToWaveFile($Wav, $fmt)
$words = New-Object System.Collections.Generic.List[string]
$null = Register-ObjectEvent -InputObject $s -EventName SpeakProgress -SourceIdentifier kt_sp
$s.Speak([System.IO.File]::ReadAllText($Text))
$s.SetOutputToNull()
Get-Event -SourceIdentifier kt_sp | ForEach-Object { "{0}`t{1}`t{2}" -f [int]$_.SourceEventArgs.AudioPosition.TotalMilliseconds, $_.SourceEventArgs.CharacterPosition, $_.SourceEventArgs.Text } | Set-Content -Encoding UTF8 $Times
# unsubscribe before disposing: an event source disposed under a live subscription
# makes PowerShell exit with an error after the script has finished
Unregister-Event -SourceIdentifier kt_sp
Remove-Event -SourceIdentifier kt_sp -ErrorAction SilentlyContinue
$s.Dispose()
exit 0
"#;
    std::fs::write(&ps1, script).map_err(|e| e.to_string())?;
    let r = run(
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&ps1)
            .arg("-Text")
            .arg(&txt)
            .arg("-Voice")
            .arg(&voice.id)
            .arg("-Rate")
            .arg(rate.to_string())
            .arg("-Wav")
            .arg(&wav)
            .arg("-Times")
            .arg(&times),
        "the system voice",
    );
    let bytes = std::fs::read(&wav);
    let tsv = std::fs::read_to_string(&times).unwrap_or_default();
    for p in [&txt, &wav, &times, &ps1] {
        let _ = std::fs::remove_file(p);
    }
    r?;
    let bytes = bytes.map_err(|e| format!("the system voice wrote no audio: {e}"))?;
    let words = sapi_words(&tsv, crate::audio::decode(0, "", "wav", bytes.clone()).map(|d| d.duration()).unwrap_or(0.));
    Ok(Speech { bytes, ext: "wav".into(), words: (!words.is_empty()).then_some(words) })
}

#[cfg(target_os = "macos")]
fn system(text: &str, voice: &Voice, opts: &VoiceOpts) -> Result<Speech, String> {
    let base = temp_base("say");
    let (txt, wav) = (base.with_extension("txt"), base.with_extension("wav"));
    std::fs::write(&txt, text).map_err(|e| e.to_string())?;
    let wpm = (175. * opts.speed.clamp(0.5, 2.)).round() as i32;
    let mut cmd = std::process::Command::new("say");
    if !voice.id.is_empty() {
        cmd.args(["-v", &voice.id]);
    }
    cmd.args(["-r", &wpm.to_string(), "--file-format=WAVE", "--data-format=LEI16@22050", "-o"]).arg(&wav).arg("-f").arg(&txt);
    let r = run(&mut cmd, "say");
    let bytes = std::fs::read(&wav);
    let _ = std::fs::remove_file(&txt);
    let _ = std::fs::remove_file(&wav);
    r?;
    Ok(Speech { bytes: bytes.map_err(|e| format!("say wrote no audio: {e}"))?, ext: "wav".into(), words: None })
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn system(text: &str, _voice: &Voice, opts: &VoiceOpts) -> Result<Speech, String> {
    let wav = temp_base("espeak").with_extension("wav");
    let wpm = (175. * opts.speed.clamp(0.5, 2.)).round() as i32;
    let r = run(std::process::Command::new("espeak-ng").args(["-v", opts.language.code(), "-s", &wpm.to_string(), "-w"]).arg(&wav).arg(text), "espeak-ng");
    let bytes = std::fs::read(&wav);
    let _ = std::fs::remove_file(&wav);
    r?;
    Ok(Speech { bytes: bytes.map_err(|e| format!("espeak-ng wrote no audio: {e}"))?, ext: "wav".into(), words: None })
}

/// SAPI's word positions (`ms<TAB>char<TAB>word` lines, in event order) as timed
/// words: each ends where the next starts (the last, at the end of the audio).
pub fn sapi_words(tsv: &str, total: f32) -> Vec<TimedWord> {
    let mut rows: Vec<(f32, usize, String)> = tsv
        .lines()
        .filter_map(|l| {
            let c: Vec<&str> = l.trim_start_matches('\u{feff}').splitn(3, '\t').collect();
            Some((c.first()?.trim().parse::<f32>().ok()? / 1000., c.get(1)?.trim().parse().ok()?, c.get(2)?.trim().to_string()))
        })
        .filter(|r| !r.2.is_empty())
        .collect();
    // in the text's order (events can arrive out of order)
    rows.sort_by_key(|r| r.1);
    let starts: Vec<f32> = rows.iter().map(|r| r.0).collect();
    rows.iter()
        .enumerate()
        .map(|(i, r)| {
            let end = starts.get(i + 1).copied().unwrap_or(total).max(r.0 + 0.05);
            TimedWord { text: r.2.clone(), start: r.0, end }
        })
        .collect()
}

// ---------------------------------------------------------------- Chatterbox

/// The helper Keyternity runs for Chatterbox (its dependencies declared inline, for
/// `uv run`).
const CHATTERBOX_HELPER: &str = include_str!("../tools/voice/chatterbox_tts.py");

fn chatterbox(text: &str, opts: &VoiceOpts, cfg: &VoiceConfig) -> Result<Speech, String> {
    let helper = data_dir().join("chatterbox_tts.py");
    std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    std::fs::write(&helper, CHATTERBOX_HELPER).map_err(|e| e.to_string())?;
    let base = temp_base("chatterbox");
    let (txt, wav) = (base.with_extension("txt"), base.with_extension("wav"));
    std::fs::write(&txt, text).map_err(|e| e.to_string())?;
    let runner = if cfg.chatterbox_runner.trim().is_empty() { "uv" } else { cfg.chatterbox_runner.trim() };
    let mut cmd = std::process::Command::new(runner);
    if Path::new(runner).file_stem().is_some_and(|s| s == "uv") {
        cmd.arg("run");
    }
    cmd.arg(&helper).arg("--text-file").arg(&txt).arg("--out").arg(&wav).arg("--language").arg(opts.language.code()).arg("--exaggeration").arg(opts.exaggeration.to_string());
    if !opts.reference.trim().is_empty() {
        cmd.arg("--reference").arg(opts.reference.trim());
    }
    let r = run(&mut cmd, "Chatterbox");
    let bytes = std::fs::read(&wav);
    let _ = std::fs::remove_file(&txt);
    let _ = std::fs::remove_file(&wav);
    r?;
    Ok(Speech { bytes: bytes.map_err(|e| format!("Chatterbox wrote no audio: {e}"))?, ext: "wav".into(), words: None })
}

// ---------------------------------------------------------------- Chatterbox, hosted

/// The hosted tools service's default address.
pub const HOSTED_ENDPOINT: &str = "https://img.hvym.link";

/// The endpoint and key to use: the settings', else the environment's (as Inkternity).
pub fn hosted_target(cfg: &VoiceConfig) -> (String, String) {
    let pick = |set: &str, env: &str| if set.trim().is_empty() { std::env::var(env).unwrap_or_default() } else { set.trim().to_string() };
    let mut endpoint = pick(&cfg.hosted_endpoint, "HVYM_TOOLS_ENDPOINT");
    if endpoint.is_empty() {
        endpoint = HOSTED_ENDPOINT.into();
    }
    (endpoint.trim_end_matches('/').to_string(), pick(&cfg.hosted_key, "HVYM_TOOLS_KEY"))
}

/// A multipart/form-data body: text fields, then optionally one file.
pub fn multipart(fields: &[(&str, String)], file: Option<(&str, &str, &[u8])>) -> (String, Vec<u8>) {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let boundary = format!("----keyternity{nanos:x}");
    let mut body = vec![];
    for (name, value) in fields {
        body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    if let Some((name, filename, data)) = file {
        body.extend(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
        );
        body.extend(data);
        body.extend(b"\r\n");
    }
    body.extend(format!("--{boundary}--\r\n").as_bytes());
    (boundary, body)
}

/// A failed call's message, from its status and the service's `{"detail": ...}`.
pub fn hosted_error(status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body).ok().map(|v| match &v["detail"] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(errs) => errs.iter().filter_map(|e| e["msg"].as_str()).collect::<Vec<_>>().join("; "),
        other => other.to_string(),
    });
    let detail = detail.unwrap_or_else(|| body.chars().take(200).collect());
    match status {
        401 | 403 => "the hosted voice service didn't accept the key (Settings → Voices)".into(),
        402 => "the hosted voice service wants payment for GPU time".into(),
        413 => "the voice reference clip is too large".into(),
        422 => format!("the hosted voice service refused the line: {detail}"),
        503 => "the hosted voice service isn't set up for tts yet".into(),
        504 => "the hosted voice timed out (a cold GPU can take a few minutes); try again".into(),
        _ => format!("the hosted voice failed ({status}): {detail}"),
    }
}

/// How long to wait before retrying a 502/504, which the service says are safe to
/// retry (and its cache usually makes free).
pub const HOSTED_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

fn hosted(text: &str, opts: &VoiceOpts, cfg: &VoiceConfig) -> Result<Speech, String> {
    let (endpoint, key) = hosted_target(cfg);
    if key.is_empty() {
        return Err("add your hosted voice key in Settings → Voices".into());
    }
    if opts.language == Language::Lithuanian {
        return Err("Chatterbox doesn't speak Lithuanian; use a system voice".into());
    }
    let mut fields = vec![
        ("text", text.to_string()),
        ("language", opts.language.code().to_string()),
        ("exaggeration", format!("{:.3}", opts.exaggeration.clamp(0.25, 2.))),
        ("seed", opts.seed.to_string()),
    ];
    // Chatterbox has no speed control; slower or faster pacing comes from cfg_weight
    fields.push(("cfg_weight", format!("{:.3}", (0.5 / opts.speed.clamp(0.5, 2.)).clamp(0.2, 1.))));
    let reference = if opts.reference.trim().is_empty() {
        None
    } else {
        Some(std::fs::read(opts.reference.trim()).map_err(|e| format!("couldn't read the voice reference: {e}"))?)
    };
    let (boundary, body) = multipart(&fields, reference.as_deref().map(|r| ("reference", "reference", r)));

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(900)))
        .timeout_connect(Some(std::time::Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .new_agent();
    let url = format!("{endpoint}/tools/tts");
    let mut attempt = 0;
    loop {
        let resp = agent
            .post(&url)
            .header("X-API-Key", &key)
            .header("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
            .send(&body[..])
            .map_err(|e| format!("couldn't reach the hosted voice service at {endpoint}: {e}"))?;
        let status = resp.status().as_u16();
        let mut reader = resp.into_body().into_reader();
        let mut bytes = vec![];
        reader.read_to_end(&mut bytes).map_err(|e| format!("the hosted voice service's answer was cut off: {e}"))?;
        match status {
            200 => return Ok(Speech { bytes, ext: "wav".into(), words: None }),
            502 | 504 if attempt < 2 => {
                attempt += 1;
                std::thread::sleep(HOSTED_RETRY_DELAY);
            }
            _ => return Err(hosted_error(status, &String::from_utf8_lossy(&bytes))),
        }
    }
}

// ---------------------------------------------------------------- speaking

/// Speak `text` (blocking: run it on a thread).
pub fn synthesize(text: &str, voice: &Voice, opts: &VoiceOpts, cfg: &VoiceConfig) -> Result<Speech, String> {
    if text.trim().is_empty() {
        return Err("type the line first".into());
    }
    match voice.provider {
        Provider::Kokoro => kokoro(text, voice, opts, cfg),
        Provider::System => system(text, voice, opts),
        Provider::Chatterbox => chatterbox(text, opts, cfg),
        Provider::Hosted => hosted(text, opts, cfg),
    }
}

/// The one generation job at a time.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    Idle,
    /// what's downloading, bytes so far, megabytes in all
    Downloading(String, u64, u32),
    Speaking(i32),
    /// the dialog line it was for, and the result
    Finished(i32, Result<(Vec<u8>, String, Option<Vec<TimedWord>>), String>),
}

static JOB: Mutex<Job> = Mutex::new(Job::Idle);

pub fn job() -> Job {
    JOB.lock().unwrap().clone()
}

pub fn busy() -> bool {
    matches!(job(), Job::Downloading(..) | Job::Speaking(_))
}

/// The finished result for dialog line `line`, once.
pub fn take_result(line: i32) -> Option<Result<(Vec<u8>, String, Option<Vec<TimedWord>>), String>> {
    let mut j = JOB.lock().unwrap();
    match &*j {
        Job::Finished(l, _) if *l == line => match std::mem::replace(&mut *j, Job::Idle) {
            Job::Finished(_, r) => Some(r),
            _ => None,
        },
        _ => None,
    }
}

/// Speak a dialog line on a thread, downloading Kokoro first if it's needed.
pub fn start(line: i32, text: String, voice: Voice, opts: VoiceOpts, cfg: VoiceConfig, repaint: impl Fn() + Send + 'static) {
    if busy() {
        return;
    }
    *JOB.lock().unwrap() = Job::Speaking(line);
    std::thread::spawn(move || {
        let fetch = |what: String, mb: u32, f: &dyn Fn(&AtomicU64) -> Result<(), String>| -> Result<(), String> {
            let progress = Arc::new(AtomicU64::new(0));
            let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (p, d) = (progress.clone(), done.clone());
            let ticker = std::thread::spawn(move || {
                while !d.load(Ordering::Relaxed) {
                    *JOB.lock().unwrap() = Job::Downloading(what.clone(), p.load(Ordering::Relaxed), mb);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            });
            let r = f(&progress);
            done.store(true, Ordering::Relaxed);
            let _ = ticker.join();
            r
        };
        let result = (|| {
            if voice.provider == Provider::Kokoro {
                if kokoro_tool().is_none() {
                    fetch("sherpa-onnx".into(), kokoro_tool_mb(), &fetch_kokoro_tool)?;
                }
                let m = &KOKORO_MODELS[cfg.kokoro_model.min(KOKORO_MODELS.len() - 1)];
                if !has_kokoro_model(m) {
                    fetch(m.name.to_string(), m.mb, &|p| fetch_kokoro_model(m, p))?;
                }
            }
            *JOB.lock().unwrap() = Job::Speaking(line);
            repaint();
            synthesize(&text, &voice, &opts, &cfg).map(|s| (s.bytes, s.ext, s.words))
        })();
        *JOB.lock().unwrap() = Job::Finished(line, result);
        repaint();
    });
}

/// An audio file's bytes as audio Keyternity holds (desktop).
pub fn decode(id: i32, name: &str, bytes: Vec<u8>, ext: &str) -> Result<crate::audio::AudioData, String> {
    crate::audio::decode(id, name, ext, bytes)
}
