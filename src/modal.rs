use crate::{shared::Display, ui::EguiUi, utils, Config, PolarId};

pub fn modal_template<T: FnOnce(&mut egui::Ui), E: FnOnce(&mut egui::Ui)>(
    ctx: &egui::Context,
    id: String,
    config: &Config,
    content: T,
    buttons: E,
) {
    let modal = egui::Modal::new(id.into()).frame(egui::Frame {
        corner_radius: 0.into(),
        fill: config.colors.main.into(),
        inner_margin: egui::Margin::same(5),
        stroke: egui::Stroke::new(1., config.colors.light_accent),
        ..Default::default()
    });
    modal.show(ctx, |ui| {
        ui.set_width(300.);
        content(ui);
        ui.add_space(20.);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            buttons(ui);
        })
    });
}

pub fn polar_modal(
    ctx: &egui::Context,
    config: &Config,
    shared_ui: &mut crate::Ui,
    events: &mut crate::EventState,
) {
    let headline = shared_ui.headline.to_string();

    modal_template(
        ctx,
        "polar".to_string(),
        &config,
        |ui| {
            let mut cache = egui_commonmark::CommonMarkCache::default();
            let str = utils::markdown(headline.clone()).replace("$psd_page", "");
            egui_commonmark::CommonMarkViewer::new().show(ui, &mut cache, &str);
        },
        |ui| {
            let mut yes = false;

            // Proceeding with kb shortcut will only emulate 'yes' if modal isn't for discarding changes.
            // This is to prevent users with muscle memory from accidentally exiting
            // upon pressing 'enter' upon seeing a modal.
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if shared_ui.polar_id != PolarId::Exiting {
                    shared_ui.polar_modal = false;
                    yes = true;
                }
            }
            let mut key_col = config.colors.text;
            key_col -= crate::Color::new(50, 50, 50, 0);

            // No
            let mut str_no = egui::text::LayoutJob::default();
            let str = &shared_ui.loc("polar.no");
            crate::ui::job_text(str, Some(config.colors.text.into()), &mut str_no);
            let str_key = &format!(" ({})", &config.keys.cancel.display());
            crate::ui::job_text(str_key, Some(key_col.into()), &mut str_no);
            let pressed_no = ui.input_mut(|i| i.consume_shortcut(&config.keys.cancel));
            if ui.skf_button(str_no).clicked() || pressed_no {
                shared_ui.polar_modal = false;
            }

            // Yes
            let mut str_yes = egui::text::LayoutJob::default();
            let str = &shared_ui.loc("polar.yes");
            crate::ui::job_text(str, Some(config.colors.text.into()), &mut str_yes);
            if shared_ui.polar_id != PolarId::Exiting {
                let str_key = &format!(" ({})", &config.keys.polar_yes.display());
                crate::ui::job_text(str_key, Some(key_col.into()), &mut str_yes);
            }
            let pressed_yes = ui.input_mut(|i| i.consume_shortcut(&config.keys.polar_yes));
            if ui.skf_button(str_yes).clicked()
                || (pressed_yes && shared_ui.polar_id != PolarId::Exiting)
            {
                shared_ui.polar_modal = false;
                yes = true
            }

            // psd help page link, if $psd_page is in the loc
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let psd_button = headline.contains("$psd_page");
                let str = egui::RichText::new(shared_ui.loc("psd_help")).color(config.colors.link);
                if psd_button && ui.clickable_label(str).clicked() {
                    utils::open_docs(false, "psd.html");
                };
            });

            if !yes {
                return;
            }

            // reset the last selected element, so deletion won't accidentally
            // allow it again
            shared_ui.last_selected = "".to_string();

            // extract first arg from context ID
            let mut ctx0: usize = 0;
            if shared_ui.context_menu.id != "" {
                let parsed_context = shared_ui.context_id_parsed();
                ctx0 = parsed_context[1].parse().unwrap();
            }
            shared_ui.context_menu.close();

            match shared_ui.polar_id {
                PolarId::DeleteBone => {
                    events.delete_bone(ctx0);
                }
                PolarId::Exiting => {
                    shared_ui.confirmed_exit = true;
                }
                PolarId::DeleteAnim => {
                    events.delete_anim(ctx0);
                    shared_ui.context_menu.close();
                }
                PolarId::DeleteFile => {
                    std::fs::remove_file(&shared_ui.selected_path).unwrap();
                }
                PolarId::DeleteSelectedTextures => {
                    events.delete_sel_texes();
                }
                PolarId::DeleteStyle => {
                    events.delete_style(ctx0);
                }
                PolarId::NewUpdate => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let base_url = "https://github.com/inviti8/keyternity/releases/latest";
                        _ = open::that(base_url);
                    }
                }
                PolarId::OpenCrashlog => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        _ = open::that(utils::crashlog_file());
                    }
                }
                PolarId::DeleteKeyframeLine => {
                    events.delete_keyframe_line(
                        shared_ui.deleting_line_bone_id as usize,
                        &shared_ui.deleting_line_element,
                    );
                }
                PolarId::ImportedPsd => {
                    events.import_psd_armature();
                }
                PolarId::Retriangulate => {
                    events.retriangulate_verts();
                }
            }
        },
    );
}

