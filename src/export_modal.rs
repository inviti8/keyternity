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
                    let str_image = shared_ui.loc("export_modal.header_image").clone();
                    let str_video = shared_ui.loc("export_modal.header_video").clone();
                    #[cfg(not(target_arch = "wasm32"))]
                    let str_dragonbones = shared_ui.loc("export_modal.header_dragonbones").clone();

                    let h = &mut is_hovered;
                    type SS = SettingsState;
                    settings_button(str_armature, SS::Ui, ui, shared_ui, &config, width, h);
                    settings_button(str_image, SS::Editing, ui, shared_ui, &config, width, h);
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
                            SettingsState::Editing => image_export(ui, shared_ui, config, armature),
                            SettingsState::Keyboard => {
                                video_export(ui, shared_ui, config, armature)
                            }
                            SettingsState::Rendering => {
                                dragonbones_export(ui, shared_ui, edit_mode, events, config)
                            }
                            _ => {}
                        });
                        ui.add_space(5.);
                    })
                });
            });

            // hide export button on these conditions
            let image_or_vid = shared_ui.settings_state == SettingsState::Editing
                || shared_ui.settings_state == SettingsState::Keyboard;
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
        // Image Export
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
                    let total_frames = anim.keyframes.last();
                    if total_frames == None {
                        return;
                    }
                    let str = anim.fps.to_string()
                        + &" FPS  -  ".to_string()
                        + &total_frames.unwrap().frame.to_string()
                        + &shared_ui.loc("export_modal.image.frames");
                    let mut meta_col = config.colors.text;
                    meta_col -= crate::Color::new(40, 40, 40, 0);
                    ui.label(egui::RichText::new(str).color(meta_col));
                });
            });
        });
    }
}
