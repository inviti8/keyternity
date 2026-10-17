//! Animation keyframe editor. Very early and only proof-of-concept.

use egui::{HoveredFile, Stroke};

use ui::{EguiUi, TextInputOptions};

use crate::*;

use crate::timeline_view::{self, BarHandle, TimelineView};

/// the ruler's height (labels and summary keys)
const RULER_H: f32 = 30.;
/// the range bar's height (docs/TIMELINE.md §2.2)
const BAR_H: f32 = 14.;
/// the bottom bar's height
const BOTTOM_H: f32 = 20.;

pub fn draw(
    egui_ctx: &egui::Context,
    shared_ui: &mut crate::Ui,
    input: &InputStates,
    armature: &mut Armature,
    config: &Config,
    selections: &mut SelectionState,
    events: &mut EventState,
    edit_mode: &EditMode,
    camera: &Camera,
) {
    let sel = selections.clone();
    let mut sel_frame = selections.anim_frame;

    // navigating frames with kb input
    if shared_ui.rename_id == "" && selections.anim != usize::MAX {
        let kfs = &armature.sel_anim(&sel).unwrap().keyframes;
        // modified Is first: shortcuts match regardless of extra modifiers
        if egui_ctx.input_mut(|i| i.consume_shortcut(&config.keys.delete_keyframe)) {
            events.delete_bone_keyframes();
        } else if egui_ctx.input_mut(|i| i.consume_shortcut(&config.keys.insert_keyframe_all)) {
            events.insert_keyframe_all();
        } else if egui_ctx.input_mut(|i| i.consume_shortcut(&config.keys.insert_keyframe))
            && !selections.bone_ids.is_empty()
        {
            events.insert_keyframe();
        }
        let next_kf = egui_ctx.input_mut(|i| i.consume_shortcut(&config.keys.next_keyframe));
        let prev_kf = egui_ctx.input_mut(|i| i.consume_shortcut(&config.keys.prev_keyframe));
        let right = egui_ctx.input_mut(|i| i.consume_shortcut(&config.keys.next_anim_frame));
        let left = egui_ctx.input_mut(|i| i.consume_shortcut(&config.keys.prev_anim_frame));

        if next_kf {
            for kf in kfs {
                if kf.frame > sel_frame {
                    sel_frame = kf.frame;
                    break;
                }
            }
        } else if prev_kf {
            let mut prev = 0;
            for kf in kfs {
                if kf.frame < sel_frame {
                    prev = kf.frame;
                } else {
                    break;
                }
            }
            sel_frame = prev;
        } else if right || left {
            // step, wrapping at the animation's End
            let anim = armature.sel_anim(&sel).unwrap();
            let has_end = anim.end_frame > 0 || !kfs.is_empty();
            let last = anim.last_frame();
            sel_frame += if right { 1 } else { -1 };
            if has_end && sel_frame > last {
                sel_frame = 0;
            } else if has_end && sel_frame < 0 {
                sel_frame = last;
            }
        }
    }
    if sel_frame != selections.anim_frame {
        events.select_anim_frame(sel_frame as usize, false, false);
    }

    let panel_id = "Keyframe";
    let panel = egui::TopBottomPanel::bottom(panel_id)
        .min_height(150.)
        .resizable(true);
    ui::draw_resizable_panel(
        panel_id,
        panel.show(egui_ctx, |ui| {
            ui.gradient(
                ui.ctx().content_rect(),
                egui::Color32::TRANSPARENT,
                config.colors.gradient.into(),
            );

            let full_height = ui.available_height();
            ui.horizontal(|ui| {
                ui.set_height(full_height);

                // animations list
                let resize = egui::Resize::default()
                    .min_height(full_height) // make height unadjustable
                    .max_height(full_height) //
                    .default_width(175.)
                    .max_width(300.)
                    .with_stroke(false);
                resize.show(ui, |ui| {
                    egui::Frame::new().show(ui, |ui| {
                        ui.vertical(|ui| {
                            draw_animations_list(
                                ui, shared_ui, armature, config, selections, events,
                            );
                        })
                    });
                });

                if selections.anim != usize::MAX {
                    timeline_editor(
                        ui, selections, armature, events, shared_ui, config, input, edit_mode,
                    );
                }
            });
            shared_ui.keyframe_panel_rect = Some(ui.min_rect());
        }),
        events,
        &egui_ctx,
        &camera,
    );
}

