//! Render cameras (docs/FRAME_SEQUENCES.md §4): named rectangles on the canvas that set
//! what frame-sequence, spritesheet and video exports render, and at what resolution.
//!
//! A camera's world size is `resolution / scale`, so a character's pixel size depends
//! only on `scale`: every animation exported through one camera lines up. The pivot is
//! a world point (the character's origin by default), reported in output pixels.
//!
//! World space is y-up; output pixels are y-down from the top-left corner.

use crate::renderer;
use crate::shared::{Armature, Bone, Camera, Color, Vec2};

/// What resizing a camera changes.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum CameraLock {
    /// resizing changes the resolution; the character keeps its pixel size
    #[default]
    Scale,
    /// resizing changes the scale; the output stays the same size in pixels
    Resolution,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct RenderCamera {
    pub id: i32,
    pub name: String,
    /// centre, in world units
    pub pos: Vec2,
    /// output size in pixels
    pub resolution: Vec2,
    /// output pixels per world unit
    pub scale: f32,
    /// world point where the character's origin sits
    pub pivot: Vec2,
    pub lock: CameraLock,
    /// None: transparent
    pub background: Option<Color>,
    /// the resolution is a multiple of this (2: even, for video encoders)
    pub round_to: u32,
    /// a bone the camera follows through animations (its id), so moving animations stay
    /// in frame and export in place (§4.5)
    pub follow: Option<i32>,
    pub follow_x: bool,
    pub follow_y: bool,
}

impl Default for RenderCamera {
    fn default() -> Self {
        RenderCamera {
            id: 0,
            name: "Camera".to_string(),
            pos: Vec2::ZERO,
            resolution: Vec2::new(512., 512.),
            scale: 1.,
            pivot: Vec2::ZERO,
            lock: CameraLock::Scale,
            background: None,
            round_to: 2,
            follow: None,
            follow_x: true,
            follow_y: true,
        }
    }
}

/// A draggable part of a camera on the canvas.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Handle {
    /// 0 top-left, 1 top-right, 2 bottom-right, 3 bottom-left
    Corner(u8),
    /// 0 top, 1 right, 2 bottom, 3 left
    Side(u8),
    Move,
    Pivot,
}

/// Direction of each corner from the centre (y-up).
const CORNERS: [(f32, f32); 4] = [(-1., 1.), (1., 1.), (1., -1.), (-1., -1.)];
/// Direction of each side from the centre (y-up).
const SIDES: [(f32, f32); 4] = [(0., 1.), (1., 0.), (0., -1.), (-1., 0.)];

/// Smallest resolution a resize can make, in pixels.
const MIN_PX: f32 = 8.;

impl RenderCamera {
    /// Size in world units.
    pub fn world_size(&self) -> Vec2 {
        self.resolution / self.scale.max(1e-6)
    }

    /// Bottom-left corner, in world units.
    pub fn min(&self) -> Vec2 {
        self.pos - self.world_size() / 2.
    }

    /// Top-right corner, in world units.
    pub fn max(&self) -> Vec2 {
        self.pos + self.world_size() / 2.
    }

    /// A world point in output pixels (from the top-left, y down).
    pub fn to_px(&self, world: Vec2) -> Vec2 {
        let (min, max) = (self.min(), self.max());
        Vec2::new((world.x - min.x) * self.scale, (max.y - world.y) * self.scale)
    }

    pub fn pivot_px(&self) -> Vec2 {
        self.to_px(self.pivot)
    }

    /// The output size in whole pixels.
    pub fn px_size(&self) -> (u32, u32) {
        (self.resolution.x.round().max(1.) as u32, self.resolution.y.round().max(1.) as u32)
    }

    /// The camera that renders exactly this rectangle into the output, and the aspect
    /// ratio (height / width) to render with: `renderer::world_vert` maps x by
    /// `(x - pos.x) / zoom * aspect` and y by `(y - pos.y) / zoom`.
    pub fn view(&self) -> (Camera, f32) {
        let size = self.world_size();
        let camera = Camera {
            pos: self.pos,
            zoom: size.y / 2.,
            ..Default::default()
        };
        (camera, size.y / size.x)
    }

