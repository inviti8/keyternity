//! Frame sequence export (docs/FRAME_SEQUENCES.md §5–7): planning, naming, output size,
//! the manifest and Phaser files, and the resize.

use keyternity_lib::export_sequence::{self as es, AnimChoice, SequenceOptions, SizeMode};
use keyternity_lib::render_camera::RenderCamera;
use keyternity_lib::shared::*;

fn anim(id: i32, name: &str, fps: i32, last: i32) -> Animation {
    Animation {
        id,
        name: name.to_string(),
        fps,
        keyframes: vec![Keyframe { frame: 0, ..Default::default() }, Keyframe { frame: last, ..Default::default() }],
        ..Default::default()
    }
}

fn rig() -> Armature {
    Armature {
        animations: vec![anim(0, "walk", 24, 5), anim(1, "idle", 12, 3)],
        ..Default::default()
    }
}

#[test]
fn plans_every_frame_with_padded_names() {
    let arm = rig();
    let frames = es::plan(&arm, &SequenceOptions::default()).unwrap();
    assert_eq!(frames.len(), 6 + 4);
    assert_eq!(frames[0].path, "walk/walk_0000.png");
    assert_eq!(frames[0].key, "walk/walk_0000");
    assert_eq!(frames[5].path, "walk/walk_0005.png");
    assert_eq!(frames[6].path, "idle/idle_0000.png", "each animation numbers from the start");
    assert!((frames[0].duration - 1. / 24.).abs() < 1e-6);
    assert!((frames[6].duration - 1. / 12.).abs() < 1e-6);
}

#[test]
fn range_step_and_skip_last() {
    let arm = rig();
    let mut opts = SequenceOptions {
        step: 2,
        start: 1,
        anims: vec![AnimChoice { id: 0, export: true, start: 1, end: 5 }, AnimChoice { id: 1, export: false, ..Default::default() }],
        ..Default::default()
    };
    let frames = es::plan(&arm, &opts).unwrap();
    // frames 1, 3, 5; the last only shows for one frame
    assert_eq!(frames.iter().map(|f| f.frame).collect::<Vec<_>>(), vec![1, 3, 5]);
    assert_eq!(frames[0].path, "walk/walk_0001.png", "{{index}} starts at 1");
    assert!((frames[0].duration - 2. / 24.).abs() < 1e-6);
    assert!((frames[2].duration - 1. / 24.).abs() < 1e-6);

    opts.skip_last = true;
    let frames = es::plan(&arm, &opts).unwrap();
    assert_eq!(frames.iter().map(|f| f.frame).collect::<Vec<_>>(), vec![1, 3]);
    assert!((frames[1].duration - 2. / 24.).abs() < 1e-6);
}

#[test]
fn names_must_be_unique() {
    let arm = rig();
    let opts = SequenceOptions { pattern: "frame".into(), ..Default::default() };
    assert!(es::plan(&arm, &opts).is_err(), "no number in the pattern");
    // flat, without {anim}: walk and idle collide
    let opts = SequenceOptions { pattern: "f_{index}".into(), anim_folders: false, ..Default::default() };
    assert!(es::plan(&arm, &opts).unwrap_err().contains("f_0000.png"));
    let opts = SequenceOptions { pattern: "{anim}-{frame}".into(), anim_folders: false, digits: 2, ..Default::default() };
    assert_eq!(es::plan(&arm, &opts).unwrap()[0].path, "walk-00.png");
    let none = SequenceOptions { anims: vec![AnimChoice { id: 0, export: false, ..Default::default() }, AnimChoice { id: 1, export: false, ..Default::default() }], ..Default::default() };
    assert!(es::plan(&arm, &none).is_err());
}

#[test]
fn sanitizes_names() {
    assert_eq!(es::sanitize("run/left: fast?"), "run_left_ fast_");
    assert_eq!(es::sanitize("  "), "_");
}

#[test]
fn sync_follows_the_animations() {
    let mut arm = rig();
    let mut opts = SequenceOptions {
        anims: vec![AnimChoice { id: 1, export: false, ..Default::default() }, AnimChoice { id: 9, ..Default::default() }],
        ..Default::default()
    };
    opts.sync(&arm);
    assert_eq!(opts.anims.iter().map(|c| (c.id, c.export)).collect::<Vec<_>>(), vec![(0, true), (1, false)]);
    arm.animations.remove(0);
    opts.sync(&arm);
    assert_eq!(opts.anims.len(), 1);
}