fn draw_animations_list(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    armature: &Armature,
    config: &Config,
    selections: &crate::SelectionState,
    events: &mut EventState,
) {
    ui.horizontal(|ui| {
        let str_anim = shared_ui.loc("keyframe_editor.heading");
        ui.heading(str_anim);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
            ui.add_space(13.);
            let str_new = shared_ui.loc("new");
            let button = ui.skf_button(str_new);
            if !button.clicked() {
                return;
            }
            events.new_animation();
            shared_ui.just_made_anim = true;
        });
    });
    ui.add_space(5.);
    egui::ScrollArea::vertical().show(ui, |ui| {
        let frame = egui::Frame::new()
            .fill(config.colors.dark_accent.into())
            .outer_margin(egui::Margin {
                right: 13,
                ..Default::default()
            });
        ui.dnd_drop_zone::<i32, _>(frame, |ui| {
            let width = ui.available_width();
            let mut hovered = false;
            let mut idx = -1;
            for i in 0..armature.animations.len() {
                let name = &mut armature.animations[i].name.clone();
                let context_id = format!("anim_{}", armature.animations[i].id);
                idx += 1;

                // show input field if renaming
                if shared_ui.rename_id == context_id {
                    let str_new_anim = &shared_ui.loc("keyframe_editor.new_animation");
                    let options = Some(TextInputOptions {
                        focus: true,
                        placeholder: str_new_anim.to_string(),
                        default: str_new_anim.to_string(),
                        ..Default::default()
                    });
                    let (edited, value, _) =
                        ui.text_input(context_id, shared_ui, name.to_string(), options);
                    if edited {
                        events.rename_animation(i, value);
                        events.select_anim(i);
                    }
                    continue;
                }

                let id = egui::Id::new(format!("anim_{}", idx.to_string()));
                let dnd_button = ui
                    .dnd_drag_source(id, idx, |ui| {
                        ui.horizontal(|ui| {
                            let has_kf = armature.animations[i].keyframes.len() > 0;
                            let button_padding = if has_kf { 25. } else { 0. };
                            let mut col = config.colors.dark_accent;
                            if i == shared_ui.hovering_anim as usize {
                                col += crate::Color::new(20, 20, 20, 0);
                            }
                            if i == selections.anim {
                                col += crate::Color::new(20, 20, 20, 0);
                            }
                            let cursor_icon = if selections.anim != i {
                                egui::CursorIcon::PointingHand
                            } else {
                                egui::CursorIcon::Default
                            };

                            // set up a focusable frame for the animation button
                            let rect = egui::Rect::from_min_size(
                                egui::Pos2::new(ui.cursor().left(), ui.cursor().top()),
                                egui::Vec2::new(width, 21.),
                            );
                            let button = ui
                                .interact(rect, id, egui::Sense::click())
                                .on_hover_cursor(cursor_icon);
                            ui.scope(|ui| {
                                ui.style_mut().interaction.selectable_labels = false;
                                egui::Frame::new().fill(col.into()).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.set_width(width - button_padding);
                                        ui.set_height(21.);
                                        ui.add_space(5.);
                                        let col = config.colors.text;
                                        ui.label(egui::RichText::new(name.clone()).color(col));
                                    });
                                });
                            });

                            if button.contains_pointer() || button.has_focus() {
                                shared_ui.hovering_anim = i as i32;
                                hovered = true;
                            }
                            if button.clicked() {
                                if selections.anim != i {
                                    events.select_anim(i);
                                } else {
                                    shared_ui.rename_id = context_id.clone();
                                    shared_ui.edit_value = Some(name.to_string());
                                }
                                shared_ui.last_selected = "anim".to_string();
                            }
                            if button.secondary_clicked() {
                                shared_ui.context_menu.show(&context_id);
                            }

                            if armature.animations[i].keyframes.len() > 0 {
                                let anim = &armature.animations[i];
                                let align = egui::Layout::right_to_left(egui::Align::Center);
                                ui.with_layout(align, |ui| {
                                    let icon = if anim.elapsed == None { "⏵" } else { "⏹" };
                                    if ui.skf_button(icon).clicked() {
                                        events.toggle_anim_playing(i, anim.elapsed == None);
                                    }
                                });
                            }
                        });
                    })
                    .response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .interact(egui::Sense::click());

                let pointer = ui.input(|i| i.pointer.interact_pos());
                let hovered_payload = dnd_button.dnd_hover_payload::<i32>();
                let dragged_payload = dnd_button.dnd_release_payload::<i32>();

                if pointer == None || hovered_payload == None {
                    continue;
                }

                let rect = dnd_button.rect;
                let stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
                ui.painter().hline(rect.x_range(), rect.top(), stroke);

                if dragged_payload == None {
                    continue;
                }

                events.move_animation(*hovered_payload.clone().unwrap(), idx);
            }

            if !hovered {
                shared_ui.hovering_anim = -1;
            }
        });
    });
}

fn timeline_editor(
    ui: &mut egui::Ui,
    selections: &mut SelectionState,
    armature: &mut Armature,
    events: &mut EventState,
    shared_ui: &mut crate::Ui,
    config: &Config,
    input: &InputStates,
    edit_mode: &EditMode,
) {
    let frame = egui::Frame::new().outer_margin(egui::Margin {
        left: 0,
        ..Default::default()
    });
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_height(ui.available_height());

        // track the Y of bone change labels for their diamonds
        shared_ui.bone_tops = BoneTops::default();

        let sel = selections.clone();

        if armature.sel_anim(&sel).unwrap().keyframes.len() > 0 {
            // line the rows up with the graph, between the ruler and the range bar
            let frame = egui::Frame::new().inner_margin(egui::Margin {
                top: RULER_H as i8 + 7,
                bottom: (BAR_H + BOTTOM_H) as i8 + 7,
                left: 0,
                right: 0,
            });
            frame.show(ui, |ui| {
                ui.vertical(|ui| {
                    draw_bones_list(ui, selections, armature, config, shared_ui, events);
                });
            });
        }

        // a fresh view for each animation opened, fitted to its keys
        let anim = armature.sel_anim(&sel).unwrap();
        let last_key = end_of(anim);
        let total = timeline_view::content_frames(last_key, anim.fps);
        if shared_ui.timeline_view.anim_id != anim.id || !shared_ui.timeline_view.is_valid() {
            shared_ui.timeline_view = timeline_view::initial_view(last_key, anim.fps);
            shared_ui.timeline_view.anim_id = anim.id;
        }

        let column = ui.vertical(|ui| {
            let area = ui.available_rect_before_wrap();
            shared_ui.timeline_graph = (area.left(), area.width());
            let zoom_mode = navigate(ui, shared_ui, config, selections, armature, total, area);

            draw_ruler(ui, shared_ui, selections, armature, config, events);

            // The options bar has to be at the bottom, but it needs to be created first
            // so that the remaining height can be taken up by timeline graph.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                draw_bottom_bar(
                    ui, selections, &config, &armature, shared_ui, events, edit_mode,
                );
                draw_range_bar(ui, shared_ui, config, selections, armature, total);
                draw_timeline_graph(
                    ui, shared_ui, config, selections, armature, events, input, zoom_mode,
                );
            });
        });

        shared_ui.pointer_on_timeline = column.response.contains_pointer();
    });
}

