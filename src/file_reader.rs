//! Reading uploaded images to turn into textures.
// test


use egui::TextureHandle;
use wgpu::*;

use crate::*;


// web-only imports
#[cfg(target_arch = "wasm32")]
mod web {
    pub use std::io::Read;
    pub use wasm_bindgen::prelude::wasm_bindgen;
    pub use web_sys::*;
}
#[cfg(target_arch = "wasm32")]
pub use web::*;

pub const EXPORT_VID_DONE: &str = "Done!";
pub const IMPORT_IMG_ERR: &str = "Could not extract image data.";

pub fn read(shared: &mut Shared, renderer: &Option<BackendRenderer>, context: &egui::Context) {
    macro_rules! func {
        ($func:expr) => {
            $func(
                shared,
                Some(&renderer.as_ref().unwrap().gpu.queue),
                Some(&renderer.as_ref().unwrap().gpu.device),
                Some(&renderer.as_ref().unwrap().bind_group_layout),
                Some(&context),
            )
        };
    }

    match renderer.as_ref() {
        None => return,
        _ => {}
    }

    #[cfg(target_arch = "wasm32")]
    {
        let file = crate::getLang();
        if file.len() != 0 {
            shared.ui.init_lang(&file, "");
            crate::clearLang();
        }
    }

    func!(read_image_loaders);
    func!(read_import);
    #[cfg(not(target_arch = "wasm32"))]
    read_audio(shared);
    if shared.ui.done_pending {
        func!(add_pending_textures);
        shared.ui.done_pending = false;
    }
}

/// Import dialog audio (docs/LIP_SYNC.md §5): each picked file becomes audio the rig
/// carries, placed as a clip at the selected animation's current frame (or 0).
#[cfg(not(target_arch = "wasm32"))]
pub fn read_audio(shared: &mut Shared) {
    if *shared.ui.file_type.lock().unwrap() != 3 {
        return;
    }
    *shared.ui.file_type.lock().unwrap() = 0;
    let paths = shared.ui.file_path.lock().unwrap().clone();
    let anim = shared.selections.anim;
    if anim >= shared.armature.animations.len() {
        shared.events.open_modal("audio.no_animation", false);
        return;
    }
    shared.undo_states.new_undo_anim(&shared.armature.animations[anim]);
    for path in paths {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("audio").to_string();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("wav").to_string();
        let Ok(bytes) = fs::read(&path) else { continue };
        let id = crate::audio::next_id(shared.armature.audio.iter().map(|a| a.id));
        match crate::audio::decode(id, &name, &ext, bytes) {
            Ok(data) => {
                shared.armature.audio.push(data);
                let a = &mut shared.armature.animations[anim];
                let clip_id = crate::audio::next_id(a.clips.iter().map(|c| c.id));
                let start_frame = shared.selections.anim_frame.max(0);
                a.clips.push(crate::audio::AudioClip { id: clip_id, audio: id, name, start_frame, ..Default::default() });
            }
            Err(e) => {
                shared.ui.custom_error = e;
                shared.events.open_modal("audio.decode_failed", false);
            }
        }
    }
}

