use egui::IntoAtoms;

use crate::{
    settings_modal::{alt_hor, basic_checkbox, settings_button},
    ui::EguiUi,
    Armature, Config, EditMode, EventState, ExportImgFormat, SettingsState,
};

#[cfg(target_arch = "wasm32")]
mod web {
    pub use web_time::Instant;
}
#[cfg(target_arch = "wasm32")]
pub use web::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    pub use std::{
        fs::OpenOptions,
        io::{Read, Write},
        time::Instant,
    };
    pub use zip::ZipArchive;
}
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

pub use crate::utils;

pub fn draw(
    ctx: &egui::Context,
    shared_ui: &mut crate::Ui,
    edit_mode: &EditMode,
    config: &Config,
    events: &mut EventState,
    armature: &Armature,
) {
    let mut pressed_export = false;

    let frame = egui::Frame {
        corner_radius: 0.into(),
        fill: config.colors.main.into(),
        inner_margin: egui::Margin::same(5),
        stroke: egui::Stroke::new(1., config.colors.light_accent),
        ..Default::default()
    };
    let modal = egui::Modal::new("export_modal".into()).frame(frame);
    modal.show(ctx, |ui| {
        ui.set_width(400.);
        ui.set_height(350.);

        ui.horizontal(|ui| {
            let col: egui::Color32 = config.colors.dark_accent.into();
            let frame = egui::Frame::new()
                .fill(col)
                .inner_margin(egui::Margin::same(5));
            frame.show(ui, |ui| {
                ui.set_width(100.);
                ui.set_height(400.);
                let width = ui.min_rect().width();
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    let mut is_hovered = false;

                    // tab selectors (Armature, Sprite, etc)
                    let str_armature = shared_ui.loc("export_modal.header_armature").clone();
                    // on desktop, the Image tab is the Spritesheet export (docs/FRAME_SEQUENCES.md §6.2)
                    #[cfg(target_arch = "wasm32")]
                    let str_image = shared_ui.loc("export_modal.header_image").clone();
                    #[cfg(not(target_arch = "wasm32"))]
                    let str_image = shared_ui.loc("export_modal.header_spritesheet").clone();
                    let str_video = shared_ui.loc("export_modal.header_video").clone();
                    #[cfg(not(target_arch = "wasm32"))]
                    let str_dragonbones = shared_ui.loc("export_modal.header_dragonbones").clone();
                    #[cfg(not(target_arch = "wasm32"))]
                    let str_sequence = shared_ui.loc("export_modal.header_sequence").clone();

                    let h = &mut is_hovered;
                    type SS = SettingsState;
                    settings_button(str_armature, SS::Ui, ui, shared_ui, &config, width, h);
                    settings_button(str_image, SS::Editing, ui, shared_ui, &config, width, h);
                    #[cfg(not(target_arch = "wasm32"))]
                    settings_button(str_sequence, SS::FrameSequence, ui, shared_ui, &config, width, h);
                    if settings_button(str_video, SS::Keyboard, ui, shared_ui, &config, width, h)
                        .clicked()
                    {
                        #[cfg(target_arch = "wasm32")]
                        {
                            crate::ensureFFmpeg();
                        }
                    };
                    #[cfg(not(target_arch = "wasm32"))]
                    settings_button(
                        str_dragonbones,
                        SS::Rendering,
                        ui,
                        shared_ui,
                        &config,
                        width,
                        h,
                    );

                    if !is_hovered {
                        shared_ui.hovering_setting = None;
                    }
                });
            });

            let height = ui.available_height();

            // show selected tab
            ui.vertical(|ui| {
                let bottom_height = 50.;
                ui.set_max_height(height - bottom_height);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let layout = egui::Layout::top_down(egui::Align::Min);
                        ui.with_layout(layout, |ui| match shared_ui.settings_state {
                            SettingsState::Ui => {
                                armature_export(ui, shared_ui, edit_mode, events, config)
                            }
                            #[cfg(target_arch = "wasm32")]
                            SettingsState::Editing => image_export(ui, shared_ui, config, armature),
                            #[cfg(not(target_arch = "wasm32"))]
                            SettingsState::Editing => {
                                sequence_export(ui, shared_ui, config, armature, true)
                            }
                            SettingsState::Keyboard => {
                                video_export(ui, shared_ui, config, armature)
                            }
                            SettingsState::Rendering => {
                                dragonbones_export(ui, shared_ui, edit_mode, events, config)
                            }
                            SettingsState::FrameSequence => {
                                sequence_export(ui, shared_ui, config, armature, false)
                            }
                            _ => {}
                        });
                        ui.add_space(5.);
                    })
                });
            });

            // hide export button on these conditions
            let image_or_vid = shared_ui.settings_state == SettingsState::Editing
                || shared_ui.settings_state == SettingsState::Keyboard
                || shared_ui.settings_state == SettingsState::FrameSequence;
            if image_or_vid && armature.animations.len() == 0 {
                return;
            }

            // export button
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // cancel button
                        let str = &shared_ui.loc("settings_modal.cancel");
                        if ui.skf_button(str).clicked() {
                            shared_ui.export_modal = false;
                        }

                        // export button
                        let str = &shared_ui.loc("export_modal.save_button");
                        if ui.skf_button(str).clicked() {
                            pressed_export = true;
                        }
                    });
                });

                if shared_ui.warnings.len() == 0 {
                    return;
                }

                // show warning note, if the armature has any
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let warn_str = shared_ui.loc("export_modal.armature.warnings");
                        let str = format!("{} {}", shared_ui.warnings.len(), warn_str);
                        let text = egui::RichText::new(str).color(config.colors.warning_text);
                        if ui.clickable_label(text).clicked() {
                            shared_ui.flash_warn_timer = Some(Instant::now());
                            shared_ui.export_modal = false;
                            shared_ui.warnings_open = true;
                        }
                    });
                });
            });
        });
    });

    // process selected export
    if !pressed_export {
        return;
    }
    let sui = shared_ui;
    match sui.settings_state {
        // Armature Export
        // this triggers the same saving flow, and export options are handled in utils::prepare_files()
        SettingsState::Ui => {
            #[cfg(target_arch = "wasm32")]
            {
                *sui.saving.lock().unwrap() = crate::Saving::Spritesheet;
            }
            #[cfg(not(target_arch = "wasm32"))]
            utils::open_save_dialog(&sui.file_path, &sui.saving, crate::Saving::Exporting);
            sui.export_modal = false;
        }
        // Spritesheet (desktop): the frame sequence export, onto sheets
        #[cfg(not(target_arch = "wasm32"))]
        SettingsState::Editing => {
            sui.sequence_sheets = true;
            if sui.sequence_opts.zip {
                utils::open_save_dialog(&sui.file_path, &sui.saving, crate::Saving::FrameSequence);
            } else {
                utils::open_folder_dialog(&sui.file_path, &sui.saving, crate::Saving::FrameSequence);
            }
            sui.export_modal = false;
        }
        // Image Export (web)
        #[cfg(target_arch = "wasm32")]
        SettingsState::Editing => {
            sui.exporting_video_type = crate::ExportVideoType::None;
            #[cfg(target_arch = "wasm32")]
            {
                *sui.saving.lock().unwrap() = crate::Saving::Spritesheet;
                sui.spritesheet_elapsed = Some(Instant::now());
            }
            #[cfg(not(target_arch = "wasm32"))]
            utils::open_save_dialog(&sui.file_path, &sui.saving, crate::Saving::Spritesheet);
        }
        // Video Export
        SettingsState::Keyboard => {
            #[cfg(target_arch = "wasm32")]
            {
                *sui.saving.lock().unwrap() = crate::Saving::Video;
                sui.spritesheet_elapsed = Some(Instant::now());
            }
            #[cfg(not(target_arch = "wasm32"))]
            utils::open_save_dialog(&sui.file_path, &sui.saving, crate::Saving::Video);
        }
        // Frame Sequence (native only): a folder, or a zip
        SettingsState::FrameSequence => {
            sui.sequence_sheets = false;
            #[cfg(not(target_arch = "wasm32"))]
            if sui.sequence_opts.zip {
                utils::open_save_dialog(&sui.file_path, &sui.saving, crate::Saving::FrameSequence);
            } else {
                utils::open_folder_dialog(&sui.file_path, &sui.saving, crate::Saving::FrameSequence);
            }
            sui.export_modal = false;
        }
        // DragonBones Export (native only)
        SettingsState::Rendering => {
            #[cfg(not(target_arch = "wasm32"))]
            utils::open_save_dialog(&sui.file_path, &sui.saving, crate::Saving::DragonBones);
            sui.export_modal = false;
        }
        _ => {}
    }
}

