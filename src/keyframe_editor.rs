//! Animation keyframe editor. Very early and only proof-of-concept.



use ui::{EguiUi, TextInputOptions};

use crate::*;

use crate::timeline_rows::{self, Row};
use crate::timeline_view::{self, BarHandle, TimelineView};

/// the ruler's height
const RULER_H: f32 = 20.;
/// a row's height (docs/TIMELINE.md §3.1)
const ROW_H: f32 = 18.;
/// the label column's width, by default and at least
const LABEL_W: f32 = 190.;
const LABEL_MIN: f32 = 110.;
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

        // the bone rows' positions, for scrolling to a bone selected elsewhere
        shared_ui.bone_tops = BoneTops::default();

        // a fresh view for each animation opened, fitted to its keys
        let sel = selections.clone();
        let anim = armature.sel_anim(&sel).unwrap();
        let last_key = end_of(anim);
        let total = timeline_view::content_frames(last_key, anim.fps);
        if shared_ui.timeline_view.anim_id != anim.id || !shared_ui.timeline_view.is_valid() {
            shared_ui.timeline_view = timeline_view::initial_view(last_key, anim.fps);
            shared_ui.timeline_view.anim_id = anim.id;
        }

        let column = ui.vertical(|ui| {
            // labels on the left, the graph on the right: rows share one scroll area,
            // so a label and its keys always line up
            let area = ui.available_rect_before_wrap();
            if shared_ui.timeline_label_w <= 0. {
                shared_ui.timeline_label_w = LABEL_W;
            }
            let label_w = shared_ui.timeline_label_w.clamp(LABEL_MIN, (area.width() - 160.).max(LABEL_MIN));
            shared_ui.timeline_graph = (area.left() + label_w, area.width() - label_w);
            let graph = egui::Rect::from_x_y_ranges(area.left() + label_w..=area.right(), area.y_range());
            let zoom_mode = navigate(ui, shared_ui, config, selections, armature, total, graph);

            draw_ruler(ui, shared_ui, selections, armature, config, events, area.left());

            // The options bar has to be at the bottom, but it needs to be created first
            // so that the remaining height can be taken up by the rows.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                draw_bottom_bar(
                    ui, selections, &config, &armature, shared_ui, events, edit_mode,
                );
                draw_range_bar(ui, shared_ui, config, selections, armature, total);
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    draw_rows(ui, shared_ui, armature, config, input, selections, events, zoom_mode);
                });
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

/// The ruler: labels spaced to the zoom (docs/TIMELINE.md §2.4). Press or drag to
/// scrub; Alt-click selects every key on a frame.
pub fn draw_ruler(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    selections: &SelectionState,
    armature: &Armature,
    config: &Config,
    events: &mut EventState,
    labels_left: f32,
) {
    let (left, width) = shared_ui.timeline_graph;
    let top = ui.cursor().top();
    let rect = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, RULER_H));
    let header = egui::Rect::from_x_y_ranges(labels_left..=left, rect.y_range());
    let ruler = ui.allocate_rect(rect.union(header), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::CornerRadius::ZERO, config.colors.light_accent);

    let anim = armature.sel_anim(selections).unwrap();
    let fps = anim.fps;
    let view = shared_ui.timeline_view;
    let x_of = |frame: i32| view.frame_to_x(frame as f32, left, width);

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
            painter.vline(x, egui::Rangef::new(rect.bottom() - 6., rect.bottom()), egui::Stroke::new(1., text_col));
            let label = timeline_view::ruler_label(f, fps, shared_ui.timeline_seconds);
            let font = egui::FontId::proportional(11.);
            painter.text(egui::pos2(x, rect.top() + 1.), egui::Align2::CENTER_TOP, label, font, text_col);
        } else {
            painter.vline(x, egui::Rangef::new(rect.bottom() - 3., rect.bottom()), egui::Stroke::new(1., tick_col));
        }
        f += minor;
    }

    // the current frame
    if selections.anim_frame >= 0 {
        let x = x_of(selections.anim_frame);
        let col: egui::Color32 = (config.colors.frameline + Color::new(100, 100, 100, 0)).into();
        let tri = vec![egui::pos2(x - 4., rect.bottom() - 6.), egui::pos2(x + 4., rect.bottom() - 6.), egui::pos2(x, rect.bottom())];
        painter.add(egui::Shape::convex_polygon(tri, col, egui::Stroke::NONE));
    }

    let Some(p) = ui.ctx().pointer_interact_pos() else { return };
    if p.x < left {
        return;
    }
    let frame = view.nearest_frame(p.x, left, width);
    let alt = ui.input(|i| i.modifiers.alt);
    if alt && ruler.clicked() {
        // select the column: every key on this frame
        events.select_anim_frame(frame as usize, true, false);
    } else if ruler.is_pointer_button_down_on() && !alt {
        // scrub
        if frame != selections.anim_frame {
            events.select_anim_frame(frame as usize, false, false);
        }
        shared_ui.last_selected = "keyframe".to_string();
    }
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