/// Wheel zoom and pan, middle-drag pan, Home/Shift+Home fitting, and following the
/// playhead while playing (docs/TIMELINE.md §2.3). Returns whether the zoom modifier
/// is held, so the rows don't also scroll.
fn navigate(
    ui: &egui::Ui,
    shared_ui: &mut crate::Ui,
    config: &Config,
    selections: &SelectionState,
    armature: &Armature,
    total: f32,
    area: egui::Rect,
) -> bool {
    let (left, width) = shared_ui.timeline_graph;
    let anim = &armature.animations[selections.anim];
    let blocked = shared_ui.modal || shared_ui.settings_modal || shared_ui.export_modal;
    let (raw, mods, pointer, middle, delta) = ui.input(|i| {
        let p = &i.pointer;
        (i.raw_scroll_delta, i.modifiers, p.hover_pos(), p.middle_down(), p.delta())
    });
    let zoom_mode = mods.matches_any(config.keys.timeline_zoom_mode.modifiers);
    let view = &mut shared_ui.timeline_view;

    if let Some(p) = pointer.filter(|_| !blocked && ui.rect_contains_pointer(area)) {
        // plain wheel over the ruler or range bar zooms too: there are no rows to scroll
        let ruler = p.y < area.top() + RULER_H;
        let bar = p.y > area.bottom() - BOTTOM_H - BAR_H && p.y < area.bottom() - BOTTOM_H;
        if raw.y != 0. && (zoom_mode || ruler || bar) {
            let anchor = view.x_to_frame(p.x, left, width);
            view.zoom_at(anchor, timeline_view::ZOOM_STEP.powf(raw.y / 40.), total);
        } else if raw.x != 0. {
            // Shift+wheel, or a sideways trackpad swipe: scroll in time
            view.pan(-raw.x / view.px_per_frame(width));
        }
        if middle && delta.x != 0. {
            view.pan(-delta.x / view.px_per_frame(width));
        }
    }

    // Home: the whole animation. Shift+Home: the selected keys (all, if none).
    if let Some(selected) = shared_ui.timeline_fit.take() {
        let picked: Vec<i32> = shared_ui.selected_keyframes.iter().map(|kf| kf.frame).collect();
        let frames: Vec<i32> = if selected && !picked.is_empty() {
            picked
        } else {
            end_of(anim).into_iter().collect()
        };
        let first = if selected { frames.iter().min().copied().unwrap_or(0) } else { 0 };
        match frames.iter().max() {
            Some(last) => view.fit(first as f32, *last as f32, total),
            None => *view = TimelineView { anim_id: view.anim_id, ..timeline_view::initial_view(None, anim.fps) },
        }
    }

    // keep the playhead in view while playing, a page at a time
    if anim.elapsed.is_some() && shared_ui.timeline_bar_drag.is_none() {
        let f = anim.get_frame() as f32;
        if f < view.start || f > view.end {
            view.pan(f - view.start - view.span() * 0.05);
        }
    }
    zoom_mode
}

pub fn draw_bones_list(
    ui: &mut egui::Ui,
    selections: &SelectionState,
    armature: &Armature,
    config: &Config,
    shared_ui: &mut crate::Ui,
    events: &mut EventState,
) {
    ui.set_width(150.);
    let sel = &selections;
    let scroll_area = egui::ScrollArea::vertical()
        .id_salt("bones_list")
        .vertical_scroll_offset(shared_ui.timeline_offset.y)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    let response = scroll_area.show(ui, |ui| {
        // sort keyframes by element & bone
        let mut keyframes = armature.sel_anim(sel).unwrap().keyframes.clone();
        keyframes.sort_by(|a, b| (a.element.clone() as i32).cmp(&(b.element.clone() as i32)));
        keyframes.sort_by(|a, b| a.bone_id.cmp(&b.bone_id));

        let mut last_bone_id = -1;
        let mut first = true;

        // keep track of elements, to prevent showing multiple of the same
        let mut added_elements: Vec<AnimElement> = vec![];

        for i in 0..keyframes.len() {
            let kf = &keyframes[i];
            let bones = &armature.bones;
            let bone = bones.iter().find(|b| b.id == kf.bone_id).unwrap();
            let highlighted =
                selections.bone_ids.len() != 0 && selections.bone_ids[0] == kf.bone_id;

            // bone header (name label & fold button)
            if last_bone_id != kf.bone_id {
                if !first {
                    ui.separator();
                }
                first = false;

                // highlight bone name if it's selected
                let mut bone_str = egui::RichText::new(bone.name.clone());
                if highlighted {
                    bone_str = bone_str.strong();
                }

                ui.horizontal(|ui| {
                    let label = ui
                        .label(bone_str)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .interact(egui::Sense::click());
                    if label.clicked() {
                        // go to bone if clicked
                        let kf_id = kf.bone_id;
                        let sel = armature.bones.iter().position(|b| b.id == kf_id);
                        events.select_bone(sel.unwrap(), false);
                        events.toggle_bone_anim_folded(sel.unwrap(), false);

                        // unfold this bone's parent(s) so it appears in the hierarchy
                        let parents = armature.get_all_parents(false, kf.bone_id);
                        for parent in &parents {
                            let bones = &armature.bones;
                            let idx = bones.iter().position(|b| b.id == parent.id).unwrap();
                            events.toggle_bone_folded(idx, false);
                        }
                    }
                    last_bone_id = kf.bone_id;

                    // fold button
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let fold_icon = if bone.anim_folded { "⏴" } else { "⏷" };
                        let hand = egui::CursorIcon::PointingHand;
                        if ui.label(fold_icon).on_hover_cursor(hand).clicked() {
                            let idx = bones.iter().position(|bone| bone.id == kf.bone_id).unwrap();
                            events.toggle_bone_anim_folded(idx, !bone.anim_folded);
                        }
                    });
                });

                // reset element tracker, since this is a new bone
                added_elements = vec![];
            }
            if added_elements.contains(&kf.element) || bone.anim_folded {
                continue;
            }
            added_elements.push(kf.element.clone());
            ui.horizontal(|ui| {
                ui.add_space(30.);
                let str = format!("keyframe_editor.elements.{}", kf.element.to_string());
                let mut element_str = egui::RichText::new(shared_ui.loc(&str));
                if highlighted {
                    element_str = element_str.strong();
                }
                let label = ui.label(element_str);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut col = config.colors.text;
                    col -= Color::new(60, 60, 60, 0);
                    let text = egui::RichText::new("🗑").size(15.).color(col);
                    let pointing_hand = egui::CursorIcon::PointingHand;
                    let label = ui.label(text).on_hover_cursor(pointing_hand);
                    if label.clicked() {
                        shared_ui.deleting_line_bone_id = kf.bone_id;
                        shared_ui.deleting_line_element = kf.element.clone();
                        let line = shared_ui.loc("polar.delete_keyframe_line");
                        events.open_polar_modal(PolarId::DeleteKeyframeLine, line);
                    }
                });
                shared_ui.bone_tops.tops.push(BoneTop {
                    id: kf.bone_id,
                    element: kf.element.clone(),
                    height: label.rect.top(),
                });
            });
        }

        ui.add_space(20.);
    });

    if ui.ui_contains_pointer() {
        shared_ui.timeline_offset.y = response.state.offset.y;
    }
}