pub fn armature_export(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    edit_mode: &EditMode,
    events: &mut EventState,
    config: &Config,
) {
    ui.heading(shared_ui.loc("export_modal.armature.header"));

    ui.add_space(10.);

    let ik_str = shared_ui.loc("export_modal.armature.inverse_kinematics");
    ui.heading(ik_str);

    alt_hor(ui, config, true, |ui| {
        ui.label(shared_ui.loc("export_modal.armature.bake_ik"))
            .on_hover_text(shared_ui.loc("export_modal.armature.bake_ik_desc"));
        let mut bake_ik = edit_mode.export_bake_ik;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.scope(|ui| {
                ui.style_mut().visuals.widgets.inactive.bg_fill = config.colors.main.into();
                ui.checkbox(&mut bake_ik, "".into_atoms());
            });
        });
        if bake_ik != edit_mode.export_bake_ik {
            events.toggle_baking_ik(if bake_ik { 1 } else { 0 });
        }
    });

    ui.add_enabled_ui(edit_mode.export_bake_ik, |ui| {
        ui.horizontal(|ui| {
            ui.label(shared_ui.loc("export_modal.armature.exclude_ik"))
                .on_hover_text(shared_ui.loc("export_modal.armature.exclude_ik_desc"));
            let mut exclude_ik = edit_mode.export_exclude_ik;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut exclude_ik, "".into_atoms());
            });
            if exclude_ik != edit_mode.export_exclude_ik {
                events.toggle_exclude_ik(if exclude_ik { 1 } else { 0 });
            }
        });
    });

    ui.add_space(20.);

    let text = shared_ui.loc("export_modal.armature.tex_atlas");
    ui.heading(text);

    alt_hor(ui, config, true, |ui| {
        ui.label(shared_ui.loc("export_modal.armature.img_format"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let dropdown = egui::ComboBox::new("img_format", "")
                .selected_text(&edit_mode.export_img_format.to_string())
                .width(80.);
            dropdown.show_ui(ui, |ui| {
                let mut selected = edit_mode.export_img_format.clone();
                ui.selectable_value(&mut selected, ExportImgFormat::PNG, "PNG");
                ui.selectable_value(&mut selected, ExportImgFormat::JPG, "JPG");
                if selected != edit_mode.export_img_format {
                    events.set_export_img_format(selected as usize);
                }
            });
        });
    });

    ui.add_enabled_ui(edit_mode.export_img_format == ExportImgFormat::JPG, |ui| {
        ui.horizontal(|ui| {
            ui.label(shared_ui.loc("export_modal.armature.clear_color"))
                .on_hover_text(shared_ui.loc("export_modal.armature.clear_color_desc"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let cc = &edit_mode.export_clear_color;
                let mut col: [f32; 3] =
                    [cc.r as f32 / 255., cc.g as f32 / 255., cc.b as f32 / 255.];
                ui.color_edit_button_rgb(&mut col);
                events.set_export_clear_color(col[0], col[1], col[2]);
            });
        });
    });

    alt_hor(ui, config, true, |ui| {
        ui.label("Padding:");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let pad = edit_mode.export_tex_padding;
            let mut either = false;
            let mut result = pad;
            let (edited, value, _) = ui.float_input("padding_x".into(), shared_ui, pad.x, 1., None);
            either |= edited;
            result.x = value;
            let (edited, value, _) = ui.float_input("padding_y".into(), shared_ui, pad.y, 1., None);
            either |= edited;
            result.y = value;
            if either {
                events.set_export_tex_padding(result);
            }
        });
    });
}

