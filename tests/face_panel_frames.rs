//! The Face Rig panel draws the same thing frame after frame when nothing changes
//! (a hovered manipulator once flickered between two positions).

use keyternity_lib::face;
use keyternity_lib::face_panel;
use keyternity_lib::shared::*;

fn vertex(id: u32, x: f32, y: f32) -> Vertex {
    Vertex { id, pos: Vec2::new(x, y), uv: Vec2::new(0.5, 0.5), init_pos: Vec2::new(x, y), ..Default::default() }
}

fn rigged() -> Shared {
    let mut s = Shared::default();
    let head = Bone { id: 0, parent_id: -1, name: "head".into(), scale: Vec2::new(1., 1.), pivot_scale: Vec2::new(1., 1.), ..Default::default() };
    let mut mesh = Bone {
        id: 1,
        parent_id: 0,
        name: "face".into(),
        scale: Vec2::new(1., 1.),
        pivot_scale: Vec2::new(1., 1.),
        vertices: vec![vertex(0, -70., -110.), vertex(1, 70., -110.), vertex(2, 70., 110.), vertex(3, -70., 110.)],
        indices: vec![0, 1, 2, 0, 2, 3],
        verts_edited: true,
        ..Default::default()
    };
    face::add_standard_shapes(&mut mesh, false);
    s.armature.bones = vec![head, mesh];
    face::set_up(&mut s.armature, 1, "Face");
    s.armature.animated_bones = s.armature.bones.clone();
    s.ui.face_panel_open = true;
    s
}

/// Every painted vertex position, frame by frame.
fn frame(s: &mut Shared, ctx: &egui::Context, pointer: Option<egui::Pos2>) -> Vec<(f32, f32)> {
    let mut input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000., 800.))), ..Default::default() };
    if let Some(p) = pointer {
        input.events.push(egui::Event::PointerMoved(p));
    }
    let out = ctx.run(input, |ctx| {
        face_panel::draw(ctx, &mut s.ui, &s.config, &s.armature, &s.selections, &s.edit_mode, &mut s.events);
    });
    let mut pts = vec![];
    for shape in out.shapes {
        if let egui::epaint::Shape::Mesh(m) = shape.shape {
            for v in &m.vertices {
                pts.push((v.pos.x, v.pos.y));
            }
        }
    }
    pts
}

#[test]
fn the_panel_is_steady_while_hovered() {
    let mut s = rigged();
    let ctx = egui::Context::default();
    // let the window settle, then rest the pointer in its middle
    for _ in 0..4 {
        frame(&mut s, &ctx, None);
    }
    let mid = ctx.memory(|m| m.area_rect(egui::Id::new("face_rig_panel"))).map(|r| r.center()).unwrap_or(egui::pos2(170., 210.));
    let frames: Vec<Vec<(f32, f32)>> = (0..6).map(|_| frame(&mut s, &ctx, Some(mid))).collect();
    for i in 2..frames.len() {
        assert_eq!(frames[i].len(), frames[i - 1].len(), "frame {i}: a different number of vertices");
        let moved = frames[i].iter().zip(&frames[i - 1]).filter(|(a, b)| (a.0 - b.0).abs() > 0.01 || (a.1 - b.1).abs() > 0.01).count();
        assert_eq!(moved, 0, "frame {i}: {moved} vertices moved with nothing changing");
    }
}

/// After a panel frame, as the app does it: the pose is rebuilt (rendering) before the
/// frame's events apply, so the next frame's panel sees a pose one frame behind.
fn run_events(s: &mut Shared) {
    keyternity_lib::utils::animate_bones(&mut s.armature, &s.selections, &s.edit_mode);
    while !s.events.events.is_empty() {
        #[rustfmt::skip]
        keyternity_lib::editor::iterate_events(
            &s.input, &mut s.config, &mut s.events, &mut s.camera, &mut s.edit_mode, &mut s.selections,
            &mut s.undo_states, &mut s.armature, &mut s.psd_armature, &mut s.copy_buffer, &mut s.ui, &mut s.renderer,
        );
    }
}

fn jaw(s: &Shared) -> Vec2 {
    s.armature.bones.iter().find(|b| b.face_control.as_ref().is_some_and(|c| c.name == "Jaw")).unwrap().pos
}