pub fn modal(ctx: &egui::Context, shared_ui: &mut crate::Ui, config: &Config) {
    let headline = shared_ui.headline.to_string();
    modal_template(
        ctx,
        "modal".to_string(),
        &config,
        |ui| {
            let psd_button = headline.contains("$psd_page");
            let mut cache = egui_commonmark::CommonMarkCache::default();
            let str = utils::markdown(headline).replace("$psd_page", "");
            egui_commonmark::CommonMarkViewer::new().show(ui, &mut cache, &str);
            ui.add_space(5.);
            let str = egui::RichText::new("PSD Rigging").color(config.colors.link);
            if psd_button && ui.clickable_label(str).clicked() {
                utils::open_docs(false, "psd.html");
            };
        },
        |ui| {
            if shared_ui.forced_modal || !ui.button("OK").clicked() {
                return;
            }
            shared_ui.modal = false;
            shared_ui.headline = "".to_string();
        },
    )
}

pub fn lang_import_modal(
    ctx: &egui::Context,
    shared_ui: &mut crate::Ui,
    config: &Config,
    events: &mut crate::EventState,
) {
    let mut input = shared_ui.lang_input.clone();
    let str_lang_import = shared_ui.loc("startup.resources.lang_import");
    modal_template(
        ctx,
        "lang_import".to_string(),
        config,
        |ui| {
            // language multi-line input field
            let scroll_area = egui::ScrollArea::vertical().max_height(100.);
            scroll_area.show(ui, |ui| {
                egui::TextEdit::multiline(&mut input)
                    .clip_text(true)
                    .show(ui);
            });
            ui.label(str_lang_import);
        },
        |ui| {
            if ui.skf_button("Import").clicked() {
                let input = shared_ui.lang_input.clone();
                let err = shared_ui.init_lang(&[], &input);
                if err != "" {
                    shared_ui.custom_error = err;
                    events.open_modal("error_skf", false);
                }
                shared_ui.lang_import_modal = false;
            }
            if ui.skf_button("Cancel").clicked() {
                shared_ui.lang_input = "".to_string();
                shared_ui.lang_import_modal = false;
            }
            if ui.skf_button("Show i18n Folder").clicked() {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    _ = open::that(utils::bin_path().join("assets").join("i18n"));
                }
                #[cfg(target_arch = "wasm32")]
                {
                    crate::openLink(
                        "https://github.com/inviti8/keyternity/tree/main/assets/i18n"
                            .to_string(),
                    );
                }
            }
        },
    );
    shared_ui.lang_input = input;
}

pub fn feedback_modal(
    ctx: &egui::Context,
    shared_ui: &mut crate::Ui,
    config: &Config,
    events: &mut crate::EventState,
) {
    let mut input = shared_ui.lang_input.clone();
    let mut cancelled = false;
    modal_template(
        ctx,
        "feedback_modal".to_string(),
        config,
        |ui| {
            ui.label(
                "All suggestions and/or bug reports welcome!\nImages may be uploaded as links.",
            );
            ui.add_space(5.);
            egui::TextEdit::multiline(&mut input)
                .hint_text("I think you should add/fix...")
                .show(ui);
            ui.add_space(5.);
            ui.horizontal(|ui| {
                let col = config.colors.link;
                let issues = ui.clickable_label(egui::RichText::new("Keyternity issues on GitHub").color(col));
                if issues.clicked() {
                    utils::open_link(&format!("{}/issues", crate::REPO_URL));
                }
            });
        },
        |ui| {
            if ui.skf_button("Cancel").clicked() {
                shared_ui.feedback_modal = false;
                cancelled = true;
            }
            // opens a new GitHub issue pre-filled with the text; the user submits it there
            ui.add_enabled_ui(shared_ui.lang_input != "", |ui| {
                if !ui.skf_button("Open as GitHub issue").clicked() {
                    return;
                }
                let url = format!(
                    "{}/issues/new?body={}",
                    crate::REPO_URL,
                    utils::url_encode(&shared_ui.lang_input)
                );
                utils::open_link(&url);
                shared_ui.feedback_modal = false;
                events.open_modal(&shared_ui.loc("feedback_sent"), false);
            });
        },
    );
    if !cancelled {
        shared_ui.lang_input = input;
    } else {
        shared_ui.lang_input = "".to_string()
    }
}

// top-right X label for modals
pub fn modal_x<T: FnOnce()>(ui: &mut egui::Ui, offset: egui::Vec2, after_close: T) {
    let x_rect = egui::Rect::from_min_size(ui.min_rect().right_top() + offset, egui::Vec2::ZERO);
    let label = egui::Label::new(egui::RichText::new("X").size(18.));
    let hand = egui::CursorIcon::PointingHand;
    if ui.put(x_rect, label).on_hover_cursor(hand).clicked() {
        after_close();
    }
}