pub fn dragonbones_export(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    edit_mode: &EditMode,
    events: &mut EventState,
    config: &Config,
) {
    ui.heading(shared_ui.loc("export_modal.dragonbones.header"));
    ui.add_space(10.);
    ui.label(shared_ui.loc("export_modal.dragonbones.desc"));
    ui.add_space(20.);

    alt_hor(ui, config, true, |ui| {
        ui.label(shared_ui.loc("export_modal.dragonbones.padding"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let pad = edit_mode.export_tex_padding;
            let mut either = false;
            let mut result = pad;
            let (edited, value, _) =
                ui.float_input("db_padding_x".into(), shared_ui, pad.x, 1., None);
            either |= edited;
            result.x = value;
            let (edited, value, _) =
                ui.float_input("db_padding_y".into(), shared_ui, pad.y, 1., None);
            either |= edited;
            result.y = value;
            if either {
                events.set_export_tex_padding(result);
            }
        });
    });
}

/// Ctrl+Shift+E: re-run the last frame sequence or spritesheet export.
pub const EXPORT_AGAIN: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT), egui::Key::E);

/// Re-run the last frame sequence or spritesheet export to the same place, overwriting
/// its files (§8.4). False if there's no last export to repeat.
pub fn export_again(shared_ui: &mut crate::Ui) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(dest) = shared_ui.sequence_opts.last_dest.clone() {
        *shared_ui.file_path.lock().unwrap() = vec![std::path::PathBuf::from(dest)];
        shared_ui.sequence_sheets = shared_ui.sequence_opts.last_sheets;
        shared_ui.sequence_overwrite_ok = true;
        *shared_ui.saving.lock().unwrap() = crate::Saving::FrameSequence;
        return true;
    }
    let _ = shared_ui;
    false
}

