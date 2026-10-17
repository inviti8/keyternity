//! A visual check of lip sync (opt-in): a line lip-synced onto a face rig, drawn frame
//! by frame the way the Face Rig panel draws FaceBot's mask (the face skinned to the
//! controls, lit, with the manipulators on top), as PNG frames and contact sheets.
//!
//! `LIPSYNC_PREVIEW_DIR=<out> LIPSYNC_PREVIEW_WAV=<line.wav> LIPSYNC_PREVIEW_TEXT=<words>
//! cargo test --test all -- lipsync_preview --ignored --nocapture`
//!
//! Each sheet frame has a bar under it, coloured by the viseme cued at or before it,
//! and the frame list (with visemes) is written to `cues.txt`.

use keyternity_lib::audio::{self, AudioClip};
use keyternity_lib::face;
use keyternity_lib::face_panel;
use keyternity_lib::layers;
use keyternity_lib::lipsync::{self, LipSyncTrack};
use keyternity_lib::shared::*;
use std::collections::HashMap;

type Img = image::RgbaImage;

/// Fill a triangle (screen pixels), supersampled by the image's own scale.
fn fill(img: &mut Img, t: [(f32, f32); 3], c: [u8; 3]) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let min_x = t.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor().max(0.) as i32;
    let max_x = t.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil().min(w as f32 - 1.) as i32;
    let min_y = t.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.) as i32;
    let max_y = t.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil().min(h as f32 - 1.) as i32;
    let edge = |a: (f32, f32), b: (f32, f32), p: (f32, f32)| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    let area = edge(t[0], t[1], t[2]);
    if area.abs() < 1e-6 {
        return;
    }
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let (w0, w1, w2) = (edge(t[1], t[2], p), edge(t[2], t[0], p), edge(t[0], t[1], p));
            let inside = if area > 0. { w0 >= 0. && w1 >= 0. && w2 >= 0. } else { w0 <= 0. && w1 <= 0. && w2 <= 0. };
            if inside {
                img.put_pixel(x as u32, y as u32, image::Rgba([c[0], c[1], c[2], 255]));
            }
        }
    }
}

fn lit(c: [u8; 3], shade: f32) -> [u8; 3] {
    c.map(|v| (v as f32 * shade).clamp(0., 255.) as u8)
}

const W: u32 = 240;
const H: u32 = 300;
const SS: u32 = 2;

/// One frame, as the Face Rig panel draws it.
fn draw(arm: &Armature, posed: &[Bone], controls: &[(String, i32)]) -> Img {
    let art = face_panel::art();
    let mut img = Img::from_pixel(W * SS, H * SS, image::Rgba([38, 40, 46, 255]));
    let scale = (W * SS) as f32 / 18.;
    let to = |x: f32, y: f32| ((W * SS) as f32 / 2. + x * scale, (H * SS) as f32 * 0.42 - y * scale);
    for part in &art.mask {
        for (i, t) in part.tris.iter().enumerate() {
            let col = lit(part.color, part.shade.get(i).copied().unwrap_or(1.));
            fill(&mut img, [to(t[0][0], t[0][1]), to(t[1][0], t[1][1]), to(t[2][0], t[2][1])], col);
        }
    }
    let mut xf: HashMap<String, face_panel::ControlPose> = HashMap::new();
    for a in &art.controls {
        if let Some((_, bone)) = controls.iter().find(|(n, _)| *n == a.name) {
            let mut p = face_panel::placement(arm, posed, *bone);
            p.rest = a.rest;
            xf.insert(a.name.clone(), p);
        }
    }
    if let Some(f) = &art.face {
        let v = face_panel::deform(f, &xf);
        for t in &f.tris {
            let (a, b, c) = (v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]);
            fill(&mut img, [to(a[0], a[1]), to(b[0], b[1]), to(c[0], c[1])], lit(f.color, face_panel::shade_of(a, b, c)));
        }
    }
    for a in &art.controls {
        let Some((_, bone)) = controls.iter().find(|(n, _)| *n == a.name) else { continue };
        let p = face_panel::placement(arm, posed, *bone);
        let (c, s) = (p.twist.cos(), p.twist.sin());
        let grow = (1. + 0.08 * p.depth).max(0.5);
        let at = |x: f32, y: f32| {
            let (x, y) = (x * grow, y * p.scale_y.max(0.15) * grow);
            to(a.rest[0] + p.off.x + x * c - y * s, a.rest[1] + p.off.y + x * s + y * c)
        };
        for (i, t) in a.tris.iter().enumerate() {
            fill(&mut img, [at(t[0][0], t[0][1]), at(t[1][0], t[1][1]), at(t[2][0], t[2][1])], lit(a.color, a.shade.get(i).copied().unwrap_or(1.)));
        }
    }
    image::imageops::resize(&img, W, H, image::imageops::FilterType::Triangle)
}

