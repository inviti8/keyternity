//! Core rendering logic, abstracted from the rest of WGPU.

use crate::*;
use image::GenericImageView;
use topology::VertSite;
use utils::shortest_angle_delta;
use wgpu::{BindGroup, BindGroupLayout, Device, Queue, RenderPass};

/// The `main` of this module.
pub fn render(
    render_pass: &mut RenderPass,
    queue: &wgpu::Queue,
    camera: &Camera,
    input: &InputStates,
    armature: &Armature,
    config: &Config,
    edit_mode: &EditMode,
    selections: &SelectionState,
    renderer: &mut Renderer,
    events: &mut EventState,
) {
    if camera.window == Vec2::ZERO {
        return;
    }
    let sel = selections.clone();
    let raw_input = input;

    // pending Pen cuts only live while the Pen is out (D7)
    if edit_mode.tool != Tool::Pen && !renderer.pen_cuts.is_empty() {
        renderer.pen_cuts.clear();
        renderer.pen_open = false;
        renderer.pen_preview = None;
    }

    // no edits are being made if the LMB isn't down
    if !input.left_down && (edit_mode.is_moving || edit_mode.is_rotating || edit_mode.is_scaling) {
        events.update_current_editing(1);
    }

    // inform HTML if canvas has successfully loaded
    #[cfg(target_arch = "wasm32")]
    if !renderer.has_loaded {
        loaded();
        renderer.has_loaded = true;
    }

    // create vert on cursor
    let space = utils::screen_to_world_space(input.mouse, camera.window);
    let mut mouse_world_vert = vert(Some(space), None, None);
    mouse_world_vert.pos.x *= camera.window.y / camera.window.x;

    // mouse pos in world space
    let mouse_pos = Vec2::new(
        mouse_world_vert.pos.x * camera.zoom / camera.aspect_ratio() + camera.pos.x,
        mouse_world_vert.pos.y * camera.zoom + camera.pos.y,
    );

    if !config.gridline_front {
        draw_gridline(render_pass, renderer, &camera, &config, queue);
    }

    // turn off hovering vert if not editing mesh
    if !edit_mode.editing_mesh && selections.hovering_vert_id != -1 {
        events.set_hovering_vert_id(-1);
    }

    // temporary armature, to be used for rendering
    let mut temp_arm = Armature::default();
    temp_arm.bones = renderer.temp_bones.clone();

    // animate next and previous frame armatures for onions
    let mut next_arm = Armature::default();
    let mut prev_arm = Armature::default();
    if selections.anim_frame != -1 && edit_mode.onion_layers {
        let keyframes = &armature.sel_anim(selections).unwrap().keyframes;

        // set up previous onion
        prev_arm.bones = armature.bones.clone();
        prev_arm.animations = armature.animations.clone();
        let mut prev_sel = selections.clone();
        let idx = keyframes.iter().position(|kf| kf.frame > sel.anim_frame);
        prev_sel.anim_frame = keyframes[idx.unwrap_or_else(|| 1) - 1].frame;
        utils::animate_bones(&mut prev_arm, &prev_sel, edit_mode);
        prev_arm.bones = prev_arm.animated_bones.clone();
        construction(&mut prev_arm.bones, &prev_arm.animated_bones);

        // set up next onion
        next_arm.bones = armature.bones.clone();
        next_arm.animations = armature.animations.clone();
        let mut next_sel = prev_sel.clone();
        if let Some(next_kf) = keyframes.iter().find(|kf| kf.frame > selections.anim_frame) {
            next_sel.anim_frame = next_kf.frame;
        }
        utils::animate_bones(&mut next_arm, &next_sel, edit_mode);
        next_arm.bones = next_arm.animated_bones.clone();
        construction(&mut next_arm.bones, &next_arm.animated_bones);
    }

    // get all children of selected bone(s)
    let mut selected_bone_ids = vec![];
    if armature.sel_bone(&sel) != None {
        for id in &selections.bone_ids {
            let bone = temp_arm.bones.iter().find(|bone| bone.id == *id).unwrap();
            let mut children = vec![bone.clone()];
            armature_window::get_all_children(&temp_arm.bones, &mut children, &bone);
            for child in children {
                selected_bone_ids.push(child.id);
            }
        }
    }

    // setup propagated group colors
    for b in 0..temp_arm.bones.len() {
        if temp_arm.bones[b].group_color.a != 0 {
            continue;
        }
        let parent_id = &temp_arm.bones[b].parent_id;
        let parent = temp_arm.bones.iter().find(|b| b.id == *parent_id);
        if parent != None {
            temp_arm.bones[b].group_color = parent.unwrap().group_color;
        }
    }

    // sort bones by highest zindex first, so that hover logic will pick the top-most one
    temp_arm.bones.sort_by(|a, b| b.zindex.cmp(&a.zindex));
    prev_arm.bones.sort_by(|a, b| b.zindex.cmp(&a.zindex));
    next_arm.bones.sort_by(|a, b| b.zindex.cmp(&a.zindex));

    // many fight for spot of newest vertex; only one will emerge victorious.
    let mut new_vert: Option<(Vertex, VertSite)> = None;
    let mut hovered_vert = false;

    let mut hovering_bone_id = -1;
    let mut on_click_id = -1;

    // pre-draw bone setup
    for b in 0..temp_arm.bones.len() {
        let tex = armature.tex_of(temp_arm.bones[b].id);
        let parent_id = temp_arm.bones[b].parent_id;
        if parent_id != -1 {
            let parent = temp_arm.bones.iter().find(|b| b.id == parent_id);
            temp_arm.bones[b].hidden |= parent.unwrap().hidden;
        }

        if tex == None || temp_arm.bones[b].hidden {
            continue;
        }

        let parents = temp_arm.get_all_parents(false, temp_arm.bones[b].id);

        // setup world verts
        let cam = world_camera(&camera, &config);
        for v in 0..temp_arm.bones[b].vertices.len() {
            let tb = &mut temp_arm.bones[b];
            let left = if is_facing_left(tb.scale) { -1. } else { 1. };
            let final_pivot =
                utils::rotate(&(tex.unwrap().size * tb.pivot_pos), tb.rot * left) * tb.scale;
            let mut vert = world_vert(tb.vertices[v], &cam, camera.aspect_ratio(), final_pivot);
            vert.tint = tb.tint;

            // make this translucent if not the selected bone
            if selections.bone_idx != usize::MAX {
                let not_child = parents.iter().find(|b| b.id == selections.bone_ids[0]) == None;
                if not_child && selections.bone_ids[0] != tb.id {
                    vert.tint.a = config.bone_translucency;
                }
            }

            tb.world_verts.push(vert);
        }

        // setup onion world verts
        if selections.anim_frame != -1 && edit_mode.onion_layers {
            macro_rules! prep_arm {
                ($armature:expr, $color:expr) => {
                    for v in 0..$armature.bones[b].vertices.len() {
                        let tb = &mut $armature.bones[b];
                        let ratio = camera.aspect_ratio();
                        let final_pivot =
                            utils::rotate(&(tex.unwrap().size * tb.pivot_pos), tb.rot) * tb.scale;
                        let mut vert = world_vert(tb.vertices[v], &camera, ratio, final_pivot);
                        vert.tint = $color;
                        tb.world_verts.push(vert);
                    }
                };
            }
            prep_arm!(prev_arm, TintColor::new(255., 0., 0., 0.4));
            prep_arm!(next_arm, TintColor::new(0., 0., 255., 0.4));
        }
        for vert in &mut temp_arm.bones[b].world_verts {
            vert.add_color = Color::new(0, 0, 0, 0);
        }

        // check if cursor is on an opaque pixel of this bone's texture
        let tb = &temp_arm.bones[b];
        let selected_mesh = !edit_mode.editing_mesh
            || edit_mode.editing_mesh
                && sel.bone_idx != usize::MAX
                && armature.sel_bone(&sel).unwrap().id == tb.id;
        if on_click_id == -1
            && (!input.left_down || input.left_pressed) // allow detection on LMB press
            && !camera.on_ui
            && selected_mesh
            && renderer.render_textures
        {
            let wv = &temp_arm.bones[b].world_verts;
            for (t, chunk) in temp_arm.bones[b].indices.chunks_exact(3).enumerate() {
                let c0 = chunk[0] as usize;
                let c1 = chunk[1] as usize;
                let c2 = chunk[2] as usize;

                let bary = tri_point(&mouse_world_vert.pos, &wv[c0].pos, &wv[c1].pos, &wv[c2].pos);
                if bary.0 == -1. {
                    continue;
                }

                // initiate vertex position
                let bones = &temp_arm.bones;
                let v = &bones.iter().find(|bone| bone.id == tb.id).unwrap().vertices;
                let uv = v[c0].uv * bary.3 + v[c1].uv * bary.1 + v[c2].uv * bary.2;
                let mut pos = (utils::rotate(&(v[c0].pos - tb.pos), -tb.rot)) * bary.3
                    + (utils::rotate(&(v[c1].pos - tb.pos), -tb.rot)) * bary.1
                    + (utils::rotate(&(v[c2].pos - tb.pos), -tb.rot)) * bary.2;
                pos /= tb.scale;

                // editing this bone's mesh, add this as new vertex candidate
                let mesh_tool = edit_mode.tool.edits_mesh();
                if edit_mode.editing_mesh && !mesh_tool && input.left_clicked && new_vert == None {
                    new_vert = Some((vert(Some(pos), None, Some(uv)), VertSite::Tri(t)));
                    break;
                }

                // set this bone as hovered, if the cursor is on its texture
                let tex = armature.tex_of(temp_arm.bones[b].id).unwrap();
                let img = &armature.tex_data(tex).unwrap().image;
                let pos = Vec2::new(
                    (uv.x * img.width() as f32).min(img.width() as f32 - 1.),
                    (uv.y * img.height() as f32).min(img.height() as f32 - 1.),
                );
                let pixel_alpha = img.get_pixel(pos.x as u32, pos.y as u32).0[3];
                if pixel_alpha == 255 && !edit_mode.editing_mesh {
                    hovering_bone_id = temp_arm.bones[b].id;
                    break;
                }
            }
        }

        let mut parent_selected = false;
        if on_click_id == -1 {
            on_click_id = hovering_bone_id;

            // QoL: select parent of textured bone if it's called 'Texture'
            // this is because most textured bones are meant to represent their parents
            if !config.exact_bone_select
                && on_click_id == temp_arm.bones[b].id
                && parent_id != -1
                && temp_arm.bones[b].name.to_lowercase() == "texture"
            {
                on_click_id = parent_id;
                parent_selected = true;
            }
        }

        // hovering glow animation
        let idx = selections.bone_idx;
        let not_selected = idx == usize::MAX || armature.bones[idx].id != hovering_bone_id;
        let is_hovering = selections.hovering_bone_id == temp_arm.bones[b].id || parent_selected;
        if is_hovering && not_selected && !renderer.on_point {
            let fade = (64. * ((edit_mode.time * 3.).sin()).abs()).min(255.);
            let min = 25;
            for vert in &mut temp_arm.bones[b].world_verts {
                vert.add_color =
                    Color::new(min + fade as u8, min + fade as u8, min + fade as u8, 0);
            }

            // select bone if clicked
            if !camera.on_ui && input.left_pressed && !renderer.on_point {
                let bones = &armature.bones;
                let idx = bones.iter().position(|bone| bone.id == on_click_id);
                if idx != None {
                    events.select_bone(idx.unwrap(), true);
                }
            }
        } else {
            for vert in &mut temp_arm.bones[b].world_verts {
                vert.add_color = Color::new(0, 0, 0, 0);
            }
        }
    }

    // set hovering bone, if not already selected
    let selected = selections.bone_ids.len() > 0 && selections.bone_ids[0] == on_click_id;
    if on_click_id != -1 && selections.hovering_bone_id != on_click_id && !selected {
        events.set_hovering_bone_id(on_click_id);
    }
    renderer.on_point = false;

    renderer.temp_bones = temp_arm.bones.clone();

    // runtime: sort bones by z-index for drawing
    temp_arm.bones.sort_by(|a, b| a.zindex.cmp(&b.zindex));

    // sort onions by zindex as well
    if selections.anim_frame != -1 && edit_mode.onion_layers {
        prev_arm.bones.sort_by(|a, b| a.zindex.cmp(&b.zindex));
        next_arm.bones.sort_by(|a, b| a.zindex.cmp(&b.zindex));
    }

    // render onion layers
    if renderer.render_textures && selections.anim_frame != -1 && edit_mode.onion_layers {
        #[rustfmt::skip]
        draw_armature(&prev_arm, armature, edit_mode.showing_mesh, &sel, queue, render_pass, &renderer.prev_onion_buffer);
        #[rustfmt::skip]
        draw_armature(&next_arm, armature, edit_mode.showing_mesh, &sel, queue, render_pass, &renderer.next_onion_buffer);
    }

    // render bones
    if renderer.render_textures {
        #[rustfmt::skip]
        draw_armature(&temp_arm, armature, edit_mode.showing_mesh, &sel, queue, render_pass, &renderer.bone_buffer);
    }

    // show selected bone's mesh wireframe if editing it
    let mut hovering_vert_id = -1;
    let mut is_hovering_tri = false;
    let mut is_hovering_line = false;
    let sel_bone = temp_arm.sel_bone(&sel);
    if edit_mode.editing_mesh && sel_bone != None && !sel_bone.unwrap().hidden {
        let id = armature.sel_bone(&sel).unwrap().id;
        let bone = temp_arm.bones.iter().find(|bone| bone.id == id).unwrap();

        // render texture, so it appears above everything else
        let tex = armature.tex_of(bone.id);
        if renderer.render_textures && tex != None {
            let bind_group = &armature.tex_data(tex.unwrap()).unwrap().bind_group;
            let sel_bone_buffer = &mut renderer.sel_bone_buffer;
            let mut world_verts = bone.world_verts.clone();
            for vert in &mut world_verts {
                vert.color = Color::new(255, 255, 255, 255);
            }
            render_pass.set_bind_group(0, bind_group, &[]);
            setup_render_buffer(sel_bone_buffer, &world_verts, &bone.indices, queue);
            draw(&sel_bone_buffer, render_pass, 0, bone.indices.len());
        }

        render_pass.set_bind_group(0, &renderer.generic_bindgroup, &[]);
        let mouse = mouse_world_vert;
        let wv = bone.world_verts.clone();

        // Pen and Eraser handle the mouse themselves: the usual click, drag and
        // right-click mesh edits see an idle mouse
        let mesh_tool = edit_mode.tool.edits_mesh();
        let idle_input = InputStates {
            mouse: input.mouse,
            mouse_prev: input.mouse_prev,
            ..Default::default()
        };
        let input = if mesh_tool { &idle_input } else { input };

        // prepare drawing buffers for vertex points and lines
        let current_hover_id = selections.hovering_vert_id;
        #[rustfmt::skip]
        let (mut verts, indices, on_vert) =
            bone_vertices(&wv, true, selections, input, camera, config, events, armature, renderer, current_hover_id);
        if on_vert != -1 {
            new_vert = None;
            hovering_vert_id = on_vert;
        } else {
            renderer.clicked_vert_id = -1;
        }
        #[rustfmt::skip]
        let edges = vert_lines(bone, &temp_arm.bones, &mouse, &mut new_vert, true, on_vert != -1, config.colors.mesh_edge, camera, input, selections, events, config);
        let on_line = edges.hovered;
        is_hovering_line |= on_line;

        // patterned edges first, under the solid (hovered/selected) edges and the vertices
        let (edge_v, edge_i) = &edges.patterned;
        render_pass.set_bind_group(0, edge_bindgroup(renderer, config.mesh_edge_style), &[]);
        setup_render_buffer(&renderer.edge_buffer, edge_v, edge_i, queue);
        draw(&renderer.edge_buffer, render_pass, 0, edge_i.len());
        render_pass.set_bind_group(0, &renderer.generic_bindgroup, &[]);

        let (mut lines_v, mut lines_i) = edges.solid;
        let base = lines_v.len() as u32;
        lines_v.append(&mut verts);
        lines_i.extend(indices.iter().map(|i| i + base));

        // draw hovered triangle if neither a vertex nor a line is hovered
        let (idx, mut hovering_tri) = bone_triangle(&bone, &mouse, wv.clone());
        if hovering_tri.len() > 0 && on_vert == -1 && !on_line && !camera.on_ui && !mesh_tool {
            is_hovering_tri = true;
            hovering_tri[0].color = Color::new(0, 200, 0, 100);
            hovering_tri[1].color = Color::new(0, 200, 0, 100);
            hovering_tri[2].color = Color::new(0, 200, 0, 100);
            lines_v.append(&mut hovering_tri.clone());
            add_offseted_indices(&mut vec![0, 1, 2], &mut lines_i);

            // verts of this triangle will be dragged
            if input.left_pressed {
                events.select_vertex(hovering_tri[0].id as i32, false);
                events.select_vertex(hovering_tri[1].id as i32, true);
                events.select_vertex(hovering_tri[2].id as i32, true);
            }

            // remove this triangle if right-clicking
            if edit_mode.editing_mesh && input.right_clicked {
                events.delete_triangle(idx as usize * 3);
            }
        }
        hovered_vert = on_vert != -1 && !camera.on_ui;

        if edit_mode.tool == Tool::Eraser {
            let (mut ov, mut oi) = eraser(&bone, &wv, &mouse, raw_input, camera, config, edit_mode, renderer, events);
            lines_v.append(&mut ov);
            add_offseted_indices(&mut oi, &mut lines_i);
        }
        if edit_mode.tool == Tool::Pen {
            let sel_bone = armature.sel_bone(&sel).unwrap();
            #[rustfmt::skip]
            let (mut ov, mut oi) = pen_tool(&bone, sel_bone, armature, &wv, &mouse, raw_input, camera, config, edit_mode, renderer);
            lines_v.append(&mut ov);
            add_offseted_indices(&mut oi, &mut lines_i);
        }

        // draw vertex points and lines
        setup_render_buffer(&mut renderer.meshframe_buffer, &lines_v, &lines_i, queue);
        draw(&renderer.meshframe_buffer, render_pass, 0, lines_i.len());
    }

    // increment hovering tri countdown, to show tooltip on UI
    if is_hovering_tri && input.mouse == input.mouse_prev {
        events.set_hovering_tri(selections.hovering_tri_dur + 1);
    } else if selections.hovering_tri_dur != 0 {
        events.set_hovering_tri(0);
    }

    // increment hovering line countdown, to show tooltip on UI
    if is_hovering_line && input.mouse == input.mouse_prev {
        events.set_hovering_line(selections.hovering_line_dur + 1);
    } else if selections.hovering_line_dur != 0 {
        events.set_hovering_line(0);
    }

    // set hovering vert, to display ID on UI
    if hovering_vert_id != -1 {
        events.set_hovering_vert_id(hovering_vert_id);
    }

    // draw render rects if enabled
    if renderer.render_rects {
        let mut verts = vec![];
        let mut indices = vec![];
        render_pass.set_bind_group(0, &renderer.generic_bindgroup, &[]);
        for bone in &temp_arm.bones {
            if bone.verts_edited || bone.hidden {
                continue;
            }

            let mut world_verts = bone.world_verts.clone();

            // color wireframes based on bone group color
            for vert in &mut world_verts {
                if bone.group_color.a == 0 {
                    vert.color = config.colors.center_point;
                    vert.color.a -= 50;
                } else {
                    vert.color = bone.group_color;
                    vert.color.a -= 25;
                }
            }

            // add this rect to drawing buffers
            verts.append(&mut world_verts);
            let mut bone_indices = bone.indices.clone();
            add_offseted_indices(&mut bone_indices, &mut indices);
        }

        // draw rects
        setup_render_buffer(&mut renderer.rect_buffer, &verts, &indices, queue);
        draw(&renderer.rect_buffer, render_pass, 0, indices.len());
    }

    // render mesh wireframes if on
    if renderer.render_mesh_wf {
        let mut verts = vec![];
        let mut indices = vec![];
        let mut edge_verts = vec![];
        let mut edge_indices = vec![];
        render_pass.set_bind_group(0, &renderer.generic_bindgroup, &[]);
        for bone in &temp_arm.bones {
            let already_editing =
                edit_mode.editing_mesh && armature.sel_bone(&sel).unwrap().id == bone.id;
            if !bone.verts_edited || bone.hidden || already_editing {
                continue;
            }

            let mouse = mouse_world_vert;
            let nw = &mut new_vert;
            let wv = bone.world_verts.clone();

            #[rustfmt::skip]
            let (mut verts_p, mut indices_p, _) = bone_vertices(&wv, false, selections, input, camera, config, events, armature, renderer, -1);

            // color wireframes
            let mut point_color = if bone.group_color.a == 0 { config.colors.center_point } else { bone.group_color };
            let mut line_color = point_color;
            point_color.a = point_color.a.saturating_sub(75);
            line_color.a = line_color.a.saturating_sub(125);
            for vert in &mut verts_p {
                vert.color = point_color;
            }

            #[rustfmt::skip]
            let edges = vert_lines(bone, &temp_arm.bones, &mouse, nw, false, false, line_color, camera, input, selections, events, config);
            let (mut ev, mut ei) = edges.patterned;
            add_offseted_indices(&mut ei, &mut edge_indices);
            edge_verts.append(&mut ev);

            verts.append(&mut verts_p);
            add_offseted_indices(&mut indices_p, &mut indices);
        }
        render_pass.set_bind_group(0, edge_bindgroup(renderer, config.mesh_edge_style), &[]);
        setup_render_buffer(&renderer.wireframe_edge_buffer, &edge_verts, &edge_indices, queue);
        draw(&renderer.wireframe_edge_buffer, render_pass, 0, edge_indices.len());
        render_pass.set_bind_group(0, &renderer.generic_bindgroup, &[]);
        setup_render_buffer(&renderer.wireframe_buffer, &verts, &indices, queue);
        draw(&renderer.wireframe_buffer, render_pass, 0, indices.len());
    }

    if let Some((vert, site)) = new_vert {
        renderer.new_vert = Some(vert);
        renderer.new_vert_site = site;
        events.select_vertex(-1, false);
        events.new_vertex();
    }

    if config.gridline_front {
        draw_gridline(render_pass, renderer, &camera, &config, queue);
    }

    // envelope tool overlay, under the bone points
    #[rustfmt::skip]
    envelope_tool(config, camera, input, edit_mode, armature, &temp_arm, selections, renderer, queue, render_pass, events);

    // draw bone center points, and kites representing hierarchy
    #[rustfmt::skip]
    draw_points(config, camera, input, edit_mode, &mut temp_arm, selected_bone_ids.clone(), selections, renderer, queue, render_pass, events, armature);
    #[rustfmt::skip]
    draw_kites(config, camera, edit_mode, &mut temp_arm, selected_bone_ids, renderer, queue, render_pass);

    // is a bone being hovered on, in the renderer?
    renderer.is_hovering_bone = renderer.on_point || on_click_id != -1;

    // IK owns this bone's rotation and/or position (docs/IK.md §4.2)
    let (ik_rot_lock, ik_pos_lock) =
        armature.sel_bone(&sel).map_or((false, false), |bone| ik::locks(&armature.bones, bone.id));
    let has_ik = ik_rot_lock;

    // show transform rings when editing a bone
    let ring_enabled = config.transform_rot_radius > 0. && config.transform_scale_radius > 0.;
    let idle_mouse = !input.left_down && !input.left_clicked || camera.on_ui;
    let selected = armature.sel_bone(&sel) != None && selections.bone_ids.len() == 1;
    let ring_tool = edit_mode.tool != Tool::Envelope;
    if !edit_mode.editing_mesh && !has_ik && idle_mouse && selected && ring_enabled && ring_tool {
        #[rustfmt::skip]
        transform_ring(config, camera, armature, &mut temp_arm, render_pass, renderer, events, edit_mode, &sel, queue, &mouse_pos);
    } else if edit_mode.temporary != None {
        events.set_temporary_edit_mode(3);
    }

    // if no SelectBone events have been called, unselect current if mouse is pressed
    if !camera.on_ui
        && armature.bones.len() > 0
        && edit_mode.sel_time > 0.25
        && input.left_clicked
        && !edit_mode.editing_mesh
        && !renderer.mobile
        && !renderer.tail_hovered
        && !renderer.dragging_tail
        && renderer.envelope_hovered.is_none()
        && renderer.envelope_drag.is_none()
    {
        let mut unselect = true;
        for event in &events.events {
            if *event == Events::SelectBone {
                unselect = false;
                break;
            }
        }
        if unselect {
            events.select_bone(usize::MAX, true);
        }
    }

    // if cursor is not hovering on any verts, unselect on click
    if !camera.on_ui
        && input.left_clicked
        && (edit_mode.editing_mesh || selections.bind != -1)
        && !hovered_vert
    {
        events.select_vertex(-1, false);
    }

    // dragging vert stuff
    if !input.left_down {
        renderer.editing_bone = false;
        renderer.started_dragging_verts = false;
        renderer.dragging_tail = false;
        renderer.envelope_drag = None;
    } else if sel.vert_ids.len() > 0 && armature.sel_bone(&sel) != None && !camera.on_ui {
        if !renderer.started_dragging_verts {
            events.save_bone(selections.bone_idx);
            renderer.started_dragging_verts = true
        }
        for vert_id in sel.vert_ids.clone() {
            events.drag_vertex(vert_id);
        }

        return;
    }

    // keep track of selected bone's initial rotation, if mouse isn't being pressed
    if input.mouse_init == None {
        if let Some(bone) = armature.sel_bone(&sel) {
            let anim_bones = &armature.animated_bones;
            let sel_anim_bone = anim_bones.iter().find(|b| b.id == bone.id).unwrap();
            renderer.bone_init_rot = sel_anim_bone.rot;
            renderer.bone_init_pivot_rot = sel_anim_bone.pivot_rot;
        }
    }

    if !input.left_down && !input.right_down {
        return;
    }

    // move camera
    if input.right_down && !camera.on_ui {
        let vel = renderer::mouse_vel(&input, &camera) * camera.zoom;
        events.edit_camera(camera.pos.x + vel.x, camera.pos.y + vel.y, camera.zoom);
        return;
    }

    // dragging the selected bone's tail handle
    if renderer.dragging_tail {
        if let Some(bone) = armature.sel_bone(&sel) {
            drag_tail(events, input, armature, &temp_arm, bone.id, mouse_pos);
        }
        return;
    }

    // dragging an envelope radius handle; the envelope tool doesn't move bones
    if let Some(field) = renderer.envelope_drag {
        if let Some(bone) = armature.sel_bone(&sel) {
            drag_envelope(events, input, &temp_arm, bone.id, field, mouse_pos);
        }
        return;
    }
    if edit_mode.tool == Tool::Envelope {
        return;
    }

    // bone's transforms can't be edited if editing its verts or it's IK,
    // but an exception is made if editing pivot
    let is_editing_pivot = !(ik_rot_lock && ik_pos_lock) || edit_mode.editing_pivot;
    if edit_mode.editing_mesh || !is_editing_pivot {
        return;
    }

    // editing bone transforms (move, rotate, scale)
    let idx = sel.bone_idx;
    let input = &input;
    let mouse_moved = input.mouse != input.mouse_prev_left;
    if camera.on_ui {
        renderer.editing_bone = false;
    } else if idx != usize::MAX && input.left_down && selections.hovering_bone_id == -1 {
        // only register edits if mouse is moving
        if mouse_moved {
            events.update_current_editing(0);
        }

        // prioritize temporary edits (from transform rings) over edit mode
        let current_edit = if edit_mode.temporary == None {
            &edit_mode.current
        } else {
            edit_mode.temporary.as_ref().unwrap()
        };

        // save bone to undo stack
        if !renderer.editing_bone {
            events.save_edited_bone(selections.bone_idx);
            renderer.editing_bone = true;
        }

        let mut line_verts = vec![];
        let mut line_indices = vec![];

        // move all selected (root) bones
        for sel_id in &selections.only_root_bones(&armature.bones) {
            if *current_edit == EditModes::Rotate {
                let mut mouse = utils::screen_to_world_space(input.mouse, camera.window);
                mouse.x *= camera.aspect_ratio();
                let bone = temp_arm.bones.iter().find(|b| b.id == *sel_id).unwrap();
                let center = vert(Some(bone.pos), None, None);
                let cam = &world_camera(&camera, &config);
                let aspect_ratio = camera.aspect_ratio();
                let cw = world_vert(center, cam, aspect_ratio, Vec2::default());
                let color = Color::new(0, 255, 0, 255);
                let (mut verts, mut indices) = draw_line(cw.pos, mouse, color);
                line_verts.append(&mut verts);
                add_offseted_indices(&mut indices, &mut line_indices);
            }
            if !mouse_moved {
                continue;
            }
            let bone = temp_arm.bones.iter().find(|b| b.id == *sel_id).unwrap();
            #[rustfmt::skip]
            edit_bone(
                events, edit_mode, current_edit.clone(), &selections, &camera,
                &config, &input, &renderer, bone, &temp_arm.bones, &armature
            );
        }

        if *current_edit == EditModes::Rotate {
            render_pass.set_bind_group(0, &renderer.generic_bindgroup, &[]);
            setup_render_buffer(&mut renderer.ring_buffer, &line_verts, &line_indices, queue);
            let buffer = &mut renderer.ring_buffer;
            draw(buffer, render_pass, 0, line_indices.len());
        }
    }
}