/// The Frame Sequence tab, or with `sheets` the Spritesheet tab (docs/FRAME_SEQUENCES.md
/// §5, §6.1, §6.2): the same options, plus how the sheets are laid out.
pub fn sequence_export(ui: &mut egui::Ui, shared_ui: &mut crate::Ui, config: &Config, armature: &Armature, sheets: bool) {
    use crate::export_sequence::{self as es, SizeMode};
    let loc = |shared_ui: &crate::Ui, key: &str| shared_ui.loc(&format!("export_modal.sequence.{key}"));
    ui.heading(loc(shared_ui, if sheets { "header_sheets" } else { "header" }));
    let width = ui.available_width() - 20.;
    if armature.animations.is_empty() {
        ui.label(shared_ui.loc("export_modal.no_anims"));
        return;
    }
    let mut opts = shared_ui.sequence_opts.clone();
    opts.sync(armature);
    ui.add_space(10.);

    // camera
    alt_hor(ui, config, true, |ui| {
        ui.label(loc(shared_ui, "camera")).on_hover_text(loc(shared_ui, "camera_desc"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let auto = loc(shared_ui, "camera_auto");
            let name = |id: Option<i32>| {
                id.and_then(|id| armature.cameras.iter().find(|c| c.id == id)).map_or(auto.clone(), |c| c.name.clone())
            };
            egui::ComboBox::new("sequence_camera", "")
                .selected_text(name(opts.camera))
                .width(130.)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut opts.camera, None, auto.clone());
                    for cam in &armature.cameras {
                        ui.selectable_value(&mut opts.camera, Some(cam.id), cam.name.clone());
                    }
                });
        });
    });

    // output size (§5.1)
    alt_hor(ui, config, false, |ui| {
        ui.label(loc(shared_ui, "size")).on_hover_text(loc(shared_ui, "size_desc"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (e, v, _) = ui.float_input("sequence_size".into(), shared_ui, opts.size_value, 1., None);
            if e && v > 0. {
                opts.size_value = v;
            }
            let mode_name = |m: SizeMode| match m {
                SizeMode::Percent => loc(shared_ui, "size_percent"),
                SizeMode::Width => loc(shared_ui, "size_width"),
                SizeMode::Height => loc(shared_ui, "size_height"),
            };
            let before = opts.size_mode;
            egui::ComboBox::new("sequence_size_mode", "")
                .selected_text(mode_name(opts.size_mode))
                .width(90.)
                .show_ui(ui, |ui| {
                    for m in [SizeMode::Percent, SizeMode::Width, SizeMode::Height] {
                        ui.selectable_value(&mut opts.size_mode, m, mode_name(m));
                    }
                });
            // a new mode starts from the current output size
            if opts.size_mode != before {
                if let Some(cam) = opts.camera.and_then(|id| armature.cameras.iter().find(|c| c.id == id)) {
                    let (factor, (w, h)) = es::output_size(cam, &shared_ui.sequence_opts);
                    opts.size_value = match opts.size_mode {
                        SizeMode::Percent => (factor * 100.).round(),
                        SizeMode::Width => w as f32,
                        SizeMode::Height => h as f32,
                    };
                } else {
                    opts.size_value = if opts.size_mode == SizeMode::Percent { 100. } else { 512. };
                }
            }
        });
    });
    let mut muted = config.colors.text;
    muted -= crate::Color::new(60, 60, 60, 0);
    let size_note = match opts.camera.and_then(|id| armature.cameras.iter().find(|c| c.id == id)) {
        Some(cam) => {
            let (_, (w, h)) = es::output_size(cam, &opts);
            let (cw, ch) = cam.px_size();
            loc(shared_ui, "size_note")
                .replace("$w", &w.to_string())
                .replace("$h", &h.to_string())
                .replace("$cw", &cw.to_string())
                .replace("$ch", &ch.to_string())
        }
        None => loc(shared_ui, "size_note_auto"),
    };
    ui.label(egui::RichText::new(size_note).color(muted));
    ui.add_space(8.);

    // frames
    alt_hor(ui, config, true, |ui| {
        ui.label(loc(shared_ui, "step")).on_hover_text(loc(shared_ui, "step_desc"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (e, v, _) = ui.float_input("sequence_step".into(), shared_ui, opts.step as f32, 1., None);
            if e {
                opts.step = (v.round() as i64).clamp(1, 1000) as u32;
            }
        });
    });
    basic_checkbox(ui, &loc(shared_ui, "skip_last"), &loc(shared_ui, "skip_last_desc"), &mut opts.skip_last, config, false);
    ui.add_space(8.);

    // sheet layout
    if sheets {
        basic_checkbox(ui, &loc(shared_ui, "trim"), &loc(shared_ui, "trim_desc"), &mut opts.trim, config, false);
        ui.add_enabled_ui(!opts.trim, |ui| alt_hor(ui, config, true, |ui| {
            ui.label(loc(shared_ui, "columns")).on_hover_text(loc(shared_ui, "columns_desc"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let auto = loc(shared_ui, "columns_auto");
                if opts.sheet_columns > 0 {
                    let (e, v, _) = ui.float_input("sheet_columns".into(), shared_ui, opts.sheet_columns as f32, 1., None);
                    if e {
                        opts.sheet_columns = (v.round() as i64).clamp(1, 1000) as u32;
                    }
                }
                let mut fixed = opts.sheet_columns > 0;
                egui::ComboBox::new("sheet_columns_mode", "")
                    .selected_text(if fixed { loc(shared_ui, "columns_fixed") } else { auto.clone() })
                    .width(80.)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut fixed, false, auto.clone());
                        ui.selectable_value(&mut fixed, true, loc(shared_ui, "columns_fixed"));
                    });
                if fixed && opts.sheet_columns == 0 {
                    opts.sheet_columns = 4;
                } else if !fixed {
                    opts.sheet_columns = 0;
                }
            });
        }));
        alt_hor(ui, config, false, |ui| {
            ui.label(loc(shared_ui, "sheet_max")).on_hover_text(loc(shared_ui, "sheet_max_desc"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::new("sheet_max", "")
                    .selected_text(opts.sheet_max.to_string())
                    .width(70.)
                    .show_ui(ui, |ui| {
                        for m in [1024, 2048, 4096, 8192, 16384] {
                            ui.selectable_value(&mut opts.sheet_max, m, m.to_string());
                        }
                    });
            });
        });
        alt_hor(ui, config, true, |ui| {
            ui.label(loc(shared_ui, "sheet_padding")).on_hover_text(loc(shared_ui, "sheet_padding_desc"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (e, v, _) = ui.float_input("sheet_padding".into(), shared_ui, opts.sheet_padding as f32, 1., None);
                if e {
                    opts.sheet_padding = (v.round() as i64).clamp(0, 64) as u32;
                }
            });
        });
        basic_checkbox(ui, &loc(shared_ui, "sheet_per_anim"), &loc(shared_ui, "sheet_per_anim_desc"), &mut opts.sheet_per_anim, config, false);
        ui.add_space(8.);
    }

    // files
    alt_hor(ui, config, true, |ui| {
        ui.label(loc(shared_ui, "output"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (folder, zip) = (loc(shared_ui, "output_folder"), loc(shared_ui, "output_zip"));
            egui::ComboBox::new("sequence_output", "")
                .selected_text(if opts.zip { zip.clone() } else { folder.clone() })
                .width(90.)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut opts.zip, false, folder);
                    ui.selectable_value(&mut opts.zip, true, zip);
                });
        });
    });
    if !sheets {
        basic_checkbox(ui, &loc(shared_ui, "anim_folders"), &loc(shared_ui, "anim_folders_desc"), &mut opts.anim_folders, config, false);
    }
    alt_hor(ui, config, true, |ui| {
        let (name, desc) = if sheets { ("frame_names", "frame_names_desc") } else { ("pattern", "pattern_desc") };
        ui.label(loc(shared_ui, name)).on_hover_text(loc(shared_ui, desc));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let options = Some(crate::ui::TextInputOptions {
                size: crate::Vec2::new(130., 20.),
                default: es::SequenceOptions::default().pattern,
                ..Default::default()
            });
            let (e, v, _) = ui.text_input("sequence_pattern".into(), shared_ui, opts.pattern.clone(), options);
            if e && !v.trim().is_empty() {
                opts.pattern = v.trim().to_string();
            }
        });
    });
    alt_hor(ui, config, false, |ui| {
        ui.label(loc(shared_ui, "digits"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::new("sequence_start", "")
                .selected_text(opts.start.to_string())
                .width(40.)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut opts.start, 0, "0");
                    ui.selectable_value(&mut opts.start, 1, "1");
                });
            ui.label(loc(shared_ui, "start"));
            egui::ComboBox::new("sequence_digits", "")
                .selected_text(opts.digits.to_string())
                .width(40.)
                .show_ui(ui, |ui| {
                    for d in 1..=6 {
                        ui.selectable_value(&mut opts.digits, d, d.to_string());
                    }
                });
        });
    });
    alt_hor(ui, config, true, |ui| {
        ui.label(loc(shared_ui, "format")).on_hover_text(loc(shared_ui, "format_desc"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            use crate::export_sequence::ImageFormat;
            if opts.format == ImageFormat::Jpg {
                let (e, v, _) = ui.float_input("sequence_jpg_quality".into(), shared_ui, opts.jpg_quality as f32, 1., None);
                if e {
                    opts.jpg_quality = (v.round() as i64).clamp(1, 100) as u8;
                }
                ui.label(loc(shared_ui, "quality"));
            }
            let name = |f: ImageFormat| match f {
                ImageFormat::Png => "PNG",
                ImageFormat::Webp => "WebP",
                ImageFormat::Jpg => "JPG",
            };
            egui::ComboBox::new("sequence_format", "")
                .selected_text(name(opts.format))
                .width(70.)
                .show_ui(ui, |ui| {
                    for f in [ImageFormat::Png, ImageFormat::Webp, ImageFormat::Jpg] {
                        ui.selectable_value(&mut opts.format, f, name(f));
                    }
                });
        });
    });
    basic_checkbox(ui, &loc(shared_ui, "phaser"), &loc(shared_ui, "phaser_desc"), &mut opts.phaser, config, false);
    basic_checkbox(ui, &loc(shared_ui, "godot"), &loc(shared_ui, "godot_desc"), &mut opts.godot, config, true);
    if opts.godot {
        alt_hor(ui, config, true, |ui| {
            ui.label(loc(shared_ui, "godot_dir")).on_hover_text(loc(shared_ui, "godot_dir_desc"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let options = Some(crate::ui::TextInputOptions {
                    size: crate::Vec2::new(160., 20.),
                    default: "res://".to_string(),
                    ..Default::default()
                });
                let (e, v, _) = ui.text_input("sequence_godot_dir".into(), shared_ui, opts.godot_dir.clone(), options);
                if e {
                    opts.godot_dir = es::godot_prefix(&v);
                }
            });
        });
    }

    // what it'll write, or why it can't
    ui.add_space(4.);
    let planned = es::effective(&opts, sheets);
    let camera = opts.camera.and_then(|id| armature.cameras.iter().find(|c| c.id == id));
    let note = es::plan(armature, &planned).and_then(|frames| {
        if !sheets {
            return Ok(loc(shared_ui, "plan_note").replace("$n", &frames.len().to_string()).replace("$first", &frames[0].path));
        }
        if planned.trim {
            return Ok(loc(shared_ui, "plan_note_trimmed").replace("$n", &frames.len().to_string()));
        }
        let Some(cam) = camera else {
            return Ok(loc(shared_ui, "plan_note_sheets_auto").replace("$n", &frames.len().to_string()));
        };
        let (_, out) = es::output_size(cam, &planned);
        let plan = es::pack_sheets("rig", armature, &frames, out, &planned)?;
        let (w, h) = plan.sizes[0];
        Ok(loc(shared_ui, "plan_note_sheets")
            .replace("$n", &frames.len().to_string())
            .replace("$s", &plan.files.len().to_string())
            .replace("$w", &w.to_string())
            .replace("$h", &h.to_string()))
    });
    match note {
        Ok(note) => ui.label(egui::RichText::new(note).color(muted)),
        Err(e) => ui.label(egui::RichText::new(e).color(config.colors.warning_text)),
    };

    // animations, with their ranges
    ui.add_space(12.);
    ui.heading(shared_ui.loc("export_modal.image.animations"));
    ui.add_space(5.);
    for a in 0..armature.animations.len() {
        let anim = &armature.animations[a];
        let last = es::last_frame(armature, a);
        let col = if a % 2 == 0 { config.colors.dark_accent } else { config.colors.main };
        let Some(choice) = opts.anims.iter_mut().find(|c| c.id == anim.id) else { continue };
        egui::Frame::new().fill(col.into()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_width(width + 20.);
                ui.checkbox(&mut choice.export, "");
                ui.label(anim.name.to_string());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(choice.export, |ui| {
                        let end = if choice.end < 0 { last } else { choice.end.min(last) };
                        let opt = Some(crate::ui::TextInputOptions {
                            size: crate::Vec2::new(30., 20.),
                            ..Default::default()
                        });
                        let (e, v, _) = ui.float_input(format!("sequence_end_{}", anim.id), shared_ui, end as f32, 1., opt.clone());
                        if e {
                            let v = (v.round() as i32).clamp(0, last);
                            choice.end = if v >= last { -1 } else { v };
                            choice.start = choice.start.min(v);
                        }
                        ui.label("–");
                        let (e, v, _) = ui.float_input(format!("sequence_start_{}", anim.id), shared_ui, choice.start as f32, 1., opt);
                        if e {
                            choice.start = (v.round() as i32).clamp(0, end);
                        }
                        ui.label(egui::RichText::new(format!("{} FPS", anim.fps)).color(muted));
                    });
                });
            });
        });
    }

    shared_ui.sequence_opts = opts;
}

