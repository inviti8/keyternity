//! Render cameras (docs/FRAME_SEQUENCES.md §4): geometry, dragging and fitting.

use keyternity_lib::render_camera::{self, CameraLock, Handle, RenderCamera};
use keyternity_lib::renderer;
use keyternity_lib::shared::*;

fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).mag() < 1e-3
}

fn cam(pos: Vec2, res: Vec2, scale: f32) -> RenderCamera {
    RenderCamera {
        pos,
        resolution: res,
        scale,
        round_to: 1,
        ..Default::default()
    }
}

#[test]
fn world_size_and_pixels() {
    let c = cam(Vec2::new(10., 20.), Vec2::new(200., 100.), 2.);
    assert!(close(c.world_size(), Vec2::new(100., 50.)));
    assert!(close(c.min(), Vec2::new(-40., -5.)));
    assert!(close(c.max(), Vec2::new(60., 45.)));
    // top-left is pixel (0, 0); y goes down
    assert!(close(c.to_px(Vec2::new(-40., 45.)), Vec2::ZERO));
    assert!(close(c.to_px(Vec2::new(60., -5.)), Vec2::new(200., 100.)));
    assert!(close(c.to_px(c.pos), Vec2::new(100., 50.)));
}

#[test]
fn view_maps_the_rectangle_onto_the_output() {
    // renderer::world_vert: x by (x - pos.x) / zoom * aspect, y by (y - pos.y) / zoom
    let c = cam(Vec2::new(5., -3.), Vec2::new(300., 100.), 0.5);
    let (view, aspect) = c.view();
    let clip = |p: Vec2| {
        let v = Vertex { pos: p, ..Default::default() };
        renderer::world_vert(v, &view, aspect, Vec2::ZERO).pos
    };
    assert!(close(clip(c.min()), Vec2::new(-1., -1.)));
    assert!(close(clip(c.max()), Vec2::new(1., 1.)));
    assert!(close(clip(c.pos), Vec2::ZERO));
}

#[test]
fn corner_drag_keeps_the_opposite_corner() {
    let start = cam(Vec2::ZERO, Vec2::new(100., 100.), 1.);
    // drag the top-right corner out by (20, 10)
    let c = render_camera::drag(&start, Handle::Corner(1), Vec2::new(20., 10.));
    assert!(close(c.resolution, Vec2::new(120., 110.)));
    assert!(close(c.min(), start.min()));
    assert_eq!(c.scale, 1.);
}

#[test]
fn side_drag_changes_one_axis() {
    let start = cam(Vec2::ZERO, Vec2::new(100., 100.), 2.);
    // the left side, dragged left by 10 world units = 20 px
    let c = render_camera::drag(&start, Handle::Side(3), Vec2::new(-10., 7.));
    assert!(close(c.resolution, Vec2::new(120., 100.)));
    assert!((c.max().x - start.max().x).abs() < 1e-3);
    assert!((c.pos.y - start.pos.y).abs() < 1e-3);
}

#[test]
fn resolution_lock_scales_instead() {
    let mut start = cam(Vec2::ZERO, Vec2::new(200., 100.), 1.);
    start.lock = CameraLock::Resolution;
    // the bottom-right corner, doubling the width
    let c = render_camera::drag(&start, Handle::Corner(2), Vec2::new(200., 0.));
    assert!(close(c.resolution, start.resolution));
    assert!((c.scale - 0.5).abs() < 1e-4);
    // top-left stays put
    let tl = |c: &RenderCamera| Vec2::new(c.min().x, c.max().y);
    assert!(close(tl(&c), tl(&start)));
}

#[test]
fn rounding_and_minimum_size() {
    let mut start = cam(Vec2::ZERO, Vec2::new(100., 100.), 1.);
    start.round_to = 2;
    let c = render_camera::drag(&start, Handle::Side(1), Vec2::new(3.4, 0.));
    assert_eq!(c.resolution.x, 104.);
    // can't be dragged inside out
    let c = render_camera::drag(&start, Handle::Side(1), Vec2::new(-500., 0.));
    assert!(c.resolution.x >= 8.);
}