pub fn draw_armature(
    armature: &Armature,
    src_arm: &Armature,
    showing_mesh: bool,
    sel: &SelectionState,
    queue: &wgpu::Queue,
    render_pass: &mut RenderPass,
    buffer: &RenderBuffer,
) {
    let mut all_verts = vec![];
    let mut all_indices = vec![];

    // keep track of which bones should be rendered
    let mut bone_ids_to_draw = vec![];

    let hiddens = src_arm.get_propagated_hidden();

    for b in 0..armature.bones.len() {
        let tex = src_arm.anim_tex_of(armature.bones[b].id);
        let id = armature.bones[b].id;
        let bone = &armature.bones[b];
        if bone.world_verts.len() == 0 || tex == None || *hiddens.get(&id).unwrap() {
            continue;
        }
        if showing_mesh
            && (src_arm.sel_bone(&sel) != None && src_arm.sel_bone(&sel).unwrap().id == id)
        {
            continue;
        }
        let mut world_verts = bone.world_verts.clone();
        for vert in &mut world_verts {
            vert.color = Color::new(255, 255, 255, 255);
        }
        bone_ids_to_draw.push(armature.bones[b].id);
        all_verts.append(&mut world_verts);
        add_offseted_indices(&mut bone.indices.clone(), &mut all_indices);
    }
    setup_render_buffer(buffer, &all_verts, &all_indices, queue);

    let mut curr_indices = 0;
    for bone_id in bone_ids_to_draw {
        let tex = src_arm.anim_tex_of(bone_id);
        let bone = &armature.bones.iter().find(|b| b.id == bone_id).unwrap();
        let bg = &src_arm.tex_data(tex.unwrap()).unwrap().bind_group;
        let indices_end = curr_indices + bone.indices.len();
        render_pass.set_bind_group(0, bg, &[]);
        draw(&buffer, render_pass, curr_indices, indices_end);
        curr_indices += bone.indices.len();
    }
}