#[test]
fn output_size_keeps_proportions() {
    let cam = RenderCamera { resolution: Vec2::new(412., 588.), round_to: 1, ..Default::default() };
    let mut opts = SequenceOptions { size_value: 50., ..Default::default() };
    assert_eq!(es::output_size(&cam, &opts).1, (206, 294));
    opts.size_mode = SizeMode::Width;
    opts.size_value = 103.;
    let (factor, size) = es::output_size(&cam, &opts);
    assert!((factor - 0.25).abs() < 1e-6);
    assert_eq!(size, (103, 147));
    opts.size_mode = SizeMode::Height;
    opts.size_value = 1176.;
    assert_eq!(es::output_size(&cam, &opts).1, (824, 1176));
}

#[test]
fn manifest_uses_the_output_size() {
    let arm = rig();
    let cam = RenderCamera {
        resolution: Vec2::new(200., 400.),
        scale: 2.,
        pivot: Vec2::new(0., -50.),
        round_to: 1,
        ..Default::default()
    };
    let opts = SequenceOptions { size_value: 50., ..Default::default() };
    let frames = es::plan(&arm, &opts).unwrap();
    let m = es::manifest("alice", &arm, &cam, &opts, &frames, None);
    assert_eq!(m["size"], serde_json::json!([100, 200]));
    assert_eq!(m["scale"], 1.0);
    // world (0, -50) is 100 world units below the centre... in camera px (100, 300); halved
    assert_eq!(m["pivot"], serde_json::json!([50.0, 150.0]));
    assert_eq!(m["animations"][0]["frames"][0]["file"], "walk/walk_0000.png");
    assert_eq!(m["animations"].as_array().unwrap().len(), 2);
}

#[test]
fn phaser_files() {
    let arm = rig();
    let opts = SequenceOptions {
        step: 2,
        anims: vec![AnimChoice { id: 0, ..Default::default() }, AnimChoice { id: 1, export: false, ..Default::default() }],
        ..Default::default()
    };
    let frames = es::plan(&arm, &opts).unwrap();
    // frames 0, 2, 4 at 24 fps on twos: 12 fps; all full length
    let anims = es::phaser_anims(&arm, &opts, &frames, None);
    let walk = &anims["anims"][0];
    assert_eq!(walk["key"], "walk");
    assert_eq!(walk["frameRate"], 12.0);
    assert_eq!(walk["repeat"], -1);
    assert_eq!(walk["frames"].as_array().unwrap().len(), 3);
    assert_eq!(walk["frames"][0], serde_json::json!({ "key": "walk/walk_0000" }));

    let pack = es::phaser_pack("alice", &frames, None);
    let files = pack["alice"]["files"].as_array().unwrap();
    assert_eq!(files[0], serde_json::json!({ "type": "image", "key": "walk/walk_0000", "url": "walk/walk_0000.png" }));
    assert_eq!(files.last().unwrap()["url"], "alice_anims.json");

    let extra = es::extra_files("alice", &arm, &RenderCamera::default(), &opts, &frames, None);
    let names: Vec<&str> = extra.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["alice_frames.json", "alice_anims.json", "alice_pack.json"]);
}

#[test]
fn short_last_frame_gets_a_duration_offset() {
    let arm = Armature { animations: vec![anim(0, "walk", 10, 4)], ..Default::default() };
    let opts = SequenceOptions { step: 2, ..Default::default() };
    // frames 0, 2, 4: the last shows 1 frame (100 ms) instead of 2 (200 ms)
    let frames = es::plan(&arm, &opts).unwrap();
    let anims = es::phaser_anims(&arm, &opts, &frames, None);
    assert_eq!(anims["anims"][0]["frames"][2]["duration"], -100.0);
}