/// The ruler: labels spaced to the zoom (docs/TIMELINE.md §2.4), the summary keys
/// for each frame, and scrubbing.
pub fn draw_ruler(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    selections: &SelectionState,
    armature: &Armature,
    config: &Config,
    events: &mut EventState,
) {
    let (left, width) = shared_ui.timeline_graph;
    let rect = egui::Rect::from_min_size(egui::pos2(left, ui.cursor().top()), egui::vec2(width, RULER_H));
    let ruler = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::CornerRadius::ZERO, config.colors.light_accent);

    let sel = selections.clone();
    let anim = armature.sel_anim(&sel).unwrap();
    let fps = anim.fps;
    let view = shared_ui.timeline_view;
    let x_of = |frame: i32| view.frame_to_x(frame as f32, left, width);
    let visible = |frame: i32| frame as f32 >= view.start - 1. && frame as f32 <= view.end + 1.;

    // labels, with ticks between them
    let ppf = view.px_per_frame(width);
    let step = timeline_view::ruler_step(ppf, fps, shared_ui.timeline_seconds);
    let minor = if step % 5 == 0 { step / 5 } else if step % 2 == 0 { step / 2 } else { step }.max(1);
    let text_col: egui::Color32 = config.colors.text.into();
    let tick_col = text_col.gamma_multiply(0.45);
    let frames = view.visible_frames();
    let mut f = *frames.start() - *frames.start() % minor;
    while f <= *frames.end() {
        let x = x_of(f);
        if f % step == 0 {
            painter.vline(x, egui::Rangef::new(rect.bottom() - 7., rect.bottom()), egui::Stroke::new(1., text_col));
            let label = timeline_view::ruler_label(f, fps, shared_ui.timeline_seconds);
            let font = egui::FontId::proportional(11.);
            painter.text(egui::pos2(x, rect.top() + 2.), egui::Align2::CENTER_TOP, label, font, text_col);
        } else {
            painter.vline(x, egui::Rangef::new(rect.bottom() - 3., rect.bottom()), egui::Stroke::new(1., tick_col));
        }
        f += minor;
    }

    // scrub: press or drag on the ruler to move the current frame
    if ruler.is_pointer_button_down_on() && !shared_ui.hovering_diamond {
        if let Some(p) = ui.ctx().pointer_interact_pos() {
            let frame = view.nearest_frame(p.x, left, width);
            if frame != selections.anim_frame {
                events.select_anim_frame(frame as usize, false, false);
            }
            shared_ui.last_selected = "keyframe".to_string();
        }
    }

    let mut last_unique_frame = -1;
    let mut alt = false;
    for i in 0..anim.keyframes.len() {
        let frame = anim.keyframes[i].frame;
        if frame == last_unique_frame {
            continue;
        }
        last_unique_frame = frame;

        // alternate diamond color
        let mut diamond_color;
        if alt {
            diamond_color = Color::new(255, 255, 255, 0);
            diamond_color -= Color::new(40, 40, 40, 0);
        } else {
            diamond_color = Color::new(255, 255, 255, 0);
        }
        alt = !alt;

        // only the frames in view
        if !visible(frame) {
            continue;
        }
        let pos = Vec2::new(x_of(frame), rect.top() + 21.);

        // create dragging area for diamond
        let rect = egui::Rect::from_center_size(pos.into(), egui::Vec2::splat(9.));
        let response: egui::Response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        let mut diamond_size = 4.;

        // enlarge diamond if all of its keyframes are selected
        let keyframes = &anim.keyframes;
        // used to determine if 'cmd' should be held when selecting
        // this diamond
        let mut is_selected = false;
        if shared_ui.selected_keyframes.len() > 0 {
            let this_keyframes: Vec<&Keyframe> = keyframes
                .iter()
                .filter(|kf| kf.frame == last_unique_frame)
                .collect();
            let this_sel_kf: Vec<&Keyframe> = shared_ui
                .selected_keyframes
                .iter()
                .filter(|kf| kf.frame == last_unique_frame)
                .collect();
            is_selected = this_keyframes.len() == this_sel_kf.len();
            if is_selected {
                diamond_color = Color::new(100, 255, 100, 0);
                diamond_size += 1.;
            }
        }

        if response.drag_started() {
            events.select_anim_frame(frame as usize, true, is_selected);
        }

        if response.hovered() {
            shared_ui.hovering_diamond = true;
            shared_ui.cursor_icon = egui::CursorIcon::PointingHand;
            diamond_size += 1.;
        }

        if response.clicked() {
            shared_ui.last_selected = "keyframe".to_string();
            events.select_anim_frame(frame as usize, true, is_selected);
        }

        if response.secondary_clicked() {
            events.select_anim_frame(frame as usize, true, is_selected);
            shared_ui.last_selected = "keyframe".to_string();
            let kf = &anim.keyframes[i];
            let el = &(kf.element.clone() as usize);
            let context_id =
                &format!("keyframe_{}_{}_{}_{}", el, &kf.bone_id, &kf.frame, &i);
            shared_ui.context_menu.show(context_id);
        }

        let pointer = ui.ctx().pointer_latest_pos().unwrap_or_default();
        // dragged above the ruler: being removed
        let removing = pointer.y < ruler.rect.top();
        if response.dragged() {
            shared_ui.cursor_icon = egui::CursorIcon::Grabbing;
            shared_ui.dragged_keyframe = Keyframe {
                frame,
                bone_id: -1,
                ..Default::default()
            };

            // draw diamond following mouse
            let color = if removing { egui::Color32::RED } else { diamond_color.into() };
            draw_diamond(&ui.ctx().debug_painter(), pointer.into(), color, 4.);
        }

        let kf = &shared_ui.dragged_keyframe;
        let not_dragging = kf.frame != frame || kf.bone_id != -1;
        if not_dragging {
            // draw regular stationary diamond
            draw_diamond(ui.painter(), pos, diamond_color.into(), diamond_size);
        } else {
            // draw stationary diamond with lower opacity when dragging
            let dc = diamond_color;
            let white = egui::Color32::from_rgba_unmultiplied(dc.r, dc.g, dc.b, 30);
            draw_diamond(ui.painter(), pos, white, 4.);
        }

        if !response.drag_stopped() {
            continue;
        }

        shared_ui.cursor_icon = egui::CursorIcon::Grabbing;

        // remove keyframe if dragged out
        if removing {
            events.delete_keyframes_by_frame(anim.keyframes[i].frame);
            // break loop to prevent OOB errors
            break;
        }

        // move all keyframes under this one over
        events.move_selected_keyframes(view.nearest_frame(pointer.x, left, width));
        return;
    }
}