pub fn image_export(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    config: &Config,
    armature: &Armature,
) {
    ui.heading(shared_ui.loc("export_modal.image.header"));
    let width = ui.available_width() - 20.;

    if armature.animations.len() == 0 {
        ui.label(shared_ui.loc("export_modal.no_anims"));
        return;
    }

    ui.add_space(10.);

    // export type (spritesheet, sequence, etc)
    alt_hor(ui, config, true, |ui| {
        ui.label(shared_ui.loc("export_modal.image.export_type"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let str_sequences = shared_ui.loc("export_modal.image.sequences");
            let str_spritesheets = shared_ui.loc("export_modal.image.spritesheets");
            let selected_str = if shared_ui.image_sequences {
                &str_sequences
            } else {
                &str_spritesheets
            };
            let combo_box = egui::ComboBox::new("transition_dropdown".to_string(), "")
                .selected_text(selected_str.to_string());
            combo_box.show_ui(ui, |ui| {
                ui.selectable_value(&mut shared_ui.image_sequences, false, str_spritesheets);
                ui.selectable_value(&mut shared_ui.image_sequences, true, str_sequences);
            });
        });
    });

    // sprites per row if spritesheet is selected
    ui.add_enabled_ui(!shared_ui.image_sequences, |ui: &mut egui::Ui| {
        ui.horizontal(|ui| {
            ui.label(shared_ui.loc("export_modal.image.sprites_per_row"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let spr = shared_ui.sprites_per_row as f32;
                let (edited, value, _) =
                    ui.float_input("sprite_row".into(), shared_ui, spr, 1., None);
                if edited {
                    shared_ui.sprites_per_row = value as i32;
                }
            });
        });
    });

    // size per sprite (width & height)
    alt_hor(ui, config, true, |ui| {
        ui.label(shared_ui.loc("export_modal.image.size_per_sprite"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let x = shared_ui.sprite_size.x;
            let (edited, value, _) = ui.float_input("sprite_size_x".into(), shared_ui, x, 1., None);
            if edited {
                shared_ui.sprite_size.x = value;
            }
            ui.label("x");
            let y = shared_ui.sprite_size.y;
            let (edited, value, _) = ui.float_input("sprite_size_y".into(), shared_ui, y, 1., None);
            if edited {
                shared_ui.sprite_size.y = value;
            }
        });
    });

    basic_checkbox(
        ui,
        &shared_ui.loc("export_modal.video.global_bounds"),
        &shared_ui.loc("export_modal.video.global_bounds_desc"),
        &mut shared_ui.export_global_bounds,
        config,
        false,
    );

    ui.add_space(20.);
    animations_list(ui, shared_ui, armature, width, config);
}

