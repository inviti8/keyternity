//! PSD flattening with layers that extend past the canvas, as GIMP leaves
//! them after cropping the canvas without cropping the layers.

/// A minimal 8-bit RGB PSD of `canvas` size with one raw (uncompressed)
/// layer at `(left, top)` whose red channel is `40 * x` for layer column x.
fn psd_bytes(canvas: (u32, u32), left: i32, top: i32, size: (u32, u32)) -> Vec<u8> {
    let (cw, ch) = canvas;
    let (lw, lh) = size;
    let mut b = vec![];

    // file header
    b.extend(b"8BPS");
    b.extend(1u16.to_be_bytes());
    b.extend([0u8; 6]);
    b.extend(3u16.to_be_bytes()); // channels
    b.extend(ch.to_be_bytes());
    b.extend(cw.to_be_bytes());
    b.extend(8u16.to_be_bytes()); // depth
    b.extend(3u16.to_be_bytes()); // RGB

    // color mode data, image resources
    b.extend(0u32.to_be_bytes());
    b.extend(0u32.to_be_bytes());

    // layer record
    let channel_len = 2 + lw * lh;
    let mut rec = vec![];
    rec.extend(top.to_be_bytes());
    rec.extend(left.to_be_bytes());
    rec.extend((top + lh as i32).to_be_bytes());
    rec.extend((left + lw as i32).to_be_bytes());
    rec.extend(4u16.to_be_bytes());
    for id in [-1i16, 0, 1, 2] {
        rec.extend(id.to_be_bytes());
        rec.extend(channel_len.to_be_bytes());
    }
    rec.extend(b"8BIMnorm");
    rec.extend([255, 0, 0, 0]); // opacity, clipping, flags (visible), filler
    let name = b"\x05layer\0\0"; // pascal string padded to 4 bytes
    rec.extend((8 + name.len() as u32).to_be_bytes());
    rec.extend(0u32.to_be_bytes()); // mask data
    rec.extend(0u32.to_be_bytes()); // blending ranges
    rec.extend(name);

    // channel image data: alpha, red, green, blue
    let mut chans = vec![];
    for c in 0..4 {
        chans.extend(0u16.to_be_bytes()); // raw
        for _y in 0..lh {
            for x in 0..lw {
                chans.push(match c {
                    0 => 255,
                    1 => (40 * x) as u8,
                    _ => 0,
                });
            }
        }
    }

    let mut layer_info = vec![];
    layer_info.extend(1i16.to_be_bytes());
    layer_info.extend(rec);
    layer_info.extend(chans);

    let mut lmi = vec![];
    lmi.extend((layer_info.len() as u32).to_be_bytes());
    lmi.extend(layer_info);
    lmi.extend(0u32.to_be_bytes()); // global mask info
    b.extend((lmi.len() as u32).to_be_bytes());
    b.extend(lmi);

    // composite image data
    b.extend(0u16.to_be_bytes());
    b.extend(vec![0u8; (cw * ch * 3) as usize]);
    b
}

fn flatten(bytes: &[u8]) -> (Vec<u8>, u32, u32, u32, u32) {
    let psd = psd::Psd::from_bytes(bytes).unwrap();
    psd.flatten_layers_rgba(&|_| true).unwrap()
}

#[test]
fn layer_larger_than_canvas_is_clipped() {
    // 6x3 layer at (0, 0) on a 4x2 canvas
    let (px, w, h, x, y) = flatten(&psd_bytes((4, 2), 0, 0, (6, 3)));
    assert_eq!((w, h, x, y), (4, 2, 0, 0));
    for row in 0..2 {
        for col in 0..4 {
            let i = (row * 4 + col) * 4;
            assert_eq!(&px[i..i + 4], &[40 * col as u8, 0, 0, 255], "pixel ({col}, {row})");
        }
    }
}

#[test]
fn layer_at_negative_offset_is_clipped() {
    // 6x3 layer at (-1, -1) on a 4x2 canvas: canvas x shows layer column x + 1
    let (px, w, h, x, y) = flatten(&psd_bytes((4, 2), -1, -1, (6, 3)));
    assert_eq!((w, h, x, y), (4, 2, 0, 0));
    for row in 0..2 {
        for col in 0..4 {
            let i = (row * 4 + col) * 4;
            assert_eq!(&px[i..i + 4], &[40 * (col + 1) as u8, 0, 0, 255], "pixel ({col}, {row})");
        }
    }
}

