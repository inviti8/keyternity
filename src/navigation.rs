//! Canvas navigation: the Pan and Zoom tools, cursor-anchored zoom, and Fit
//! (docs/TOPOLOGY_TOOLS.md §3).
//!
//! Screen points are physical pixels (`InputStates.mouse`). `Camera.zoom` is the
//! world half-height of the window, so a larger zoom shows more of the world.

use crate::*;

pub const MIN_ZOOM: f32 = 1.;
/// Zoom factor for one step: `=`/`-`, or a click with the Zoom tool.
pub const ZOOM_STEP: f32 = 1.25;
/// Zoom factor per pixel of scroll.
const SCROLL_RATE: f32 = 0.001;
/// Zoom factor per pixel of horizontal drag with the Zoom tool.
const DRAG_RATE: f32 = 0.005;
/// Pixels the pointer must move before a Zoom tool press counts as a drag.
const DRAG_THRESHOLD: f32 = 3.;
/// Room left around fitted content, as a fraction of its size.
const FIT_MARGIN: f32 = 1.2;

/// The world point under a screen point, without the layout's camera offset
/// (`renderer::world_camera`). The offset is a constant translation, so it cancels
/// out of anchored zooms; `fit` adds it back.
pub fn screen_to_world(camera: &Camera, screen: Vec2) -> Vec2 {
    let ndc = Vec2::new(
        screen.x / camera.window.x * 2. - 1.,
        1. - screen.y / camera.window.y * 2.,
    );
    Vec2::new(
        ndc.x * camera.zoom / camera.aspect_ratio() + camera.pos.x,
        ndc.y * camera.zoom + camera.pos.y,
    )
}

/// `camera` zoomed to `zoom`, keeping the world point under `anchor` where it is.
pub fn zoom_at(camera: &Camera, anchor: Vec2, zoom: f32) -> Camera {
    let mut cam = camera.clone();
    let before = screen_to_world(&cam, anchor);
    cam.zoom = zoom.max(MIN_ZOOM);
    cam.pos += before - screen_to_world(&cam, anchor);
    cam
}

/// `camera` zoomed by scroll (positive `delta` zooms in), anchored on `anchor`.
pub fn zoom_scroll(camera: &Camera, anchor: Vec2, delta: f32) -> Camera {
    zoom_at(camera, anchor, camera.zoom * (-delta * SCROLL_RATE).exp())
}

/// `camera` framing the world-space box `min..max` inside the screen rect
/// `canvas_min..canvas_max`. `offset` is the layout's camera offset. A box with no
/// size (a single bone) is centred at the current zoom.
pub fn fit(
    camera: &Camera,
    min: Vec2,
    max: Vec2,
    canvas_min: Vec2,
    canvas_max: Vec2,
    offset: Vec2,
) -> Camera {
    let mut cam = camera.clone();
    let size = max - min;
    let canvas = canvas_max - canvas_min;
    if canvas.x <= 0. || canvas.y <= 0. {
        return cam;
    }

    // a screen pixel is 2 * zoom / window height world units on both axes
    let needed = (size.x / canvas.x).max(size.y / canvas.y) * camera.window.y / 2.;
    if needed > 0. {
        cam.zoom = (needed * FIT_MARGIN).max(MIN_ZOOM);
    }

    // put the box's centre under the canvas's centre
    let centre = (min + max) / 2.;
    cam.pos = Vec2::ZERO;
    let at_origin = screen_to_world(&cam, (canvas_min + canvas_max) / 2.);
    cam.pos = centre - offset - at_origin;
    cam
}

/// World-space bounds of the given bones (all visible bones if `ids` is None):
/// textured bones by their drawn vertices, others by their position.
pub fn bones_bounds(bones: &[Bone], armature: &Armature, ids: Option<&[i32]>) -> Option<(Vec2, Vec2)> {
    let mut points = vec![];
    for bone in bones {
        if bone.hidden || ids.map_or(false, |ids| !ids.contains(&bone.id)) {
            continue;
        }
        points.push(bone.pos);
        let Some(tex) = armature.tex_of(bone.id) else {
            continue;
        };
        // the same pivot offset the renderer applies (`renderer::world_vert`)
        let left = if renderer::is_facing_left(bone.scale) { -1. } else { 1. };
        let pivot = utils::rotate(&(tex.size * bone.pivot_pos), bone.rot * left) * bone.scale;
        points.extend(bone.vertices.iter().map(|v| v.pos + pivot));
    }

    let first = *points.first()?;
    let mut min = first;
    let mut max = first;
    for p in points {
        min = Vec2::new(min.x.min(p.x), min.y.min(p.y));
        max = Vec2::new(max.x.max(p.x), max.y.max(p.y));
    }
    Some((min, max))
}

/// Pan and Zoom tool input on the canvas. Runs before the renderer, which then
/// doesn't see the left button (see `lib.rs`).
pub fn navigate(
    camera: &Camera,
    input: &InputStates,
    edit_mode: &EditMode,
    renderer: &mut Renderer,
    events: &mut EventState,
) {
    match edit_mode.tool {
        Tool::Pan => {
            if input.left_down && !camera.on_ui {
                let vel = renderer::mouse_vel(input, camera) * camera.zoom;
                events.edit_camera(camera.pos.x + vel.x, camera.pos.y + vel.y, camera.zoom);
            }
        }
        Tool::Zoom => {
            if input.left_pressed && !camera.on_ui {
                renderer.zoom_drag = Some((input.mouse, camera.zoom));
                renderer.zoom_dragged = false;
            }
            let Some((anchor, start_zoom)) = renderer.zoom_drag else {
                return;
            };
            let cam = if input.left_down {
                // drag right to zoom in, left to zoom out, around where it started
                let dx = input.mouse.x - anchor.x;
                renderer.zoom_dragged |= dx.abs() > DRAG_THRESHOLD;
                if !renderer.zoom_dragged {
                    return;
                }
                zoom_at(camera, anchor, start_zoom * (-dx * DRAG_RATE).exp())
            } else {
                // released: a click steps in (or out, holding Alt)
                renderer.zoom_drag = None;
                if renderer.zoom_dragged {
                    return;
                }
                let step = if input.holding_alt { ZOOM_STEP } else { 1. / ZOOM_STEP };
                zoom_at(camera, anchor, camera.zoom * step)
            };
            events.edit_camera(cam.pos.x, cam.pos.y, cam.zoom);
        }
        _ => {}
    }
}