pub fn world_camera(camera: &Camera, config: &Config) -> Camera {
    let mut cam = camera.clone();
    match config.layout {
        UiLayout::Right => cam.pos.x += 1500. * camera.aspect_ratio(),
        UiLayout::Left => cam.pos.x -= 1500. * camera.aspect_ratio(),
        _ => {}
    };
    cam
}

pub fn sel_tex_img(bone: &Bone, armature: &Armature) -> image::DynamicImage {
    let tex = armature.tex_of(bone.id).unwrap();
    armature.tex_data(tex).unwrap().image.clone()
}

pub fn mouse_vel(input: &InputStates, camera: &Camera) -> Vec2 {
    let mouse_world = utils::screen_to_world_space(input.mouse, camera.window);
    let mouse_prev_world = utils::screen_to_world_space(input.mouse_prev, camera.window);
    mouse_prev_world - mouse_world
}

fn vert(pos: Option<Vec2>, col: Option<Color>, uv: Option<Vec2>) -> Vertex {
    Vertex {
        pos: pos.unwrap_or_default(),
        color: col.unwrap_or_default(),
        uv: uv.unwrap_or_default(),
        ..Default::default()
    }
}

fn tri_point(p: &Vec2, a: &Vec2, b: &Vec2, c: &Vec2) -> (f32, f32, f32, f32) {
    let s = a.y * c.x - a.x * c.y + (c.y - a.y) * p.x + (a.x - c.x) * p.y;
    let t = a.x * b.y - a.y * b.x + (a.y - b.y) * p.x + (b.x - a.x) * p.y;

    if (s < 0.0) != (t < 0.0) && s != 0.0 && t != 0.0 {
        return (-1., -1., -1., -1.);
    }

    let area = -b.y * c.x + a.y * (c.x - b.x) + a.x * (b.y - c.y) + b.x * c.y;
    if area == 0.0 {
        return (-1., -1., -1., -1.);
    }

    let s_normalized = s / area;
    let t_normalized = t / area;

    if s_normalized >= 0.0 && t_normalized >= 0.0 && (s_normalized + t_normalized) <= 1.0 {
        let third = 1. - (s_normalized + t_normalized);
        return (area, s_normalized, t_normalized, third);
    }

    (-1., -1., -1., -1.)
}

pub fn get_sprite_boundary(armature: &Armature, camera: &Camera, config: &Config) -> (Vec2, Vec2) {
    let mut temp_arm = Armature::default();
    temp_arm.bones = armature.bones.clone();
    construction(&mut temp_arm.bones, &armature.bones);
    temp_arm.bones.sort_by(|a, b| a.zindex.cmp(&b.zindex));

    let mut cam = world_camera(&camera, &config).clone();
    cam.pos = Vec2::new(0., 0.);
    cam.zoom = 1.;

    let mut left_top = Vec2::new(f32::MAX, -f32::MAX);
    let mut right_bot = Vec2::new(-f32::MAX, f32::MAX);

    for b in 0..temp_arm.bones.len() {
        if armature.tex_of(temp_arm.bones[b].id) == None || temp_arm.bones[b].hidden {
            continue;
        }

        for v in 0..temp_arm.bones[b].vertices.len() {
            let tb = &temp_arm.bones[b];
            let new_vert = world_vert(tb.vertices[v], &cam, 1., Vec2::default());

            let pos = new_vert.pos;
            left_top = Vec2::new(left_top.x.min(pos.x), left_top.y.max(pos.y));
            right_bot = Vec2::new(right_bot.x.max(pos.x), right_bot.y.min(pos.y));
        }
    }

    (left_top, right_bot)
}

/// Stripped-down renderer for screenshot purposes.
pub fn render_screenshot(
    render_pass: &mut RenderPass,
    armature: &mut Armature,
    camera: &Camera,
    renderer: &Renderer,
    queue: &wgpu::Queue,
) {
    let mut temp_arm = Armature::default();
    temp_arm.bones = armature.animated_bones.clone();
    armature.bones = runtime_construction(
        &mut temp_arm.bones,
        &armature.animated_bones,
        armature.bones.clone(),
    );
    temp_arm.bones.sort_by(|a, b| a.zindex.cmp(&b.zindex));
    let sel = SelectionState::default();

    let hiddens = armature.get_propagated_hidden();

    for b in 0..temp_arm.bones.len() {
        let id = &temp_arm.bones[b].id;
        let tex = armature.tex_of(*id);
        let is_hidden = hiddens.get(&id) == None || *hiddens.get(&id).unwrap();
        if tex == None || is_hidden {
            continue;
        }

        for v in 0..temp_arm.bones[b].vertices.len() {
            let tb = &temp_arm.bones[b];
            let final_pivot = utils::rotate(&(tex.unwrap().size * tb.pivot_pos), tb.rot);
            let mut new_vert = world_vert(tb.vertices[v], camera, 1., final_pivot);
            new_vert.tint = temp_arm.bones[b].tint;
            temp_arm.bones[b].world_verts.push(new_vert);
        }
    }
    #[rustfmt::skip]
    draw_armature(&temp_arm, armature, false, &sel, queue, render_pass, &renderer.bone_buffer);
}

/// World transforms of `og_bones` (the animated local pose), with IK applied.
pub fn construction(bones: &mut Vec<Bone>, og_bones: &Vec<Bone>) {
    let ik_rot = ik::solve(og_bones);
    *bones = og_bones.clone();
    inheritance(bones, ik_rot, &vec![]);
    construct_verts(bones);
}

/// `construction`, plus physics.
pub fn runtime_construction(
    bones: &mut Vec<Bone>,
    og_bones: &Vec<Bone>,
    mut armature_bones: Vec<Bone>,
) -> Vec<Bone> {
    let ik_rot = ik::solve(og_bones);
    *bones = og_bones.clone();
    inheritance(bones, ik_rot.clone(), &vec![]);

    simulate_physics(&mut armature_bones, bones);

    // re-construct bones, accounting for physics
    *bones = og_bones.clone();
    inheritance(bones, ik_rot, &armature_bones);

    construct_verts(bones);

    return armature_bones;
}

// simulate physics on the armature, then apply it to constructed bones
fn simulate_physics(armature_bones: &mut Vec<Bone>, constructed_bones: &mut Vec<Bone>) {
    for b in 0..armature_bones.len() {
        let s = Vec2::new(0.3, 0.3);
        let e = Vec2::new(0.6, 0.6);
        let arm_bone = &mut armature_bones[b];
        let const_bone = &constructed_bones[b];
        let prev_pos = arm_bone.phys_global_pos;

        // interpolate position
        if arm_bone.phys_pos_damping > 0. || arm_bone.phys_sway > 0. {
            let phys_pos = &mut arm_bone.phys_global_pos;
            let mut damping = Vec2::new(arm_bone.phys_pos_damping, arm_bone.phys_pos_damping);

            // ratio
            if arm_bone.phys_pos_ratio < 0. {
                damping.y *= 1. - arm_bone.phys_pos_ratio.abs();
            } else if arm_bone.phys_pos_ratio > 0. {
                damping.x *= 1. - arm_bone.phys_pos_ratio;
            }

            phys_pos.x = utils::interp(2, damping.x as i32, phys_pos.x, const_bone.pos.x, s, e);
            phys_pos.y = utils::interp(2, damping.y as i32, phys_pos.y, const_bone.pos.y, s, e);
        }

        // interpolate scale
        if arm_bone.phys_scale_damping > 0. {
            let phys_scale = &mut arm_bone.phys_global_scale;
            let mut damping = Vec2::new(arm_bone.phys_scale_damping, arm_bone.phys_scale_damping);

            // ratio
            if arm_bone.phys_scale_ratio < 0. {
                damping.y *= 1. - arm_bone.phys_scale_ratio.abs();
            } else if arm_bone.phys_scale_ratio > 0. {
                damping.x *= 1. - arm_bone.phys_scale_ratio;
            }

            phys_scale.x =
                utils::interp(2, damping.x as i32, phys_scale.x, const_bone.scale.x, s, e);
            phys_scale.y =
                utils::interp(2, damping.y as i32, phys_scale.y, const_bone.scale.y, s, e);
        }

        // interpolate rotation
        if arm_bone.phys_rot_damping > 0. {
            let rot = utils::shortest_angle_delta(arm_bone.phys_global_rot, const_bone.rot);
            arm_bone.phys_global_rot += rot / arm_bone.phys_rot_damping;
        }

        // interpolate parent orbit (sway, bounce, etc)
        let bones = &constructed_bones;
        let parent = bones.iter().find(|b| b.id == const_bone.parent_id);
        if arm_bone.phys_sway > 0. && parent != None {
            // interpolate bone's orbit to where it would be, in relation to bone's parent.
            // eg;
            // bone is top-left of parent, around 45°.
            // This bone's orbit will interpolate to that angle of 45°.
            // This is only used for swaying (orbit_diff).

            let diff = (const_bone.pos - parent.unwrap().pos).normalize();
            let diff_angle = diff.y.atan2(diff.x);
            let mut rest_rot = shortest_angle_delta(arm_bone.phys_global_orbit, diff_angle);

            // apply bounce
            // eg;
            // re-apply the orbit's rotation from previous frame so it overshoots.
            // each frame it'll get weaker until eventually resting.
            if arm_bone.phys_rot_bounce > 0. && arm_bone.phys_rot_bounce <= 1. {
                rest_rot += arm_bone.phys_global_orbit_vel / (2. - arm_bone.phys_rot_bounce);
                arm_bone.phys_global_orbit_vel = rest_rot;
            }
            arm_bone.phys_global_orbit += rest_rot / 10.;

            // swing orbit based on position momentum
            // eg;
            // moving to the right would have a velocity of (1, 0).
            // this vel will be reversed ((-1, 0)), and its angle will be 180°.
            // the orbit will be influenced towards this angle.
            // The strength of this influence is based on the user's sway value.

            let vel = (arm_bone.phys_global_pos - prev_pos).normalize();
            let angle = (-vel.y).atan2(-vel.x);
            let vel_rot = utils::shortest_angle_delta(arm_bone.phys_global_orbit, angle);
            let strength = (arm_bone.phys_global_pos - prev_pos).mag() / 1000.;
            arm_bone.phys_global_orbit += vel_rot * strength * arm_bone.phys_sway;

            // save the difference in orbit, which will be used later in inheritance()
            arm_bone.phys_global_orbit_diff = diff_angle - arm_bone.phys_global_orbit;
        }
    }
}

pub fn construct_verts(bones: &mut Vec<Bone>) {
    for b in 0..bones.len() {
        let bone = bones[b].clone();

        // track vertex init pos for binds
        let mut init_vert_pos = vec![];
        for vert in &mut bones[b].vertices {
            init_vert_pos.push(vert.pos);
        }

        // move vertex to main bone.
        // this will be overridden if vertex has a bind.
        for vert in &mut bones[b].vertices {
            vert.pos = inherit_vert(vert.pos, &bone, bone.pivot_rot, bone.pivot_scale);
            vert.offset_rot = 0.;
        }

        for bi in 0..bones[b].binds.len() {
            let b_id = bones[b].binds[bi].bone_id;
            if b_id == -1 {
                continue;
            }

            // delete bind if its bind bone doesn't exist
            let bind_bone_raw = bones.iter().find(|bone| bone.id == b_id);
            if bind_bone_raw == None {
                bones[b].binds.remove(bi);
                break;
            }

            let bind_bone = bind_bone_raw.unwrap().clone();
            let bind = bones[b].binds[bi].clone();
            for v_id in 0..bind.verts.len() {
                let id = bind.verts[v_id].id as u32;
                let idx = bones[b].vertices.iter().position(|vert| vert.id == id);

                if !bind.is_path {
                    // weights
                    let vert = &mut bones[b].vertices[idx.unwrap()];
                    let weight = bind.verts[v_id].weight;
                    let end_pos = inherit_vert(
                        init_vert_pos[idx.unwrap()],
                        &bind_bone,
                        bone.pivot_rot,
                        bone.pivot_scale,
                    ) - vert.pos;
                    vert.pos += end_pos * weight;
                    continue;
                }

                // pathing:
                // Bone binds are treated as one continuous line.
                // Vertices will follow along this path.

                // get previous and next bone
                let normal_angle = get_path_normal_angle(bones, &bone, bi);
                if normal_angle == f32::MAX {
                    continue;
                }

                // move vertex to bind bone, then just adjust it to 'bounce' off the line's surface
                let vert = &mut bones[b].vertices[idx.unwrap()];
                vert.pos = init_vert_pos[idx.unwrap()] + bind_bone.pos;
                let rotated = utils::rotate(&(vert.pos - bind_bone.pos), normal_angle);
                vert.pos = bind_bone.pos + (rotated * bind.verts[v_id].weight);
                vert.offset_rot = normal_angle;
            }
        }
    }
}