#[test]
fn layer_inside_canvas_keeps_its_full_size() {
    // 2x1 layer at (1, 1) on a 4x2 canvas: crop is exactly the layer
    let (px, w, h, x, y) = flatten(&psd_bytes((4, 2), 1, 1, (2, 1)));
    assert_eq!((w, h, x, y), (2, 1, 1, 1));
    assert_eq!(px, vec![0, 0, 0, 255, 40, 0, 0, 255]);
}

// Building the rig: every visible layer is its own bone (src/psd_import.rs)

use keyternity_lib::psd_import::{self, Gpu, PsdTree, TreeGroup, TreeLayer};
use keyternity_lib::shared::*;

/// A solid `w` x `h` art layer at `(x, y)`; `shade` makes its pixels distinct.
fn art(name: &str, parent: Option<u32>, x: f32, y: f32, w: u32, h: u32, shade: u8) -> TreeLayer {
    TreeLayer {
        name: name.to_string(),
        parent,
        tl: Vec2::new(x, y),
        size: Vec2::new(w as f32, h as f32),
        image: Some(image::RgbaImage::from_pixel(w, h, image::Rgba([shade, 0, 0, 255]))),
    }
}

fn marker(name: &str, parent: Option<u32>, x: f32, y: f32) -> TreeLayer {
    TreeLayer { name: name.to_string(), parent, tl: Vec2::new(x, y), size: Vec2::new(2., 2.), image: None }
}

fn group(id: u32, name: &str, parent: Option<u32>) -> TreeGroup {
    TreeGroup { id, name: name.to_string(), parent }
}

fn build(groups: Vec<TreeGroup>, layers: Vec<TreeLayer>) -> Armature {
    let tree = PsdTree { width: 100, height: 100, groups, layers };
    let mut arm = Armature::default();
    psd_import::build_armature(&tree, &mut arm, "Default", Gpu::default());
    arm
}

fn bone<'a>(arm: &'a Armature, name: &str) -> &'a Bone {
    arm.bones.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no bone {name}"))
}

/// World position (rotation-aware, like `renderer::inheritance`): the
/// parent's world position plus the local position turned by the parent's
/// world rotation.
fn world(arm: &Armature, name: &str) -> Vec2 {
    world_of(arm, bone(arm, name))
}

fn world_of(arm: &Armature, b: &Bone) -> Vec2 {
    match arm.bones.iter().find(|x| x.id == b.parent_id) {
        None => b.pos,
        Some(p) => world_of(arm, p) + keyternity_lib::utils::rotate(&b.pos, rot_of(arm, p)),
    }
}

fn rot_of(arm: &Armature, b: &Bone) -> f32 {
    let mut rot = b.rot;
    let mut p = b.parent_id;
    while p != -1 {
        let pb = arm.bones.iter().find(|x| x.id == p).unwrap();
        rot += pb.rot;
        p = pb.parent_id;
    }
    rot
}

fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).mag() < 1e-3
}

#[test]
fn layers_in_one_group_stay_separate_bones() {
    // the Alice case: two layers in one group, top layer first
    let arm = build(
        vec![group(1, "alice", None)],
        vec![art("body", Some(1), 10., 10., 40, 80, 1), art("back_arm", Some(1), 50., 20., 20, 50, 2)],
    );
    let g = bone(&arm, "alice");
    assert!(g.tex.is_empty(), "group bone shouldn't have a texture");
    for name in ["body", "back_arm"] {
        let b = bone(&arm, name);
        assert_eq!(b.parent_id, g.id, "{name} not under its group");
        assert_eq!(b.tex, name);
    }
    let style = arm.styles.iter().find(|s| s.name == "Default").unwrap();
    assert_eq!(style.textures.len(), 2);
    // the top layer draws above the one below it
    assert!(bone(&arm, "body").zindex > bone(&arm, "back_arm").zindex);
}

#[test]
fn layers_outside_groups_are_imported() {
    let arm = build(vec![], vec![art("loose", None, 0., 0., 10, 10, 1)]);
    let root = bone(&arm, "Root");
    assert_eq!(bone(&arm, "loose").parent_id, root.id);
}

#[test]
fn bones_sit_where_the_art_was() {
    // canvas 100x100: armature origin at the canvas center, y up
    let arm = build(
        vec![group(1, "arm", None), group(2, "hand", Some(1))],
        vec![art("upper", Some(1), 10., 10., 20, 10, 1), art("palm", Some(2), 60., 70., 10, 10, 2)],
    );
    assert!(close(world(&arm, "upper"), Vec2::new(-30., 35.)));
    assert!(close(world(&arm, "palm"), Vec2::new(15., -25.)));
    assert_eq!(bone(&arm, "hand").parent_id, bone(&arm, "arm").id);
    // a group without a $pivot sits at the center of all its art
    assert!(close(world(&arm, "hand"), Vec2::new(15., -25.)));
    assert!(close(world(&arm, "arm"), Vec2::new(-10., 5.)));
}

