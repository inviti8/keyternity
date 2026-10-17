//! The Dialog window (docs/LIP_SYNC.md §8): type a line, pick a voice, Generate. Each
//! generation is kept as a take to audition and swap; the take in use is the line's
//! clip, lip-synced on its words.

#[cfg_attr(target_arch = "wasm32", allow(unused_imports))]
use crate::dialog::{DialogLine, Provider, Voice};
use crate::*;

fn loc(shared_ui: &crate::Ui, key: &str) -> String {
    shared_ui.loc(&format!("dialog.{key}"))
}

pub fn window(egui_ctx: &egui::Context, shared_ui: &mut crate::Ui, armature: &Armature, config: &Config, selections: &SelectionState, events: &mut EventState) {
    if !shared_ui.dialog_open {
        return;
    }
    let Some(anim) = armature.animations.get(selections.anim) else { return };
    let mut open = true;
    egui::Window::new(format!("{}: {}", loc(shared_ui, "title"), anim.name))
        .id(egui::Id::new("dialog_window"))
        .open(&mut open)
        .resizable(true)
        .default_width(430.)
        .show(egui_ctx, |ui| {
            #[cfg(target_arch = "wasm32")]
            {
                let _ = (config, events);
                ui.label(loc(shared_ui, "desktop_only"));
            }
            #[cfg(not(target_arch = "wasm32"))]
            body(ui, shared_ui, armature, anim, config, selections, events);
        });
    if !open {
        shared_ui.dialog_open = false;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn body(ui: &mut egui::Ui, shared_ui: &mut crate::Ui, armature: &Armature, anim: &Animation, config: &Config, selections: &SelectionState, events: &mut EventState) {
    ui.label(egui::RichText::new(loc(shared_ui, "desc")).small());
    ui.add_space(4.);
    if ui.button(loc(shared_ui, "new_line")).on_hover_text(loc(shared_ui, "new_line_desc")).clicked() {
        let line = DialogLine {
            id: crate::audio::next_id(anim.dialog.iter().map(|l| l.id)),
            voice: Voice { provider: Provider::Kokoro, id: "af_heart".into(), name: crate::voice::kokoro_voice_info("af_heart").1 },
            start_frame: selections.anim_frame.max(0),
            ..Default::default()
        };
        events.set_dialog_line(&line, true);
    }
    ui.add_space(4.);
    egui::ScrollArea::vertical().max_height(520.).auto_shrink([false, true]).show(ui, |ui| {
        for line in &anim.dialog {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                line_ui(ui, shared_ui, armature, config, events, line);
            });
            ui.add_space(4.);
        }
    });
}

/// What runs voice generation, from the settings.
#[cfg(not(target_arch = "wasm32"))]
fn voice_config(config: &Config) -> crate::voice::VoiceConfig {
    crate::voice::VoiceConfig { elevenlabs_key: config.elevenlabs_key.clone(), kokoro_model: config.kokoro_model, chatterbox_runner: config.chatterbox_runner.clone() }
}