/// selected keys
const SELECTED: egui::Color32 = egui::Color32::from_rgb(110, 240, 120);

fn shade(c: egui::Color32, d: i16) -> egui::Color32 {
    let f = |v: u8| (v as i16 + d).clamp(0, 255) as u8;
    egui::Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// A key mark: a diamond, or a square for a stepped key (docs/TIMELINE.md §3.2).
fn draw_mark(painter: &egui::Painter, c: egui::Pos2, stepped: bool, r: f32, fill: egui::Color32, stroke: egui::Stroke) {
    if stepped {
        let rect = egui::Rect::from_center_size(c, egui::Vec2::splat(r * 1.5));
        painter.rect(rect, egui::CornerRadius::same(1), fill, stroke, egui::StrokeKind::Middle);
    } else {
        let pts = vec![c + egui::vec2(0., -r), c + egui::vec2(r, 0.), c + egui::vec2(0., r), c + egui::vec2(-r, 0.)];
        painter.add(egui::Shape::convex_polygon(pts, fill, stroke));
    }
}

/// The rows (docs/TIMELINE.md §3): labels on the left, keys on the right, in one
/// scroll area. Draws only the rows and frames in view.
fn draw_rows(
    ui: &mut egui::Ui,
    shared_ui: &mut crate::Ui,
    armature: &Armature,
    config: &Config,
    input: &InputStates,
    selections: &SelectionState,
    events: &mut EventState,
    zoom_mode: bool,
) {
    let anim = &armature.animations[selections.anim];
    let rows = timeline_rows::build_rows(anim, &armature.bones, &shared_ui.timeline_opened);
    let (gl, gw) = shared_ui.timeline_graph;
    let view = shared_ui.timeline_view;
    let ppf = view.px_per_frame(gw);
    let x_of = |frame: i32| view.frame_to_x(frame as f32, gl, gw);
    let visible = |frame: i32| frame as f32 >= view.start - 1. && frame as f32 <= view.end + 1.;
    let blocked = shared_ui.modal || shared_ui.settings_modal || shared_ui.export_modal;
    let is_multi = input.holding_mod || input.holding_shift;

    let text: egui::Color32 = config.colors.text.into();
    let light: egui::Color32 = config.colors.light_accent.into();
    let dark: egui::Color32 = config.colors.dark_accent.into();

    // rows scroll vertically; time is the view range (the range bar)
    let source = egui::scroll_area::ScrollSource { mouse_wheel: !zoom_mode, ..egui::scroll_area::ScrollSource::ALL };
    let mut area = egui::ScrollArea::vertical().id_salt("timeline_rows").auto_shrink(false).scroll_source(source);
    // a scroll asked for elsewhere (selecting a bone on the canvas)
    if (shared_ui.timeline_offset.y - shared_ui.timeline_offset_seen).abs() > 0.5 {
        area = area.vertical_scroll_offset(shared_ui.timeline_offset.y);
    }

    let out = area.show(ui, |ui| {
        let width = ui.available_width();
        let height = (rows.len() as f32 * ROW_H + 40.).max(ui.clip_rect().height());
        let (content, bg) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
        let clip = ui.clip_rect();
        let painter = ui.painter().clone();
        let graph = egui::Rect::from_x_y_ranges(gl..=gl + gw, clip.y_range());
        let labels = egui::Rect::from_x_y_ranges(content.left()..=gl - 1., clip.y_range());
        let gp = painter.with_clip_rect(graph.intersect(clip));
        let lp = painter.with_clip_rect(labels.intersect(clip));
        let row_rect = |i: usize| {
            egui::Rect::from_min_size(egui::pos2(content.left(), content.top() + i as f32 * ROW_H), egui::vec2(width, ROW_H))
        };
        let first = (((clip.top() - content.top()) / ROW_H).floor().max(0.)) as usize;
        let last = ((((clip.bottom() - content.top()) / ROW_H).ceil()).max(0.) as usize).min(rows.len());
        let shown = first.min(last)..last;

        // backgrounds: the graph, then header rows a shade darker
        gp.rect_filled(graph, egui::CornerRadius::ZERO, light);
        for i in shown.clone() {
            let r = row_rect(i);
            match rows[i] {
                Row::Summary => {
                    gp.rect_filled(r, egui::CornerRadius::ZERO, shade(light, -22));
                    lp.rect_filled(r, egui::CornerRadius::ZERO, shade(dark, -8));
                }
                Row::Bone(_) => {
                    gp.rect_filled(r, egui::CornerRadius::ZERO, shade(light, -11));
                    lp.rect_filled(r, egui::CornerRadius::ZERO, shade(dark, 6));
                }
                _ => {}
            }
        }

        // the hovered key's column, lightly (docs/TIMELINE.md §3.3)
        let column = shared_ui.timeline_hover_column;
        if let Some(column) = column.filter(|c| visible(*c)) {
            let w = ppf.clamp(4., 14.);
            let r = egui::Rect::from_x_y_ranges(x_of(column) - w / 2.0..=x_of(column) + w / 2.0, graph.y_range());
            gp.rect_filled(r, egui::CornerRadius::ZERO, text.gamma_multiply(0.07));
        }

        // frame lines: every frame while there's room, else every 2nd, 5th, 10th…
        let end = end_of(anim);
        let step = timeline_view::line_step(ppf);
        let line_width = if ppf >= 8. { 1.5 } else { 1. };
        let frames = view.visible_frames();
        let mut f = *frames.start() - *frames.start() % step;
        while f <= *frames.end() {
            let mut color: egui::Color32 = config.colors.frameline.into();
            if end.is_some_and(|end| f > end) {
                color = dark;
            }
            if f == anim.get_frame() && anim.elapsed != None {
                color = color + egui::Color32::from_rgb(60, 60, 60);
            } else if f != 0 && f % anim.fps.max(1) == 0 {
                color = color + egui::Color32::from_rgb(20, 20, 20);
            }
            gp.vline(x_of(f), graph.y_range(), egui::Stroke::new(line_width, color));
            f += step;
        }
        if anim.end_frame > 0 && visible(anim.end_frame) {
            gp.vline(x_of(anim.end_frame), graph.y_range(), egui::Stroke::new(2., text.gamma_multiply(0.6)));
        }
        // between bones
        for i in shown.clone() {
            if matches!(rows[i], Row::Bone(_)) {
                painter.hline(content.x_range(), row_rect(i).top(), egui::Stroke::new(1., dark));
            }
        }

        // the current frame
        if visible(selections.anim_frame) {
            let color: egui::Color32 = (config.colors.frameline + Color::new(100, 100, 100, 0)).into();
            gp.vline(x_of(selections.anim_frame), graph.y_range(), egui::Stroke::new(2., color));
        }

        // empty graph space: hover a frame, click to go to it, right-click for its
        // menu, drag to box-select (docs/TIMELINE.md §3.4)
        let pointer = ui.ctx().pointer_hover_pos().filter(|p| !blocked && graph.contains(*p));
        let over_mark = shared_ui.hovering_diamond;
        shared_ui.hovering_frame = -1;
        if let Some(p) = pointer.filter(|_| !over_mark && bg.hovered() && shared_ui.timeline_box.is_none()) {
            let frame = view.nearest_frame(p.x, gl, gw);
            shared_ui.hovering_frame = frame;
            gp.vline(x_of(frame), graph.y_range(), egui::Stroke::new(2., egui::Color32::from_rgb(175, 175, 175)));
        }
        let origin = ui.input(|i| i.pointer.press_origin());
        let scaling = shared_ui.timeline_scale.is_some();
        if bg.clicked() && !scaling {
            if let Some(p) = bg.interact_pointer_pos().filter(|p| p.x > gl) {
                let frame = view.nearest_frame(p.x, gl, gw);
                events.select_anim_frame(frame as usize, false, false);
                shared_ui.last_selected = "keyframe".to_string();
                if !is_multi {
                    shared_ui.selected_keyframes = vec![];
                }
            }
        }
        if bg.secondary_clicked() && !scaling {
            if let Some(p) = bg.interact_pointer_pos().filter(|p| p.x > gl) {
                let frame = view.nearest_frame(p.x, gl, gw);
                shared_ui.context_menu.show(&format!("kfline_{}", frame));
            }
        }
        if bg.drag_started() && !scaling {
            if let Some(o) = origin.filter(|o| o.x > gl) {
                shared_ui.timeline_box = Some((view.x_to_frame(o.x, gl, gw), o.y - content.top()));
            }
        }
        if let (Some((f0, y0)), Some(p)) = (shared_ui.timeline_box, ui.ctx().pointer_latest_pos()) {
            let f1 = view.x_to_frame(p.x, gl, gw);
            let y1 = p.y - content.top();
            let r = egui::Rect::from_two_pos(
                egui::pos2(view.frame_to_x(f0, gl, gw), content.top() + y0),
                egui::pos2(view.frame_to_x(f1, gl, gw), content.top() + y1),
            );
            gp.rect(r, egui::CornerRadius::ZERO, SELECTED.gamma_multiply(0.08), egui::Stroke::new(1., SELECTED.gamma_multiply(0.7)), egui::StrokeKind::Inside);
            if !bg.dragged() {
                // select every key of the rows and frames inside
                let (fa, fb) = (f0.min(f1).ceil() as i32, f0.max(f1).floor() as i32);
                let ra = (y0.min(y1) / ROW_H).floor().max(0.) as usize;
                let rb = ((y0.max(y1) / ROW_H).floor().max(0.) as usize).min(rows.len().saturating_sub(1));
                let mut picked: Vec<Keyframe> = if is_multi { shared_ui.selected_keyframes.clone() } else { vec![] };
                if !rows.is_empty() && ra < rows.len() {
                    for kf in &anim.keyframes {
                        let inside = kf.frame >= fa && kf.frame <= fb && rows[ra..=rb].iter().any(|r| r.matches(kf));
                        if inside && !picked.iter().any(|s| timeline_rows::same_key(s, kf)) {
                            picked.push(kf.clone());
                        }
                    }
                }
                shared_ui.selected_keyframes = picked;
                shared_ui.last_selected = "keyframe".to_string();
                shared_ui.timeline_box = None;
            }
        }

        // keys
        let mut hovered_column = None;
        shared_ui.hovering_diamond = false;
        let selected = shared_ui.selected_keyframes.clone();
        let is_selected = |k: &Keyframe| selected.iter().any(|s| timeline_rows::same_key(s, k));
        let mut tops = vec![];
        for i in shown.clone() {
            let row = &rows[i];
            let r = row_rect(i);
            if let Row::Bone(id) = row {
                tops.push(BoneTop { id: *id, element: AnimElement::default(), height: i as f32 * ROW_H });
            }
            let hue = row.group().map(|g| {
                let (r, g, b) = g.hue();
                egui::Color32::from_rgb(r, g, b)
            });
            let base = hue.unwrap_or(if matches!(row, Row::Summary) { text.gamma_multiply(0.8) } else { text });

            for frame in timeline_rows::row_frames(anim, row).into_iter().filter(|f| visible(*f)) {
                let keys = timeline_rows::keys_at(anim, row, frame);
                let all = keys.iter().all(|k| is_selected(k));
                let any = keys.iter().any(|k| is_selected(k));
                let stepped = keys.iter().all(|k| timeline_rows::is_stepped_key(k));
                let c = egui::pos2(x_of(frame), r.center().y);
                let hit = egui::Rect::from_center_size(c, egui::vec2(12., ROW_H));
                let id = ui.id().with(("timeline_key", i, frame));
                let sense = if scaling { egui::Sense::hover() } else { egui::Sense::click_and_drag() };
                let resp = ui.interact(hit.intersect(graph), id, sense);

                let lit = resp.hovered() || column == Some(frame);
                let radius = if lit { 5. } else { 4. };
                let (fill, stroke) = if all {
                    (SELECTED, egui::Stroke::new(1., shade(SELECTED, -90)))
                } else if any {
                    (base, egui::Stroke::new(1.5, SELECTED))
                } else if resp.hovered() {
                    (base, egui::Stroke::new(1., egui::Color32::WHITE))
                } else {
                    (base, egui::Stroke::new(1., shade(base, -110)))
                };
                draw_mark(&gp, c, stepped, radius, fill, stroke);

                if resp.hovered() {
                    shared_ui.hovering_diamond = true;
                    shared_ui.cursor_icon = egui::CursorIcon::PointingHand;
                    hovered_column = Some(frame);
                }

                let keys: Vec<Keyframe> = keys.into_iter().cloned().collect();
                if resp.clicked() || resp.double_clicked() {
                    shared_ui.last_selected = "keyframe".to_string();
                    events.select_anim_frame(frame as usize, false, false);
                    // double-click: every key of this bone (or all bones) on the frame
                    let picked: Vec<Keyframe> = if resp.double_clicked() {
                        let column = row.bone_id().map_or(Row::Summary, Row::Bone);
                        timeline_rows::keys_at(anim, &column, frame).into_iter().cloned().collect()
                    } else {
                        keys.clone()
                    };
                    if input.holding_mod && all && !resp.double_clicked() {
                        // Ctrl-click a selected mark: deselect it
                        shared_ui.selected_keyframes.retain(|s| !keys.iter().any(|k| timeline_rows::same_key(s, k)));
                    } else if is_multi {
                        for k in picked {
                            add_selected_keyframes(&mut shared_ui.selected_keyframes, &k);
                        }
                    } else {
                        shared_ui.selected_keyframes = picked;
                    }
                }
                if resp.drag_started() {
                    if !all {
                        if !is_multi {
                            shared_ui.selected_keyframes = vec![];
                        }
                        for k in &keys {
                            add_selected_keyframes(&mut shared_ui.selected_keyframes, k);
                        }
                    }
                    shared_ui.last_selected = "keyframe".to_string();
                    if input.holding_alt {
                        // Alt-drag: scale around the current frame instead of moving
                        let pivot = pivot_frame(selections, &shared_ui.selected_keyframes);
                        let start_x = origin.map_or(c.x, |o| o.x);
                        shared_ui.timeline_scale = Some(timeline_rows::ScaleDrag { pivot, start_x, from_drag: true });
                    } else {
                        shared_ui.dragged_keyframe = Keyframe { frame, bone_id: -2, ..Default::default() };
                    }
                }
                if resp.dragged() {
                    shared_ui.cursor_icon = egui::CursorIcon::Grabbing;
                }
                if resp.drag_stopped() {
                    let p = ui.ctx().pointer_latest_pos().unwrap_or(c);
                    if p.y < clip.top() {
                        // dropped above the rows: delete
                        events.delete_selected_keyframes();
                        shared_ui.dragged_keyframe.frame = -1;
                    } else {
                        events.move_selected_keyframes(view.nearest_frame(p.x, gl, gw));
                    }
                }
                if resp.secondary_clicked() {
                    if !all {
                        if !is_multi {
                            shared_ui.selected_keyframes = vec![];
                        }
                        for k in &keys {
                            add_selected_keyframes(&mut shared_ui.selected_keyframes, k);
                        }
                    }
                    shared_ui.last_selected = "keyframe".to_string();
                    let k = &keys[0];
                    let idx = anim.keyframes.iter().position(|a| timeline_rows::same_key(a, k)).unwrap_or(0);
                    let el = k.element.clone() as usize;
                    shared_ui.context_menu.show(&format!("keyframe_{}_{}_{}_{}", el, k.bone_id, k.frame, idx));
                }
                if matches!(row, Row::Summary | Row::Bone(_)) && resp.hovered() {
                    resp.on_hover_text(column_summary(shared_ui, armature, anim, row, frame));
                }
            }
        }
        shared_ui.timeline_hover_column = hovered_column;

        // dragging keys: where they'll land, red when dropping above the rows deletes
        let dragging = shared_ui.dragged_keyframe.bone_id == -2 && shared_ui.dragged_keyframe.frame >= 0;
        if let Some(p) = ui.ctx().pointer_latest_pos().filter(|_| dragging && ui.input(|i| i.pointer.primary_down())) {
            let offset = view.nearest_frame(p.x, gl, gw) - shared_ui.dragged_keyframe.frame;
            let col = if p.y < clip.top() { egui::Color32::RED } else { SELECTED };
            for i in shown.clone() {
                let r = row_rect(i);
                let row = &rows[i];
                let mut frames: Vec<i32> = selected.iter().filter(|k| row.matches(k)).map(|k| (k.frame + offset).max(0)).collect();
                frames.sort();
                frames.dedup();
                for f in frames.into_iter().filter(|f| visible(*f)) {
                    draw_mark(&gp, egui::pos2(x_of(f), r.center().y), false, 4., col.gamma_multiply(0.55), egui::Stroke::NONE);
                }
            }
        }

        // scaling keys: preview, then apply or cancel
        if std::mem::take(&mut shared_ui.timeline_scale_start) && !shared_ui.selected_keyframes.is_empty() {
            let pivot = pivot_frame(selections, &shared_ui.selected_keyframes);
            let start_x = ui.ctx().pointer_latest_pos().map_or(x_of(pivot) + 60., |p| p.x);
            shared_ui.timeline_scale = Some(timeline_rows::ScaleDrag { pivot, start_x, from_drag: false });
        }
        if let Some(drag) = shared_ui.timeline_scale {
            let selected = shared_ui.selected_keyframes.clone();
            let frames: Vec<i32> = selected.iter().map(|k| k.frame).collect();
            let px = x_of(drag.pivot);
            let p = ui.ctx().pointer_latest_pos().unwrap_or(egui::pos2(drag.start_x, clip.center().y));
            let mut factor = timeline_rows::scale_factor(&drag, px, p.x, 24.);
            if input.holding_mod {
                factor = (factor * 4.).round() / 4.;
            }
            let factor = timeline_rows::clamp_scale(&frames, drag.pivot, factor);
            let to = |f: i32| timeline_rows::scale_frame(f, drag.pivot, factor);
            let (_, moved, merged) = timeline_rows::retime(&anim.keyframes, &selected, to);

            // the pivot, and where the keys will land
            let amber = egui::Color32::from_rgb(240, 190, 80);
            gp.vline(px, graph.y_range(), egui::Stroke::new(1.5, amber));
            for i in shown.clone() {
                let r = row_rect(i);
                let mut fs: Vec<i32> = moved.iter().filter(|k| rows[i].matches(k)).map(|k| k.frame).collect();
                fs.sort();
                fs.dedup();
                for f in fs.into_iter().filter(|f| visible(*f)) {
                    draw_mark(&gp, egui::pos2(x_of(f), r.center().y), false, 4., amber.gamma_multiply(0.75), egui::Stroke::NONE);
                }
            }
            let span = |fs: &[i32]| fs.iter().max().unwrap_or(&0) - fs.iter().min().unwrap_or(&0);
            let new_frames: Vec<i32> = moved.iter().map(|k| k.frame).collect();
            let mut label = shared_ui
                .loc("keyframe_editor.scale_readout")
                .replace("$factor", &format!("{:.2}", factor))
                .replace("$from", &span(&frames).to_string())
                .replace("$to", &span(&new_frames).to_string());
            if merged > 0 {
                label += &shared_ui.loc("keyframe_editor.scale_merge").replace("$n", &merged.to_string());
            }
            let font = egui::FontId::proportional(12.);
            let at = egui::pos2(p.x + 14., (p.y - 18.).max(clip.top() + 2.));
            let galley = painter.layout_no_wrap(label, font, text);
            let bg_rect = egui::Rect::from_min_size(at, galley.size()).expand(3.);
            painter.rect_filled(bg_rect, egui::CornerRadius::same(3), shade(dark, -10));
            painter.galley(at, galley, text);
            shared_ui.cursor_icon = egui::CursorIcon::ResizeHorizontal;

            let (released, cancel, enter) =
                ui.input(|i| (i.pointer.primary_released(), i.pointer.secondary_pressed(), i.key_pressed(egui::Key::Enter)));
            if cancel {
                shared_ui.timeline_scale = None;
            } else if released || (enter && !drag.from_drag) {
                events.scale_selected_keyframes(drag.pivot, factor);
                shared_ui.last_selected = "keyframe".to_string();
                shared_ui.timeline_scale = None;
            }
        }

        // labels
        for i in shown.clone() {
            draw_row_label(ui, &lp, shared_ui, armature, anim, config, selections, events, &rows[i], row_rect(i).intersect(labels), i);
        }
        shared_ui.bone_tops.tops = tops;

        // the label column's right edge: drag to resize
        let edge = egui::Rect::from_x_y_ranges(gl - 3.0..=gl + 2.0, clip.y_range());
        let resize = ui.interact(edge, ui.id().with("timeline_label_resize"), egui::Sense::drag());
        painter.vline(gl - 0.5, clip.y_range(), egui::Stroke::new(1., if resize.hovered() || resize.dragged() { text.gamma_multiply(0.6) } else { dark }));
        if resize.hovered() || resize.dragged() {
            shared_ui.cursor_icon = egui::CursorIcon::ResizeHorizontal;
        }
        if resize.dragged() {
            shared_ui.timeline_label_w = (shared_ui.timeline_label_w + resize.drag_delta().x).max(LABEL_MIN);
        }
    });
    shared_ui.timeline_offset_seen = out.state.offset.y;
    shared_ui.timeline_offset.y = out.state.offset.y;
    shared_ui.timeline_rows_h = out.inner_rect.height();
}

/// A row's label: fold arrows, the property icon in its group's hue, the name, and its
/// menu (docs/TIMELINE.md §3.1).
fn draw_row_label(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    shared_ui: &mut crate::Ui,
    armature: &Armature,
    anim: &Animation,
    config: &Config,
    selections: &SelectionState,
    events: &mut EventState,
    row: &Row,
    rect: egui::Rect,
    i: usize,
) {
    if rect.width() <= 0. {
        return;
    }
    let text: egui::Color32 = config.colors.text.into();
    let dim = text.gamma_multiply(0.6);
    let font = egui::FontId::proportional(12.5);
    let y = rect.center().y;
    let id = ui.id().with(("timeline_label", i));
    let hand = egui::CursorIcon::PointingHand;
    let bone_idx = |id: i32| armature.bones.iter().position(|b| b.id == id);

    // a fold arrow at `x`; true when clicked
    let arrow = |ui: &mut egui::Ui, x: f32, open: bool, salt: &str| -> bool {
        let r = egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(14., ROW_H));
        let resp = ui.interact(r, id.with(salt), egui::Sense::click()).on_hover_cursor(hand);
        let col = if resp.hovered() { text } else { dim };
        let s = 3.5;
        let pts = if open {
            vec![egui::pos2(x - s, y - s * 0.6), egui::pos2(x + s, y - s * 0.6), egui::pos2(x, y + s * 0.8)]
        } else {
            vec![egui::pos2(x - s * 0.6, y - s), egui::pos2(x + s * 0.8, y), egui::pos2(x - s * 0.6, y + s)]
        };
        painter.add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
        resp.clicked()
    };

    match row {
        Row::Summary => {
            // fold or open every bone at once
            let all_folded = armature.bones.iter().all(|b| b.anim_folded);
            if arrow(ui, rect.left() + 10., !all_folded, "fold_all") {
                for (idx, _) in armature.bones.iter().enumerate() {
                    events.toggle_bone_anim_folded(idx, !all_folded);
                }
            }
            painter.text(egui::pos2(rect.left() + 20., y), egui::Align2::LEFT_CENTER, shared_ui.loc("keyframe_editor.summary"), font, dim);
        }
        Row::Bone(bone_id) => {
            let Some(idx) = bone_idx(*bone_id) else { return };
            let bone = &armature.bones[idx];
            if arrow(ui, rect.left() + 10., !bone.anim_folded, "fold") {
                events.toggle_bone_anim_folded(idx, !bone.anim_folded);
            }
            let selected = selections.bone_ids.contains(bone_id);
            let name_rect = egui::Rect::from_x_y_ranges(rect.left() + 18.0..=rect.right(), rect.y_range());
            let resp = ui.interact(name_rect, id.with("name"), egui::Sense::click()).on_hover_cursor(hand);
            let col = if selected || resp.hovered() { text } else { text.gamma_multiply(0.85) };
            let font = egui::FontId::proportional(if selected { 13.5 } else { 13. });
            painter.text(egui::pos2(rect.left() + 20., y), egui::Align2::LEFT_CENTER, &bone.name, font, col);
            if resp.clicked() {
                // go to the bone, unfolding its parents so it shows in the hierarchy
                events.select_bone(idx, false);
                for parent in &armature.get_all_parents(false, *bone_id) {
                    if let Some(p) = bone_idx(parent.id) {
                        events.toggle_bone_folded(p, false);
                    }
                }
            }
        }
        Row::Group(bone_id, group) => {
            let x = rect.left() + 22.;
            let (r, g, b) = group.hue();
            let hue = egui::Color32::from_rgb(r, g, b);
            let first = timeline_rows::channels(anim, *bone_id, *group);
            if let Some(el) = first.first() {
                let icon = shared::ANIM_ICON_ID[(el.clone() as usize).min(shared::ANIM_ICON_ID.len() - 1)];
                let icon_rect = egui::Rect::from_center_size(egui::pos2(x + 6., y), egui::Vec2::splat(13.));
                egui::Image::new(&shared_ui.icon_images[icon]).tint(hue).paint_at(ui, icon_rect);
            }
            let name = shared_ui.loc(&format!("keyframe_editor.groups.{}", group.key()));
            painter.text(egui::pos2(x + 16., y), egui::Align2::LEFT_CENTER, name, font, text.gamma_multiply(0.9));
            if first.len() > 1 {
                let open = shared_ui.timeline_opened.contains(&(*bone_id, *group));
                if arrow(ui, rect.right() - 10., open, "open") {
                    if open {
                        shared_ui.timeline_opened.retain(|o| *o != (*bone_id, *group));
                    } else {
                        shared_ui.timeline_opened.push((*bone_id, *group));
                    }
                }
            }
            let menu = egui::Rect::from_x_y_ranges(rect.left()..=rect.right() - 20., rect.y_range());
            if ui.interact(menu, id.with("menu"), egui::Sense::click()).secondary_clicked() {
                shared_ui.context_menu.show(&format!("kfrow_{}_g_{}", bone_id, *group as usize));
            }
        }
        Row::Channel(bone_id, el) => {
            let name = shared_ui.loc(&format!("keyframe_editor.elements.{}", el.to_string()));
            painter.text(egui::pos2(rect.left() + 54., y), egui::Align2::LEFT_CENTER, name, font, dim);
            if ui.interact(rect, id.with("menu"), egui::Sense::click()).secondary_clicked() {
                shared_ui.context_menu.show(&format!("kfrow_{}_e_{}", bone_id, el.clone() as usize));
            }
        }
    }
}