pub fn video_export(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    config: &Config,
    armature: &Armature,
) {
    ui.heading(shared_ui.loc("export_modal.video.header"));
    let _width = ui.available_width() - 20.;

    ui.add_space(10.);

    // don't show video export if armature has no animations
    if armature.animations.len() == 0 {
        ui.label(shared_ui.loc("export_modal.no_anims"));
        return;
    }

    // video format (mp4, gif, etc)
    alt_hor(ui, config, true, |ui| {
        ui.label(shared_ui.loc("export_modal.video.format"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let dropdown = egui::ComboBox::new("export_video", "")
                .selected_text(&shared_ui.exporting_video_type.to_string().to_uppercase())
                .width(80.);
            dropdown.show_ui(ui, |ui| {
                let export = &mut shared_ui.exporting_video_type;
                ui.selectable_value(export, crate::ExportVideoType::Mp4, "MP4");
                ui.selectable_value(export, crate::ExportVideoType::Gif, "GIF");
            });
        });
    });

    // resolution (width & height)
    ui.horizontal(|ui| {
        ui.label(shared_ui.loc("export_modal.video.resolution"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let x = shared_ui.sprite_size.x;
            let (edited, value, _) = ui.float_input("sprite_size_x".into(), shared_ui, x, 1., None);
            if edited {
                shared_ui.sprite_size.x = value;
            }
            ui.label("x");
            let y = shared_ui.sprite_size.y;
            let (edited, value, _) = ui.float_input("sprite_size_y".into(), shared_ui, y, 1., None);
            if edited {
                shared_ui.sprite_size.y = value;
            }
        });
    });

    // background (clear) color
    let is_mp4 = shared_ui.exporting_video_type == crate::ExportVideoType::Mp4;
    ui.add_enabled_ui(is_mp4, |ui| {
        alt_hor(ui, config, true, |ui| {
            ui.label(shared_ui.loc("export_modal.video.background_color"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let real = &mut shared_ui.video_clear_bg;
                let mut col: [u8; 3] = [real.r, real.g, real.b];
                ui.color_edit_button_srgb(&mut col);
                *real = crate::shared::Color::new(col[0], col[1], col[2], 255);
            });
        });
    });

    // loop cycles
    if !is_mp4 {
        shared_ui.anim_cycles = 1;
    }
    ui.add_enabled_ui(is_mp4, |ui| {
        ui.horizontal(|ui| {
            ui.label(shared_ui.loc("export_modal.video.cycles"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let cycles = "anim_cycles".to_string();
                let (edited, value, _) =
                    ui.float_input(cycles, shared_ui, shared_ui.anim_cycles as f32, 1., None);
                if edited {
                    shared_ui.anim_cycles = value as i32;
                }
            });
        });
    });

    basic_checkbox(
        ui,
        &shared_ui.loc("export_modal.video.global_bounds"),
        &shared_ui.loc("export_modal.video.global_bounds_desc"),
        &mut shared_ui.export_global_bounds,
        config,
        true,
    );

    #[cfg(not(target_arch = "wasm32"))]
    {
        // disabled: encoder dropdown - default is always used for now
        if false {
            ui.horizontal(|ui| {
                ui.label(shared_ui.loc("export_modal.video.encoder"));
                ui.add_enabled_ui(is_mp4, |ui| {
                    let encoder_str = &shared_ui.exporting_video_encoder.to_string().to_lowercase();
                    let dropdown = egui::ComboBox::new("export_encoder", "")
                        .selected_text(encoder_str)
                        .width(80.);
                    dropdown.show_ui(ui, |ui| {
                        let export = &mut shared_ui.exporting_video_encoder;
                        ui.selectable_value(export, crate::ExportVideoEncoder::Libx264, "libx264");
                        ui.selectable_value(export, crate::ExportVideoEncoder::AV1, "av1");
                    });
                });
            });
        }

        // Windows: a verified official ffmpeg next to the app, or the system one if preferred
        #[cfg(target_os = "windows")]
        {
            basic_checkbox(
                ui,
                &shared_ui.loc("export_modal.video.use_system_ffmpeg"),
                &shared_ui.loc("export_modal.video.use_system_ffmpeg_desc"),
                &mut shared_ui.use_system_ffmpeg,
                config,
                false,
            );
            download_ffmpeg_button(ui);
        }
        // Linux/macOS: always the system ffmpeg (package manager / Homebrew)
        #[cfg(all(not(target_os = "windows"), not(target_arch = "wasm32")))]
        {
            ui.add_space(10.);
            ui.label(shared_ui.loc("export_modal.video.system_ffmpeg_note"));
        }
    }

    ui.add_space(20.);
    animations_list(ui, shared_ui, armature, _width, config);
}