#[test]
fn resize_has_no_dark_fringes() {
    // opaque red beside transparent black
    let mut img = image::RgbaImage::new(8, 8);
    for (x, _, p) in img.enumerate_pixels_mut() {
        *p = if x < 4 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 0, 0]) };
    }
    let small = es::resize_premultiplied(&img, 4, 4);
    for p in small.pixels() {
        if p[3] > 10 {
            assert!(p[0] > 240 && p[1] < 15, "edge pixel darkened: {p:?}");
        }
    }
}

#[test]
fn render_size_supersamples_up_to_twice() {
    // 25% of 1600×2400: render at 50% (twice the output), not full size
    assert_eq!(es::render_size((1600, 2400), (400, 600)), (800, 1200));
    // 75%: twice would exceed the camera, so its full size
    assert_eq!(es::render_size((1600, 2400), (1200, 1800)), (1600, 2400));
    // 100% and larger render straight at the output size
    assert_eq!(es::render_size((1600, 2400), (1600, 2400)), (1600, 2400));
    assert_eq!(es::render_size((100, 100), (300, 300)), (300, 300));
}

fn wait(writer: &mut es::Writer) -> Result<(), String> {
    let start = std::time::Instant::now();
    loop {
        if let Some(r) = writer.result() {
            return r;
        }
        assert!(start.elapsed().as_secs() < 30, "writer never finished");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn writer_writes_frames_then_extra_files() {
    let dir = std::env::temp_dir().join(format!("keyternity_seq_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let extra = vec![("rig_frames.json".to_string(), "{}".to_string())];
    let mut writer = es::Writer::spawn(es::Target::Folder(dir.clone()), (4, 4), (2, 2), extra, None);
    // blue in BGRA order: B=255
    let blue_bgra: Vec<u8> = [255u8, 0, 0, 255].repeat(16);
    for i in 0..3 {
        assert!(writer.send(es::RawFrame { path: format!("walk/walk_{i:04}.png"), pixels: blue_bgra.clone(), bgra: true }));
    }
    writer.close();
    wait(&mut writer).unwrap();
    assert_eq!(writer.written.load(std::sync::atomic::Ordering::SeqCst), 3);
    let img = image::open(dir.join("walk/walk_0002.png")).unwrap().into_rgba8();
    assert_eq!(img.dimensions(), (2, 2));
    assert_eq!(img.get_pixel(0, 0).0, [0, 0, 255, 255], "BGRA swapped to RGBA");
    assert!(dir.join("rig_frames.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn writer_stops_on_a_bad_frame() {
    let dir = std::env::temp_dir().join(format!("keyternity_seq_bad_{}", std::process::id()));
    let mut writer = es::Writer::spawn(es::Target::Folder(dir.clone()), (4, 4), (4, 4), vec![], None);
    writer.send(es::RawFrame { path: "a.png".into(), pixels: vec![0; 7], bgra: false });
    writer.close();
    assert!(wait(&mut writer).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}


fn sheet_opts() -> SequenceOptions {
    es::effective(&SequenceOptions { sheet_padding: 2, ..Default::default() }, true)
}

#[test]
fn packs_a_square_grid() {
    let arm = rig();
    let opts = sheet_opts();
    let frames = es::plan(&arm, &opts).unwrap();
    assert_eq!(frames[0].key, "walk_0000", "spritesheet frames are named without folders");
    // 10 frames of 100×100: 4 columns, 3 rows
    let plan = es::pack_sheets("alice", &arm, &frames, (100, 100), &opts).unwrap();
    assert_eq!(plan.files, vec!["alice.png"]);
    assert_eq!(plan.sizes, vec![(4 * 100 + 3 * 2, 3 * 100 + 2 * 2)]);
    assert_eq!(plan.placements[0], (0, 0, 0));
    assert_eq!(plan.placements[1], (0, 102, 0));
    assert_eq!(plan.placements[4], (0, 0, 102));
    // tall frames: fewer rows, more columns
    let plan = es::pack_sheets("alice", &arm, &frames, (50, 200), &opts).unwrap();
    let (w, h) = plan.sizes[0];
    assert!((w as f32 / h as f32 - 1.).abs() < 0.6, "{w}×{h}");
}

#[test]
fn spills_onto_more_sheets() {
    let arm = rig();
    let mut opts = sheet_opts();
    opts.sheet_max = 1024;
    let frames = es::plan(&arm, &opts).unwrap();
    // 400×600 cells: 2 across, 1 down per 1024 sheet: 10 frames on 5 sheets
    let plan = es::pack_sheets("alice", &arm, &frames, (400, 600), &opts).unwrap();
    assert_eq!(plan.files, vec!["alice_0.png", "alice_1.png", "alice_2.png", "alice_3.png", "alice_4.png"]);
    assert!(plan.sizes.iter().all(|(w, h)| *w <= 1024 && *h <= 1024));
    assert_eq!(plan.placements[3], (1, 402, 0));
    // too big for any sheet
    assert!(es::pack_sheets("alice", &arm, &frames, (2000, 600), &opts).is_err());
}

#[test]
fn sheets_per_animation_and_fixed_columns() {
    let arm = rig();
    let mut opts = sheet_opts();
    opts.sheet_per_anim = true;
    opts.sheet_columns = 2;
    let frames = es::plan(&arm, &opts).unwrap();
    let plan = es::pack_sheets("alice", &arm, &frames, (10, 10), &opts).unwrap();
    assert_eq!(plan.files, vec!["walk.png", "idle.png"]);
    // walk: 6 frames in 2 columns, 3 rows
    assert_eq!(plan.sizes[0], (22, 34));
    assert_eq!(plan.placements[6], (1, 0, 0), "idle starts its own sheet");
}

#[test]
fn atlas_formats() {
    let arm = rig();
    let opts = sheet_opts();
    let frames = es::plan(&arm, &opts).unwrap();
    let cam = RenderCamera { resolution: Vec2::new(100., 200.), pivot: Vec2::new(0., -50.), round_to: 1, ..Default::default() };
    let one = es::pack_sheets("alice", &arm, &frames, (100, 200), &opts).unwrap();
    let hash = es::atlas_json(&cam, &opts, &frames, &one);
    let f = &hash["frames"]["walk_0001"];
    assert_eq!(f["frame"], serde_json::json!({ "x": 102, "y": 0, "w": 100, "h": 200 }));
    assert_eq!(f["sourceSize"], serde_json::json!({ "w": 100, "h": 200 }));
    // pivot at world (0, -50): pixel (50, 150) of 100×200
    assert_eq!(f["anchor"], serde_json::json!({ "x": 0.5, "y": 0.75 }));
    assert_eq!(hash["meta"]["image"], "alice.png");

    let mut small = opts.clone();
    small.sheet_max = 256;
    let many = es::pack_sheets("alice", &arm, &frames, (100, 200), &small).unwrap();
    assert!(many.files.len() > 1);
    let multi = es::atlas_json(&cam, &small, &frames, &many);
    let textures = multi["textures"].as_array().unwrap();
    assert_eq!(textures.len(), many.files.len());
    assert_eq!(textures[0]["image"], "alice_0.png");
    assert_eq!(textures[0]["frames"][0]["filename"], "walk_0000");
    let total: usize = textures.iter().map(|t| t["frames"].as_array().unwrap().len()).sum();
    assert_eq!(total, frames.len());

    // Phaser: the animations use the atlas's frames; the pack loads the atlas
    let anims = es::phaser_anims(&arm, &opts, &frames, Some("alice"));
    assert_eq!(anims["anims"][0]["frames"][0], serde_json::json!({ "key": "alice", "frame": "walk_0000" }));
    let pack = es::phaser_pack("alice", &frames, Some(&one));
    assert_eq!(pack["alice"]["files"][0], serde_json::json!({ "type": "atlas", "key": "alice", "textureURL": "alice.png", "atlasURL": "alice_atlas.json" }));
    let pack = es::phaser_pack("alice", &frames, Some(&many));
    assert_eq!(pack["alice"]["files"][0]["type"], "multiatlas");
    let names: Vec<String> = es::extra_files("alice", &arm, &cam, &opts, &frames, Some(&one)).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, vec!["alice_frames.json", "alice_atlas.json", "alice_anims.json", "alice_pack.json"]);
}

#[test]
fn writer_fills_sheets() {
    let arm = rig();
    let opts = sheet_opts();
    let frames: Vec<_> = es::plan(&arm, &opts).unwrap().into_iter().take(3).collect();
    let mut small = opts.clone();
    small.sheet_max = 64;
    small.sheet_columns = 1;
    // 3 frames of 2×2 in one column of a 64 sheet: one sheet, 2 wide, 2+2+2 + 2 gaps of 2
    let plan = es::pack_sheets("alice", &arm, &frames, (2, 2), &small).unwrap();
    assert_eq!(plan.sizes, vec![(2, 10)]);
    let dir = std::env::temp_dir().join(format!("keyternity_sheet_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut writer = es::Writer::spawn(es::Target::Folder(dir.clone()), (2, 2), (2, 2), vec![], Some(plan));
    for (i, f) in frames.iter().enumerate() {
        let px = [i as u8 * 100, 0, 0, 255].repeat(4);
        writer.send(es::RawFrame { path: f.path.clone(), pixels: px, bgra: false });
    }
    writer.close();
    wait(&mut writer).unwrap();
    let sheet = image::open(dir.join("alice.png")).unwrap().into_rgba8();
    assert_eq!(sheet.dimensions(), (2, 10));
    assert_eq!(sheet.get_pixel(1, 1).0, [0, 0, 0, 255]);
    assert_eq!(sheet.get_pixel(0, 2).0, [0, 0, 0, 0], "padding stays clear");
    assert_eq!(sheet.get_pixel(0, 4).0, [100, 0, 0, 255]);
    assert_eq!(sheet.get_pixel(1, 9).0, [200, 0, 0, 255]);
    assert!(!dir.join("walk_0000.png").exists(), "no loose frames");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn finds_frames_outside_the_camera() {
    // a 20×10 textured bone that moves right by 10 a frame over frames 0..=4
    let size = Vec2::new(20., 10.);
    let (vertices, indices) = keyternity_lib::renderer::create_tex_rect(&size);
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
    let key = |frame: i32, x: f32| Keyframe { frame, bone_id: 0, element: AnimElement::PositionX, value: x, ..Default::default() };
    let tex = Texture { name: "body".into(), size, ..Default::default() };
    let arm = Armature {
        bones: vec![bone],
        styles: vec![Style { id: 0, name: "Default".into(), active: true, textures: vec![tex] }],
        animations: vec![Animation { id: 0, name: "slide".into(), fps: 10, keyframes: vec![key(0, 0.), key(4, 40.)], ..Default::default() }],
        ..Default::default()
    };
    let frames = es::plan(&arm, &SequenceOptions::default()).unwrap();
    // a camera 60 wide centred at 0: x -30..30; the art spans x-10..x+10, so x > 20 is out
    let cam = RenderCamera { resolution: Vec2::new(60., 40.), round_to: 1, ..Default::default() };
    assert_eq!(es::outside_frames(&arm, &frames, &cam), vec!["slide: frames 3–4".to_string()]);
    let wide = RenderCamera { resolution: Vec2::new(200., 40.), round_to: 1, ..Default::default() };
    assert!(es::outside_frames(&arm, &frames, &wide).is_empty());
}


#[test]
fn engine_notes_for_the_summary() {
    // 100×200 output, pivot at pixel (50, 150)
    let cam = RenderCamera { resolution: Vec2::new(100., 200.), pivot: Vec2::new(0., -50.), round_to: 1, ..Default::default() };
    let opts = SequenceOptions::default();
    let notes = es::engine_notes("alice", &cam, &opts, None);
    assert!(notes.contains("Offset (0, -50)"), "{notes}");
    assert!(notes.contains("Pivot Custom (0.5, 0.25)"), "{notes}");
    assert!(notes.contains("this.load.pack('alice', 'alice_pack.json')"), "{notes}");
    assert!(!notes.contains("Slice Grid"));
    let sheet = es::engine_notes("alice", &cam, &es::effective(&opts, true), Some(1));
    assert!(sheet.contains("frameWidth: 100, frameHeight: 200, spacing: 2"), "{sheet}");
    assert!(sheet.contains("Slice Grid By Cell Size 100 × 200, Padding 2"), "{sheet}");
    let no_phaser = SequenceOptions { phaser: false, ..Default::default() };
    assert!(!es::engine_notes("alice", &cam, &no_phaser, None).contains("Phaser"));
}