/// A colour per viseme, for the bars under the sheet's frames.
fn viseme_color(v: &str) -> [u8; 3] {
    match v {
        "rest" => [70, 70, 70],
        "MBP" => [220, 60, 60],
        "FV" => [230, 150, 40],
        "etc" => [150, 150, 150],
        "L" => [240, 220, 60],
        "AI" => [60, 200, 90],
        "E" => [60, 190, 220],
        "O" => [70, 110, 240],
        "U" => [150, 80, 230],
        "WQ" => [230, 90, 200],
        _ => [255, 255, 255],
    }
}

#[test]
#[ignore]
fn lipsync_preview() {
    let Ok(out) = std::env::var("LIPSYNC_PREVIEW_DIR") else { return };
    let wav = std::fs::read(std::env::var("LIPSYNC_PREVIEW_WAV").unwrap()).unwrap();
    let text = std::env::var("LIPSYNC_PREVIEW_TEXT").unwrap();
    let out = std::path::Path::new(&out);
    std::fs::create_dir_all(out.join("frames")).unwrap();

    // a face mesh with the standard shapes, and a face rig over it
    let v = |id: u32, x: f32, y: f32| Vertex { id, pos: Vec2::new(x, y), init_pos: Vec2::new(x, y), ..Default::default() };
    let mut mesh = Bone {
        id: 1,
        parent_id: -1,
        name: "face".into(),
        scale: Vec2::new(1., 1.),
        pivot_scale: Vec2::new(1., 1.),
        vertices: vec![v(0, -70., -130.), v(1, 70., -130.), v(2, 70., 86.), v(3, -70., 86.)],
        indices: vec![0, 1, 2, 0, 2, 3],
        ..Default::default()
    };
    face::add_standard_shapes(&mut mesh, false);
    let mut arm = Armature { bones: vec![mesh], ..Default::default() };
    face::set_up(&mut arm, 1, "Face");
    let fps = 24;
    arm.animations = vec![Animation { id: 0, name: "talk".into(), fps, ..Default::default() }];
    arm.audio = vec![audio::decode(1, "line", "wav", wav).unwrap()];
    arm.animations[0].clips = vec![AudioClip { id: 1, audio: 1, name: "line".into(), ..Default::default() }];
    let (root, controls) = face_panel::rigs(&arm)[0].clone();
    let track = LipSyncTrack { id: 1, clip: 1, mouth: root, text: text.clone(), ..Default::default() };
    arm.animations[0].lipsync = vec![track.clone()];
    let n = lipsync::generate(&mut arm, 0, 1).unwrap();

    // the cues, for the bars and the list
    let rig = lipsync::rig_for(&arm, root, None);
    let anim = &arm.animations[0];
    let words = &anim.lipsync[0].words;
    let cues = lipsync::frame_cues(&lipsync::cues_from_words(words, track.settings.rest_gap, track.language), &rig, fps, 0, &track.settings);
    let last = anim.last_frame() + 6;
    let viseme_at = |f: i32| cues.iter().rev().find(|c| c.0 <= f).map_or("rest", |c| c.1.as_str());
    let mut list = format!("{n} keys; words: {}\n", words.iter().map(|w| format!("{}@{:.2}", w.text, w.start)).collect::<Vec<_>>().join(" "));
    for f in 0..=last {
        list += &format!("{f:4} {:6.3}s {}\n", f as f32 / fps as f32, viseme_at(f));
    }
    std::fs::write(out.join("cues.txt"), &list).unwrap();

    // frames, and contact sheets of 40 frames (8 × 5)
    let frames: Vec<Img> = (0..=last)
        .map(|f| {
            let posed = layers::pose(&arm.animations[0], &arm.bones, f);
            let img = draw(&arm, &posed, &controls);
            img.save(out.join("frames").join(format!("f{f:04}.png"))).unwrap();
            img
        })
        .collect();
    let (cw, ch, cols, rows) = (W / 2, H / 2 + 8, 8u32, 5u32);
    for (s, chunk) in frames.chunks((cols * rows) as usize).enumerate() {
        let mut sheet = Img::from_pixel(cw * cols, ch * rows, image::Rgba([20, 20, 24, 255]));
        for (i, img) in chunk.iter().enumerate() {
            let f = s as i32 * (cols * rows) as i32 + i as i32;
            let small = image::imageops::resize(img, cw, H / 2, image::imageops::FilterType::Triangle);
            let (x, y) = ((i as u32 % cols) * cw, (i as u32 / cols) * ch);
            image::imageops::overlay(&mut sheet, &small, x as i64, y as i64);
            let c = viseme_color(viseme_at(f));
            for yy in y + H / 2 + 1..y + ch - 1 {
                for xx in x + 2..x + cw - 2 {
                    sheet.put_pixel(xx, yy, image::Rgba([c[0], c[1], c[2], 255]));
                }
            }
        }
        sheet.save(out.join(format!("sheet{s}.png"))).unwrap();
    }
    println!("{} frames, {n} keys -> {}", frames.len(), out.display());
}