/// Official FFmpeg build for Windows: gyan.dev's "essentials" build, from its GitHub release
/// archive. Pinned, and checked by SHA-256 before anything is written. To update, change these
/// together (see docs/FFMPEG.md); the installer pins the same build.
#[cfg(target_os = "windows")]
const FFMPEG_ZIP_URL: &str = "https://github.com/GyanD/codexffmpeg/releases/download/2026-02-09-git-9bfa1635ae/ffmpeg-2026-02-09-git-9bfa1635ae-essentials_build.zip";
#[cfg(target_os = "windows")]
const FFMPEG_ZIP_SHA256: &str = "170c57f56e116416ff09494f77bcee6cb4d6be34cc9c1170cbaa923b296616bf";
#[cfg(target_os = "windows")]
const FFMPEG_ZIP_MB: u32 = 108;

#[cfg(target_os = "windows")]
#[derive(Clone, PartialEq)]
enum FfmpegDownload {
    Idle,
    Running,
    Done,
    Failed(String),
}

#[cfg(target_os = "windows")]
static FFMPEG_DOWNLOAD: std::sync::Mutex<FfmpegDownload> =
    std::sync::Mutex::new(FfmpegDownload::Idle);

/// Download the pinned FFmpeg build, verify it, and put `ffmpeg.exe` next to the app.
#[cfg(target_os = "windows")]
pub fn fetch_ffmpeg() -> Result<(), String> {
    use sha2::{Digest, Sha256};

    let resp = ureq::get(FFMPEG_ZIP_URL)
        .call()
        .map_err(|e| format!("couldn't download: {e}"))?;
    let mut zip_bytes = vec![];
    resp.into_body()
        .into_reader()
        .read_to_end(&mut zip_bytes)
        .map_err(|e| format!("download interrupted: {e}"))?;

    let hash: String = Sha256::digest(&zip_bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if hash != FFMPEG_ZIP_SHA256 {
        return Err("the download doesn't match the official build (SHA-256 mismatch); nothing was installed".into());
    }

    let mut zip = ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| format!("couldn't open the archive: {e}"))?;
    let name = zip
        .file_names()
        .find(|n| n.ends_with("/bin/ffmpeg.exe"))
        .map(|n| n.to_string())
        .ok_or("ffmpeg.exe not found in the archive")?;
    let mut exe = vec![];
    let mut entry = zip
        .by_name(&name)
        .map_err(|e| format!("couldn't extract ffmpeg.exe: {e}"))?;
    entry
        .read_to_end(&mut exe)
        .map_err(|e| format!("couldn't extract ffmpeg.exe: {e}"))?;

    // write beside the app, then swap in, so a failed write never leaves a broken ffmpeg.exe
    let dir = utils::bin_path();
    let part = dir.join("ffmpeg.exe.part");
    let written = std::fs::write(&part, &exe)
        .and_then(|_| std::fs::rename(&part, dir.join("ffmpeg.exe")));
    written.map_err(|e| {
        _ = std::fs::remove_file(&part);
        format!(
            "couldn't write to {} ({e}). If the app is installed, re-run the installer with the FFmpeg option, or use the system ffmpeg",
            dir.display()
        )
    })
}