pub fn get_path_normal_angle(bones: &Vec<Bone>, bone: &Bone, bind_idx: usize) -> f32 {
    let prev = if bind_idx > 0 { bind_idx - 1 } else { bind_idx };
    let next = (bind_idx + 1).min(bone.binds.len() - 1);
    if bone.binds[prev].bone_id == -1 || bone.binds[next].bone_id == -1 {
        return f32::MAX;
    }
    let bind_bone = bones.iter().find(|b| b.id == bone.binds[bind_idx].bone_id);
    let prev_bone = bones.iter().find(|b| b.id == bone.binds[prev].bone_id);
    let next_bone = bones.iter().find(|b| b.id == bone.binds[next].bone_id);

    // get the average of normals between previous bone, this bone, and next bone
    let prev_dir = bind_bone.unwrap().pos - prev_bone.unwrap().pos;
    let next_dir = next_bone.unwrap().pos - bind_bone.unwrap().pos;
    let prev_normal = Vec2::new(-prev_dir.y, prev_dir.x).normalize();
    let next_normal = Vec2::new(-next_dir.y, next_dir.x).normalize();
    let average = prev_normal + next_normal;
    let normal_angle = average.y.atan2(average.x);

    normal_angle
}

pub fn inherit_vert(mut pos: Vec2, bone: &Bone, pivot_rot: f32, pivot_scale: Vec2) -> Vec2 {
    pos *= bone.scale * pivot_scale;
    pos = utils::rotate(&pos, bone.rot + pivot_rot);
    pos += bone.pos;
    pos
}

pub fn edit_bone(
    events: &mut EventState,
    edit_mode: &EditMode,
    current_edit: EditModes,
    selections: &SelectionState,
    camera: &Camera,
    config: &Config,
    input: &InputStates,
    renderer: &Renderer,
    bone: &Bone,
    temp_bones: &Vec<Bone>,
    armature: &Armature,
) {
    let mut anim_id = selections.anim;
    let anim_frame = selections.anim_frame;
    if !edit_mode.anim_open {
        anim_id = usize::MAX;
    }

    macro_rules! edit {
        ($bone:expr, $element:expr, $value:expr, $animate:expr) => {
            if $animate {
                events.edit_bone($bone.id, &$element, $value, "", anim_id, anim_frame);
            } else {
                events.edit_bone($bone.id, &$element, $value, "", usize::MAX, -1);
            }
        };
    }

    let bone_vert = vert(Some(bone.pos), None, None);
    let cam = &world_camera(&camera, &config);
    let bone_center = world_vert(bone_vert, cam, camera.aspect_ratio(), Vec2::default());

    // mouse velocity to be used for moving and scaling
    let mut mouse_vel = mouse_vel(&input, &camera) * camera.zoom;

    // snap mouse velocity to X or Y, depending on which is faster
    let strictness = 3.; // when an axis is triggered, how much should the other be to overrride it?
    let deadzone = 5.; // axis must be faster than this to be triggered
    if edit_mode.holding_edit_snap {
        let norm = Vec2::new(mouse_vel.normalize().x.abs(), mouse_vel.normalize().y.abs());
        if mouse_vel.x.abs() > deadzone && norm.x > norm.y * strictness {
            mouse_vel.y = 0.;
        } else if mouse_vel.y.abs() > deadzone && norm.y > norm.x * strictness {
            mouse_vel.x = 0.;
        } else {
            mouse_vel = Vec2::new(0., 0.);
        }
    }

    // the IK solver owns these (docs/IK.md §4.2); pivots stay editable
    let (rot_locked, pos_locked) = ik::locks(&armature.bones, bone.id);
    if !edit_mode.editing_pivot
        && (current_edit == EditModes::Move && pos_locked || current_edit == EditModes::Rotate && rot_locked)
    {
        return;
    }

    if current_edit == EditModes::Move {
        let mut pos = bone.pos;
        pos -= mouse_vel;

        // restore universal position by offsetting against parents' attributes
        if bone.parent_id != -1 {
            let parent = temp_bones.iter().find(|b| b.id == bone.parent_id).unwrap();
            pos -= parent.pos;
            pos = utils::rotate(&pos, -parent.rot);
            pos /= parent.scale;
            if pos.x.is_nan() {
                pos.x = 0.;
            }
            if pos.y.is_nan() {
                pos.y = 0.;
            }
        }

        let tex = armature.tex_of(bone.id);
        let is_editing_pivot = edit_mode.editing_pivot && tex != None;
        if is_editing_pivot {
            // move texture pivot if holding edit_alt key
            let mut vel = mouse_vel;
            vel = utils::rotate(&vel, -bone.rot);

            let mut pivot = bone.pivot_pos;
            pivot -= vel / tex.unwrap().size;

            if pivot.x != bone.pivot_pos.x {
                edit!(bone, AnimElement::PivotX, pivot.x, false);
            }
            if pivot.y != bone.pivot_pos.y {
                edit!(bone, AnimElement::PivotY, pivot.y, false);
            }
        } else {
            // move bone
            if pos.x != bone.pos.x {
                edit!(bone, AnimElement::PositionX, pos.x, true);
            };
            if pos.y != bone.pos.y {
                edit!(bone, AnimElement::PositionY, pos.y, true);
            }
        }
    } else if current_edit == EditModes::Rotate {
        // remember the initial mouse world position
        let mouse_init = utils::screen_to_world_space(input.mouse_init.unwrap(), camera.window);
        let mut mouse_init_vert = vert(Some(mouse_init), None, None);
        mouse_init_vert.pos.x *= camera.window.y / camera.window.x;
        let dir_init = mouse_init_vert.pos - bone_center.pos;
        let rot_init = dir_init.y.atan2(dir_init.x);

        // create a vertex on the mouse, as a reference for its world space position
        let mouse_pos = utils::screen_to_world_space(input.mouse, camera.window);
        let mut mouse_vert = vert(Some(mouse_pos), None, None);
        mouse_vert.pos.x *= camera.window.y / camera.window.x;
        let dir = mouse_vert.pos - bone_center.pos;
        let rot = dir.y.atan2(dir.x);

        let is_editing_pivot = armature.tex_of(bone.id) != None && edit_mode.editing_pivot;
        if is_editing_pivot {
            // get the target rotation from bone to mouse
            let mut rot = renderer.bone_init_pivot_rot + (rot - rot_init);

            // snap target rotation to user-defined steps if holding snap key
            let step = config.rot_snap_step * 3.14 / 180.;
            if edit_mode.holding_edit_snap {
                rot = (rot / step).round() * step
            }

            // get difference in angle, so that it doesn't lock to 0-360
            let diff = utils::shortest_angle_delta(bone.pivot_rot, rot);

            if rot != bone.rot {
                edit!(bone, AnimElement::PivotRot, bone.pivot_rot + diff, false);
            }
        } else {
            // get the target rotation from bone to mouse
            let mut rot = renderer.bone_init_rot + (rot - rot_init);

            // snap target rotation to user-defined steps if holding snap key
            let step = config.rot_snap_step * 3.14 / 180.;
            if edit_mode.holding_edit_snap {
                rot = (rot / step).round() * step
            }

            // get difference in angle, so that it doesn't lock to 0-360
            let diff = utils::shortest_angle_delta(bone.rot, rot);

            if rot != bone.rot {
                edit!(bone, AnimElement::Rotation, bone.rot + diff, true);
            }
        }
    } else if current_edit == EditModes::Scale {
        let mut scale = if edit_mode.editing_pivot {
            bone.pivot_scale
        } else {
            bone.scale
        };

        // restore universal scale, by offsetting against parent's
        if bone.parent_id != -1 {
            let parent = temp_bones.iter().find(|b| b.id == bone.parent_id).unwrap();
            scale /= parent.scale;
        }

        // maintain aspect ratio (same X and Y scale) if holding edit mod
        if edit_mode.holding_edit_mod {
            let norm = mouse_vel.normalize();
            if norm.x.abs() > norm.y.abs() {
                mouse_vel.y = mouse_vel.x;
            } else {
                mouse_vel.x = mouse_vel.y;
            }
        }

        scale -= mouse_vel / camera.zoom;
        if edit_mode.editing_pivot {
            if scale.x != bone.scale.x {
                edit!(bone, AnimElement::PivotScaleX, scale.x, false);
            }
            if scale.y != bone.scale.y {
                edit!(bone, AnimElement::PivotScaleY, scale.y, false);
            }
        } else {
            if scale.x != bone.scale.x {
                edit!(bone, AnimElement::ScaleX, scale.x, true);
            }
            if scale.y != bone.scale.y {
                edit!(bone, AnimElement::ScaleY, scale.y, true);
            }
        }
    }
}

pub fn inheritance(
    bones: &mut Vec<Bone>,
    ik_rot: std::collections::HashMap<i32, f32>,
    arm_bones: &Vec<Bone>,
) {
    for i in 0..bones.len() {
        let mut parent: Option<Bone> = None;
        for b in 0..bones.len() {
            if bones[b].id == bones[i].parent_id {
                parent = Some(bones[b].clone());
                break;
            }
        }

        // inherit parent
        if let Some(parent) = parent.clone() {
            let mut orbit_rot = parent.rot;

            // apply orbital difference, if rotation sway is active
            if arm_bones.len() > 0 && bones[i].phys_sway > 0. {
                orbit_rot -= bones[i].phys_global_orbit_diff
            }

            if is_facing_left(parent.scale) {
                bones[i].rot = -bones[i].rot;
            }

            bones[i].rot += orbit_rot;
            bones[i].scale *= parent.scale;

            // adjust bone's position based on parent's scale
            bones[i].pos *= parent.scale;

            bones[i].pos = utils::rotate(&bones[i].pos, orbit_rot);

            // inherit position from parent
            bones[i].pos += parent.pos;
        }

        // apply rotations from IK, if provided
        let ik_rot = ik_rot.get(&bones[i].id);
        if ik_rot != None {
            bones[i].rot = *ik_rot.unwrap();
        }

        // apply physics, if armature_bones is provided
        if arm_bones.len() > 0 {
            if bones[i].phys_rot_damping > 0. {
                bones[i].rot = arm_bones[i].phys_global_rot;
            }
            if bones[i].phys_pos_damping > 0. {
                bones[i].pos = arm_bones[i].phys_global_pos;
            }
            if bones[i].phys_scale_damping > 0. {
                bones[i].scale = arm_bones[i].phys_global_scale;
            }
        }
    }
}

pub fn draw(
    buffer: &RenderBuffer,
    render_pass: &mut RenderPass,
    indices_start: usize,
    indices_end: usize,
) {
    render_pass.set_vertex_buffer(0, buffer.vertex.as_ref().unwrap().slice(..));
    let slice = buffer.index.as_ref().unwrap().slice(..);
    render_pass.set_index_buffer(slice, wgpu::IndexFormat::Uint32);
    render_pass.draw_indexed(indices_start as u32..indices_end as u32, 0, 0..1);
}

pub fn bone_vertices(
    world_verts: &Vec<Vertex>,
    editable: bool,
    selections: &SelectionState,
    input: &InputStates,
    camera: &Camera,
    config: &Config,
    events: &mut EventState,
    armature: &Armature,
    renderer: &mut Renderer,
    hover_id: i32,
) -> (Vec<Vertex>, Vec<u32>, i32) {
    let mut all_verts = vec![];
    let mut all_indices = vec![];
    let mut hovering_vert_id = -1;
    let v2z = Vec2::ZERO;
    let sel = selections.clone();
    let radius = config.center_point_radius * config.mesh_vert_size.clamp(0.1, 3.);

    #[rustfmt::skip]
    macro_rules! point {
        ($idx:expr, $color:expr, $size:expr, $rot:expr) => {
            draw_point(&world_verts[$idx].pos, &camera, &config, &v2z, $color, v2z, $rot, radius * camera.zoom * $size)
        };
    }
    macro_rules! add_point {
        ($verts:expr, $indices:expr, $wv:expr) => {
            let base = all_verts.len() as u32;
            for i in &mut $indices {
                *i += base;
            }
            all_verts.append(&mut $verts);
            all_indices.append(&mut $indices);
        };
    }

    // the selected bind's vertices, by id: (row in the bind panel, weight)
    let mut bind_rows: std::collections::HashMap<i32, (usize, f32)> = Default::default();
    let bind_idx = selections.bind as usize;
    if selections.bind != -1 {
        if let Some(bind) = armature.sel_bone(&sel).and_then(|bone| bone.binds.get(bind_idx)) {
            for (row, v) in bind.verts.iter().enumerate() {
                bind_rows.insert(v.id, (row, v.weight));
            }
        }
    }
    let white = Color::new(255, 255, 255, 255);

    for wv in 0..world_verts.len() {
        let size = if world_verts[wv].id == hover_id as u32 {
            1.5
        } else {
            1.
        };

        let mut rot = 45. * 3.14 / 180.;

        // square vertex if bound: colored by bind row and weight, or the single
        // bound color (docs/MESH_DISPLAY.md §3.1)
        let bind_row = bind_rows.get(&(world_verts[wv].id as i32)).copied();
        let weight_colored = bind_row.is_some() && config.bind_weight_colors;
        let mut col = match bind_row {
            Some((row, weight)) => {
                rot = 0.;
                if config.bind_weight_colors {
                    mesh_tools::bind_vert_color(row, weight)
                } else {
                    config.colors.bound_vert
                }
            }
            None => config.colors.mesh_base,
        };

        // selected: white, or a white outline around a weight-colored vertex (§3.5)
        let selected = selections.vert_ids.contains(&(world_verts[wv].id as usize));
        if selected && weight_colored {
            let (mut verts, mut indices) = point!(wv, white, size * 1.45, rot);
            add_point!(verts, indices, wv);
        } else if selected {
            col = white;
        }
        if !selected {
            col.a = if editable { 200 } else { 38 };
        }

        let (mut verts, mut indices) = point!(wv, col, size, rot);
        let mouse_on_it = utils::in_bounding_box(&input.mouse, &verts, &camera.window).1;
        if mouse_on_it {
            hovering_vert_id = world_verts[wv].id as i32;
        }

        if camera.on_ui || !mouse_on_it || !editable {
            add_point!(verts, indices, wv);
            continue;
        }

        // mouse hovering on this vert

        let (mut verts, mut indices) = point!(wv, col, size, rot);
        add_point!(verts, indices, wv);
        if input.right_clicked {
            events.delete_vertex(wv);
            break;
        }

        if input.left_pressed {
            events.select_vertex(world_verts[wv].id as i32, false);
        }

        if input.left_clicked {
            // simulate double-click for binding verts
            if renderer.clicked_vert_id != world_verts[wv].id as i32 {
                renderer.clicked_vert_id = world_verts[wv].id as i32;
            } else if selections.bind != -1 {
                renderer.clicked_vert_id = -1;
                events.click_vertex(wv);
                events.select_vertex(-1, false);
            }
            break;
        }
    }

    (all_verts, all_indices, hovering_vert_id)
}

