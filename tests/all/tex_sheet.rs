//! The texture atlas written on save (`utils::create_tex_sheet`): every texture lands
//! intact at the offset recorded for it.

use keyternity_lib::shared::*;
use keyternity_lib::utils;

fn texture(arm: &mut Armature, name: &str, w: u32, h: u32, seed: u8) -> Texture {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([(x as u8).wrapping_mul(7) ^ seed, (y as u8).wrapping_mul(13), seed, 255])
    });
    let id = arm.tex_data.len() as i32;
    arm.tex_data.push(TextureData { id, image: image::DynamicImage::ImageRgba8(img), bind_group: None, ui_img: None });
    Texture { name: name.into(), size: Vec2::new(w as f32, h as f32), data_id: id, ..Default::default() }
}

#[test]
fn textures_are_copied_into_the_atlas_where_their_offsets_say() {
    let mut arm = Armature::default();
    let a = texture(&mut arm, "a", 300, 170, 1);
    let b = texture(&mut arm, "b", 90, 260, 2);
    let c = texture(&mut arm, "c", 33, 17, 3);
    arm.styles.push(Style { name: "default".into(), active: true, textures: vec![a, b, c], ..Default::default() });

    let (bufs, sizes) = utils::create_tex_sheet(&mut arm, &EditMode::default());
    assert_eq!(bufs.len(), 1);
    let atlas = image::load_from_memory(&bufs[0]).unwrap().to_rgba8();
    assert_eq!(atlas.width() as i32, sizes[0]);

    for tex in &arm.styles[0].textures {
        let src = arm.tex_data.iter().find(|d| d.id == tex.data_id).unwrap().image.to_rgba8();
        let (ox, oy) = (tex.offset.x as u32, tex.offset.y as u32);
        for y in 0..src.height() {
            for x in 0..src.width() {
                assert_eq!(atlas.get_pixel(ox + x, oy + y), src.get_pixel(x, y), "{} at {x},{y}", tex.name);
            }
        }
    }
}
