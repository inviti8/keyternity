//! CPU pictures of the rig for the skinning agent (docs/AI_SKINNING.md §3.2–3.3): the
//! art as drawn (at rest or in a test pose), with bones, wireframe, vertex ids or one
//! bone's weights on top. On the CPU so it runs between frames and in tests.

use crate::*;
use image::{Rgba, RgbaImage};

/// What to draw.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// meshes to draw (empty: every textured bone)
    pub meshes: Vec<i32>,
    /// world rectangle (x0, y0, x1, y1) to show; `None` frames the drawn meshes
    pub crop: Option<(f32, f32, f32, f32)>,
    /// longest side in px
    pub size: u32,
    pub bones: bool,
    pub wireframe: bool,
    pub vertex_ids: bool,
    /// color vertices by this bone's share (blue 0 → red 1)
    pub weights_bone: Option<i32>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions { meshes: vec![], crop: None, size: 1024, bones: true, wireframe: false, vertex_ids: false, weights_bone: None }
    }
}

/// A picture and how its pixels map to world units.
pub struct Picture {
    pub image: RgbaImage,
    /// world rectangle shown (x0, y0, x1, y1), y up
    pub world: (f32, f32, f32, f32),
    /// px per world unit
    pub scale: f32,
    /// bones drawn: (id, name, color)
    pub legend: Vec<(i32, String, [u8; 3])>,
}

impl Picture {
    /// How to turn a pixel into world units, for the tool result.
    pub fn mapping(&self) -> String {
        let (x0, _, _, y1) = self.world;
        format!(
            "Image {}x{} px shows world x {:.1}..{:.1}, y {:.1}..{:.1} (y up). world_x = {:.2} + px_x / {:.4}; world_y = {:.2} - px_y / {:.4}.",
            self.image.width(),
            self.image.height(),
            self.world.0,
            self.world.2,
            self.world.1,
            self.world.3,
            x0,
            self.scale,
            y1,
            self.scale
        )
    }
}

/// Distinct bone colors, cycled.
const PALETTE: [[u8; 3]; 12] = [
    [230, 25, 75],
    [60, 180, 75],
    [255, 225, 25],
    [0, 130, 200],
    [245, 130, 48],
    [145, 30, 180],
    [70, 240, 240],
    [240, 50, 230],
    [210, 245, 60],
    [250, 190, 212],
    [0, 128, 128],
    [170, 110, 40],
];

/// One drawn mesh: its vertex ids, positions (world), UVs, triangles, texture, z.
struct Drawn<'a> {
    id: i32,
    ids: Vec<u32>,
    pos: Vec<Vec2>,
    uv: Vec<Vec2>,
    indices: Vec<u32>,
    image: Option<&'a image::DynamicImage>,
    zindex: i32,
}

fn drawn_meshes<'a>(armature: &'a Armature, world: &Vec<Bone>, only: &[i32]) -> Vec<Drawn<'a>> {
    let mut out = vec![];
    for b in world {
        if b.hidden || b.bind_owner.is_some() || (!only.is_empty() && !only.contains(&b.id)) {
            continue;
        }
        let Some(tex) = armature.tex_of(b.id) else { continue };
        let (ids, uv, indices, pos) = if b.vertices.is_empty() {
            // a plain textured rectangle (no mesh yet): placed as the renderer places it
            let (verts, indices) = renderer::create_tex_rect(&tex.size);
            let left = if renderer::is_facing_left(b.scale) { -1. } else { 1. };
            let pivot = utils::rotate(&(tex.size * b.pivot_pos), b.rot * left) * b.scale;
            let pos = verts.iter().map(|v| renderer::inherit_vert(v.pos, b, b.pivot_rot, b.pivot_scale) + pivot).collect();
            (verts.iter().map(|v| v.id).collect(), verts.iter().map(|v| v.uv).collect(), indices, pos)
        } else {
            let pos = bind_pose::drawn_vertices(armature, world, b.id);
            (b.vertices.iter().map(|v| v.id).collect(), b.vertices.iter().map(|v| v.uv).collect(), b.indices.clone(), pos)
        };
        out.push(Drawn { id: b.id, ids, pos, uv, indices, image: armature.tex_data(tex).map(|d| &d.image), zindex: b.zindex });
    }
    out.sort_by_key(|d| d.zindex);
    out
}