pub fn draw_timeline_graph(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    config: &Config,
    selections: &SelectionState,
    armature: &Armature,
    events: &mut EventState,
    input: &InputStates,
    zoom_mode: bool,
) {
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    ui.with_layout(layout, |ui| {
        let frame = egui::Frame::new()
            .fill(config.colors.light_accent.into())
            .inner_margin(egui::Margin::symmetric(0, 3));
        frame.show(ui, |ui| {
            // rows scroll vertically; time is the view range (the range bar)
            let source = egui::scroll_area::ScrollSource { mouse_wheel: !zoom_mode, ..egui::scroll_area::ScrollSource::ALL };
            let area = egui::ScrollArea::vertical()
                .vertical_scroll_offset(shared_ui.timeline_offset.y)
                .scroll_source(source)
                .auto_shrink(false)
                .id_salt("timeline_rows");
            let response = area.show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());

                let mut cursor = get_cursor(ui);
                // keep cursor on the frame
                cursor.y -= shared_ui.timeline_offset.y;

                draw_frame_lines(ui, shared_ui, armature, config, input, selections, events, cursor);
            });
            if !input.left_clicked
                && !input.left_down
                && !input.left_pressed
                && ui.ui_contains_pointer()
            {
                shared_ui.timeline_offset.y = response.state.offset.y;
            }
        });
    });
}

/// The range bar (docs/TIMELINE.md §2.2): the whole animation, with a thumb for the
/// visible part. Drag the thumb to scroll, its ends to zoom; click the track to jump
/// there, double-click to fit the animation.
fn draw_range_bar(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    config: &Config,
    selections: &SelectionState,
    armature: &Armature,
    total: f32,
) {
    let (left, width) = shared_ui.timeline_graph;
    let (row, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), BAR_H), egui::Sense::hover());
    let rect = egui::Rect::from_min_size(egui::pos2(left, row.top()), egui::vec2(width, BAR_H));
    let response = ui.interact(rect, ui.id().with("timeline_range_bar"), egui::Sense::click_and_drag());
    let mut view = shared_ui.timeline_view;
    let bar_total = view.bar_total.unwrap_or(total.max(view.end));
    let to_frames = |dx: f32| dx / width.max(1.) * bar_total;

    let hit = |view: &TimelineView, x: f32| timeline_view::bar_hit(view, bar_total, left, width, x);
    if response.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(rect.center());
        let handle = hit(&view, origin.x).unwrap_or_else(|| {
            // grabbed the track: centre the view there, then drag it
            let jump = to_frames(origin.x - left) - view.span() / 2. - view.start;
            timeline_view::bar_drag(&mut view, BarHandle::Middle, jump, bar_total);
            BarHandle::Middle
        });
        shared_ui.timeline_bar_drag = Some(handle);
        view.bar_total = Some(bar_total);
    }
    if let Some(handle) = shared_ui.timeline_bar_drag.filter(|_| response.dragged()) {
        timeline_view::bar_drag(&mut view, handle, to_frames(response.drag_delta().x), bar_total);
    }
    if response.drag_stopped() || (shared_ui.timeline_bar_drag.is_some() && !response.dragged()) {
        shared_ui.timeline_bar_drag = None;
        view.bar_total = None;
    }
    if response.double_clicked() {
        shared_ui.timeline_fit = Some(false);
    } else if response.clicked() {
        if let Some(p) = response.interact_pointer_pos().filter(|p| hit(&view, p.x).is_none()) {
            let jump = to_frames(p.x - left) - view.span() / 2. - view.start;
            timeline_view::bar_drag(&mut view, BarHandle::Middle, jump, bar_total);
        }
    }

    // cursor: resize on the ends, grab on the middle
    let hovered = response.hover_pos().and_then(|p| hit(&view, p.x));
    match shared_ui.timeline_bar_drag.or(hovered) {
        Some(BarHandle::Start | BarHandle::End) => shared_ui.cursor_icon = egui::CursorIcon::ResizeHorizontal,
        Some(BarHandle::Middle) if shared_ui.timeline_bar_drag.is_some() => {
            shared_ui.cursor_icon = egui::CursorIcon::Grabbing
        }
        Some(BarHandle::Middle) => shared_ui.cursor_icon = egui::CursorIcon::Grab,
        None => {}
    }
    shared_ui.timeline_view = view;

    // draw: track, the last key and the current frame, then the thumb and its ends
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::CornerRadius::ZERO, config.colors.dark_accent);
    let bar_x = |frame: f32| left + frame / bar_total * width;
    let text: egui::Color32 = config.colors.text.into();
    let anim = &armature.animations[selections.anim];
    if let Some(last) = end_of(anim) {
        painter.vline(bar_x(last as f32), rect.y_range(), egui::Stroke::new(1., text.gamma_multiply(0.5)));
    }
    let (a, b) = timeline_view::bar_thumb(&view, bar_total, left, width);
    let thumb = egui::Rect::from_x_y_ranges(a..=b, rect.shrink2(egui::vec2(0., 2.)).y_range());
    let lit = hovered.is_some() || shared_ui.timeline_bar_drag.is_some();
    let mut thumb_col = config.colors.light_accent;
    thumb_col += Color::new(if lit { 45 } else { 25 }, if lit { 45 } else { 25 }, if lit { 45 } else { 25 }, 0);
    painter.rect_filled(thumb, egui::CornerRadius::same(3), thumb_col);
    let grip = egui::Stroke::new(1., text.gamma_multiply(if lit { 0.9 } else { 0.55 }));
    let grip_y = thumb.shrink2(egui::vec2(0., 2.)).y_range();
    for x in [a + 3., a + 5., b - 3., b - 5.] {
        painter.vline(x, grip_y, grip);
    }
    if selections.anim_frame >= 0 {
        let x = bar_x(selections.anim_frame as f32);
        let accent = config.colors.frameline + Color::new(100, 100, 100, 0);
        painter.vline(x, rect.y_range(), egui::Stroke::new(1.5, accent));
    }
    let tip = shared_ui.loc("keyframe_editor.range_bar_desc");
    response.on_hover_text_at_pointer(tip);
}

