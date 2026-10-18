//! Dialog lines and voice generation (docs/LIP_SYNC.md §8): providers' answers, takes
//! through the editor (adding, swapping, deleting, undo), exact timings into lip sync,
//! and saving every take.

use keyternity_lib::audio;
use keyternity_lib::dialog::{DialogLine, Provider, Reference, Take, Voice};
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
    let s = voice::synthesize("Hello there, how are you?", &voices[0], &Default::default(), &Default::default(), None).unwrap();
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
    let s = voice::synthesize("Hello there. Would you like some water?", &v, &Default::default(), &Default::default(), None).unwrap();
    let d = audio::decode(1, "x", &s.ext, s.bytes).unwrap();
    assert!(d.duration() > 1.5, "{} s", d.duration());
    let spans = lipsync::speech_spans(&d.samples, d.channels, d.sample_rate);
    assert!(!spans.is_empty());
}

/// A one-shot HTTP server: answers each request in turn with (status, content type,
/// body), recording what it was sent.
fn stub_server(answers: Vec<(u16, &'static str, Vec<u8>)>) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let log = seen.clone();
    std::thread::spawn(move || {
        for (status, ctype, body) in answers {
            let (mut s, _) = listener.accept().unwrap();
            let mut req = vec![];
            let mut buf = [0u8; 65536];
            // read the headers, then exactly Content-Length bytes of body
            loop {
                let n = s.read(&mut buf).unwrap();
                req.extend_from_slice(&buf[..n]);
                if let Some(end) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&req[..end]).to_lowercase();
                    let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                    while req.len() < end + 4 + len {
                        let n = s.read(&mut buf).unwrap();
                        req.extend_from_slice(&buf[..n]);
                    }
                    break;
                }
            }
            log.lock().unwrap().push(req);
            let head = format!("HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            s.write_all(head.as_bytes()).unwrap();
            s.write_all(&body).unwrap();
        }
    });
    (url, seen)
}

/// A hosted voice config with a wallet of its own (never Keyternity's real one).
fn hosted_cfg(url: &str) -> voice::VoiceConfig {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let wallet = std::env::temp_dir().join(format!("keyternity_test_wallet_{}_{n}.json", std::process::id()));
    let _ = std::fs::remove_file(&wallet);
    voice::VoiceConfig { hosted_endpoint: url.into(), hosted_key: "k-123".into(), wallet_path: Some(wallet), ..Default::default() }
}