/// read temporary files created from file dialogs (native & WASM)
pub fn read_image_loaders(
    shared: &mut Shared,
    queue: Option<&Queue>,
    device: Option<&Device>,
    bind_group_layout: Option<&BindGroupLayout>,
    ctx: Option<&egui::Context>,
) {
    let mut images: Vec<image::DynamicImage> = vec![];
    #[allow(unused_assignments, unused_mut)]
    let mut names: Vec<String> = vec![];

    #[cfg(not(target_arch = "wasm32"))]
    {
        if *shared.ui.file_type.lock().unwrap() != 1 {
            return;
        }
        *shared.ui.file_type.lock().unwrap() = 0;
        let len = shared.ui.file_path.lock().unwrap().len();
        for f in 0..len {
            let file = &shared.ui.file_path.lock().unwrap()[f];
            let raw_name = file.as_path().file_name().unwrap();
            let name = raw_name.to_str().unwrap().to_string();
            if name.contains(".") {
                names.push(name.split('.').collect::<Vec<_>>()[0].to_string());
            } else {
                names.push(name.clone());
            }

            let path = file.as_path().to_str().unwrap().to_string();
            let buf = fs::read(path).unwrap();
            images.push(image::load_from_memory(&buf).unwrap());
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        if !hasLoadedAllImages() {
            return;
        }

        let mut idx = 0;
        while hasElement(&("image-".to_string() + &idx.to_string())) {
            let img = load_image_wasm("image-".to_string() + &idx.to_string());
            let pixels = img.as_ref().unwrap().0.clone();
            let dims = img.as_ref().unwrap().1;
            images.push(image::DynamicImage::ImageRgba8(
                image::ImageBuffer::from_raw(dims.x as u32, dims.y as u32, pixels).unwrap(),
            ));

            if getImgName(idx).contains(".") {
                names.push(getImgName(idx).split('.').collect::<Vec<_>>()[0].to_string());
            } else {
                names.push(getImgName(idx))
            }
            idx += 1;
        }

        removeImage();
    }

    let sel = &shared.selections;
    let style = shared.armature.sel_style(sel).unwrap().clone();
    shared.undo_states.new_undo_style(&style);

    for i in 0..images.len() {
        // trim transparent padding
        //let image: image::DynamicImage = trim_transparent(&images[i]).into();
        let dims = Vec2::new(images[i].width() as f32, images[i].height() as f32);

        if images[i].clone().into_rgba8().to_vec().len() == 0 {
            shared.events.open_modal("img_err", false);
            return;
        }

        #[rustfmt::skip]
        add_texture(images[i].clone(), shared.selections.style_id, dims, &names[i], &mut shared.armature, queue, device, bind_group_layout, ctx);
    }

    *shared.ui.file_path.lock().unwrap() = vec![];
    shared.ui.atlas_modal = images.len() == 1;
}

pub fn trim_transparent(img: &image::DynamicImage) -> image::DynamicImage {
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();

    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0;
    let mut max_y = 0;

    let mut found = false;

    for y in 0..height {
        for x in 0..width {
            let alpha = rgba.get_pixel(x, y)[3];
            if alpha > 125 {
                found = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    if !found {
        return img.clone();
    }

    image::imageops::crop_imm(&rgba, min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
        .to_image()
        .into()
}

pub fn add_pending_textures(
    shared: &mut Shared,
    queue: Option<&Queue>,
    device: Option<&Device>,
    bind_group_layout: Option<&BindGroupLayout>,
    ctx: Option<&egui::Context>,
) {
    // get last texture of selected style (will be the atlas)
    let sel = &shared.selections;
    let style = shared.armature.sel_style(sel).unwrap();
    let textures = style.textures.last().unwrap().clone();
    let image = shared.armature.tex_data(&textures).unwrap().image.clone();

    // now that we have the atlas, remove it from the list
    shared.armature.sel_style_mut(sel).unwrap().textures.pop();
    shared.armature.tex_data.pop();

    for tex in &shared.ui.pending_textures {
        #[rustfmt::skip]
        let crop = image.crop_imm(tex.offset.x as u32, tex.offset.y as u32, tex.size.x as u32, tex.size.y as u32);
        let crop_size = Vec2::new(crop.width() as f32, crop.height() as f32);
        #[rustfmt::skip]
        add_texture(crop.clone(), shared.selections.style_id, crop_size, &tex.name, &mut shared.armature, queue, device, bind_group_layout, ctx);
    }

    shared.ui.pending_textures = vec![];
}

pub fn read_psd(
    bytes: Vec<u8>,
    shared: &mut Shared,
    queue: Option<&Queue>,
    device: Option<&Device>,
    bind_group_layout: Option<&BindGroupLayout>,
    ctx: Option<&egui::Context>,
) {
    let Ok(psd) = psd::Psd::from_bytes(&bytes) else {
        let str = shared.ui.loc("import_unrecognized");
        editor::open_modal(&mut shared.ui, false, str);
        return;
    };

    // the PSD's rig replaces the bones; its textures join the current styles
    shared.psd_armature.styles = shared.armature.styles.clone();
    shared.psd_armature.tex_data = shared.armature.tex_data.clone();

    let tree = psd_import::tree_from_psd(&psd);
    drop(psd);
    let str_default = shared.ui.loc("settings_modal.default");
    let gpu = psd_import::Gpu { queue, device, layout: bind_group_layout, ctx };
    psd_import::build_armature(&tree, &mut shared.psd_armature, &str_default, gpu);

    // remove unused texture data
    shared.psd_armature.tex_data.retain(|d| {
        let mut retain = false;
        for style in &shared.psd_armature.styles {
            retain |= style.textures.iter().find(|t| t.data_id == d.id) != None;
        }
        retain
    });

    // immediately import textures to real armature
    shared.psd_armature.styles.reverse();
    shared.armature.styles = shared.psd_armature.styles.clone();
    shared.armature.tex_data = shared.psd_armature.tex_data.clone();

    // show polar modal to replace real armature bones as well
    let str = shared.ui.loc("psd_imported");
    shared.events.open_polar_modal(PolarId::ImportedPsd, str);

    shared.ui.startup_window = false;
}
pub fn create_texture(
    image: &image::DynamicImage,
    dimensions: Vec2,
    queue: Option<&Queue>,
    device: Option<&Device>,
    bind_group_layout: Option<&BindGroupLayout>,
    ctx: Option<&egui::Context>,
) -> (Option<BindGroup>, Option<TextureHandle>) {
    let img_buf = <image::ImageBuffer<image::Rgba<u8>, _>>::from_raw(
        dimensions.x as u32,
        dimensions.y as u32,
        image.clone().into_rgba8().to_vec(),
    )
    .unwrap();

    let mut bind_group = None;
    if queue != None && device != None && bind_group_layout != None {
        bind_group = Some(renderer::create_texture_bind_group(
            image.clone().into_rgba8().to_vec(),
            dimensions,
            queue.unwrap(),
            device.unwrap(),
            bind_group_layout.unwrap(),
        ));
    }

    let mut ui_img = None;
    if ctx != None {
        ui_img = Some(utils::add_texture_img(
            &ctx.unwrap(),
            img_buf,
            Vec2::new(300., 300.),
            &format!("{}_{}", dimensions.x, dimensions.y),
        ));
    }

    (bind_group, ui_img)
}

/// add texture to style, including it's bind group and UI image.
pub fn add_texture(
    image: image::DynamicImage,
    style_id: i32,
    dimensions: Vec2,
    tex_name: &str,
    armature: &mut Armature,
    queue: Option<&Queue>,
    device: Option<&Device>,
    bind_group_layout: Option<&BindGroupLayout>,
    ctx: Option<&egui::Context>,
) {
    let (bind_group, ui_img) =
        create_texture(&image, dimensions, queue, device, bind_group_layout, ctx);

    let ids = armature.tex_data.iter().map(|td| td.id).collect();
    let id = generate_id(ids);
    armature.tex_data.push(TextureData {
        id,
        image,
        bind_group,
        ui_img,
    });

    let style = &mut armature.styles.iter_mut().find(|set| set.id == style_id);
    if *style == None {
        return;
    }
    style.as_mut().unwrap().textures.push(crate::Texture {
        offset: Vec2::ZERO,
        size: dimensions,
        name: tex_name.to_string(),
        ser_offset: Vec2I::new(0, 0),
        ser_size: Vec2I::new(0, 0),
        data_id: id,
        atlas_idx: 0,
    });
}

pub fn read_import(
    shared: &mut Shared,
    queue: Option<&Queue>,
    device: Option<&Device>,
    bgl: Option<&BindGroupLayout>,
    context: Option<&egui::Context>,
) {
    let file;
    let filename: String;
    #[cfg(not(target_arch = "wasm32"))]
    let filepath: String;

    #[cfg(not(target_arch = "wasm32"))]
    {
        if *shared.ui.file_type.lock().unwrap() != 2 {
            return;
        }
        *shared.ui.file_type.lock().unwrap() = 0;
        let raw_filepath = &shared.ui.file_path.lock().unwrap()[0];

        filepath = raw_filepath.as_path().to_str().unwrap().to_string();
        file = std::fs::File::open(filepath.to_string());
        if let Err(err) = file {
            shared.events.open_file_err_modal(err.to_string());
            return;
        }

        let raw_filename = raw_filepath.as_path().file_name();
        filename = raw_filename.unwrap().to_str().unwrap().to_string();
    }

    #[cfg(target_arch = "wasm32")]
    {
        filename = getFileName();
        if filename == "" {
            return;
        }
        file = getFile();
    }

    let ext = filename.split('.').last().unwrap();
    match ext {
        "swf" => {
            shared.events.open_modal("import_swf", false);
        }
        "skf" | "skfe" | "skf~" => {
            #[cfg(target_arch = "wasm32")]
            {
                let cursor = std::io::Cursor::new(getFile());
                utils::import(cursor, shared, queue, device, bgl, context);
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                utils::import(file.unwrap(), shared, queue, device, bgl, context);
                if !shared.ui.recent_file_paths.contains(&filepath) {
                    shared.ui.recent_file_paths.push(filepath);
                }
                utils::save_to_recent_files(shared.ui.recent_file_paths.clone());
            }
        }
        "psd" => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let file = std::fs::read(filepath).unwrap();
                read_psd(file, shared, queue, device, bgl, context);
            }
            #[cfg(target_arch = "wasm32")]
            read_psd(file, shared, queue, device, bgl, context)
        }
        "json" => {
            #[cfg(not(target_arch = "wasm32"))]
            read_dragonbones(std::path::Path::new(&filepath), shared, queue, device, bgl, context);
            #[cfg(target_arch = "wasm32")]
            shared.events.open_modal("import_dragonbones_web", false);
        }
        _ => {
            shared.events.open_modal("import_unrecognized", false);
        }
    };
    shared.ui.selected_bone_first_time = true;

    #[cfg(target_arch = "wasm32")]
    removeFile();
}

/// Import a DragonBones rig (`<name>_ske.json` + its `_tex` atlas pages) as the armature.
#[cfg(not(target_arch = "wasm32"))]
fn read_dragonbones(
    path: &std::path::Path,
    shared: &mut Shared,
    queue: Option<&Queue>,
    device: Option<&Device>,
    bgl: Option<&BindGroupLayout>,
    context: Option<&egui::Context>,
) {
    let result = crate::dragonbones_import::read_files(path)
        .and_then(|(json, atlases)| crate::dragonbones_import::import(&json, &atlases));
    let imported = match result {
        Ok(imported) => imported,
        Err(err) => {
            shared.events.open_file_err_modal(err);
            return;
        }
    };

    let mut armature = imported.armature;
    for data in &mut armature.tex_data {
        let size = Vec2::new(data.image.width() as f32, data.image.height() as f32);
        let (bind_group, ui_img) = create_texture(&data.image, size, queue, device, bgl, context);
        data.bind_group = bind_group;
        data.ui_img = ui_img;
    }
    shared.armature = armature;
    shared.events.unselect_all();
    shared.ui.startup_window = false;

    // tell the user what couldn't be carried over exactly
    if !imported.warnings.is_empty() {
        let notes: Vec<String> = imported.warnings.iter().map(|w| format!("- {w}")).collect();
        shared.ui.custom_error = notes.join("\n");
        let headline = shared.ui.loc("import_dragonbones_notes");
        editor::open_modal(&mut shared.ui, false, headline);
    }
}

/// Load image by reading an `img` tag with the specified ID.
#[cfg(target_arch = "wasm32")]
pub fn load_image_wasm(id: String) -> Option<(Vec<u8>, Vec2)> {
    let mut result: Vec<u8> = vec![];
    let mut dimensions = Vec2::default();

    let document: Document = window().unwrap().document().unwrap();
    if let Some(img_element) = document.get_element_by_id(&id) {
        let img = img_element.dyn_into::<HtmlImageElement>().unwrap();

        let canvas = document
            .create_element("canvas")
            .unwrap()
            .dyn_into::<HtmlCanvasElement>()
            .unwrap();

        canvas.set_width(img.width());
        canvas.set_height(img.height());

        // get 2D rendering context
        let context = canvas
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .unwrap();

        if img.width() == 0 && img.height() == 0 {
            return None;
        }

        // draw image onto canvas
        context
            .draw_image_with_html_image_element(&img, 0.0, 0.0)
            .unwrap();

        // extract image data (RGBA pixels)
        let image_data = context
            .get_image_data(0.0, 0.0, img.width() as f64, img.height() as f64)
            .unwrap();
        let pixels = image_data.data();

        // convert js_sys::Uint8ClampedArray to Vec<u8>
        pixels.to_vec();

        dimensions = Vec2::new(img.width() as f32, img.height() as f32);
        result = pixels.to_vec();
    }

    return Some((result, dimensions));
}