pub fn draw_bottom_bar(
    ui: &mut egui::Ui,
    selections: &mut SelectionState,
    config: &Config,
    armature: &Armature,
    shared_ui: &mut crate::Ui,
    events: &mut EventState,
    edit_mode: &EditMode,
) {
    let sel = selections.clone();
    egui::Frame::new().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_height(20.);
        ui.horizontal(|ui| {
            ui.painter_at(ui.min_rect()).rect_filled(
                ui.min_rect(),
                egui::CornerRadius::ZERO,
                config.colors.main,
            );

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                let play_str = if armature.sel_anim(&sel).unwrap().elapsed != None {
                    &shared_ui.loc("keyframe_editor.pause")
                } else {
                    &shared_ui.loc("keyframe_editor.play")
                };

                let play_text = egui::RichText::new(play_str).color(config.colors.text);
                let button = egui::Button::new(play_text)
                    .fill(config.colors.light_accent)
                    .corner_radius(0.);

                let button = ui
                    .add_sized([50., 20.], button)
                    .on_hover_cursor(egui::CursorIcon::PointingHand);

                let mut pressed = ui.input_mut(|i| i.consume_shortcut(&config.keys.play_animation));
                if button.clicked() {
                    pressed = true;
                }
                if !pressed || armature.sel_anim(&sel).unwrap().keyframes.len() == 0 {
                    return;
                }

                let anim = armature.sel_anim(&sel).unwrap();
                events.toggle_anim_playing(selections.anim, anim.elapsed == None);
            });

            // zoom about the current frame (or the middle of the view)
            let anim = armature.sel_anim(&sel).unwrap();
            let total = timeline_view::content_frames(end_of(anim), anim.fps);
            let view = shared_ui.timeline_view;
            let playhead = selections.anim_frame as f32;
            let anchor = if playhead >= view.start && playhead <= view.end {
                playhead
            } else {
                (view.start + view.end) / 2.
            };
            let kb_tip = shared_ui
                .loc("keyframe_editor.zoom_kb_tip")
                .replace("$kb", &config.keys.timeline_zoom_mode.display());
            let desc = shared_ui.loc("top_bar.view.zoom_in");
            if ui.skf_button("+").on_hover_text(desc + &kb_tip).clicked() {
                shared_ui.timeline_view.zoom_at(anchor, timeline_view::ZOOM_STEP, total);
            }
            let desc = shared_ui.loc("top_bar.view.zoom_out");
            if ui.skf_button("-").on_hover_text(desc + &kb_tip).clicked() {
                shared_ui.timeline_view.zoom_at(anchor, 1. / timeline_view::ZOOM_STEP, total);
            }

            // the ruler counts frames or seconds
            let (label, tip) = if shared_ui.timeline_seconds {
                ("s", shared_ui.loc("keyframe_editor.ruler_seconds"))
            } else {
                ("f", shared_ui.loc("keyframe_editor.ruler_frames"))
            };
            if ui.skf_button(label).on_hover_text(tip).clicked() {
                shared_ui.timeline_seconds = !shared_ui.timeline_seconds;
            }

            ui.add_space(5.);

            // current frame input
            ui.label(&shared_ui.loc("keyframe_editor.frame"));
            ui.add(
                egui::DragValue::new(&mut selections.anim_frame)
                    .speed(0.1)
                    .update_while_editing(false),
            );

            // FPS input
            let fps = armature.sel_anim(&sel).unwrap().fps;
            ui.label(&shared_ui.loc("keyframe_editor.fps"))
                .on_hover_text(&shared_ui.loc("keyframe_editor.frames_per_second"));
            let (edited, value, _) =
                ui.float_input("fps".to_string(), shared_ui, fps as f32, 1., None);
            if edited {
                events.adjust_keyframes_by_fps(value as usize);
            }

            // End: the last frame. Unset, it follows the last key (and shows dimmed).
            let anim = armature.sel_anim(&sel).unwrap();
            let end = anim.last_frame();
            let set = anim.end_frame > 0;
            let secs = timeline_view::ruler_label(end, fps, true);
            let tip = shared_ui.loc(if set { "keyframe_editor.end_set_desc" } else { "keyframe_editor.end_auto_desc" })
                .replace("$frames", &end.to_string())
                .replace("$secs", &secs);
            let mut label_col = config.colors.text;
            if !set {
                label_col -= Color::new(60, 60, 60, 0);
            }
            ui.label(egui::RichText::new(shared_ui.loc("keyframe_editor.end")).color(label_col)).on_hover_text(&tip);
            let (edited, value, _) = ui.float_input("anim_end".to_string(), shared_ui, end as f32, 1., None);
            if edited {
                events.set_anim_end(value.round() as i32);
            }
            if set {
                let auto = ui.skf_button("⟲").on_hover_text(shared_ui.loc("keyframe_editor.end_reset"));
                if auto.clicked() {
                    events.set_anim_end(0);
                }
            }

            // onion toggle
            let mut col = config.colors.text;
            if !edit_mode.onion_layers {
                col -= Color::new(60, 60, 60, 0);
            }
            let has_keyframes = armature.sel_anim(&sel).unwrap().keyframes.len() > 0;
            let button = ui.add_enabled_ui(has_keyframes, |ui| {
                ui.skf_button(egui::RichText::new("🌓").color(col))
                    .on_hover_text(shared_ui.loc("keyframe_editor.onion_desc"))
            });
            if button.inner.clicked() {
                events.toggle_onion_layers(if edit_mode.onion_layers { 0 } else { 1 });
            }

            // keying the selected bones on this frame
            ui.add_space(5.);
            let frame = selections.anim_frame;
            let ids = &selections.bone_ids;
            let any_key = armature.sel_anim(&sel).unwrap().keyframes.iter().any(|k| {
                k.frame == frame && (ids.is_empty() || ids.contains(&k.bone_id))
            });
            let set = format!("{} ({})", shared_ui.loc("keyframe_editor.set_key_desc"), config.keys.insert_keyframe.display());
            let button = ui.add_enabled_ui(!ids.is_empty(), |ui| {
                ui.skf_button(shared_ui.loc("keyframe_editor.set_key")).on_hover_text(&set).on_disabled_hover_text(&set)
            });
            if button.inner.clicked() {
                events.insert_keyframe();
            }
            let all = format!("{} ({})", shared_ui.loc("keyframe_editor.key_all_desc"), config.keys.insert_keyframe_all.display());
            if ui.skf_button(shared_ui.loc("keyframe_editor.key_all")).on_hover_text(&all).clicked() {
                events.insert_keyframe_all();
            }
            // breakdown: key between the previous and next poses
            ui.add_space(5.);
            ui.add_enabled_ui(!ids.is_empty(), |ui| {
                let desc = shared_ui.loc("keyframe_editor.breakdown_desc");
                ui.label(shared_ui.loc("keyframe_editor.breakdown")).on_hover_text(&desc);
                let mut percent = shared_ui.breakdown * 100.;
                let slider = egui::Slider::new(&mut percent, 0.0..=100.0).suffix("%").max_decimals(0);
                let slider = ui.add_sized([110., 18.], slider).on_hover_text(&desc);
                if slider.drag_started() || slider.changed() && !slider.dragged() {
                    events.save_animation();
                }
                if slider.changed() {
                    shared_ui.breakdown = percent / 100.;
                    events.breakdown(shared_ui.breakdown);
                }
            });

            let delete = format!("{} ({})", shared_ui.loc("keyframe_editor.delete_key_desc"), config.keys.delete_keyframe.display());
            let button = ui.add_enabled_ui(any_key, |ui| {
                ui.skf_button(shared_ui.loc("keyframe_editor.delete_key")).on_hover_text(&delete).on_disabled_hover_text(&delete)
            });
            if button.inner.clicked() {
                events.delete_bone_keyframes();
            }
        });
    });
}