#[test]
fn pivot_marker_places_the_group_bone() {
    let arm = build(
        vec![group(1, "head", None)],
        vec![marker("$pivot", Some(1), 49., 79.), art("face", Some(1), 30., 30., 40, 40, 1)],
    );
    assert!(close(world(&arm, "head"), Vec2::new(0., -30.)));
    assert!(close(world(&arm, "face"), Vec2::new(0., 0.)));
    assert!(arm.bones.iter().all(|b| !b.name.contains('$')), "marker became a bone");
}

#[test]
fn same_named_layers_get_their_own_textures() {
    let arm = build(
        vec![group(1, "left", None), group(2, "right", None)],
        vec![art("line", Some(1), 0., 0., 10, 10, 1), art("line", Some(2), 50., 0., 10, 12, 2)],
    );
    let texs: Vec<&String> = arm.bones.iter().filter(|b| b.name == "line").map(|b| &b.tex).collect();
    assert_eq!(texs.len(), 2);
    assert_ne!(texs[0], texs[1]);
}

#[test]
fn style_group_layers_become_variants() {
    let arm = build(
        vec![group(1, "head", None), group(2, "head $\"Angry\"", Some(1))],
        vec![art("mouth", Some(2), 40., 60., 10, 4, 9), art("mouth", Some(1), 40., 60., 10, 4, 1)],
    );
    let angry = arm.styles.iter().find(|s| s.name == "Angry").expect("no Angry style");
    assert_eq!(angry.textures.len(), 1);
    assert_eq!(angry.textures[0].name, bone(&arm, "mouth").tex);
    assert!(arm.bones.iter().all(|b| !b.name.contains("Angry")), "style group became a bone");
    assert_eq!(arm.bones.iter().filter(|b| b.name == "mouth").count(), 1);
}

#[test]
fn ik_chain_keeps_its_art_in_place_once_solved() {
    // a two-joint arm going right and down, with art under each joint
    let arm = build(
        vec![group(1, "upper", None), group(2, "lower", Some(1))],
        vec![
            marker("$ik_1", Some(1), 0., 0.),
            marker("$pivot", Some(1), 19., 19.),
            art("upper_art", Some(1), 20., 20., 20, 20, 1),
            marker("$ik_1", Some(2), 0., 0.),
            marker("$pivot", Some(2), 39., 39.),
            art("lower_art", Some(2), 40., 40., 20, 20, 2),
        ],
    );
    let upper = bone(&arm, "upper");
    let lower = bone(&arm, "lower");
    assert!(upper.ik.is_none());
    let c = lower.ik.as_ref().expect("no IK on the chain's last bone");
    assert_eq!(c.chain, 1);
    let target = arm.bones.iter().find(|b| b.id == c.target_id).expect("no IK target");
    assert_eq!(target.name, "lower Target");
    assert!(close(target.pos, world(&arm, "lower")), "target at {:?}", target.pos);

    // nothing is straightened or counter-rotated, and solving moves nothing
    assert!(close(world(&arm, "upper_art"), Vec2::new(-20., 20.)), "upper art at {:?}", world(&arm, "upper_art"));
    assert!(close(world(&arm, "lower_art"), Vec2::new(0., 0.)), "lower art at {:?}", world(&arm, "lower_art"));
    let mut solved = arm.bones.clone();
    keyternity_lib::renderer::construction(&mut solved, &arm.bones);
    for name in ["upper_art", "lower_art"] {
        let b = solved.iter().find(|b| b.name == name).unwrap();
        assert!(close(b.pos, world(&arm, name)), "{name} moved to {:?}", b.pos);
    }
}

#[test]
fn opaque_bounds_trims_transparent_margins() {
    let mut img = image::RgbaImage::new(10, 8);
    img.put_pixel(3, 2, image::Rgba([1, 2, 3, 255]));
    img.put_pixel(6, 5, image::Rgba([1, 2, 3, 1]));
    assert_eq!(psd_import::opaque_bounds(&img), Some((3, 2, 4, 4)));
    assert_eq!(psd_import::opaque_bounds(&image::RgbaImage::new(4, 4)), None);
}