    /// Where each corner and side handle sits, in world units.
    pub fn handles(&self) -> Vec<(Handle, Vec2)> {
        let half = self.world_size() / 2.;
        let at = |(x, y): (f32, f32)| self.pos + Vec2::new(x * half.x, y * half.y);
        let mut handles = vec![];
        for (i, c) in CORNERS.iter().enumerate() {
            handles.push((Handle::Corner(i as u8), at(*c)));
        }
        for (i, s) in SIDES.iter().enumerate() {
            handles.push((Handle::Side(i as u8), at(*s)));
        }
        handles
    }

    /// Round the resolution up to a multiple of `round_to`, keeping the centre.
    pub fn round(&mut self) {
        let step = self.round_to.max(1) as f32;
        let up = |v: f32| ((v.max(1.) / step).ceil() * step).max(step);
        // nearly whole values shouldn't grow by a step from float error
        let snap = |v: f32| if (v - v.round()).abs() < 1e-3 { v.round() } else { v };
        self.resolution = Vec2::new(up(snap(self.resolution.x)), up(snap(self.resolution.y)));
    }
}

/// `start` with `handle` dragged by `delta` world units.
pub fn drag(start: &RenderCamera, handle: Handle, delta: Vec2) -> RenderCamera {
    let mut cam = start.clone();
    let (dir_x, dir_y) = match handle {
        Handle::Move => {
            cam.pos += delta;
            return cam;
        }
        Handle::Pivot => {
            cam.pivot += delta;
            return cam;
        }
        Handle::Corner(i) => CORNERS[i as usize % 4],
        Handle::Side(i) => SIDES[i as usize % 4],
    };

    // the opposite corner or side stays put
    let half = start.world_size() / 2.;
    let anchor = start.pos - Vec2::new(dir_x * half.x, dir_y * half.y);
    let dragged = start.pos + Vec2::new(dir_x * half.x, dir_y * half.y) + delta;
    let old = start.world_size();
    let min_world = MIN_PX / start.scale.max(1e-6);

    // new world size along each axis the handle moves; the other axis is unchanged
    let along = |dir: f32, d: f32, o: f32| if dir == 0. { o } else { (d * dir).max(min_world) };
    let mut size = Vec2::new(
        along(dir_x, dragged.x - anchor.x, old.x),
        along(dir_y, dragged.y - anchor.y, old.y),
    );

    match start.lock {
        CameraLock::Scale => {
            cam.resolution = size * start.scale;
            cam.round();
            size = cam.world_size();
        }
        CameraLock::Resolution => {
            // uniform: the axis that moved furthest sets the zoom
            let kx = if dir_x == 0. { 0. } else { size.x / old.x };
            let ky = if dir_y == 0. { 0. } else { size.y / old.y };
            let k = kx.max(ky).max(1e-3);
            cam.scale = start.scale / k;
            size = cam.world_size();
        }
    }

    // keep the anchor where it was (a side keeps its centre on the other axis)
    let centre = |dir: f32, a: f32, s: f32, p: f32| if dir == 0. { p } else { a + dir * s / 2. };
    cam.pos = Vec2::new(
        centre(dir_x, anchor.x, size.x, start.pos.x),
        centre(dir_y, anchor.y, size.y, start.pos.y),
    );
    cam
}

/// `cam` fitted around the world box `min..max`, with `pad` pixels on each side.
/// Scale lock grows or shrinks the resolution; Resolution lock changes the scale.
pub fn fit(cam: &RenderCamera, min: Vec2, max: Vec2, pad: f32) -> RenderCamera {
    let mut fitted = cam.clone();
    let size = Vec2::new((max.x - min.x).max(1e-3), (max.y - min.y).max(1e-3));
    fitted.pos = (min + max) / 2.;
    match cam.lock {
        CameraLock::Scale => {
            fitted.resolution = size * cam.scale + Vec2::new(pad * 2., pad * 2.);
            fitted.round();
        }
        CameraLock::Resolution => {
            let inner = cam.resolution - Vec2::new(pad * 2., pad * 2.);
            fitted.scale = (inner.x / size.x).min(inner.y / size.y).max(1e-3);
        }
    }
    fitted
}