/// Eraser tool on the selected mesh (docs/TOPOLOGY_TOOLS.md §6): highlights the
/// vertex or edge under the cursor and erases what a held left button passes over.
/// Returns the highlight's geometry.
fn eraser(
    bone: &Bone,
    world_verts: &Vec<Vertex>,
    mouse: &Vertex,
    input: &InputStates,
    camera: &Camera,
    config: &Config,
    edit_mode: &EditMode,
    renderer: &mut Renderer,
    events: &mut EventState,
) -> (Vec<Vertex>, Vec<u32>) {
    if !input.left_down {
        renderer.erasing = false;
    }
    if camera.on_ui {
        return (vec![], vec![]);
    }

    // a stroke starts on press; its first erase takes the undo snapshot
    if input.left_pressed {
        renderer.erasing = true;
        renderer.erase_last = None;
    }

    let pick = mesh_tools::pick(world_verts, &bone.indices, mouse.pos, camera.window);
    let Some(pick) = pick else {
        return (vec![], vec![]);
    };

    if renderer.erasing {
        let spacing = mesh_tools::ERASE_SPACING_PX;
        let moved = renderer.erase_last.map_or(true, |last| (input.mouse - last).mag() > spacing);
        if moved {
            if renderer.erase_last == None {
                events.erase_start();
            }
            // Ctrl swaps dissolve and delete for this stroke
            let delete = edit_mode.eraser_delete != input.holding_mod;
            renderer.erase_pick = Some((pick, delete));
            renderer.erase_last = Some(input.mouse);
            events.erase_mesh();
        }
    }

    // highlight what would be erased
    let red = Color::new(230, 60, 60, 255);
    match pick {
        mesh_tools::Pick::Vertex(v) => {
            let pos = world_verts[v as usize].pos;
            let size = config.center_point_radius * camera.zoom * 1.8;
            let v2z = Vec2::ZERO;
            draw_point(&pos, camera, config, &v2z, red, v2z, 45. * 3.14 / 180., size)
        }
        mesh_tools::Pick::Edge(i, j) => {
            let (v0, v1) = (world_verts[i as usize], world_verts[j as usize]);
            let dir = v0.pos - v1.pos;
            let base = utils::rotate(&Vec2::new(0.006, 0.006), dir.y.atan2(dir.x));
            #[rustfmt::skip]
            macro_rules! vert { ($pos:expr) => { Vertex { pos: $pos, color: red, ..v0 } }; }
            let verts = vec![vert!(v0.pos + base), vert!(v0.pos - base), vert!(v1.pos + base), vert!(v1.pos - base)];
            (verts, vec![0, 1, 2, 1, 2, 3])
        }
    }
}

/// A thick line from `a` to `b` in renderer space, as two triangles.
fn segment_quad(a: Vec2, b: Vec2, width: f32, color: Color) -> (Vec<Vertex>, Vec<u32>) {
    let dir = a - b;
    let base = utils::rotate(&Vec2::new(width, width), dir.y.atan2(dir.x));
    #[rustfmt::skip]
    macro_rules! v { ($pos:expr) => { Vertex { pos: $pos, color, ..Default::default() } }; }
    (vec![v!(a + base), v!(a - base), v!(b + base), v!(b - base)], vec![0, 1, 2, 1, 2, 3])
}

/// Pen tool on the selected mesh (docs/TOPOLOGY_TOOLS.md §5): places points on
/// click, ends a cut on right-click, and draws the cuts, the segment to the
/// cursor, the snap target, and a preview of the faces Enter would add.
fn pen_tool(
    bone: &Bone,
    sel_bone: &Bone,
    armature: &Armature,
    world_verts: &Vec<Vertex>,
    mouse: &Vertex,
    input: &InputStates,
    camera: &Camera,
    config: &Config,
    edit_mode: &EditMode,
    renderer: &mut Renderer,
) -> (Vec<Vertex>, Vec<u32>) {
    use mesh_tools::PenTarget;
    let map = mesh_tools::UvMap::new(world_verts, &bone.indices);
    let point_pos = |p: &pen::PenPoint| mesh_tools::pen_point_pos(p, world_verts, &map);

    // the cut being drawn, in renderer space
    let active: Vec<Vec2> = match (renderer.pen_open, renderer.pen_cuts.last()) {
        (true, Some(cut)) => cut.points.iter().filter_map(|p| point_pos(p)).collect(),
        _ => vec![],
    };
    let snapping = mesh_tools::PenSnapping {
        snap: !edit_mode.pen_no_snap && !input.holding_shift,
        midpoint: input.holding_mod,
        angle_lock: edit_mode.pen_angle_lock,
    };
    let target = if camera.on_ui {
        None
    } else {
        #[rustfmt::skip]
        let t = mesh_tools::pen_target(world_verts, &bone.indices, &map, mouse.pos, camera.window, &active, snapping);
        t
    };

    // clicks place points; right-click ends the cut being drawn
    if !camera.on_ui && input.left_clicked {
        match target {
            Some(PenTarget::Close) => {
                renderer.pen_cuts.last_mut().unwrap().closed = true;
                renderer.pen_open = false;
            }
            Some(PenTarget::Finish) => renderer.pen_open = false,
            Some(PenTarget::Place(point, _)) => {
                if !renderer.pen_open {
                    renderer.pen_cuts.push(pen::PenCut::default());
                    renderer.pen_open = true;
                }
                renderer.pen_cuts.last_mut().unwrap().points.push(point);
            }
            None => {}
        }
        renderer.pen_rev += 1;
    }
    if !camera.on_ui && input.right_clicked && renderer.pen_open {
        renderer.pen_open = false;
        renderer.pen_rev += 1;
    }

    // the result of applying the cuts as they are, cached until they change
    let key = (renderer.pen_rev, sel_bone.id, sel_bone.vertices.len(), sel_bone.indices.len());
    if renderer.pen_preview.as_ref().map_or(true, |(k, ..)| *k != key) {
        renderer.pen_preview = None;
        if !renderer.pen_cuts.is_empty() {
            let mut result = sel_bone.clone();
            let tex_size = armature.tex_of(sel_bone.id).map(|t| t.size).unwrap_or_default();
            if let Ok(report) = pen::apply_cuts(&mut result, &renderer.pen_cuts, tex_size) {
                renderer.pen_preview = Some((key, result, report));
            }
        }
    }

    let mut verts: Vec<Vertex> = vec![];
    let mut indices: Vec<u32> = vec![];
    macro_rules! add {
        ($geo:expr) => {{
            let (mut v, mut i) = $geo;
            let offset = verts.len() as u32;
            for idx in &mut i {
                *idx += offset;
            }
            verts.append(&mut v);
            indices.append(&mut i);
        }};
    }

    // preview: the faces Enter would add
    let mut dropped = vec![];
    if let Some((_, result, report)) = &renderer.pen_preview {
        let key = |ids: [u32; 3]| {
            let mut k = ids;
            k.sort();
            k
        };
        let ids = |b: &Bone, t: &[u32]| key([0, 1, 2].map(|k| b.vertices[t[k] as usize].id));
        let before: Vec<[u32; 3]> = sel_bone.indices.chunks_exact(3).map(|t| ids(sel_bone, t)).collect();
        let fill = Color::new(80, 210, 120, 70);
        for t in result.indices.chunks_exact(3) {
            if before.contains(&ids(result, t)) {
                continue;
            }
            let corners: Vec<Vec2> =
                t.iter().filter_map(|i| map.pos_of(result.vertices[*i as usize].uv)).collect();
            if corners.len() == 3 {
                let tri = corners.iter().map(|p| Vertex { pos: *p, color: fill, ..Default::default() });
                add!((tri.collect::<Vec<_>>(), vec![0, 1, 2]));
            }
        }
        dropped = report.dropped.clone();
    }

    // the cuts: segments that will produce nothing are drawn dim red
    let width = 0.004;
    let live = Color::new(240, 200, 60, 255);
    let dead = Color::new(230, 60, 60, 150);
    let point_size = config.center_point_radius * camera.zoom * 1.2;
    let v2z = Vec2::ZERO;
    for (c, cut) in renderer.pen_cuts.iter().enumerate() {
        let n = cut.points.len();
        let mut pairs: Vec<(usize, usize)> = (1..n).map(|k| (k - 1, k)).collect();
        if cut.closed && n > 2 {
            pairs.push((n - 1, 0));
        }
        for (a, b) in pairs {
            let (Some(pa), Some(pb)) = (point_pos(&cut.points[a]), point_pos(&cut.points[b])) else {
                continue;
            };
            let gone = dropped.contains(&(c, a)) || dropped.contains(&(c, b));
            add!(segment_quad(pa, pb, width, if gone { dead } else { live }));
        }
        for (k, p) in cut.points.iter().enumerate() {
            if let Some(pos) = point_pos(p) {
                let col = if dropped.contains(&(c, k)) { dead } else { live };
                add!(draw_point(&pos, camera, config, &v2z, col, v2z, 0., point_size));
            }
        }
    }

    // the segment to the cursor, and what a click would hit
    let white = Color::new(255, 255, 255, 220);
    match target {
        Some(PenTarget::Place(point, pos)) => {
            if let Some(last) = active.last() {
                add!(segment_quad(*last, pos, width * 0.6, Color::new(255, 255, 255, 140)));
            }
            let col = match point.snap {
                pen::Snap::Vertex(_) => white,
                pen::Snap::Edge => Color::new(80, 200, 255, 255),
                pen::Snap::Free => live,
            };
            add!(draw_point(&pos, camera, config, &v2z, col, v2z, 45. * 3.14 / 180., point_size * 1.4));
        }
        Some(PenTarget::Close) => {
            let green = Color::new(80, 230, 120, 255);
            add!(draw_point(&active[0], camera, config, &v2z, green, v2z, 0., point_size * 2.));
        }
        Some(PenTarget::Finish) => {
            let last = *active.last().unwrap();
            add!(draw_point(&last, camera, config, &v2z, white, v2z, 0., point_size * 2.));
        }
        None => {}
    }

    (verts, indices)
}

fn bone_triangle(tb: &Bone, mouse_world_vert: &Vertex, wv: Vec<Vertex>) -> (u32, Vec<Vertex>) {
    let mut hovering_tri = vec![];
    let mut idx: usize = 0;
    for (i, chunk) in tb.indices.chunks_exact(3).enumerate() {
        let c0 = chunk[0] as usize;
        let c1 = chunk[1] as usize;
        let c2 = chunk[2] as usize;

        let bary = tri_point(&mouse_world_vert.pos, &wv[c0].pos, &wv[c1].pos, &wv[c2].pos);
        if bary.0 == -1. {
            continue;
        }

        hovering_tri.push(tb.world_verts[tb.indices[i * 3 + 0] as usize]);
        hovering_tri.push(tb.world_verts[tb.indices[i * 3 + 1] as usize]);
        hovering_tri.push(tb.world_verts[tb.indices[i * 3 + 2] as usize]);
        idx = i;
    }
    (idx as u32, hovering_tri)
}

/// Mesh edges, drawn per docs/MESH_DISPLAY.md §2.
pub struct EdgeGeometry {
    /// Outline and normal edges, in `config.mesh_edge_style`: draw with the
    /// matching pattern bind group (`edge_bindgroup`), before everything else
    pub patterned: (Vec<Vertex>, Vec<u32>),
    /// Hovered and selected edges, always solid: draw with the generic bind group
    pub solid: (Vec<Vertex>, Vec<u32>),
    pub hovered: bool,
}

/// The bind group for `style`'s pattern; solid uses the plain white texture.
pub fn edge_bindgroup(renderer: &Renderer, style: EdgeStyle) -> &BindGroup {
    let patterns = renderer.edge_pattern_bindgroups.as_ref();
    match (style, patterns) {
        (EdgeStyle::Dotted, Some(p)) => &p[0],
        (EdgeStyle::Dashed, Some(p)) => &p[1],
        _ => renderer.generic_bindgroup.as_ref().unwrap(),
    }
}

/// The repeating pattern for `style`, one texel per screen px: opaque white
/// for the "on" part, transparent for the gap.
pub fn edge_pattern_pixels(style: EdgeStyle) -> (Vec<u8>, u32) {
    let (on, off) = style.pattern().unwrap_or((1, 0));
    let mut px = vec![];
    for i in 0..on + off {
        px.extend(if i < on { [255, 255, 255, 255] } else { [255, 255, 255, 0] });
    }
    (px, on + off)
}