/// Draw `world` (the armature constructed at rest or in a test pose). `shares` gives
/// each mesh's vertex shares when a weight view is asked for.
pub fn render(armature: &Armature, world: &Vec<Bone>, opts: &RenderOptions) -> Picture {
    let meshes = drawn_meshes(armature, world, &opts.meshes);

    // the world rectangle: as asked, or the meshes' bounds with a margin
    let (x0, y0, x1, y1) = opts.crop.unwrap_or_else(|| {
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for m in &meshes {
            for p in &m.pos {
                b = (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y));
            }
        }
        if b.0 > b.2 {
            for bone in world {
                b = (b.0.min(bone.pos.x), b.1.min(bone.pos.y), b.2.max(bone.pos.x), b.3.max(bone.pos.y));
            }
        }
        let pad = ((b.2 - b.0).max(b.3 - b.1) * 0.05).max(10.);
        (b.0 - pad, b.1 - pad, b.2 + pad, b.3 + pad)
    });
    let (w, h) = ((x1 - x0).max(1.), (y1 - y0).max(1.));
    let size = opts.size.clamp(64, 2048) as f32;
    let scale = size / w.max(h);
    let (pw, ph) = ((w * scale).round().max(1.) as u32, (h * scale).round().max(1.) as u32);
    let to_px = |p: Vec2| Vec2::new((p.x - x0) * scale, (y1 - p.y) * scale);

    let mut img = RgbaImage::from_pixel(pw, ph, Rgba([46, 48, 54, 255]));
    let dim = opts.weights_bone.is_some();

    // the art
    for m in &meshes {
        let px: Vec<Vec2> = m.pos.iter().map(|p| to_px(*p)).collect();
        for t in m.indices.chunks_exact(3) {
            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
            if a >= px.len() || b >= px.len() || c >= px.len() {
                continue;
            }
            fill_triangle(&mut img, [px[a], px[b], px[c]], [m.uv[a], m.uv[b], m.uv[c]], m.image, dim);
        }
    }

    // wireframe and vertex ids
    if opts.wireframe || opts.vertex_ids || dim {
        for m in &meshes {
            let px: Vec<Vec2> = m.pos.iter().map(|p| to_px(*p)).collect();
            if opts.wireframe {
                for (i, j) in topology::edges(&m.indices) {
                    line(&mut img, px[i as usize], px[j as usize], Rgba([255, 255, 255, 110]), 1.);
                }
            }
            if let Some(bone) = opts.weights_bone {
                let mesh = armature.bones.iter().find(|b| b.id == m.id);
                let shares = mesh.map(|mesh| skin::vertex_shares(&armature.bones, mesh)).unwrap_or_default();
                for (k, p) in px.iter().enumerate() {
                    let s = shares.get(&m.ids[k]).map_or(0., |s| weights::share_of(s, bone));
                    let c = mesh_tools::hsv(240. * (1. - s.clamp(0., 1.)), 1., 1.);
                    disc(&mut img, *p, 3.5, Rgba([c.r, c.g, c.b, 255]));
                }
            }
            if opts.vertex_ids {
                for (k, p) in px.iter().enumerate() {
                    disc(&mut img, *p, 1.5, Rgba([255, 255, 255, 255]));
                    text(&mut img, *p + Vec2::new(3., -9.), &m.ids[k].to_string(), 1, Rgba([255, 255, 160, 255]));
                }
            }
        }
    }

    // bones: a blade from head to tail (a ring for a point bone) and the id
    let mut legend = vec![];
    if opts.bones {
        let mut n = 0;
        for b in world.iter().filter(|b| b.bind_owner.is_none() && armature.tex_of(b.id).is_none()) {
            let color = PALETTE[n % PALETTE.len()];
            n += 1;
            let rgba = Rgba([color[0], color[1], color[2], 255]);
            let head = to_px(b.pos);
            let tail = to_px(renderer::tail_pos(b));
            if (tail - head).mag() > 2. {
                line(&mut img, head, tail, Rgba([0, 0, 0, 200]), 5.);
                line(&mut img, head, tail, rgba, 3.);
            } else {
                ring(&mut img, head, 6., rgba);
            }
            disc(&mut img, head, 3., rgba);
            let at = if (tail - head).mag() > 2. { (head + tail) / 2. } else { head } + Vec2::new(5., -12.);
            text(&mut img, at, &b.id.to_string(), 2, rgba);
            legend.push((b.id, b.name.clone(), color));
        }
    }

    Picture { image: img, world: (x0, y0, x1, y1), scale, legend }
}