#[test]
fn dragging_a_manipulator_moves_it_steadily() {
    let mut s = rigged();
    let ctx = egui::Context::default();
    let raw = |events: Vec<egui::Event>| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000., 800.))),
        events,
        ..Default::default()
    };
    let mut step = |s: &mut Shared, events: Vec<egui::Event>| {
        ctx.run(raw(events), |ctx| {
            face_panel::draw(ctx, &mut s.ui, &s.config, &s.armature, &s.selections, &s.edit_mode, &mut s.events);
        });
        run_events(s);
    };
    for _ in 0..4 {
        step(&mut s, vec![]);
    }
    let r = ctx.memory(|m| m.area_rect(egui::Id::new("face_rig_panel"))).unwrap();
    // the jaw: low in the mask's middle
    let start = egui::pos2(r.center().x, r.top() + r.height() * 0.78);
    let press = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    step(&mut s, vec![egui::Event::PointerMoved(start)]);
    step(&mut s, vec![press(start, true)]);
    let mut p = start;
    let mut ys = vec![];
    for _ in 0..8 {
        p.y += 3.;
        step(&mut s, vec![egui::Event::PointerMoved(p)]);
        ys.push(jaw(&s).y);
    }
    // then hold still, still pressed
    let mut held = vec![];
    for _ in 0..5 {
        step(&mut s, vec![egui::Event::PointerMoved(p)]);
        held.push(jaw(&s));
    }
    assert!(s.ui.face_panel_drag.is_some(), "grabbed a manipulator");
    let grabbed = s.ui.face_panel_drag.unwrap();
    for b in s.armature.bones.iter().filter(|b| b.face_control.is_some()) {
        let fc = b.face_control.as_ref().unwrap();
        eprintln!("{} {} pos {:?} rest {:?}{}", b.id, fc.name, b.pos, fc.rest, if b.id == grabbed { "  <- grabbed" } else { "" });
    }
    eprintln!("drag ys {:?}\nheld {:?}", ys, held);
    for w in ys.windows(2) {
        assert!(w[1] <= w[0] + 1e-4, "moving down steadily: {:?}", ys);
    }
    for w in held.windows(2) {
        assert!((w[1] - w[0]).mag() < 1e-4, "holding still: {:?}", held);
    }
}

#[test]
fn pressing_and_holding_a_manipulator_is_steady() {
    hold(false);
}

#[test]
fn pressing_and_holding_while_animating_is_steady() {
    hold(true);
}

fn hold(animating: bool) {
    for frac in [0.70f32, 0.74] {
        let mut s = rigged();
        if animating {
            s.armature.animations = vec![Animation { id: 0, name: "talk".into(), fps: 24, ..Default::default() }];
            s.edit_mode.anim_open = true;
            s.selections.anim = 0;
            s.selections.anim_frame = 10;
            s.selections.bone_idx = 1;
            s.selections.bone_ids = vec![1];
            keyternity_lib::utils::animate_bones(&mut s.armature, &s.selections, &s.edit_mode);
        }
        let ctx = egui::Context::default();
        let raw = |events: Vec<egui::Event>| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000., 800.))),
            events,
            ..Default::default()
        };
        let mut pts_of = |s: &mut Shared, events: Vec<egui::Event>| {
            let out = ctx.run(raw(events), |ctx| {
                face_panel::draw(ctx, &mut s.ui, &s.config, &s.armature, &s.selections, &s.edit_mode, &mut s.events);
            });
            run_events(s);
            let mut pts = vec![];
            for shape in out.shapes {
                if let egui::epaint::Shape::Mesh(m) = shape.shape {
                    pts.extend(m.vertices.iter().map(|v| (v.pos.x, v.pos.y)));
                }
            }
            pts
        };
        for _ in 0..4 {
            pts_of(&mut s, vec![]);
        }
        let r = ctx.memory(|m| m.area_rect(egui::Id::new("face_rig_panel"))).unwrap();
        let p = egui::pos2(r.center().x + 25., r.top() + r.height() * frac);
        let press = egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() };
        pts_of(&mut s, vec![egui::Event::PointerMoved(p)]);
        pts_of(&mut s, vec![press]);
        // drag a little past egui's threshold, then hold
        let q = p + egui::vec2(0., 12.);
        pts_of(&mut s, vec![egui::Event::PointerMoved(q)]);
        let frames: Vec<_> = (0..6).map(|_| pts_of(&mut s, vec![egui::Event::PointerMoved(q)])).collect();
        let grabbed = s.ui.face_panel_drag.and_then(|id| s.armature.bones.iter().find(|b| b.id == id)).map(|b| b.face_control.as_ref().unwrap().name.clone());
        let mut moved_counts = vec![];
        for i in 1..frames.len() {
            let moved = frames[i].iter().zip(&frames[i - 1]).filter(|(a, b)| (a.0 - b.0).abs() > 0.01 || (a.1 - b.1).abs() > 0.01).count();
            moved_counts.push(moved);
        }
        eprintln!("animating {animating} at {frac}: grabbed {:?}, vertices moving while held: {:?}", grabbed, moved_counts);
        // the first held frame catches up the pose's one-frame lag; then it must be still
        assert!(moved_counts[1..].iter().all(|m| *m == 0), "steady while held: {:?}", moved_counts);
    }
}