pub fn vert_lines(
    bone: &Bone,
    bones: &Vec<Bone>,
    mouse_world_vert: &Vertex,
    new_vert: &mut Option<(Vertex, VertSite)>,
    editable: bool,
    hovering_vert: bool,
    line_color: Color,
    camera: &Camera,
    input: &InputStates,
    sel: &SelectionState,
    events: &mut EventState,
    config: &Config,
) -> EdgeGeometry {
    let window = camera.window;
    let edges = topology::edges(&bone.indices);
    let wv = &bone.world_verts;

    // hit-testing is by screen distance, separate from how thin the line is drawn
    let mut hovered = None;
    if editable && !hovering_vert && !camera.on_ui {
        hovered = mesh_tools::pick_edge(wv, &edges, mouse_world_vert.pos, window, camera.px());
    }

    if let Some((i0, i1, t)) = hovered {
        if input.left_pressed {
            let verts = &bone.vertices;
            events.select_vertex(verts[i0 as usize].id as i32, false);
            events.select_vertex(verts[i1 as usize].id as i32, true);
        } else if input.left_clicked {
            let (v0, v1) = (wv[i0 as usize], wv[i1 as usize]);
            let uv = v0.uv + (v1.uv - v0.uv) * t;
            let v = &bones.iter().find(|b| b.id == bone.id).unwrap().vertices;
            let lv0 = utils::rotate(&(v[i0 as usize].pos - bone.pos), -bone.rot);
            let lv1 = utils::rotate(&(v[i1 as usize].pos - bone.pos), -bone.rot);
            let pos = lv0 + (lv1 - lv0) * t;
            let site = VertSite::Edge(i0, i1);
            *new_vert = Some((vert(Some(pos / bone.scale), None, Some(uv)), site));
        }
    }

    // widths and the pattern are in UI points, so they match the UI on high-DPI displays
    let px = camera.px();
    let width = config.mesh_edge_width.clamp(1., 4.) * px;
    let style = config.mesh_edge_style;
    let pattern_px = style.pattern().map_or(1., |(on, off)| (on + off) as f32) * px;
    let mut outline_color = Color::new(0, 0, 0, (line_color.a as f32 * 0.35) as u8);
    let bright = Color::new(50, 50, 50, 0);

    let mut outlines = (vec![], vec![]);
    let mut lines = (vec![], vec![]);
    let mut solid = (vec![], vec![]);
    let push = |dst: &mut (Vec<Vertex>, Vec<u32>), quad: [Vertex; 4]| {
        let base = dst.0.len() as u32;
        dst.0.extend(quad);
        dst.1.extend([0, 1, 2, 1, 2, 3].map(|i| i + base));
    };

    let selected = &sel.vert_ids;
    for (i0, i1) in edges {
        let (a, b) = (wv[i0 as usize].pos, wv[i1 as usize].pos);
        let is_hovered = hovered.map_or(false, |(h0, h1, _)| (h0, h1) == (i0, i1));
        let is_selected = editable
            && selected.len() == 2
            && ((selected[0], selected[1]) == (i0 as usize, i1 as usize)
                || (selected[0], selected[1]) == (i1 as usize, i0 as usize));

        if config.mesh_edge_outline {
            outline_color.a = (line_color.a as f32 * 0.35) as u8;
            push(&mut outlines, mesh_tools::edge_quad(a, b, width + 2. * px, pattern_px, outline_color, window));
        }
        if is_hovered || is_selected {
            let mut col = line_color;
            col.a = 255;
            let mut quad = mesh_tools::edge_quad(a, b, width + px, 1., col, window);
            for v in &mut quad {
                v.add_color = bright;
            }
            push(&mut solid, quad);
        } else {
            push(&mut lines, mesh_tools::edge_quad(a, b, width, pattern_px, line_color, window));
        }
    }

    // outlines go under every line
    let mut patterned = outlines;
    let base = patterned.0.len() as u32;
    patterned.0.extend(lines.0);
    patterned.1.extend(lines.1.into_iter().map(|i| i + base));

    EdgeGeometry { patterned, solid, hovered: hovered.is_some() }
}

fn draw_line(origin: Vec2, target: Vec2, color: Color) -> (Vec<Vertex>, Vec<u32>) {
    let dir = target - origin;

    let width = 2.5;
    let mut base = Vec2::new(width, width) / 1000.;
    base = utils::rotate(&base, dir.y.atan2(dir.x) + (45. * 3.14 / 180.));

    macro_rules! vert {
        ($pos:expr) => {
            vert(Some($pos), Some(color), None)
        };
    }

    let v0_top = vert!(origin - base);
    let v0_bot = vert!(origin + base);
    let v1_top = vert!(target - base);
    let v1_bot = vert!(target + base);

    let verts = vec![v0_top, v0_bot, v1_top, v1_bot];
    let indices = vec![0, 1, 2, 1, 2, 3];

    (verts, indices)
}

pub fn create_tex_rect(tex_size: &Vec2) -> (Vec<Vertex>, Vec<u32>) {
    #[rustfmt::skip]
    macro_rules! vert {
        ($pos:expr, $uv:expr, $id:expr) => { Vertex { pos: $pos, uv: $uv, id: $id, ..Default::default() } };
    }
    let tex = *tex_size / 2.;
    let mut verts = vec![
        vert!(Vec2::new(-tex.x, tex.y), Vec2::new(0., 0.), 0),
        vert!(Vec2::new(tex.x, tex.y), Vec2::new(1., 0.), 1),
        vert!(Vec2::new(tex.x, -tex.y), Vec2::new(1., 1.), 2),
        vert!(Vec2::new(-tex.x, -tex.y), Vec2::new(0., 1.), 3),
    ];
    verts = editor::sort_vertices(verts.clone());
    let indices = vec![0, 1, 2, 0, 2, 3];
    (verts, indices)
}

fn draw_point(
    offset: &Vec2,
    camera: &Camera,
    config: &Config,
    pos: &Vec2,
    color: Color,
    camera_pos: Vec2,
    rotation: f32,
    size: f32,
) -> (Vec<Vertex>, Vec<u32>) {
    let point_size = size;
    macro_rules! vert {
        ($pos:expr, $uv:expr) => {
            vert(Some($pos), Some(color), Some($uv))
        };
    }
    let verts: [Vertex; 4] = [
        vert!(Vec2::new(-point_size, point_size), Vec2::new(0., 1.)),
        vert!(Vec2::new(point_size, point_size), Vec2::new(1., 1.)),
        vert!(Vec2::new(-point_size, -point_size), Vec2::new(0., 0.)),
        vert!(Vec2::new(point_size, -point_size), Vec2::new(1., 0.)),
    ];

    draw_rect(verts, offset, camera, config, pos, camera_pos, rotation)
}

fn draw_flow_kite(
    offset: &Vec2,
    camera: &Camera,
    config: &Config,
    pos: &Vec2,
    color: Color,
    camera_pos: Vec2,
    rotation: f32,
    width: f32,
) -> (Vec<Vertex>, Vec<u32>) {
    let height = 20.;
    macro_rules! vert {
        ($pos:expr, $uv:expr) => {
            vert(Some($pos), Some(color), Some($uv))
        };
    }
    let verts: [Vertex; 4] = [
        vert!(Vec2::new(-1., height), Vec2::new(0., 1.)),
        vert!(Vec2::new(width, height), Vec2::new(0., 0.)),
        vert!(Vec2::new(-1., -height), Vec2::new(1., 1.)),
        vert!(Vec2::new(width, -height), Vec2::new(1., 0.)),
    ];

    draw_rect(verts, offset, camera, config, pos, camera_pos, rotation)
}

fn draw_rect(
    mut temp_verts: [Vertex; 4],
    offset: &Vec2,
    camera: &Camera,
    config: &Config,
    pos: &Vec2,
    camera_pos: Vec2, // set to (0, 0) for vertex points
    rotation: f32,
) -> (Vec<Vertex>, Vec<u32>) {
    for v in &mut temp_verts {
        v.pos = utils::rotate(&v.pos, rotation);
        v.pos += *pos;
    }

    let mut point_verts = vec![];
    let ar = camera.aspect_ratio();
    let mut cam = world_camera(&camera, &config).clone();
    cam.pos = camera_pos;
    let pivot = Vec2::new(0., 0.);
    for vert in temp_verts {
        let vert = world_vert(vert, &cam, ar, pivot);
        point_verts.push(vert);
    }

    for vert in &mut point_verts {
        vert.pos += *offset;
    }

    (point_verts, vec![0, 1, 2, 1, 2, 3])
}

/// Get bind group of a texture.
pub fn create_texture_bind_group(
    pixels: Vec<u8>,
    dimensions: Vec2,
    queue: &Queue,
    device: &Device,
    bind_group_layout: &BindGroupLayout,
) -> BindGroup {
    let clamp = wgpu::AddressMode::ClampToEdge;
    texture_bind_group(pixels, dimensions, clamp, queue, device, bind_group_layout)
}

/// A 1-texel-high pattern `width` texels long that repeats along u (edge
/// patterns, docs/MESH_DISPLAY.md §2.2).
pub fn create_pattern_bind_group(
    pixels: Vec<u8>,
    width: u32,
    queue: &Queue,
    device: &Device,
    bind_group_layout: &BindGroupLayout,
) -> BindGroup {
    let dims = Vec2::new(width as f32, 1.);
    texture_bind_group(pixels, dims, wgpu::AddressMode::Repeat, queue, device, bind_group_layout)
}

fn texture_bind_group(
    pixels: Vec<u8>,
    dimensions: Vec2,
    address_mode_u: wgpu::AddressMode,
    queue: &Queue,
    device: &Device,
    bind_group_layout: &BindGroupLayout,
) -> BindGroup {
    let tex_size = wgpu::Extent3d {
        width: dimensions.x as u32,
        height: dimensions.y as u32,
        depth_or_array_layers: 1,
    };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        size: tex_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        label: Some("diffuse_texture"),
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * dimensions.x as u32),
            rows_per_image: Some(dimensions.y as u32),
        },
        tex_size,
    );

    let tex_view = tex.create_view(&wgpu::TextureViewDescriptor::default());

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        compare: None,
        ..Default::default()
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&tex_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
        label: Some("diffuse_bind_group"),
    });

    bind_group
}