/// A summary key's tooltip: what's keyed on its frame.
fn column_summary(shared_ui: &crate::Ui, armature: &Armature, anim: &Animation, row: &Row, frame: i32) -> String {
    let mut lines: Vec<String> = vec![];
    let ids: Vec<i32> = match row {
        Row::Bone(id) => vec![*id],
        _ => armature.bones.iter().map(|b| b.id).collect(),
    };
    for id in ids {
        let mut groups: Vec<timeline_rows::Group> = timeline_rows::keys_at(anim, &Row::Bone(id), frame)
            .iter()
            .map(|k| timeline_rows::Group::of(&k.element))
            .collect();
        groups.sort();
        groups.dedup();
        if groups.is_empty() {
            continue;
        }
        let names: Vec<String> = groups.iter().map(|g| shared_ui.loc(&format!("keyframe_editor.groups.{}", g.key()))).collect();
        let bone = armature.bones.iter().find(|b| b.id == id).map_or("?".to_string(), |b| b.name.clone());
        lines.push(format!("{bone}: {}", names.join(", ")));
    }
    format!("{} {}\n{}", shared_ui.loc("keyframe_editor.frame"), frame, lines.join("\n"))
}

fn add_selected_keyframes(selected_keyframes: &mut Vec<Keyframe>, kf: &Keyframe) {
    if !selected_keyframes.contains(&kf) {
        selected_keyframes.push(kf.clone());
    }
}

/// Where the animation ends (its End, or its last key), or `None` for an empty one
/// with no End set.
fn end_of(anim: &Animation) -> Option<i32> {
    (anim.end_frame > 0 || !anim.keyframes.is_empty()).then(|| anim.last_frame())
}

/// Scaling keys pivots on the current frame, or the first selected key without one.
fn pivot_frame(selections: &SelectionState, selected: &[Keyframe]) -> i32 {
    if selections.anim_frame >= 0 {
        selections.anim_frame
    } else {
        selected.iter().map(|k| k.frame).min().unwrap_or(0)
    }
}
