//! Canvas navigation maths (docs/TOPOLOGY_TOOLS.md §3), checked against the
//! renderer's own world-to-screen transform.

use skelform_lib::navigation;
use skelform_lib::renderer::{world_camera, world_vert};
use skelform_lib::shared::*;

const EPS: f32 = 1e-2;

fn camera(layout_x: f32) -> Camera {
    Camera {
        pos: Vec2::new(120. + layout_x, -40.),
        zoom: 2000.,
        on_ui: false,
        window: Vec2::new(1600., 900.),
    }
}

/// Where the renderer draws world point `p`, in screen pixels.
fn drawn_at(camera: &Camera, config: &Config, p: Vec2) -> Vec2 {
    let cam = world_camera(camera, config);
    let v = Vertex { pos: p, ..Default::default() };
    let ndc = world_vert(v, &cam, camera.aspect_ratio(), Vec2::ZERO).pos;
    Vec2::new((ndc.x + 1.) / 2. * camera.window.x, (1. - ndc.y) / 2. * camera.window.y)
}

fn offset(camera: &Camera, config: &Config) -> Vec2 {
    world_camera(camera, config).pos - camera.pos
}

fn close(a: Vec2, b: Vec2) -> bool {
    (a.x - b.x).abs() < EPS && (a.y - b.y).abs() < EPS
}

fn configs() -> Vec<Config> {
    [UiLayout::Split, UiLayout::Right, UiLayout::Left]
        .into_iter()
        .map(|layout| Config { layout, ..Default::default() })
        .collect()
}

#[test]
fn screen_to_world_inverts_the_renderer() {
    for config in configs() {
        let cam = camera(0.);
        let p = Vec2::new(350., 75.);
        let screen = drawn_at(&cam, &config, p);
        let back = navigation::screen_to_world(&cam, screen) + offset(&cam, &config);
        assert!(close(back, p), "{:?}: {back:?} != {p:?}", config.layout);
    }
}

#[test]
fn zoom_at_keeps_the_anchor_still() {
    for config in configs() {
        let cam = camera(0.);
        let anchor = Vec2::new(1200., 300.);
        let world = navigation::screen_to_world(&cam, anchor) + offset(&cam, &config);
        for zoom in [500., 1600., 9000.] {
            let zoomed = navigation::zoom_at(&cam, anchor, zoom);
            assert_eq!(zoomed.zoom, zoom);
            let screen = drawn_at(&zoomed, &config, world);
            assert!(close(screen, anchor), "{:?} at {zoom}: {screen:?}", config.layout);
        }
    }
}

#[test]
fn zoom_is_clamped() {
    let cam = camera(0.);
    let zoomed = navigation::zoom_at(&cam, Vec2::new(10., 10.), -5.);
    assert_eq!(zoomed.zoom, navigation::MIN_ZOOM);
}

#[test]
fn scroll_up_zooms_in_by_the_same_ratio_at_any_zoom() {
    let anchor = Vec2::new(800., 450.);
    let mut cam = camera(0.);
    let near = navigation::zoom_scroll(&cam, anchor, 50.).zoom / cam.zoom;
    cam.zoom = 20000.;
    let far = navigation::zoom_scroll(&cam, anchor, 50.).zoom / cam.zoom;
    assert!(near < 1.);
    assert!((near - far).abs() < 1e-5);
}

#[test]
fn fit_frames_the_box_in_the_canvas() {
    for config in configs() {
        let cam = camera(0.);
        let (min, max) = (Vec2::new(-300., -100.), Vec2::new(500., 260.));
        // canvas: right of a 250 px panel, under an 80 px header
        let (cmin, cmax) = (Vec2::new(250., 80.), Vec2::new(1600., 900.));
        let fitted = navigation::fit(&cam, min, max, cmin, cmax, offset(&cam, &config));

        let a = drawn_at(&fitted, &config, min);
        let b = drawn_at(&fitted, &config, max);
        let centre = drawn_at(&fitted, &config, (min + max) / 2.);
        assert!(close(centre, (cmin + cmax) / 2.), "{:?}: centre {centre:?}", config.layout);
        for p in [a, b] {
            assert!(p.x >= cmin.x && p.x <= cmax.x && p.y >= cmin.y && p.y <= cmax.y, "{p:?}");
        }
        // snug: the box fills the canvas on one axis, less the margin
        let filled = ((b.x - a.x) / (cmax.x - cmin.x)).max((a.y - b.y) / (cmax.y - cmin.y));
        assert!((filled - 1. / 1.2).abs() < 1e-3, "filled {filled}");
    }
}

#[test]
fn fit_on_a_point_keeps_zoom() {
    let config = Config::default();
    let cam = camera(0.);
    let p = Vec2::new(40., 40.);
    let (cmin, cmax) = (Vec2::new(0., 0.), Vec2::new(1600., 900.));
    let fitted = navigation::fit(&cam, p, p, cmin, cmax, offset(&cam, &config));
    assert_eq!(fitted.zoom, cam.zoom);
    assert!(close(drawn_at(&fitted, &config, p), Vec2::new(800., 450.)));
}