/// Fill a triangle with its texture (nearest sample), alpha-blended; `dim` darkens it
/// so a weight view on top stands out.
fn fill_triangle(img: &mut RgbaImage, p: [Vec2; 3], uv: [Vec2; 3], tex: Option<&image::DynamicImage>, dim: bool) {
    let area = (p[1].x - p[0].x) * (p[2].y - p[0].y) - (p[2].x - p[0].x) * (p[1].y - p[0].y);
    if area.abs() < 1e-6 {
        return;
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    let minx = p.iter().map(|q| q.x).fold(f32::MAX, f32::min).floor().max(0.) as i32;
    let maxx = p.iter().map(|q| q.x).fold(f32::MIN, f32::max).ceil().min(w as f32 - 1.) as i32;
    let miny = p.iter().map(|q| q.y).fold(f32::MAX, f32::min).floor().max(0.) as i32;
    let maxy = p.iter().map(|q| q.y).fold(f32::MIN, f32::max).ceil().min(h as f32 - 1.) as i32;
    let rgba = tex.map(|t| t.as_rgba8()).flatten();
    let (tw, th) = rgba.map_or((1, 1), |t| (t.width(), t.height()));
    for y in miny..=maxy {
        for x in minx..=maxx {
            let q = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((p[1].x - q.x) * (p[2].y - q.y) - (p[2].x - q.x) * (p[1].y - q.y)) / area;
            let w1 = ((p[2].x - q.x) * (p[0].y - q.y) - (p[0].x - q.x) * (p[2].y - q.y)) / area;
            let w2 = 1. - w0 - w1;
            if w0 < 0. || w1 < 0. || w2 < 0. {
                continue;
            }
            let u = uv[0] * w0 + uv[1] * w1 + uv[2] * w2;
            let mut c = match rgba {
                Some(t) => {
                    let tx = ((u.x * tw as f32) as u32).min(tw - 1);
                    let ty = ((u.y * th as f32) as u32).min(th - 1);
                    *t.get_pixel(tx, ty)
                }
                None => Rgba([200, 200, 200, 255]),
            };
            if dim {
                c = Rgba([c[0] / 2, c[1] / 2, c[2] / 2, c[3]]);
            }
            blend(img, x, y, c);
        }
    }
}

fn blend(img: &mut RgbaImage, x: i32, y: i32, c: Rgba<u8>) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 || c[3] == 0 {
        return;
    }
    let d = img.get_pixel_mut(x as u32, y as u32);
    let a = c[3] as f32 / 255.;
    for k in 0..3 {
        d[k] = (c[k] as f32 * a + d[k] as f32 * (1. - a)).round() as u8;
    }
    d[3] = 255;
}

fn line(img: &mut RgbaImage, a: Vec2, b: Vec2, c: Rgba<u8>, width: f32) {
    let len = (b - a).mag();
    let steps = (len * 2.).ceil().max(1.) as usize;
    for i in 0..=steps {
        let p = a + (b - a) * (i as f32 / steps as f32);
        disc(img, p, width / 2., c);
    }
}

fn disc(img: &mut RgbaImage, p: Vec2, r: f32, c: Rgba<u8>) {
    let r = r.max(0.5);
    for y in (p.y - r).floor() as i32..=(p.y + r).ceil() as i32 {
        for x in (p.x - r).floor() as i32..=(p.x + r).ceil() as i32 {
            if (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - p).mag() <= r {
                blend(img, x, y, c);
            }
        }
    }
}