/// The world bounds of the armature's visible textured meshes in the pose `bones`
/// (local transforms, e.g. from `Armature::animate`), as drawn by
/// `renderer::render_screenshot`. None if nothing is visible.
pub fn pose_bounds(armature: &Armature, bones: &Vec<Bone>) -> Option<(Vec2, Vec2)> {
    let posed = Armature {
        animated_bones: bones.clone(),
        ..Default::default()
    };
    let hidden = posed.get_propagated_hidden();
    let mut world = vec![];
    renderer::construction(&mut world, bones);

    let mut min = Vec2::new(f32::MAX, f32::MAX);
    let mut max = Vec2::new(f32::MIN, f32::MIN);
    for bone in &world {
        let Some(tex) = armature.tex_of(bone.id) else { continue };
        if *hidden.get(&bone.id).unwrap_or(&true) {
            continue;
        }
        let pivot = crate::utils::rotate(&(tex.size * bone.pivot_pos), bone.rot);
        for v in &bone.vertices {
            let p = v.pos + pivot;
            min = Vec2::new(min.x.min(p.x), min.y.min(p.y));
            max = Vec2::new(max.x.max(p.x), max.y.max(p.y));
        }
    }
    (min.x <= max.x).then_some((min, max))
}

/// A bone's world position in the pose `bones` (local transforms).
pub fn bone_world_pos(bones: &Vec<Bone>, id: i32) -> Option<Vec2> {
    let mut world = vec![];
    renderer::construction(&mut world, bones);
    world.iter().find(|b| b.id == id).map(|b| b.pos)
}

/// How far `cam` moves in the pose `posed` from the rest pose `rest`: by its followed
/// bone's movement, on the axes it follows. Zero if it follows nothing.
pub fn follow_offset(cam: &RenderCamera, rest: &Vec<Bone>, posed: &Vec<Bone>) -> Vec2 {
    let Some(id) = cam.follow else { return Vec2::ZERO };
    let (Some(a), Some(b)) = (bone_world_pos(rest, id), bone_world_pos(posed, id)) else { return Vec2::ZERO };
    let d = b - a;
    Vec2::new(if cam.follow_x { d.x } else { 0. }, if cam.follow_y { d.y } else { 0. })
}

/// The union of `pose_bounds` over every frame of the animations `anims` (indices).
/// Falls back to the rest pose when no animation has anything visible. With `follow`,
/// each frame is measured relative to where that camera follows to, so the bounds are
/// what the camera must cover.
pub fn animation_bounds(armature: &Armature, anims: &[usize], follow: Option<&RenderCamera>) -> Option<(Vec2, Vec2)> {
    let mut bounds: Option<(Vec2, Vec2)> = None;
    let add = |bounds: &mut Option<(Vec2, Vec2)>, b: Option<(Vec2, Vec2)>| {
        if let Some((lo, hi)) = b {
            *bounds = Some(match *bounds {
                Some((min, max)) => (
                    Vec2::new(min.x.min(lo.x), min.y.min(lo.y)),
                    Vec2::new(max.x.max(hi.x), max.y.max(hi.y)),
                ),
                None => (lo, hi),
            });
        }
    };
    // only bones and animations: cloning the textures would be slow
    let mut arm = Armature {
        bones: armature.bones.clone(),
        animations: armature.animations.clone(),
        ..Default::default()
    };
    for &a in anims {
        let Some(anim) = armature.animations.get(a) else { continue };
        let last = anim.last_frame();
        for f in 0..=last {
            let posed = arm.animate(a, f, Some(&armature.bones));
            let shift = follow.map_or(Vec2::ZERO, |cam| follow_offset(cam, &armature.bones, &posed));
            add(&mut bounds, pose_bounds(armature, &posed).map(|(lo, hi)| (lo - shift, hi - shift)));
        }
    }
    if bounds.is_none() {
        add(&mut bounds, pose_bounds(armature, &armature.bones));
    }
    bounds
}

/// A name not used by any of `cameras`: "Camera", "Camera 2", ...
pub fn unique_name(cameras: &[RenderCamera], base: &str) -> String {
    let taken = |n: &str| cameras.iter().any(|c| c.name == n);
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).unwrap()
}

/// An id not used by any of `cameras`.
pub fn next_id(cameras: &[RenderCamera]) -> i32 {
    cameras.iter().map(|c| c.id + 1).max().unwrap_or(0)
}