pub fn index_buffer(indices: Vec<u32>, device: &Device) -> wgpu::Buffer {
    wgpu::util::DeviceExt::create_buffer_init(
        device,
        &wgpu::util::BufferInitDescriptor {
            label: Some("index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        },
    )
}

pub fn vertex_buffer(vertices: &Vec<Vertex>, device: &Device) -> wgpu::Buffer {
    let gpu_verts: Vec<GpuVertex> = vertices.iter().map(|vert| (*vert).into()).collect();

    wgpu::util::DeviceExt::create_buffer_init(
        device,
        &wgpu::util::BufferInitDescriptor {
            label: Some("index Buffer"),
            contents: bytemuck::cast_slice(&gpu_verts),
            usage: wgpu::BufferUsages::VERTEX,
        },
    )
}

pub fn world_vert(mut vert: Vertex, camera: &Camera, aspect_ratio: f32, pivot: Vec2) -> Vertex {
    vert.pos.x += pivot.x;
    vert.pos.y += pivot.y;

    // offset bone with camera
    vert.pos -= camera.pos;

    // adjust for zoom level
    vert.pos /= camera.zoom;

    // adjust verts for aspect ratio
    vert.pos.x *= aspect_ratio;

    vert
}

fn draw_gridline(
    render_pass: &mut RenderPass,
    renderer: &mut Renderer,
    camera: &Camera,
    config: &Config,
    queue: &Queue,
) {
    render_pass.set_bind_group(0, &renderer.generic_bindgroup, &[]);

    let cam = world_camera(camera, config);

    let col = Color::new(
        config.colors.gridline.r,
        config.colors.gridline.g,
        config.colors.gridline.b,
        255,
    );

    let width = 0.005 * cam.zoom;
    let regular_color = Color::new(col.r, col.g, col.b, 38);
    let highlight_color = Color::new(col.r, col.g, col.b, 255);

    let mut verts = vec![];
    let mut indices: Vec<u32> = vec![];
    let mut i: u32 = 0;

    // draw vertical lines
    let mut x = (cam.pos.x - cam.zoom / camera.aspect_ratio()).round();
    x = -x.abs();

    // snap to first line
    x += (x % config.gridline_gap as f32).abs();
    let right_side = cam.pos.x + cam.zoom / camera.aspect_ratio();
    while x < right_side {
        let color = if x == 0. {
            highlight_color
        } else {
            regular_color
        };
        verts.append(&mut draw_vertical_line(x, width, camera, config, color));
        indices.append(&mut vec![i, i + 1, i + 2]);
        i += 3;
        x += config.gridline_gap as f32;
    }

    // draw horizontal lines
    let mut y = (cam.pos.y - cam.zoom).round();
    y = -y.abs();

    // snap to first line
    y += (y % config.gridline_gap as f32).abs();
    let top_side = cam.pos.y + cam.zoom;
    while y < top_side {
        let color = if y == 0. {
            highlight_color
        } else {
            regular_color
        };
        verts.append(&mut draw_horizontal_line(y, width, camera, config, color));
        indices.append(&mut vec![i, i + 1, i + 2]);
        i += 3;
        y += config.gridline_gap as f32;
    }

    if verts.len() == 0 {
        return;
    }

    setup_render_buffer(&mut renderer.gridline_buffer, &verts, &indices, queue);
    draw(&renderer.gridline_buffer, render_pass, 0, indices.len());
}

macro_rules! vert {
    ($pos:expr, $color:expr) => {
        Vertex {
            pos: $pos,
            color: $color,
            ..Default::default()
        }
    };
}

pub fn draw_horizontal_line(
    y: f32,
    width: f32,
    camera: &Camera,
    config: &Config,
    color: Color,
) -> Vec<Vertex> {
    let edge = camera.zoom * 5.;
    let c = &world_camera(camera, config);
    let vertices: Vec<Vertex> = vec![
        vert!((Vec2::new(c.pos.x - edge, y) - c.pos) / c.zoom, color),
        vert!((Vec2::new(c.pos.x, width + y) - c.pos) / c.zoom, color),
        vert!((Vec2::new(c.pos.x + edge, y) - c.pos) / c.zoom, color),
    ];
    vertices
}

pub fn draw_vertical_line(
    x: f32,
    width: f32,
    camera: &Camera,
    config: &Config,
    color: Color,
) -> Vec<Vertex> {
    let edge = camera.zoom * 5.;
    let c = &world_camera(camera, config);
    let r = camera.aspect_ratio();
    let vertices: Vec<Vertex> = vec![
        vert!((Vec2::new(x, c.pos.y - edge) - c.pos) / c.zoom * r, color),
        vert!((Vec2::new(width + x, c.pos.y) - c.pos) / c.zoom * r, color),
        vert!((Vec2::new(x, c.pos.y + edge) - c.pos) / c.zoom * r, color),
    ];
    vertices
}

pub fn add_offseted_indices(src: &mut Vec<u32>, dst: &mut Vec<u32>) {
    if dst.len() == 0 {
        dst.append(src);
        return;
    }
    let mut highest = 0;
    for s in &mut *dst {
        highest = highest.max(*s);
    }
    highest += 1;
    for idx in &mut *src {
        *idx += highest;
    }
    dst.append(src);
}

/// Whether a bone is drawn as an attachment (see `Config::attachment_display`): any bone
/// showing a texture, so the art reads separately from the rig.
fn is_attachment(config: &Config, armature: &Armature, bone: &Bone) -> bool {
    config.attachment_display && armature.tex_of(bone.id).is_some()
}

/// Color of an attachment's dot/line: full when selected, faded otherwise.
fn attachment_color(config: &Config, selected: bool) -> Color {
    let mut color = config.colors.attachment_point;
    if !selected {
        color.a /= 2;
    }
    color
}

pub fn draw_points(
    config: &Config,
    camera: &Camera,
    input: &InputStates,
    edit_mode: &EditMode,
    temp_arm: &mut Armature,
    sel_bone_ids: Vec<i32>,
    selections: &SelectionState,
    renderer: &mut Renderer,
    queue: &wgpu::Queue,
    render_pass: &mut RenderPass,
    events: &mut EventState,
    armature: &Armature,
) {
    let cam = world_camera(&camera, &config);
    let zero = Vec2::default();
    let mut point_verts = vec![];
    let mut point_indices = vec![];
    let mut on_point = false;

    for p in 0..temp_arm.bones.len() {
        let bone = &temp_arm.bones[p];
        let mut color;

        if bone.bind_owner.is_some() {
            continue;
        }

        if edit_mode.editing_mesh && !sel_bone_ids.contains(&bone.id) {
            continue;
        }

        // skip points if they shouldn't be rendered, and this isn't a selected bone
        if !renderer.render_points && !selections.bone_ids.contains(&bone.id) {
            continue;
        }

        if bone.group_color.a == 0 {
            color = config.colors.inactive_center_point;
            if sel_bone_ids.contains(&bone.id) {
                color = config.colors.center_point
            }
        } else {
            color = bone.group_color.into();
            if !sel_bone_ids.contains(&bone.id) {
                color.a /= 2;
            }
        }

        let attachment = is_attachment(config, temp_arm, bone);
        if attachment {
            color = attachment_color(config, sel_bone_ids.contains(&bone.id));
        }

        // play shrinking animation if this bone was just selected
        let fade_speed = 0.1;
        let sel_size = config.center_point_radius * 4.;
        let normal_size = if attachment {
            config.center_point_radius * 0.6
        } else {
            config.center_point_radius
        };
        let elapsed = if selections.bone_ids.len() > 1 && selections.bone_ids.contains(&bone.id) {
            (sel_size - edit_mode.sel_time * fade_speed).max(normal_size)
        } else {
            normal_size
        } * camera.zoom;

        let (mut this_verts, mut this_indices) = draw_point(
            &zero, &camera, &config, &bone.pos, color, cam.pos, 0., elapsed,
        );

        // bones can be selected by clicking on their point
        let mouse_on_it = utils::in_bounding_box(&input.mouse, &this_verts, &camera.window).1;
        if mouse_on_it && !camera.on_ui {
            color = bone.group_color.into();
            if bone.group_color.a == 0 {
                color = config.colors.center_point;
            }
            if attachment {
                color = config.colors.attachment_point;
            }
            if sel_bone_ids.contains(&bone.id) {
                color += Color::new(64, 64, 64, 255);
            }
            (this_verts, this_indices) = draw_point(
                &zero, &camera, &config, &bone.pos, color, cam.pos, 0., elapsed,
            );

            let mut on_click_id = bone.id;
            // QoL: select parent of textured bone if it's called 'Texture'
            // this is because most textured bones are meant to represent their parents
            let tex_name = bone.name.to_lowercase();
            if !config.exact_bone_select && bone.parent_id != -1 && tex_name == "texture" {
                on_click_id = bone.parent_id;
            }
            events.set_hovering_bone_id(on_click_id);

            // select this bone if point is pressed, unless it was already selected
            if input.left_pressed {
                let sel_bone = armature.sel_bone(selections);
                if sel_bone == None || (sel_bone != None && sel_bone.unwrap().id != bone.id) {
                    events.select_bone(on_click_id as usize, true);
                }
            }
            on_point = true;
        }

        point_verts.append(&mut this_verts);
        add_offseted_indices(&mut this_indices, &mut point_indices);
    }

    // the selected bone's tail handle, for its length (docs/ENVELOPES.md §2.2)
    renderer.tail_hovered = false;
    if let Some(bone) = tail_handle_bone(temp_arm, armature, selections, edit_mode) {
        let tail = tail_pos(bone);
        let size = config.center_point_radius * 0.8 * camera.zoom;
        let mut color = config.colors.center_point;
        let (hit, _) = draw_point(&zero, &camera, &config, &tail, color, cam.pos, 0., size * 1.6);
        let mouse_on_it = utils::in_bounding_box(&input.mouse, &hit, &camera.window).1;
        let hovered = mouse_on_it && !camera.on_ui && !on_point;
        if hovered || renderer.dragging_tail {
            color += Color::new(64, 64, 64, 0);
        }
        let diamond = std::f32::consts::FRAC_PI_4;
        #[rustfmt::skip]
        let (mut this_verts, mut this_indices) = draw_point(&zero, &camera, &config, &tail, color, cam.pos, diamond, size);
        point_verts.append(&mut this_verts);
        add_offseted_indices(&mut this_indices, &mut point_indices);

        if hovered {
            renderer.tail_hovered = true;
            on_point = true;
            if input.left_pressed {
                events.save_edited_bone(selections.bone_idx);
                renderer.dragging_tail = true;
            }
        }
    }

    if point_indices.len() > 0 {
        render_pass.set_bind_group(0, &renderer.circle_bindgroup, &[]);
        let point_buffer = &mut renderer.point_buffer;
        setup_render_buffer(point_buffer, &point_verts, &point_indices, queue);
        draw(&renderer.point_buffer, render_pass, 0, point_indices.len());
    }

    renderer.on_point |= on_point;
}

/// Outline of an envelope's capsule: the convex hull of a circle at each end.
pub fn capsule_outline(head: Vec2, r_head: f32, tail: Vec2, r_tail: f32) -> Vec<Vec2> {
    const N: usize = 32;
    let mut pts: Vec<Vec2> = vec![];
    for (c, r) in [(head, r_head.max(0.)), (tail, r_tail.max(0.))] {
        for i in 0..N {
            let a = i as f32 / N as f32 * std::f32::consts::TAU;
            pts.push(c + Vec2::new(a.cos(), a.sin()) * r);
        }
    }
    // monotone chain
    pts.sort_by(|a, b| (a.x, a.y).partial_cmp(&(b.x, b.y)).unwrap_or(std::cmp::Ordering::Equal));
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut hull: Vec<Vec2> = vec![];
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        for p in iter {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], *p) <= 0. {
                hull.pop();
            }
            hull.push(*p);
        }
        hull.pop();
    }
    hull
}

/// The envelope radius handles of a bone: (field, world position). The inner
/// handles sit on one side of the bone and the outer ones on the other; a point
/// bone has only the head's.
pub fn envelope_handles(bone: &Bone, e: &crate::envelope::Envelope) -> Vec<(EnvelopeField, Vec2)> {
    let (head, tail) = (bone.pos, tail_pos(bone));
    let axis = if (tail - head).mag() > 1e-3 {
        (tail - head).normalize()
    } else {
        utils::rotate(&Vec2::new(bone.scale.x.signum(), 0.), bone.rot)
    };
    let perp = Vec2::new(-axis.y, axis.x);
    let mut handles = vec![
        (EnvelopeField::HeadInner, head + perp * e.head_inner),
        (EnvelopeField::HeadOuter, head - perp * e.head_outer),
    ];
    if (tail - head).mag() > 1e-3 {
        handles.push((EnvelopeField::TailInner, tail + perp * e.tail_inner));
        handles.push((EnvelopeField::TailOuter, tail - perp * e.tail_outer));
    }
    handles
}

/// Envelope tool (docs/ENVELOPES.md §4.1, §4.4): the capsules of the selected bones
/// (and the bones skinning a selected mesh, or every deforming bone), radius handles
/// on the selected bone, and the selected bone's weights on the meshes it skins.
fn envelope_tool(
    config: &Config,
    camera: &Camera,
    input: &InputStates,
    edit_mode: &EditMode,
    armature: &Armature,
    temp_arm: &Armature,
    selections: &SelectionState,
    renderer: &mut Renderer,
    queue: &wgpu::Queue,
    render_pass: &mut RenderPass,
    events: &mut EventState,
) {
    renderer.envelope_hovered = None;
    if edit_mode.tool != Tool::Envelope {
        renderer.envelope_drag = None;
        return;
    }
    let world = skin::setup_world(armature);
    let cam = world_camera(&camera, &config);
    let ar = camera.aspect_ratio();
    let clip = |p: Vec2| world_vert(vert(Some(p), None, None), &cam, ar, Vec2::default()).pos;
    let zero = Vec2::ZERO;
    let px = camera.px();

    // which envelopes to show
    let mut ids: Vec<i32> = vec![];
    for id in &selections.bone_ids {
        ids.push(*id);
        if let Some(mesh) = armature.bones.iter().find(|b| b.id == *id) {
            ids.extend(skin::skin_bones(&armature.bones, mesh));
        }
    }
    if edit_mode.show_all_envelopes {
        ids.extend(world.iter().filter(|b| skin::envelope_of(&world, b.id).deform).map(|b| b.id));
    }
    let mut seen = vec![];
    ids.retain(|id| !seen.contains(id) && { seen.push(*id); true });

    let mut fill = (vec![], vec![]);
    let mut dashed = (vec![], vec![]);
    let push_quad = |dst: &mut (Vec<Vertex>, Vec<u32>), quad: [Vertex; 4]| {
        let base = dst.0.len() as u32;
        dst.0.extend(quad);
        dst.1.extend([0, 1, 2, 1, 2, 3].map(|i| i + base));
    };
    let push_fan = |dst: &mut (Vec<Vertex>, Vec<u32>), outline: &[Vec2], color: Color| {
        if outline.len() < 3 {
            return;
        }
        let base = dst.0.len() as u32;
        let center = outline.iter().fold(Vec2::ZERO, |a, p| a + *p) / outline.len() as f32;
        dst.0.push(vert(Some(clip(center)), Some(color), None));
        for p in outline {
            dst.0.push(vert(Some(clip(*p)), Some(color), None));
        }
        let n = outline.len() as u32;
        for i in 0..n {
            dst.1.extend([base, base + 1 + i, base + 1 + (i + 1) % n]);
        }
    };

    for id in &ids {
        let Some(bone) = temp_arm.bones.iter().find(|b| b.id == *id && b.bind_owner.is_none()) else { continue };
        let e = skin::envelope_of(&world, *id);
        let mut color = if bone.group_color.a == 0 { config.colors.center_point } else { bone.group_color };
        if !e.deform {
            color = config.colors.inactive_center_point;
        }
        let (head, tail) = (bone.pos, tail_pos(bone));
        let outer = capsule_outline(head, e.head_outer, tail, e.tail_outer);
        let inner = capsule_outline(head, e.head_inner, tail, e.tail_inner);
        push_fan(&mut fill, &outer, Color::new(color.r, color.g, color.b, 22));
        if !e.absolute {
            push_fan(&mut fill, &inner, Color::new(color.r, color.g, color.b, 45));
        }
        let line = Color::new(color.r, color.g, color.b, 220);
        for i in 0..outer.len() {
            let (a, b) = (clip(outer[i]), clip(outer[(i + 1) % outer.len()]));
            push_quad(&mut dashed, mesh_tools::edge_quad(a, b, 1.5 * px, 10. * px, line, camera.window));
        }
    }

    // the selected bone's weights on the meshes it skins (blue 0 → red 1); hand-weighted
    // vertices get a white outline
    let sel_id = if selections.bone_ids.len() == 1 { armature.sel_bone(selections).map(|b| b.id) } else { None };
    if let Some(sel_id) = sel_id {
        let radius = config.center_point_radius * config.mesh_vert_size.clamp(0.1, 3.) * camera.zoom;
        for mesh in armature.bones.iter().filter(|m| skin::skin_bones(&armature.bones, m).contains(&sel_id)) {
            let Some(drawn) = temp_arm.bones.iter().find(|b| b.id == mesh.id) else { continue };
            if drawn.hidden {
                continue;
            }
            let shares = skin::vertex_shares(&armature.bones, mesh);
            for wv in &drawn.world_verts {
                let share = shares.get(&wv.id).map_or(0., |s| s.iter().filter(|x| x.0 == sel_id).map(|x| x.1).sum());
                if mesh.locked_verts.contains(&wv.id) {
                    let white = Color::new(255, 255, 255, 255);
                    let (mut v, mut i) = draw_point(&wv.pos, camera, config, &zero, white, zero, 0., radius * 1.5);
                    add_offseted_indices(&mut i, &mut fill.1);
                    fill.0.append(&mut v);
                }
                let mut color = mesh_tools::hsv(240. * (1. - share.clamp(0., 1.)), 1., 1.);
                color.a = if share > 0. { 255 } else { 140 };
                let (mut v, mut i) = draw_point(&wv.pos, camera, config, &zero, color, zero, 0., radius);
                add_offseted_indices(&mut i, &mut fill.1);
                fill.0.append(&mut v);
            }
        }
    }

    // radius handles on the selected bone, at rest
    let at_rest = !edit_mode.anim_open && !edit_mode.pose_mode;
    if let (Some(sel_id), true) = (sel_id, at_rest) {
        if let Some(bone) = temp_arm.bones.iter().find(|b| b.id == sel_id && b.bind_owner.is_none()) {
            let e = skin::envelope_of(&world, sel_id);
            let size = config.center_point_radius * 0.8 * camera.zoom;
            let diamond = std::f32::consts::FRAC_PI_4;
            for (field, pos) in envelope_handles(bone, &e) {
                let inner = matches!(field, EnvelopeField::HeadInner | EnvelopeField::TailInner);
                let mut color = if inner { config.colors.center_point } else { Color::new(235, 235, 235, 255) };
                let (hit, _) = draw_point(&zero, camera, config, &pos, color, cam.pos, 0., size * 1.6);
                let mouse_on_it = utils::in_bounding_box(&input.mouse, &hit, &camera.window).1;
                let hovered = mouse_on_it && !camera.on_ui && renderer.envelope_hovered.is_none();
                if hovered || renderer.envelope_drag == Some(field) {
                    color += Color::new(64, 64, 64, 0);
                }
                let (mut v, mut i) = draw_point(&zero, camera, config, &pos, color, cam.pos, diamond, size);
                add_offseted_indices(&mut i, &mut fill.1);
                fill.0.append(&mut v);
                if hovered {
                    renderer.envelope_hovered = Some(field);
                    renderer.on_point = true;
                    if input.left_pressed && renderer.envelope_drag.is_none() {
                        events.save_bone(selections.bone_idx);
                        renderer.envelope_drag = Some(field);
                    }
                }
            }
        }
    }

    if !dashed.1.is_empty() {
        render_pass.set_bind_group(0, edge_bindgroup(renderer, EdgeStyle::Dashed), &[]);
        setup_render_buffer(&renderer.envelope_edge_buffer, &dashed.0, &dashed.1, queue);
        draw(&renderer.envelope_edge_buffer, render_pass, 0, dashed.1.len());
    }
    if !fill.1.is_empty() {
        render_pass.set_bind_group(0, &renderer.generic_bindgroup, &[]);
        setup_render_buffer(&renderer.envelope_buffer, &fill.0, &fill.1, queue);
        draw(&renderer.envelope_buffer, render_pass, 0, fill.1.len());
    }
}

