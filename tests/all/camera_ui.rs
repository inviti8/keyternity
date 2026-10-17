//! Render camera overlay and panel (docs/FRAME_SEQUENCES.md §4.2–4.3), run headless.

use keyternity_lib::camera_ui;
use keyternity_lib::render_camera::RenderCamera;
use keyternity_lib::shared::*;

struct Harness {
    ctx: egui::Context,
    ui: Ui,
    config: Config,
    camera: Camera,
    armature: Armature,
    sel: SelectionState,
    edit: EditMode,
    events: EventState,
}

impl Harness {
    fn new(cameras: Vec<RenderCamera>) -> Harness {
        Harness {
            ctx: egui::Context::default(),
            ui: Ui::default(),
            config: Config::default(),
            camera: Camera { pos: Vec2::ZERO, zoom: 450., window: Vec2::new(1600., 900.), ..Default::default() },
            armature: Armature { cameras, ..Default::default() },
            sel: SelectionState::default(),
            edit: EditMode { show_cameras: true, ..Default::default() },
            events: EventState::default(),
        }
    }

    /// One frame with these input events; returns whether the overlay claimed the pointer.
    fn frame(&mut self, input: Vec<egui::Event>) -> bool {
        let mut over = false;
        let out = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600., 900.))),
                events: input,
                ..Default::default()
            },
            |ctx| {
                egui::SidePanel::right("p").show(ctx, |ui| {
                    camera_ui::panel(ui, &mut self.ui, &self.config, &self.armature, &self.sel, &mut self.events);
                });
                over = camera_ui::overlay(ctx, &mut self.ui, &self.config, &self.camera, &self.armature, &self.sel, &self.edit, &mut self.events);
            },
        );
        let _ = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        over
    }
}

fn moved(p: egui::Pos2) -> egui::Event {
    egui::Event::PointerMoved(p)
}

fn button(p: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }
}

#[test]
fn draws_cameras_of_any_size() {
    let mut h = Harness::new(vec![]);
    h.sel.camera = Some(0);
    for w in [512., 1920., 1.0e5, 1.0e7, 2., 0.5] {
        h.armature.cameras = vec![RenderCamera { resolution: Vec2::new(w, 512.), ..Default::default() }];
        h.frame(vec![]);
    }
}

#[test]
fn dragging_an_edge_selects_and_moves_the_camera() {
    // zoom 450 over a 900 px window: 1 world unit = 1 px = 1 point; the origin is the centre
    let mut h = Harness::new(vec![RenderCamera { resolution: Vec2::new(200., 200.), ..Default::default() }]);
    h.frame(vec![]);
    // the left edge is at x = 800 - 100
    let edge = egui::pos2(700., 450.);
    assert!(h.frame(vec![moved(edge)]), "hovering the edge should claim the pointer");
    h.frame(vec![button(edge, true)]);
    assert!(h.events.events.contains(&Events::SelectCamera), "{:?}", h.events.events);
    h.frame(vec![moved(egui::pos2(730., 450.))]);
    assert!(h.events.events.contains(&Events::SetCamera), "{:?}", h.events.events);
    let i = h.events.events.iter().position(|e| *e == Events::SetCamera).unwrap();
    let cam: RenderCamera = serde_json::from_str(&h.events.str_values[i]).unwrap();
    assert!((cam.pos.x - 30.).abs() < 1e-3, "{:?}", cam.pos);
}

#[test]
fn dragging_a_handle_resizes() {
    let mut h = Harness::new(vec![RenderCamera { resolution: Vec2::new(200., 200.), round_to: 1, ..Default::default() }]);
    h.sel.camera = Some(0);
    h.frame(vec![]);
    // the right side's handle
    let handle = egui::pos2(900., 450.);
    assert!(h.frame(vec![moved(handle)]));
    h.frame(vec![button(handle, true)]);
    h.frame(vec![moved(egui::pos2(950., 450.))]);
    let i = h.events.events.iter().rposition(|e| *e == Events::SetCamera).expect("a SetCamera event");
    let cam: RenderCamera = serde_json::from_str(&h.events.str_values[i]).unwrap();
    assert!((cam.resolution.x - 250.).abs() < 1e-3, "{:?}", cam.resolution);
}