/// Draw the frame lines in view, and the keys on them.
fn draw_frame_lines(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    armature: &Armature,
    config: &Config,
    input: &InputStates,
    selections: &SelectionState,
    events: &mut EventState,
    cursor: Vec2,
) {
    let panel = shared_ui.keyframe_panel_rect;
    let (left, width) = shared_ui.timeline_graph;
    let view = shared_ui.timeline_view;
    let x_of = |frame: i32| view.frame_to_x(frame as f32, left, width);
    let visible = |frame: i32| frame as f32 >= view.start - 1. && frame as f32 <= view.end + 1.;
    let ppf = view.px_per_frame(width);

    let clip = ui.clip_rect();
    let range = clip.y_range();
    let painter = ui.painter().clone();
    let anim = &armature.animations[selections.anim];
    let last_keyframe = end_of(anim);

    // every frame while there's room, else every 2nd, 5th, 10th…
    let step = timeline_view::line_step(ppf);
    let line_width = if ppf >= 8. { 2. } else { 1. };
    let frames = view.visible_frames();
    let mut i = *frames.start() - *frames.start() % step;
    while i <= *frames.end() {
        let mut color: egui::Color32 = config.colors.frameline.into();

        // use different color for frameline if it exceeds last keyframe
        if last_keyframe.is_some_and(|last| i > last) {
            color = config.colors.dark_accent.into();
        }

        // brighten frameline if current animation is playing, and on this frame
        if i == anim.get_frame() && anim.elapsed != None {
            color = color + egui::Color32::from_rgb(60, 60, 60);
        } else if i != 0 && i % anim.fps.max(1) == 0 {
            color = color + egui::Color32::from_rgb(20, 20, 20);
        }

        painter.vline(x_of(i), range, Stroke { width: line_width, color });
        i += step;
    }

    // a set End: mark it, so it reads apart from the last key
    if anim.end_frame > 0 && visible(anim.end_frame) {
        let col: egui::Color32 = config.colors.text.into();
        painter.vline(x_of(anim.end_frame), range, Stroke { width: 2., color: col.gamma_multiply(0.6) });
    }

    // the frame under the pointer
    let blocked = shared_ui.modal || shared_ui.settings_modal || shared_ui.export_modal;
    let pointer = ui.ctx().pointer_hover_pos().filter(|p| !blocked && clip.contains(*p));
    let hovered = pointer.filter(|_| ui.rect_contains_pointer(clip)).map(|p| view.nearest_frame(p.x, left, width));
    shared_ui.hovering_frame = -1;
    if let Some(frame) = hovered.filter(|_| !shared_ui.hovering_diamond) {
        shared_ui.hovering_frame = frame;
        let color = egui::Color32::from_rgb(175, 175, 175);
        painter.vline(x_of(frame), range, Stroke { width: 2., color });

        // select frame if clicked/held down on.
        // holding LMB lets frames be previewed via dragging.
        if input.left_down && shared_ui.context_menu.id == "" {
            events.select_anim_frame(frame as usize, false, false);
            shared_ui.last_selected = "keyframe".to_string();
        }
        if input.right_clicked {
            let context_id = format!("kfline_{}", frame.to_string());
            shared_ui.context_menu.show(&context_id);
        }
    }

    let mut last_bone = -1;
    for top in &shared_ui.bone_tops.tops {
        if last_bone != top.id {
            let color = egui::Stroke::new(1.5, config.colors.dark_accent);
            painter.hline(clip.x_range(), top.height - 25.5, color);
        }
        last_bone = top.id;
    }

    // draw selected line
    if visible(selections.anim_frame) {
        let color = (config.colors.frameline + egui::Color32::from_rgb(100, 100, 100).into()).into();
        painter.vline(x_of(selections.anim_frame), range, Stroke { width: 2., color });
    }

    shared_ui.hovering_diamond = false;

    let is_multi = input.holding_mod || input.holding_shift;

    // draw per-change icons
    let sel_anim = &armature.animations[selections.anim];
    let mut last_unique_frame = -1;
    let mut base_color = Color::new(255, 255, 255, 0);
    base_color -= Color::new(40, 40, 40, 0);
    for i in 0..sel_anim.keyframes.len() {
        let kf = sel_anim.keyframes[i].clone();
        if kf.frame == -1 {
            continue;
        }

        let size = Vec2::new(17., 17.);

        // alternate colors between unique frames
        if last_unique_frame != kf.frame {
            last_unique_frame = kf.frame;
            if base_color == Color::new(255, 255, 255, base_color.a) {
                base_color = Color::new(255, 255, 255, 0);
                base_color -= Color::new(40, 40, 40, 0)
            } else {
                base_color = Color::new(255, 255, 255, 0);
            }
        }

        // only the keys in view (a dragged one is kept so its drag can finish)
        if !visible(kf.frame) && shared_ui.dragged_keyframe != kf {
            continue;
        }

        // the Y position is based on this diamond's respective label
        let top: f32;

        let el = kf.element.clone();
        let b_id = kf.bone_id;
        let tops = &shared_ui.bone_tops.tops;
        if let Some(b_top) = tops.iter().find(|bt| bt.id == b_id && bt.element == el) {
            top = b_top.height;
        } else {
            continue;
        }
        let pos = Vec2::new(x_of(kf.frame), top + size.y / 2.);
        if panel != None && pos.y > panel.unwrap().bottom() {
            continue;
        }

        let mut idx = kf.element.clone().clone() as usize;
        if idx > shared::ANIM_ICON_ID.len() - 1 {
            idx = shared::ANIM_ICON_ID.len() - 1;
        }

        let dkf = &shared_ui.dragged_keyframe;

        // make icon translucent if being dragged
        if *dkf == kf {
            base_color.a = 30;
        }

        let rect = egui::Rect::from_center_size(pos.into(), (size * 0.5).into());
        let response: egui::Response = ui
            .allocate_rect(rect, egui::Sense::click_and_drag())
            .on_hover_text(shared_ui.loc("keyframe_editor.double_click_tip"));

        let mut icon_size: egui::Vec2 = [20., 20.].into();
        let mut final_color = base_color;
        // expand icon if hovered on
        if response.hovered() || shared_ui.selected_keyframes.contains(&kf) {
            icon_size += [6., 6.].into();
            if shared_ui.selected_keyframes.contains(&kf) {
                final_color = Color::new(100, 255, 100, 0);
            }
            if response.hovered() {
                shared_ui.hovering_diamond = true;
                shared_ui.cursor_icon = egui::CursorIcon::PointingHand;
            }
        }

        // draw icon
        let offset: Vec2 = (icon_size / 2.).into();
        let img_rect = egui::Rect::from_min_size((pos - offset).into(), icon_size.into());
        egui::Image::new(&shared_ui.icon_images[shared::ANIM_ICON_ID[idx]])
            .tint(final_color)
            .paint_at(ui, img_rect);

        // select this frame if icon is clicked
        if response.clicked() {
            shared_ui.last_selected = "keyframe".to_string();
            events.select_anim_frame(kf.frame as usize, false, false);
            let mut kfs = vec![kf.clone()];

            // select related keyframes if clicking this while it's selected
            if shared_ui.selected_keyframes.contains(&kf) {
                kfs = sel_anim.keyframes.clone();

                type AE = AnimElement;
                match kf.element {
                    AE::PositionX | AE::PositionY => {
                        is_relevant_keyframe(&mut kfs, &kf, &vec![AE::PositionX, AE::PositionY]);
                    }
                    AE::ScaleX | AE::ScaleY => {
                        is_relevant_keyframe(&mut kfs, &kf, &vec![AE::ScaleX, AE::ScaleY]);
                    }
                    AE::TintR | AE::TintG | AE::TintB | AE::TintA => {
                        let els = &vec![AE::TintR, AE::TintG, AE::TintB, AE::TintA];
                        is_relevant_keyframe(&mut kfs, &kf, els);
                    }
                    _ => kfs = shared_ui.selected_keyframes.clone(),
                }
            }

            if is_multi {
                // add these keyframes to the selection if holding mod/shift
                for kf in kfs {
                    add_selected_keyframes(&mut shared_ui.selected_keyframes, &kf);
                }
            } else {
                shared_ui.selected_keyframes = kfs;
            }
        }

        // put this keyframe in selected if it's going to be dragged
        if response.drag_started() {
            if is_multi || shared_ui.selected_keyframes.contains(&kf) {
                add_selected_keyframes(&mut shared_ui.selected_keyframes, &kf);
            } else {
                shared_ui.selected_keyframes = vec![kf.clone()];
            }
        }

        if response.dragged() {
            shared_ui.dragged_keyframe = kf.clone();
            shared_ui.cursor_icon = egui::CursorIcon::Grabbing;
            if let Some(cursor) = ui.ctx().pointer_latest_pos() {
                let pos = egui::Pos2::new(cursor.x - offset.x, cursor.y - offset.y);
                let drag_rect = egui::Rect::from_min_size(pos, size.into());
                egui::Image::new(&shared_ui.icon_images[shared::ANIM_ICON_ID[idx]])
                    .paint_at(ui, drag_rect);
            }
        }

        if response.secondary_clicked() {
            let el = &(kf.element.clone() as usize);
            let context_id = &format!("keyframe_{}_{}_{}_{}", el, &kf.bone_id, &kf.frame, &i);
            if is_multi || shared_ui.selected_keyframes.contains(&kf) {
                add_selected_keyframes(&mut shared_ui.selected_keyframes, &kf);
            } else {
                shared_ui.selected_keyframes = vec![kf.clone()];
            }
            shared_ui.context_menu.show(context_id);
        }

        if response.drag_stopped() {
            // delete keyframes if cursor is above keyframe editor
            if cursor.y < 0. {
                events.delete_selected_keyframes();
                // break the loop to prevent OOB errors
                break;
            }

            // the frame that the pointer is on
            let x = ui.ctx().pointer_latest_pos().map_or(pos.x, |p| p.x);
            events.move_selected_keyframes(view.nearest_frame(x, left, width));
            break;
        }
    }

    // create extra space at the bottom
    if shared_ui.bone_tops.tops.len() > 0 {
        let height = shared_ui.bone_tops.tops.last().unwrap().height;
        let rect = egui::Rect::from_min_size(egui::pos2(0., height), egui::Vec2::new(1., 40.));
        ui.add_space(40.);
        ui.allocate_rect(rect, egui::Sense::empty());
    }
}