/// Drag an envelope radius handle to `mouse`: the radius is the distance from that end
/// of the bone. With Shift, the other end's radius follows.
fn drag_envelope(events: &mut EventState, input: &InputStates, temp_arm: &Armature, bone_id: i32, field: EnvelopeField, mouse: Vec2) {
    let Some(bone) = temp_arm.bones.iter().find(|b| b.id == bone_id) else { return };
    type F = EnvelopeField;
    let at_head = matches!(field, F::HeadInner | F::HeadOuter);
    let end = if at_head { bone.pos } else { tail_pos(bone) };
    let radius = (mouse - end).mag();
    events.edit_envelope(field, radius);
    if input.holding_shift {
        let other = match field {
            F::HeadInner => F::TailInner,
            F::TailInner => F::HeadInner,
            F::HeadOuter => F::TailOuter,
            _ => F::HeadOuter,
        };
        events.edit_envelope(other, radius);
    }
}

/// World position of a bone's tail: `length` along its axis (docs/ENVELOPES.md §2),
/// where a child at local (length, 0) would sit.
pub fn tail_pos(bone: &Bone) -> Vec2 {
    bone.pos + utils::rotate(&Vec2::new(bone.length * bone.scale.x, 0.), bone.rot)
}

/// The bone showing a tail handle: the only selected bone, if it has a length, with the
/// Transform tool out in the setup pose.
fn tail_handle_bone<'a>(
    temp_arm: &'a Armature,
    armature: &Armature,
    selections: &SelectionState,
    edit_mode: &EditMode,
) -> Option<&'a Bone> {
    let setup = !edit_mode.anim_open && !edit_mode.pose_mode;
    let tool = matches!(edit_mode.tool, Tool::Transform | Tool::Envelope);
    if selections.bone_ids.len() != 1 || edit_mode.editing_mesh || !tool || !setup {
        return None;
    }
    let id = armature.sel_bone(selections)?.id;
    temp_arm.bones.iter().find(|b| b.id == id && b.length > 0. && b.bind_owner.is_none())
}

/// Drag a bone's tail to `mouse`: changes its length along its axis, so neither the bone
/// nor its art turns. With the modifier key held, also aims the bone at the cursor.
fn drag_tail(
    events: &mut EventState,
    input: &InputStates,
    armature: &Armature,
    temp_arm: &Armature,
    bone_id: i32,
    mouse: Vec2,
) {
    let Some(bone) = temp_arm.bones.iter().find(|b| b.id == bone_id) else { return };
    let sx = bone.scale.x;
    if sx.abs() < 1e-6 {
        return;
    }
    let d = mouse - bone.pos;
    let axis = utils::rotate(&Vec2::new(sx.signum(), 0.), bone.rot);
    let mut length = (d.x * axis.x + d.y * axis.y).max(0.) / sx.abs();

    let rot_locked = ik::locks(&armature.bones, bone_id).0;
    if input.holding_mod && !rot_locked && d.mag() > 0. {
        let mut world_rot = d.y.atan2(d.x);
        if sx < 0. {
            world_rot -= std::f32::consts::PI;
        }
        let parent = temp_arm.bones.iter().find(|b| b.id == bone.parent_id);
        let local = match parent {
            Some(p) if is_facing_left(p.scale) => p.rot - world_rot,
            Some(p) => world_rot - p.rot,
            None => world_rot,
        };
        let current = armature.bones.iter().find(|b| b.id == bone_id).unwrap().rot;
        let rot = current + utils::shortest_angle_delta(current, local);
        events.edit_bone(bone_id, &AnimElement::Rotation, rot, "", usize::MAX, -1);
        length = d.mag() / sx.abs();
    }
    events.edit_bone(bone_id, &AnimElement::Length, length, "", usize::MAX, -1);
}

pub fn draw_kites(
    config: &Config,
    camera: &Camera,
    edit_mode: &EditMode,
    temp_arm: &mut Armature,
    sel_bone_ids: Vec<i32>,
    renderer: &mut Renderer,
    queue: &wgpu::Queue,
    render_pass: &mut RenderPass,
) {
    let cam = world_camera(&camera, &config);
    let mut kite_verts = vec![];
    let mut kite_indices = vec![];
    let mut line_verts = vec![];
    let mut line_indices = vec![];

    for p in 0..temp_arm.bones.len() {
        let bone = &temp_arm.bones[p];

        if bone.bind_owner.is_some() {
            continue;
        }

        if !renderer.render_kites || (edit_mode.editing_mesh && !sel_bone_ids.contains(&bone.id)) {
            continue;
        }

        // the bone itself, head to tail, when it has a length (docs/ENVELOPES.md §2.2)
        let blade = bone.pos - tail_pos(bone);
        if bone.length > 0. && blade.mag() > 0. {
            let mut color: Color = bone.group_color.into();
            if bone.group_color.a == 0 {
                color = config.colors.inactive_center_point;
                if sel_bone_ids.contains(&bone.id) {
                    color = config.colors.center_point
                }
            } else if !sel_bone_ids.contains(&bone.id) {
                color.a /= 2;
            }
            #[rustfmt::skip]
            let (mut this_verts, mut this_indices) = draw_flow_kite(
                &Vec2::new(0., 0.), &camera, &config, &tail_pos(bone), color, cam.pos, blade.y.atan2(blade.x), blade.mag()
            );
            kite_verts.append(&mut this_verts);
            add_offseted_indices(&mut this_indices, &mut kite_indices);
        }

        let parent = temp_arm.bones.iter().find(|b| b.id == bone.parent_id);
        if parent == None {
            continue;
        }

        // attachments: thin line to the parent instead of a kite
        if is_attachment(config, temp_arm, bone) {
            let ar = camera.aspect_ratio();
            let from = world_vert(vert(Some(bone.pos), None, None), &cam, ar, Vec2::default());
            let to = world_vert(
                vert(Some(parent.unwrap().pos), None, None),
                &cam,
                ar,
                Vec2::default(),
            );
            let color = attachment_color(config, sel_bone_ids.contains(&bone.id));
            let (mut verts, mut indices) = draw_line(from.pos, to.pos, color);
            line_verts.append(&mut verts);
            add_offseted_indices(&mut indices, &mut line_indices);
            continue;
        }

        let diff = parent.unwrap().pos - bone.pos;
        let kite_width = diff.mag() / 1.;
        let kite_rot = diff.y.atan2(diff.x);
        // skip 0 width kites, caused by the child being directly on the parent
        if kite_width == 0. {
            continue;
        }

        let mut color;
        if parent.unwrap().group_color.a == 0 {
            color = config.colors.inactive_center_point;
            if sel_bone_ids.contains(&parent.unwrap().id) {
                color = config.colors.center_point
            }
        } else {
            color = parent.unwrap().group_color.into();
            if !sel_bone_ids.contains(&parent.unwrap().id) {
                color.a /= 2;
            }
        }
        #[rustfmt::skip]
        let (mut this_verts, mut this_indices) = draw_flow_kite(
            &Vec2::new(0., 0.), &camera, &config, &temp_arm.bones[p].pos, color, cam.pos, kite_rot, kite_width
        );
        kite_verts.append(&mut this_verts);
        add_offseted_indices(&mut this_indices, &mut kite_indices);
    }

    if kite_indices.len() > 0 {
        render_pass.set_bind_group(0, &renderer.flow_kite_bindgroup, &[]);
        setup_render_buffer(&mut renderer.kite_buffer, &kite_verts, &kite_indices, queue);
        draw(&renderer.kite_buffer, render_pass, 0, kite_indices.len());
    }

    if line_indices.len() > 0 {
        render_pass.set_bind_group(0, &renderer.generic_bindgroup, &[]);
        let buffer = &mut renderer.attachment_line_buffer;
        setup_render_buffer(buffer, &line_verts, &line_indices, queue);
        draw(
            &renderer.attachment_line_buffer,
            render_pass,
            0,
            line_indices.len(),
        );
    }
}

// Add vertex and index data to the specified buffer.
fn setup_render_buffer(
    buffer: &RenderBuffer,
    verts: &Vec<Vertex>,
    indices: &Vec<u32>,
    queue: &wgpu::Queue,
) {
    let gpu_verts: Vec<GpuVertex> = verts.iter().map(|vert| (*vert).into()).collect();
    let index_buffer = &buffer.index.as_ref().unwrap();
    let vertex_buffer = &buffer.vertex.as_ref().unwrap();
    queue.write_buffer(index_buffer, 0, bytemuck::cast_slice(&indices));
    queue.write_buffer(vertex_buffer, 0, bytemuck::cast_slice(&gpu_verts));
}

fn transform_ring(
    config: &Config,
    camera: &Camera,
    armature: &Armature,
    temp_arm: &mut Armature,
    render_pass: &mut RenderPass,
    renderer: &mut Renderer,
    events: &mut EventState,
    edit_mode: &EditMode,
    sel: &SelectionState,
    queue: &wgpu::Queue,
    mouse_pos: &Vec2,
) {
    // initiate temporary edit mode if mouse is close enough to bone
    let distance_move = 0.02;
    let distance_rot = config.transform_rot_radius;
    let distance_scale = config.transform_scale_radius;
    let mut temporary = 3;

    render_pass.set_bind_group(0, &renderer.ring_bindgroup, &[]);

    let id = armature.sel_bone(&sel).unwrap().id;
    let sel_bone = temp_arm.bones.iter().find(|b| b.id == id).unwrap();
    let adjusted = Vec2::new(sel_bone.pos.x - mouse_pos.x, sel_bone.pos.y - mouse_pos.y);
    let expand_time = 0.15;
    let size_elapsed = (edit_mode.sel_time / expand_time).min(1.);
    let mut on_point = false;

    // set temporary mode based on distance from bone to cursor (the tail handle wins)
    if !camera.on_ui && size_elapsed == 1. && !renderer.tail_hovered && !renderer.dragging_tail {
        let distance = adjusted.mag() / camera.zoom;

        // prioritize rot or scale, depending on user-defined distance
        if distance < distance_move {
            on_point = true;
            temporary = 0;
            events.set_temporary_edit_mode(0);
        } else if distance_scale > distance_rot {
            if distance < distance_rot {
                on_point = true;
                temporary = 1;
                events.set_temporary_edit_mode(1);
            } else if distance < distance_scale {
                on_point = true;
                temporary = 2;
                events.set_temporary_edit_mode(2);
            }
        } else {
            if distance < distance_scale {
                on_point = true;
                temporary = 2;
                events.set_temporary_edit_mode(2);
            } else if distance < distance_rot {
                on_point = true;
                temporary = 1;
                events.set_temporary_edit_mode(1);
            }
        }
    }

    if !renderer.on_point {
        renderer.on_point = on_point;
    }

    // draw rot and scale rings
    let cam = world_camera(&camera, &config);
    let mut rot_col = config.colors.transform_rings;
    let mut scale_col = config.colors.transform_rings;
    let rot_radius = distance_rot * camera.zoom * size_elapsed;
    let scale_radius = distance_scale * camera.zoom * size_elapsed;
    scale_col -= Color::new(25, 25, 25, 0);
    #[rustfmt::skip]
    let (mut verts_rot, mut indices_rot) = draw_point(&Vec2::ZERO, camera, config, &sel_bone.pos, rot_col, cam.pos, 0., rot_radius);
    #[rustfmt::skip]
    let (mut verts_scale, mut indices_scale) = draw_point(&Vec2::ZERO, camera, config, &sel_bone.pos, scale_col, cam.pos, 0., scale_radius);
    verts_rot.extend_from_slice(&mut verts_scale);
    add_offseted_indices(&mut indices_scale, &mut indices_rot);
    setup_render_buffer(&mut renderer.ring_buffer, &verts_rot, &indices_rot, queue);
    draw(&renderer.ring_buffer, render_pass, 0, indices_rot.len());

    // setup rot background
    if temporary != 1 {
        rot_col -= Color::new(0, 0, 0, 200);
    } else {
        rot_col -= Color::new(0, 0, 0, 150);
    }
    render_pass.set_bind_group(0, &renderer.circle_bindgroup, &[]);
    #[rustfmt::skip]
    let (mut sel_verts, mut sel_indices) = draw_point(&Vec2::ZERO, camera, config, &sel_bone.pos, rot_col, cam.pos, 0., rot_radius);

    // setup scale background
    if temporary != 2 {
        scale_col -= Color::new(0, 0, 0, 200);
    } else {
        scale_col -= Color::new(0, 0, 0, 150);
    }
    render_pass.set_bind_group(0, &renderer.circle_bindgroup, &[]);
    #[rustfmt::skip]
    let (mut sel_verts1, mut sel_indices1) = draw_point(&Vec2::ZERO, camera, config, &sel_bone.pos, scale_col, cam.pos, 0., scale_radius);

    // draw both backgrounds
    sel_verts.append(&mut sel_verts1);
    add_offseted_indices(&mut sel_indices1, &mut sel_indices);
    let buffer = &mut renderer.selected_ring_buffer;
    setup_render_buffer(buffer, &sel_verts, &sel_indices, queue);
    draw(&buffer, render_pass, 0, indices_rot.len());

    // set temporary mode to None only once, to prevent event spam
    if temporary == 3 && edit_mode.temporary != None {
        events.set_temporary_edit_mode(3);
    }
}

pub fn is_facing_left(scale: Vec2) -> bool {
    let both = scale.x < 0. && scale.y < 0.;
    let either = scale.x < 0. || scale.y < 0.;
    either && !both
}