fn ring(img: &mut RgbaImage, p: Vec2, r: f32, c: Rgba<u8>) {
    let n = 32;
    for k in 0..n {
        let (a, b) = (k as f32 / n as f32 * std::f32::consts::TAU, (k + 1) as f32 / n as f32 * std::f32::consts::TAU);
        line(img, p + Vec2::new(a.cos(), a.sin()) * r, p + Vec2::new(b.cos(), b.sin()) * r, c, 2.);
    }
}

/// Digits and '-' in a 3x5 pixel font, rows top to bottom, bits left to right.
fn glyph(ch: char) -> Option<[u8; 5]> {
    Some(match ch {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 7, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 1, 1, 1],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        '-' => [0, 0, 7, 0, 0],
        _ => return None,
    })
}

/// Draw digits at `at` (top left), each font pixel `scale + 1` px, outlined in black.
fn text(img: &mut RgbaImage, at: Vec2, s: &str, scale: u32, c: Rgba<u8>) {
    let px = (scale + 1) as i32;
    let draw = |img: &mut RgbaImage, color: Rgba<u8>, grow: i32| {
        for (k, ch) in s.chars().enumerate() {
            let Some(rows) = glyph(ch) else { continue };
            let ox = at.x as i32 + k as i32 * 4 * px;
            for (row, bits) in rows.iter().enumerate() {
                for col in 0..3 {
                    if bits & (4 >> col) == 0 {
                        continue;
                    }
                    for dy in -grow..px + grow {
                        for dx in -grow..px + grow {
                            blend(img, ox + col * px + dx, at.y as i32 + row as i32 * px + dy, color);
                        }
                    }
                }
            }
        }
    };
    draw(img, Rgba([0, 0, 0, 220]), 1);
    draw(img, c, 0);
}

/// PNG bytes of a picture.
pub fn png(img: &RgbaImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(vec![]);
    let _ = image::DynamicImage::ImageRgba8(img.clone()).write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

/// How a test pose distorts one mesh (§3.3).
#[derive(Debug, Default, Clone)]
pub struct Distortion {
    pub mesh: i32,
    pub triangles: usize,
    /// triangles that turned over: folds
    pub flipped: Vec<[u32; 3]>,
    /// triangles whose area changed by more than half: collapse or ballooning
    pub area_changed: Vec<[u32; 3]>,
    /// the largest edge length ratio, posed / rest (or rest / posed when shrinking)
    pub max_stretch: f32,
}

/// Compare a mesh's triangles at rest (`rest`) and posed (`posed`), positions by vertex
/// index.
pub fn distortion(mesh: &Bone, rest: &[Vec2], posed: &[Vec2]) -> Distortion {
    let mut d = Distortion { mesh: mesh.id, max_stretch: 1., ..Default::default() };
    let area = |p: &[Vec2], t: &[u32]| {
        let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
        ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)) / 2.
    };
    for t in mesh.indices.chunks_exact(3) {
        if t.iter().any(|i| *i as usize >= rest.len() || *i as usize >= posed.len()) {
            continue;
        }
        d.triangles += 1;
        let ids = [mesh.vertices[t[0] as usize].id, mesh.vertices[t[1] as usize].id, mesh.vertices[t[2] as usize].id];
        let (ar, ap) = (area(rest, t), area(posed, t));
        if ar.abs() > 1e-3 && ar.signum() != ap.signum() && ap.abs() > 1e-3 {
            d.flipped.push(ids);
        } else if ar.abs() > 1e-3 && !(0.5..=1.5).contains(&(ap / ar)) {
            d.area_changed.push(ids);
        }
        for (i, j) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let lr = (rest[j as usize] - rest[i as usize]).mag();
            let lp = (posed[j as usize] - posed[i as usize]).mag();
            if lr > 1e-3 && lp > 1e-3 {
                d.max_stretch = d.max_stretch.max((lp / lr).max(lr / lp));
            }
        }
    }
    d
}