#[cfg(not(target_arch = "wasm32"))]
fn line_ui(ui: &mut egui::Ui, shared_ui: &mut crate::Ui, armature: &Armature, config: &Config, events: &mut EventState, line: &DialogLine) {
    use crate::voice::{self, Job, KOKORO_MODELS, KOKORO_VOICES};

    // a finished generation becomes a take
    match voice::take_result(line.id) {
        Some(Ok((bytes, ext, words))) => {
            let ids = armature.audio.iter().map(|a| a.id).chain(shared_ui.pending_audio.iter().map(|a| a.id));
            let audio_id = crate::audio::next_id(ids);
            match crate::audio::decode(audio_id, &crate::dialog::clip_name(&line.text), &ext, bytes) {
                Ok(data) => {
                    let take = crate::dialog::Take {
                        id: crate::audio::next_id(line.takes.iter().map(|t| t.id)),
                        audio: audio_id,
                        voice: line.voice.name.clone(),
                        speed: line.opts.speed,
                        secs: data.duration(),
                        words: words.unwrap_or_default(),
                    };
                    shared_ui.pending_audio.push(data);
                    events.add_take(line.id, &take);
                    shared_ui.dialog_error = None;
                }
                Err(e) => shared_ui.dialog_error = Some((line.id, e)),
            }
        }
        Some(Err(e)) => shared_ui.dialog_error = Some((line.id, e)),
        None => {}
    }

    let mut l = line.clone();
    let mut commit: Option<bool> = None;

    // the words
    let text = ui.add(egui::TextEdit::multiline(&mut l.text).desired_rows(2).desired_width(f32::INFINITY).hint_text(loc(shared_ui, "text_hint")));
    if text.changed() {
        commit = Some(shared_ui.dialog_typing != Some(line.id));
        shared_ui.dialog_typing = Some(line.id);
    }
    if text.lost_focus() {
        shared_ui.dialog_typing = None;
    }

    // language, voice, speed
    ui.horizontal(|ui| {
        let lang = l.opts.language;
        egui::ComboBox::from_id_salt(("dialog_lang", line.id)).selected_text(lang.label()).width(110.).show_ui(ui, |ui| {
            for x in crate::lipsync::Language::ALL {
                if ui.selectable_value(&mut l.opts.language, x, x.label()).clicked() {
                    commit = Some(true);
                }
            }
        });
        let shown = format!("{}: {}", l.voice.provider.label(), l.voice.name);
        egui::ComboBox::from_id_salt(("dialog_voice", line.id)).selected_text(shown).width(200.).show_ui(ui, |ui| {
            let mut pick = |ui: &mut egui::Ui, v: Voice| {
                let on = l.voice.provider == v.provider && l.voice.id == v.id;
                if ui.selectable_label(on, &v.name).clicked() {
                    l.voice = v;
                    commit = Some(true);
                }
            };
            ui.label(egui::RichText::new("Kokoro").strong());
            for (_, name) in KOKORO_VOICES {
                let (vl, pretty) = voice::kokoro_voice_info(name);
                if vl == l.opts.language {
                    pick(ui, Voice { provider: Provider::Kokoro, id: name.to_string(), name: pretty });
                }
            }
            ui.separator();
            ui.label(egui::RichText::new(loc(shared_ui, "system")).strong());
            for v in voice::system_voices() {
                pick(ui, v);
            }
            ui.separator();
            ui.label(egui::RichText::new("ElevenLabs").strong());
            match &shared_ui.elevenlabs_voices {
                Some(Ok(vs)) => {
                    for v in vs.clone() {
                        pick(ui, v);
                    }
                }
                Some(Err(e)) => {
                    ui.label(egui::RichText::new(e).small());
                }
                None => {}
            }
            if !config.elevenlabs_key.trim().is_empty() && ui.small_button(loc(shared_ui, "load_elevenlabs")).clicked() {
                shared_ui.elevenlabs_voices = Some(voice::elevenlabs_voices(config.elevenlabs_key.trim()));
            } else if config.elevenlabs_key.trim().is_empty() {
                ui.label(egui::RichText::new(loc(shared_ui, "elevenlabs_key")).small());
            }
            ui.separator();
            pick(ui, Voice { provider: Provider::Chatterbox, id: "chatterbox".into(), name: loc(shared_ui, "chatterbox") });
        });
        ui.label(loc(shared_ui, "speed"));
        let s = ui.add(egui::DragValue::new(&mut l.opts.speed).range(0.5..=2.0).speed(0.01).suffix("×"));
        if s.changed() {
            commit = Some(s.drag_started() || !s.dragged());
        }
    });

    // Chatterbox: who to clone, with their consent
    if l.voice.provider == Provider::Chatterbox {
        ui.horizontal(|ui| {
            ui.label(loc(shared_ui, "reference"));
            let r = ui.add(egui::TextEdit::singleline(&mut l.opts.reference).desired_width(200.).hint_text(loc(shared_ui, "reference_hint")));
            if r.lost_focus() && r.changed() {
                commit = Some(true);
            }
            if ui.small_button(loc(shared_ui, "choose")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Audio", &["wav", "mp3", "flac", "ogg"]).pick_file() {
                    l.opts.reference = p.to_string_lossy().to_string();
                    commit = Some(true);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(loc(shared_ui, "exaggeration")).on_hover_text(loc(shared_ui, "exaggeration_desc"));
            let s = ui.add(egui::Slider::new(&mut l.opts.exaggeration, 0.25..=2.0).max_decimals(2));
            if s.drag_stopped() || (s.changed() && !s.dragged()) {
                commit = Some(true);
            }
        });
        if !l.opts.reference.trim().is_empty() {
            let (label, tip) = (loc(shared_ui, "consent"), loc(shared_ui, "consent_desc"));
            ui.checkbox(&mut shared_ui.chatterbox_consent, label).on_hover_text(tip);
        }
    }

    if let Some(step) = commit {
        if l != *line {
            events.set_dialog_line(&l, step);
        }
    }

    // generating
    let job = voice::job();
    let busy = voice::busy();
    let cloning = l.voice.provider == Provider::Chatterbox && !l.opts.reference.trim().is_empty();
    let ready = !busy && !l.text.trim().is_empty() && (!cloning || shared_ui.chatterbox_consent);
    ui.horizontal(|ui| {
        let mut tip = loc(shared_ui, "generate_desc");
        match l.voice.provider {
            Provider::Kokoro => {
                let m = &KOKORO_MODELS[config.kokoro_model.min(KOKORO_MODELS.len() - 1)];
                let mut mb = 0;
                if voice::kokoro_tool().is_none() {
                    mb += voice::kokoro_tool_mb();
                }
                if !voice::has_kokoro_model(m) {
                    mb += m.mb;
                }
                if mb > 0 {
                    tip += "\n\n";
                    tip += &loc(shared_ui, "kokoro_download").replace("$mb", &mb.to_string());
                }
            }
            Provider::ElevenLabs => {
                tip += "\n\n";
                tip += &loc(shared_ui, "elevenlabs_note");
            }
            Provider::Chatterbox => {
                tip += "\n\n";
                tip += &loc(shared_ui, "chatterbox_note");
            }
            Provider::System => {}
        }
        let label = loc(shared_ui, if line.takes.is_empty() { "generate" } else { "another_take" });
        if ui.add_enabled(ready, egui::Button::new(label)).on_hover_text(tip).clicked() {
            let ctx = ui.ctx().clone();
            voice::start(line.id, l.text.clone(), l.voice.clone(), l.opts.clone(), voice_config(config), move || ctx.request_repaint());
        }
        let status = match &job {
            Job::Downloading(what, bytes, mb) => {
                Some(loc(shared_ui, "downloading").replace("$what", what).replace("$done", &format!("{:.0}", *bytes as f64 / 1048576.)).replace("$mb", &mb.to_string()))
            }
            Job::Speaking(id) if *id == line.id => Some(loc(shared_ui, "speaking")),
            _ => None,
        };
        if let Some(s) = status {
            ui.label(egui::RichText::new(s).small());
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("✕").on_hover_text(loc(shared_ui, "remove_line")).clicked() {
                events.delete_dialog_line(line.id);
            }
        });
    });
    if let Some((id, e)) = &shared_ui.dialog_error {
        if *id == line.id {
            ui.label(egui::RichText::new(e).small().color(egui::Color32::from_rgb(230, 110, 100)));
        }
    }

    // the takes
    for t in line.takes.iter().rev() {
        ui.horizontal(|ui| {
            let using = t.id == line.take;
            let mark = if using { "●" } else { "○" };
            let words = if t.words.is_empty() { String::new() } else { format!(" · {}", loc(shared_ui, "timed")) };
            ui.label(egui::RichText::new(format!("{mark} {} {} · {} · {:.2}× · {:.1} s{words}", loc(shared_ui, "take"), t.id, t.voice, t.speed, t.secs)).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕").on_hover_text(loc(shared_ui, "delete_take")).clicked() {
                    events.delete_take(line.id, t.id);
                }
                if !using && ui.small_button(loc(shared_ui, "use")).on_hover_text(loc(shared_ui, "use_desc")).clicked() {
                    events.use_take(line.id, t.id);
                }
                let audio = armature.audio.iter().find(|a| a.id == t.audio).or_else(|| shared_ui.pending_audio.iter().find(|a| a.id == t.audio));
                if let Some(a) = audio {
                    if ui.small_button("▶").on_hover_text(loc(shared_ui, "listen")).clicked() {
                        crate::audio::play_data(a);
                    }
                }
            });
        });
    }
}