#[test]
fn hosted_chatterbox_sends_the_line_and_gets_audio() {
    let wav = speech(0.4, &[]);
    let (url, seen) = stub_server(vec![(200, "audio/wav", wav.clone())]);
    let clip = speech(0.2, &[]);
    let reference = Reference { bytes: std::sync::Arc::new(clip.clone()), ext: "wav".into() };
    let opts = keyternity_lib::dialog::VoiceOpts { language: Language::French, exaggeration: 0.8, seed: 3, ..Default::default() };
    let cfg = hosted_cfg(&url);
    let v = Voice { provider: Provider::Hosted, id: "chatterbox".into(), name: "Chatterbox (hosted)".into() };
    let s = voice::synthesize("Bonjour.", &v, &opts, &cfg, Some(&reference)).unwrap();
    assert_eq!(s.bytes, wav);
    assert!(s.words.is_none());

    let req = seen.lock().unwrap()[0].clone();
    let text = String::from_utf8_lossy(&req);
    assert!(text.starts_with("POST /tools/tts "), "{}", &text[..40]);
    assert!(text.to_lowercase().contains("x-api-key: k-123"));
    // signed by the wallet, for the path the service sees
    let wallet = keyternity_lib::wallet::Wallet::load(cfg.wallet_path.as_ref().unwrap()).unwrap().unwrap();
    assert!(text.contains(&format!("x-ink-pubkey: {}", wallet.address)) || text.contains(&format!("X-Ink-Pubkey: {}", wallet.address)));
    assert!(text.contains(r#""p":"/tools/tts""#) || signed_payload(&text).contains(r#""p":"/tools/tts""#));
    assert!(text.contains("multipart/form-data; boundary="));
    // the fields the service's TtsInput declares
    for (field, value) in [("text", "Bonjour."), ("language", "fr"), ("exaggeration", "0.800"), ("seed", "3")] {
        assert!(text.contains(&format!("name=\"{field}\"\r\n\r\n{value}\r\n")), "{field}");
    }
    assert!(text.contains("name=\"cfg_weight\""));
    assert!(text.contains("name=\"reference\"; filename=\"reference.wav\""));
    assert!(req.windows(clip.len()).any(|w| w == clip.as_slice()), "the clip's bytes are sent");
}

#[test]
fn hosted_chatterbox_explains_failures() {
    let detail = br#"{"detail":[{"msg":"Value error, unsupported language 'xx'"}]}"#.to_vec();
    let (url, _) = stub_server(vec![
        (422, "application/json", detail),
        (401, "application/json", br#"{"detail":"bad key"}"#.to_vec()),
        (401, "application/json", br#"{"detail":"signed identity: signature is stale"}"#.to_vec()),
    ]);
    let cfg = hosted_cfg(&url);
    let v = Voice { provider: Provider::Hosted, id: "chatterbox".into(), name: String::new() };
    let e = voice::synthesize("Hi.", &v, &Default::default(), &cfg, None).unwrap_err();
    assert!(e.contains("refused the line") && e.contains("unsupported language"), "{e}");
    let e = voice::synthesize("Hi.", &v, &Default::default(), &cfg, None).unwrap_err();
    assert!(e.contains("didn't accept the key"), "{e}");
    let e = voice::synthesize("Hi.", &v, &Default::default(), &cfg, None).unwrap_err();
    assert!(e.contains("signature") && e.contains("clock"), "{e}");
    // a language Chatterbox doesn't speak: refused before any call
    let lt = keyternity_lib::dialog::VoiceOpts { language: Language::Lithuanian, ..Default::default() };
    assert!(voice::synthesize("Labas.", &v, &lt, &cfg, None).unwrap_err().contains("Lithuanian"));
}

#[test]
fn hosted_endpoint_defaults() {
    let (url, _) = voice::hosted_target(&voice::VoiceConfig { hosted_endpoint: "https://x.example/ ".into(), hosted_key: "k".into(), ..Default::default() });
    assert_eq!(url, "https://x.example");
    if std::env::var("HVYM_TOOLS_ENDPOINT").is_err() {
        assert_eq!(voice::hosted_target(&Default::default()).0, voice::HOSTED_ENDPOINT);
    }
}

/// The signed payload in a request's `X-Ink-Auth` header.
fn signed_payload(request: &str) -> String {
    use base64::Engine;
    let auth = request.lines().find_map(|l| l.strip_prefix("x-ink-auth: ").or_else(|| l.strip_prefix("X-Ink-Auth: "))).unwrap_or_default();
    let payload = auth.split_once('.').map(|(_, p)| p).unwrap_or_default();
    String::from_utf8(base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim()).unwrap_or_default()).unwrap_or_default()
}

const USDC_ISSUER: &str = "GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN";
const PAYEE: &str = "GA32CYGMEIFC7FVWEWMAUWVBG52ZSR7AEYCFDZZQ5KJFNBHB3FRYXEEZ";

/// The proxy's 402 body (hvym-img-tools X402_BILLING.md §4.1).
fn quote(network: &str, amount: &str) -> Vec<u8> {
    format!(
        r#"{{"detail":"no live paid window for this identity","x402":{{"asset":"USDC","issuer":"{USDC_ISSUER}","amount":"{amount}","pay_to":"{PAYEE}","network":"{network}","window_s":900.0,"memo":"q-123","price_id":"q-123","horizon":"x"}}}}"#
    )
    .into_bytes()
}

/// Horizon's account view: a sequence number and these balances (code, balance).
fn horizon_account(balances: &[(&str, &str)]) -> Vec<u8> {
    let bs: Vec<String> = balances
        .iter()
        .map(|(code, b)| {
            if *code == "XLM" {
                format!(r#"{{"balance":"{b}","asset_type":"native"}}"#)
            } else {
                format!(r#"{{"balance":"{b}","asset_type":"credit_alphanum4","asset_code":"{code}","asset_issuer":"{USDC_ISSUER}"}}"#)
            }
        })
        .collect();
    format!(r#"{{"sequence":"4100","balances":[{}]}}"#, bs.join(",")).into_bytes()
}

/// A request's body (after its headers).
fn body_of(req: &[u8]) -> Vec<u8> {
    let end = req.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    req[end + 4..].to_vec()
}

/// The transaction envelope a `POST /transactions` form carries, decoded.
fn submitted_tx(req: &[u8]) -> Vec<u8> {
    use base64::Engine;
    let form = String::from_utf8(body_of(req)).unwrap();
    let b64 = form.strip_prefix("tx=").unwrap().replace("%2B", "+").replace("%2F", "/").replace("%3D", "=");
    base64::engine::general_purpose::STANDARD.decode(b64).unwrap()
}

fn paying(url: &str) -> voice::VoiceConfig {
    voice::VoiceConfig { hosted_autopay: true, hosted_max_price: 1., stellar_network: "testnet".into(), horizon: url.into(), ..hosted_cfg(url) }
}

#[test]
fn a_402_is_paid_from_the_wallet_and_the_line_retried() {
    let wav = speech(0.3, &[]);
    let (url, seen) = stub_server(vec![
        (402, "application/json", quote("testnet", "0.75")),
        (200, "application/json", horizon_account(&[("XLM", "9.5000000"), ("USDC", "3.0000000")])),
        (200, "application/json", br#"{"successful":true}"#.to_vec()),
        (200, "application/json", br#"{"settled":true,"credited":true}"#.to_vec()),
        (200, "audio/wav", wav.clone()),
    ]);
    let cfg = paying(&url);
    let v = Voice { provider: Provider::Hosted, id: "chatterbox".into(), name: String::new() };
    let s = voice::synthesize("Hello.", &v, &Default::default(), &cfg, None).unwrap();
    assert_eq!(s.bytes, wav);

    let reqs = seen.lock().unwrap().clone();
    let line = |i: usize| String::from_utf8_lossy(&reqs[i]).lines().next().unwrap().to_string();
    let wallet = keyternity_lib::wallet::Wallet::load(cfg.wallet_path.as_ref().unwrap()).unwrap().unwrap();
    assert!(line(0).starts_with("POST /tools/tts "));
    assert_eq!(line(1), format!("GET /accounts/{} HTTP/1.1", wallet.address));
    assert!(line(2).starts_with("POST /transactions "));
    assert!(line(3).starts_with("POST /warm/pay "));
    assert!(line(4).starts_with("POST /tools/tts "), "the line, again");

    // the payment: from the wallet, 0.75 USDC to the payee, with the quote's memo
    let tx = submitted_tx(&reqs[2]);
    let has = |b: &[u8]| tx.windows(b.len()).any(|w| w == b);
    assert!(has(&keyternity_lib::wallet::decode_address(&wallet.address).unwrap()));
    assert!(has(&keyternity_lib::wallet::decode_address(PAYEE).unwrap()));
    assert!(has(&7_500_000i64.to_be_bytes()), "0.75 in stroops");
    assert!(has(b"q-123"), "the memo binds it to the quote");
    assert!(has(&4101i64.to_be_bytes()), "the next sequence number");

    // the proof: its hash, signed, for the quote
    let pay = String::from_utf8_lossy(&reqs[3]).to_string();
    let hash = pay.lines().find_map(|l| l.strip_prefix("x-payment: ").or_else(|| l.strip_prefix("X-Payment: "))).unwrap().trim().to_string();
    assert_eq!(hash.len(), 64);
    assert!(signed_payload(&pay).contains(r#""p":"/warm/pay""#) && signed_payload(&pay).contains(r#""tool":"tts""#));
    assert_eq!(String::from_utf8(body_of(&reqs[3])).unwrap(), r#"{"price_id":"q-123","tool":"tts"}"#);
    // a fresh signature for the retry
    assert_ne!(signed_payload(&String::from_utf8_lossy(&reqs[0])), signed_payload(&String::from_utf8_lossy(&reqs[4])));
}

#[test]
fn nothing_is_paid_without_the_owners_say_so() {
    let v = Voice { provider: Provider::Hosted, id: "chatterbox".into(), name: String::new() };
    // paying is off: the price is explained, and nothing else is called
    let (url, seen) = stub_server(vec![(402, "application/json", quote("testnet", "0.75"))]);
    let off = voice::VoiceConfig { hosted_autopay: false, ..paying(&url) };
    let e = voice::synthesize("Hi.", &v, &Default::default(), &off, None).unwrap_err();
    assert!(e.contains("0.75 USDC for 15 minutes") && e.contains("Settings"), "{e}");
    assert_eq!(seen.lock().unwrap().len(), 1);
    // over the limit
    let (url, seen) = stub_server(vec![(402, "application/json", quote("testnet", "5.00"))]);
    let e = voice::synthesize("Hi.", &v, &Default::default(), &paying(&url), None).unwrap_err();
    assert!(e.contains("more than your limit"), "{e}");
    assert_eq!(seen.lock().unwrap().len(), 1);
    // on another network than the wallet pays on
    let (url, seen) = stub_server(vec![(402, "application/json", quote("public", "0.75"))]);
    let e = voice::synthesize("Hi.", &v, &Default::default(), &paying(&url), None).unwrap_err();
    assert!(e.contains("network") && e.contains("not paying"), "{e}");
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn a_wallet_that_cant_pay_says_why() {
    let v = Voice { provider: Provider::Hosted, id: "chatterbox".into(), name: String::new() };
    // not on the network yet
    let (url, _) = stub_server(vec![(402, "application/json", quote("testnet", "0.75")), (404, "application/json", b"{}".to_vec())]);
    let e = voice::synthesize("Hi.", &v, &Default::default(), &paying(&url), None).unwrap_err();
    assert!(e.contains("empty") && e.contains("XLM"), "{e}");
    // no USDC trustline: one is opened, and the wallet still needs USDC
    let (url, seen) = stub_server(vec![
        (402, "application/json", quote("testnet", "0.75")),
        (200, "application/json", horizon_account(&[("XLM", "5.0000000")])),
        (200, "application/json", br#"{"successful":true}"#.to_vec()),
    ]);
    let e = voice::synthesize("Hi.", &v, &Default::default(), &paying(&url), None).unwrap_err();
    assert!(e.contains("can hold USDC now"), "{e}");
    let tx = submitted_tx(&seen.lock().unwrap()[2]);
    assert!(tx.windows(4).any(|w| w == b"USDC") && tx.windows(8).any(|w| w == i64::MAX.to_be_bytes()), "a change-trust at the maximum limit");
    // not enough USDC
    let refused = br#"{"extras":{"result_codes":{"transaction":"tx_failed","operations":["op_underfunded"]}}}"#.to_vec();
    let (url, _) = stub_server(vec![
        (402, "application/json", quote("testnet", "0.75")),
        (200, "application/json", horizon_account(&[("XLM", "5.0000000"), ("USDC", "0.1000000")])),
        (400, "application/json", refused),
    ]);
    let e = voice::synthesize("Hi.", &v, &Default::default(), &paying(&url), None).unwrap_err();
    assert!(e.contains("doesn't have enough"), "{e}");
    // paid, but the service still says 402: stop there, never pay twice
    let (url, seen) = stub_server(vec![
        (402, "application/json", quote("testnet", "0.75")),
        (200, "application/json", horizon_account(&[("XLM", "5.0000000"), ("USDC", "2.0000000")])),
        (200, "application/json", b"{}".to_vec()),
        (200, "application/json", b"{}".to_vec()),
        (402, "application/json", quote("testnet", "0.75")),
    ]);
    let e = voice::synthesize("Hi.", &v, &Default::default(), &paying(&url), None).unwrap_err();
    assert!(e.contains("still asks"), "{e}");
    assert_eq!(seen.lock().unwrap().len(), 5);
}

/// Opt-in: Stellar testnet against a local proxy with billing enforced, in two
/// stages a harness runs (it funds the wallet between them). `trust`: the wallet has
/// XLM only, so paying opens its USDC trustline and stops. `pay`: it has USDC, so the
/// first line buys a window and the second is spoken inside it.
#[test]
#[ignore]
fn hosted_pays_on_testnet() {
    let (Ok(url), Ok(wallet), Ok(stage)) = (std::env::var("HVYM_TTS_TEST_URL"), std::env::var("KEYTERNITY_TEST_WALLET"), std::env::var("KEYTERNITY_TESTNET_STAGE")) else { return };
    let cfg = voice::VoiceConfig {
        hosted_endpoint: url,
        hosted_key: std::env::var("HVYM_TTS_TEST_KEY").unwrap_or_default(),
        hosted_autopay: true,
        hosted_max_price: 1.,
        stellar_network: "testnet".into(),
        wallet_path: Some(wallet.into()),
        ..Default::default()
    };
    let v = Voice { provider: Provider::Hosted, id: "chatterbox".into(), name: String::new() };
    match stage.as_str() {
        "trust" => {
            let e = voice::synthesize("Hello there.", &v, &Default::default(), &cfg, None).unwrap_err();
            println!("stage trust: {e}");
            assert!(e.contains("can hold USDC now"), "{e}");
        }
        "pay" => {
            for (i, line) in ["Hello there. How are you today?", "I paid for this line, and the next one is free."].iter().enumerate() {
                let started = std::time::Instant::now();
                let s = voice::synthesize(line, &v, &Default::default(), &cfg, None).unwrap();
                let d = audio::decode(1, "x", &s.ext, s.bytes).unwrap();
                assert_eq!((d.channels, d.sample_rate), (1, 24_000));
                println!("line {i}: {:.2} s of audio in {:.1} s", d.samples.len() as f32 / 24_000., started.elapsed().as_secs_f32());
            }
        }
        _ => panic!("unknown stage {stage}"),
    }
}

/// Opt-in: against a running hvym-img-tools server (its real routing, multipart
/// parsing, validation, auth and cache; a fake model is fine).
/// `HVYM_TTS_TEST_URL=http://127.0.0.1:8765 HVYM_TTS_TEST_KEY=… cargo test --test all -- dialog::hosted_chatterbox_against_the_service --ignored`
#[test]
#[ignore]
fn hosted_chatterbox_against_the_service() {
    let (Ok(url), Ok(key)) = (std::env::var("HVYM_TTS_TEST_URL"), std::env::var("HVYM_TTS_TEST_KEY")) else { return };
    let v = Voice { provider: Provider::Hosted, id: "chatterbox".into(), name: String::new() };
    let cfg = voice::VoiceConfig { hosted_key: key, ..hosted_cfg(&url) };
    // English, no reference
    let s = voice::synthesize("Hello there.", &v, &Default::default(), &cfg, None).unwrap();
    let d = audio::decode(1, "x", &s.ext, s.bytes).unwrap();
    assert_eq!((d.channels, d.sample_rate), (1, 24_000));
    // French with a voice reference
    let reference = Reference { bytes: std::sync::Arc::new(speech(0.3, &[])), ext: "wav".into() };
    let fr = keyternity_lib::dialog::VoiceOpts { language: Language::French, seed: 2, ..Default::default() };
    voice::synthesize("Bonjour.", &v, &fr, &cfg, Some(&reference)).unwrap();
    // a wrong key is refused
    let bad = voice::VoiceConfig { hosted_key: "wrong".into(), ..cfg.clone() };
    assert!(voice::synthesize("Hi.", &v, &Default::default(), &bad, None).unwrap_err().contains("key"));
}


/// a cloned voice added the way the Dialog window adds it: its clip waits in the UI
fn add_voice(s: &mut Shared, id: i32, audio_id: i32, consent: bool) {
    let data = audio::decode(audio_id, "Alice", "wav", speech(0.5, &[])).unwrap();
    s.ui.pending_audio.push(data);
    s.events.add_cloned_voice(&keyternity_lib::dialog::ClonedVoice { id, name: "Alice".into(), audio: audio_id, consent });
    run(s);
}

#[test]
fn cloned_voices_live_with_the_rig() {
    let mut s = with_line();
    add_voice(&mut s, 1, 40, true);
    assert_eq!(s.armature.voices.len(), 1);
    assert!(s.armature.audio.iter().any(|a| a.id == 40), "the clip moved into the rig");
    // saved: listed in armature.json, and its clip among the audio files
    let (json, _) = utils::prepare_files(&s.armature, Camera::default(), vec![], &EditMode::default(), None);
    let root: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(root["voices"][0]["name"], "Alice");
    assert!(utils::used_audio(&s.armature).iter().any(|a| a.id == 40));
    // a line speaking in it gets its clip; Chatterbox's own voice gets none
    let opts = keyternity_lib::dialog::VoiceOpts { cloned: 1, ..Default::default() };
    let r = keyternity_lib::dialog::reference_of(&s.armature, &opts).unwrap().unwrap();
    assert_eq!((r.ext.as_str(), r.bytes.len()), ("wav", speech(0.5, &[]).len()));
    assert!(keyternity_lib::dialog::reference_of(&s.armature, &Default::default()).unwrap().is_none());
    // removed: lines using it are told to pick another
    s.events.delete_cloned_voice(1);
    run(&mut s);
    assert!(keyternity_lib::dialog::reference_of(&s.armature, &opts).unwrap_err().contains("gone"));
}

#[test]
fn a_voice_needs_its_owners_consent() {
    let mut s = with_line();
    add_voice(&mut s, 1, 40, false);
    assert!(s.armature.voices.is_empty(), "never added without consent");
    // and one on record without consent (an edited file) is refused at use
    s.armature.voices.push(keyternity_lib::dialog::ClonedVoice { id: 2, name: "Bob".into(), audio: 40, consent: false });
    let opts = keyternity_lib::dialog::VoiceOpts { cloned: 2, ..Default::default() };
    assert!(keyternity_lib::dialog::reference_of(&s.armature, &opts).unwrap_err().contains("consent"));
}