#[test]
fn move_and_pivot() {
    let start = cam(Vec2::ZERO, Vec2::new(100., 100.), 1.);
    let c = render_camera::drag(&start, Handle::Move, Vec2::new(5., 6.));
    assert!(close(c.pos, Vec2::new(5., 6.)));
    assert!(close(c.pivot, start.pivot), "moving the frame leaves the pivot on the character");
    let c = render_camera::drag(&start, Handle::Pivot, Vec2::new(5., 6.));
    assert!(close(c.pivot, Vec2::new(5., 6.)));
    assert!(close(c.pos, start.pos));
}

#[test]
fn fit_both_locks() {
    let min = Vec2::new(-10., 0.);
    let max = Vec2::new(30., 80.);
    let mut start = cam(Vec2::ZERO, Vec2::new(64., 64.), 2.);
    start.round_to = 2;
    let c = render_camera::fit(&start, min, max, 4.);
    assert!(close(c.pos, Vec2::new(10., 40.)));
    assert!(close(c.resolution, Vec2::new(88., 168.)));

    start.lock = CameraLock::Resolution;
    start.resolution = Vec2::new(108., 108.);
    let c = render_camera::fit(&start, min, max, 4.);
    assert!(close(c.resolution, start.resolution));
    // the taller side (80) fills 100 px
    assert!((c.scale - 1.25).abs() < 1e-4);
}

#[test]
fn names_and_ids() {
    let a = RenderCamera { id: 3, ..Default::default() };
    let b = RenderCamera { id: 7, name: "Camera 2".into(), ..Default::default() };
    assert_eq!(render_camera::unique_name(&[], "Camera"), "Camera");
    assert_eq!(render_camera::unique_name(&[a.clone(), b.clone()], "Camera"), "Camera 3");
    assert_eq!(render_camera::next_id(&[a, b]), 8);
}

#[test]
fn saved_in_editor_json() {
    let c = RenderCamera {
        id: 2,
        name: "Portrait".into(),
        background: Some(Color::new(10, 20, 30, 255)),
        lock: CameraLock::Resolution,
        ..Default::default()
    };
    let json = serde_json::to_string(&EditorOptions { cameras: vec![c.clone()], ..Default::default() }).unwrap();
    let back: EditorOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(back.cameras, vec![c]);
    // files from before cameras load with none
    let old: EditorOptions = serde_json::from_str(r#"{"bones":[],"styles":[]}"#).unwrap();
    assert!(old.cameras.is_empty());
    // and a camera missing newer fields gets defaults
    let partial: RenderCamera = serde_json::from_str(r#"{"id":1,"name":"A"}"#).unwrap();
    assert_eq!(partial.scale, 1.);
}

#[test]
fn bounds_follow_the_pose() {
    let size = Vec2::new(20., 10.);
    let (vertices, indices) = renderer::create_tex_rect(&size);
    let bone = Bone {
        id: 0,
        parent_id: -1,
        name: "body".into(),
        tex: "body".into(),
        scale: Vec2::new(1., 1.),
        pivot_scale: Vec2::new(1., 1.),
        vertices,
        indices,
        ..Default::default()
    };
    let tex = Texture { name: "body".into(), size, ..Default::default() };
    let mut armature = Armature {
        bones: vec![bone],
        styles: vec![Style { id: 0, name: "Default".into(), active: true, textures: vec![tex] }],
        ..Default::default()
    };
    let (min, max) = render_camera::pose_bounds(&armature, &armature.bones).unwrap();
    assert!(close(max - min, size));

    armature.bones[0].pos = Vec2::new(100., 50.);
    let (min2, _) = render_camera::pose_bounds(&armature, &armature.bones).unwrap();
    assert!(close(min2 - min, Vec2::new(100., 50.)));

    armature.bones[0].hidden = true;
    assert!(render_camera::pose_bounds(&armature, &armature.bones).is_none());
}