/// Windows only: elsewhere video export uses the system's ffmpeg.
#[cfg(target_os = "windows")]
pub fn download_ffmpeg_button(ui: &mut egui::Ui) {
    let state = FFMPEG_DOWNLOAD.lock().unwrap().clone();
    let running = state == FfmpegDownload::Running;

    ui.add_space(10.);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button = ui.add_enabled_ui(!running, |ui| ui.skf_button("Download ffmpeg"));
            if button.inner.clicked() {
                *FFMPEG_DOWNLOAD.lock().unwrap() = FfmpegDownload::Running;
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let result = match fetch_ffmpeg() {
                        Ok(()) => FfmpegDownload::Done,
                        Err(err) => FfmpegDownload::Failed(err),
                    };
                    *FFMPEG_DOWNLOAD.lock().unwrap() = result;
                    ctx.request_repaint();
                });
            }
        });
    });

    ui.add_space(2.5);

    let installed = std::fs::exists(utils::bin_path().join("ffmpeg.exe")).unwrap_or(false);
    let str = match state {
        FfmpegDownload::Running => {
            format!("Downloading the official FFmpeg build (about {FFMPEG_ZIP_MB} MB)...")
        }
        FfmpegDownload::Failed(err) => format!("FFmpeg download failed: {err}."),
        FfmpegDownload::Done => "FFmpeg is installed and verified.".to_string(),
        FfmpegDownload::Idle if installed => "Re-download ffmpeg if problems occur.".to_string(),
        FfmpegDownload::Idle => format!(
            "ffmpeg is not installed.\nClick above to download the official build (about {FFMPEG_ZIP_MB} MB)."
        ),
    };
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(str);
        })
    });
}

fn animations_list(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    armature: &Armature,
    width: f32,
    config: &Config,
) {
    // selecting which animations to export
    let text = shared_ui.loc("export_modal.image.animations");
    ui.heading(text);

    ui.add_space(5.);
    for a in 0..armature.animations.len() {
        #[rustfmt::skip]
        let col = if a % 2 == 0 { config.colors.dark_accent } else { config.colors.main };

        let anim = &armature.animations[a];
        egui::Frame::new().fill(col.into()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_width(width + 20.);
                ui.label(anim.name.to_string());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.scope(|ui| {
                        // different colored checkbox bg against the stripe
                        ui.style_mut().visuals.widgets.inactive.bg_fill = if a % 2 == 0 {
                            config.colors.main
                        } else {
                            config.colors.dark_accent
                        }
                        .into();

                        #[cfg(not(target_arch = "wasm32"))]
                        ui.checkbox(&mut shared_ui.exporting_anims[a], "".into_atoms())
                            .on_hover_text(
                                shared_ui.loc("export_modal.image.animations_check_desc"),
                            );
                        #[cfg(target_arch = "wasm32")]
                        {
                            let mut chosen = shared_ui.exporting_anims[a];
                            ui.checkbox(&mut chosen, "".into_atoms()).on_hover_text(
                                shared_ui.loc("export_modal.image.animations_check_desc"),
                            );
                            if chosen != shared_ui.exporting_anims[a] {
                                for anim in &mut shared_ui.exporting_anims {
                                    *anim = false;
                                }
                                shared_ui.exporting_anims[a] = chosen;
                            }
                        }
                    });

                    ui.add_space(10.);

                    // show frame info, if this animation has any
                    // (frameless anims are allowed for export)
                    if anim.keyframes.is_empty() {
                        return;
                    }
                    let str = anim.fps.to_string()
                        + &" FPS  -  ".to_string()
                        + &anim.last_frame().to_string()
                        + &shared_ui.loc("export_modal.image.frames");
                    let mut meta_col = config.colors.text;
                    meta_col -= crate::Color::new(40, 40, 40, 0);
                    ui.label(egui::RichText::new(str).color(meta_col));
                });
            });
        });
    }
}