pub fn draw_diamond(painter: &egui::Painter, pos: Vec2, color: egui::Color32, size: f32) {
    let points = vec![
        egui::Pos2::new(pos.x, pos.y - size), // Top
        egui::Pos2::new(pos.x + size, pos.y), // Right
        egui::Pos2::new(pos.x, pos.y + size), // Bottom
        egui::Pos2::new(pos.x - size, pos.y), // Left
    ];

    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::new(2.0, color),
    ));
}

pub fn get_cursor(ui: &egui::Ui) -> Vec2 {
    let cursor_pos = ui.ctx().input(|i| {
        if let Some(result) = i.pointer.hover_pos() {
            result
        } else {
            egui::Pos2::new(0., 0.)
        }
    });
    (cursor_pos - ui.min_rect().left_top()).into()
}

fn add_selected_keyframes(selected_keyframes: &mut Vec<Keyframe>, kf: &Keyframe) {
    if !selected_keyframes.contains(&kf) {
        selected_keyframes.push(kf.clone());
    }
}

pub fn is_relevant_keyframe(kfs: &mut Vec<Keyframe>, kf: &Keyframe, els: &Vec<AnimElement>) {
    kfs.retain(|okf| {
        okf.frame == kf.frame && okf.bone_id == kf.bone_id && els.contains(&okf.element)
    });
}

/// Where the animation ends (its End, or its last key), or `None` for an empty one
/// with no End set.
fn end_of(anim: &Animation) -> Option<i32> {
    (anim.end_frame > 0 || !anim.keyframes.is_empty()).then(|| anim.last_frame())
}
