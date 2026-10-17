//! Frame sequence and spritesheet export (docs/FRAME_SEQUENCES.md §5–7): the options,
//! which frames to render and what to call them, the output size, sheet packing, the
//! manifest, atlas and Phaser files, and the job that hands frames to a background
//! writer. Rendering itself is in `lib.rs`
//! (`start_sequence_export`, `step_sequence_export`), since it needs the GPU.

use crate::render_camera::{self, RenderCamera};
use crate::shared::{Armature, Color};
use serde_json::{json, Value};

/// How the output size is given (§5.1). The camera's proportions are always kept.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum SizeMode {
    /// percent of the camera's resolution
    #[default]
    Percent,
    /// width in pixels; the height follows
    Width,
    /// height in pixels; the width follows
    Height,
}

/// The image format frames and sheets are written in.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum ImageFormat {
    #[default]
    Png,
    /// lossless WebP: smaller than PNG, with alpha
    Webp,
    /// JPG: no alpha, filled with the camera's background (white if transparent)
    Jpg,
}

impl ImageFormat {
    pub fn ext(&self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Webp => "webp",
            ImageFormat::Jpg => "jpg",
        }
    }
}

/// One animation's part in an export.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct AnimChoice {
    /// the animation's id
    pub id: i32,
    pub export: bool,
    pub start: i32,
    /// -1: the last keyframe
    pub end: i32,
}

impl Default for AnimChoice {
    fn default() -> Self {
        AnimChoice { id: -1, export: true, start: 0, end: -1 }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct SequenceOptions {
    /// a render camera's id; None fits a camera around the chosen animations
    pub camera: Option<i32>,
    pub anims: Vec<AnimChoice>,
    /// render every Nth frame
    pub step: u32,
    /// leave out each animation's last frame, for seamless loops
    pub skip_last: bool,
    pub size_mode: SizeMode,
    /// percent, or pixels (see `size_mode`)
    pub size_value: f32,
    /// write a zip instead of a folder
    pub zip: bool,
    /// a folder per animation
    pub anim_folders: bool,
    /// file name, with `{anim}`, `{index}` and `{frame}`
    pub pattern: String,
    /// zero-padded width of `{index}` and `{frame}`
    pub digits: u32,
    /// the first `{index}`
    pub start: u32,
    /// write the Phaser loader pack and animations (§7.1)
    pub phaser: bool,
    /// spritesheets: columns per sheet, 0 for the most square sheet
    pub sheet_columns: u32,
    /// spritesheets: the largest a sheet may be, in pixels per side
    pub sheet_max: u32,
    /// spritesheets: pixels between cells
    pub sheet_padding: u32,
    /// spritesheets: each animation starts its own sheet
    pub sheet_per_anim: bool,
    pub format: ImageFormat,
    /// JPG quality, 1-100
    pub jpg_quality: u8,
    /// spritesheets: crop each frame to its visible pixels and pack them tightly
    pub trim: bool,
    /// write a Godot 4 SpriteFrames resource (§7.2)
    pub godot: bool,
    /// where the exported files sit in the Godot project ("res://art/alice/")
    pub godot_dir: String,
    /// the last export's folder or zip, for Export Again (§8.4)
    pub last_dest: Option<String>,
    /// the last export was spritesheets
    pub last_sheets: bool,
}

impl Default for SequenceOptions {
    fn default() -> Self {
        SequenceOptions {
            camera: None,
            anims: vec![],
            step: 1,
            skip_last: false,
            size_mode: SizeMode::Percent,
            size_value: 100.,
            zip: false,
            anim_folders: true,
            pattern: "{anim}_{index}".to_string(),
            digits: 4,
            start: 0,
            phaser: true,
            sheet_columns: 0,
            sheet_max: 4096,
            sheet_padding: 2,
            sheet_per_anim: false,
            format: ImageFormat::Png,
            jpg_quality: 90,
            trim: false,
            godot: false,
            godot_dir: "res://".to_string(),
            last_dest: None,
            last_sheets: false,
        }
    }
}

impl SequenceOptions {
    /// The choice for each of the armature's animations, in order, adding defaults for
    /// new ones and dropping those of deleted ones.
    pub fn sync(&mut self, armature: &Armature) {
        let old = std::mem::take(&mut self.anims);
        for anim in &armature.animations {
            let choice = old.iter().find(|c| c.id == anim.id).cloned();
            self.anims.push(choice.unwrap_or(AnimChoice { id: anim.id, ..Default::default() }));
        }
    }

    fn choice(&self, id: i32) -> AnimChoice {
        self.anims.iter().find(|c| c.id == id).cloned().unwrap_or(AnimChoice { id, ..Default::default() })
    }
}

/// The options as an export uses them: spritesheet frames are named without folders,
/// and only spritesheets trim (a trimmed frame on its own loses its offset).
pub fn effective(opts: &SequenceOptions, sheets: bool) -> SequenceOptions {
    let mut o = opts.clone();
    if sheets {
        o.anim_folders = false;
    } else {
        o.trim = false;
    }
    o
}

/// An animation's last frame (its last keyframe), 0 if it has none.
pub fn last_frame(armature: &Armature, anim: usize) -> i32 {
    armature.animations[anim].last_frame()
}

/// One image to render.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedFrame {
    /// animation index
    pub anim: usize,
    /// the animation frame it shows
    pub frame: i32,
    /// its file, relative to the output folder ("walk/walk_0000.png")
    pub path: String,
    /// `path` without the extension: its key in the Phaser files
    pub key: String,
    /// how long it shows, in seconds
    pub duration: f32,
}

/// Characters a file name can't have on any platform.
pub fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "_".to_string() } else { s }
}

/// A file name from `pattern`.
pub fn file_name(pattern: &str, anim: &str, index: u32, frame: i32, digits: u32) -> String {
    let d = digits.clamp(1, 9) as usize;
    let name = pattern
        .replace("{anim}", anim)
        .replace("{index}", &format!("{index:0d$}"))
        .replace("{frame}", &format!("{frame:0d$}"));
    sanitize(&name)
}

/// Every image the export writes, in order. Errors if the options can't work: nothing
/// chosen, or names that would collide.
pub fn plan(armature: &Armature, opts: &SequenceOptions) -> Result<Vec<PlannedFrame>, String> {
    if !opts.pattern.contains("{index}") && !opts.pattern.contains("{frame}") {
        return Err("The name pattern needs {index} or {frame}, so each frame gets its own file.".into());
    }
    let step = opts.step.max(1) as i32;
    let mut frames = vec![];
    for (a, anim) in armature.animations.iter().enumerate() {
        let choice = opts.choice(anim.id);
        if !choice.export {
            continue;
        }
        let last = last_frame(armature, a);
        let start = choice.start.clamp(0, last);
        let mut end = if choice.end < 0 { last } else { choice.end.clamp(start, last) };
        if opts.skip_last && end > start {
            end -= 1;
        }
        let fps = anim.fps.max(1) as f32;
        let anim_name = sanitize(&anim.name);
        let mut index = opts.start;
        let mut f = start;
        while f <= end {
            let shown = step.min(end + 1 - f);
            let name = file_name(&opts.pattern, &anim_name, index, f, opts.digits);
            let key = if opts.anim_folders { format!("{anim_name}/{name}") } else { name };
            frames.push(PlannedFrame {
                anim: a,
                frame: f,
                path: format!("{key}.{}", opts.format.ext()),
                key,
                duration: shown as f32 / fps,
            });
            index += 1;
            f += step;
        }
    }
    if frames.is_empty() {
        return Err("No animations are chosen.".into());
    }
    let mut seen = std::collections::HashSet::new();
    if let Some(dup) = frames.iter().find(|f| !seen.insert(f.path.to_lowercase())) {
        return Err(format!(
            "Two frames would both be named {}. Add {{anim}} to the name pattern, or export a folder per animation.",
            dup.path
        ));
    }
    Ok(frames)
}

/// The scale factor from the camera's resolution to the output, and the output size.
pub fn output_size(cam: &RenderCamera, opts: &SequenceOptions) -> (f32, (u32, u32)) {
    let (w, h) = cam.px_size();
    let v = opts.size_value.max(0.);
    let factor = match opts.size_mode {
        SizeMode::Percent => v / 100.,
        SizeMode::Width => v / w as f32,
        SizeMode::Height => v / h as f32,
    }
    .clamp(0.01, 4.);
    let side = |s: u32| ((s as f32 * factor).round() as u32).max(1);
    (factor, (side(w), side(h)))
}

/// The camera an export renders through: the chosen one, or one fitted around the
/// chosen animations (§4.4).
pub fn export_camera(armature: &Armature, opts: &SequenceOptions) -> Option<RenderCamera> {
    if let Some(cam) = opts.camera.and_then(|id| armature.cameras.iter().find(|c| c.id == id)) {
        return Some(cam.clone());
    }
    let chosen: Vec<usize> = armature
        .animations
        .iter()
        .enumerate()
        .filter(|(_, a)| opts.choice(a.id).export)
        .map(|(i, _)| i)
        .collect();
    let (min, max) = render_camera::animation_bounds(armature, &chosen, None)?;
    let cam = RenderCamera { name: "Auto".to_string(), ..Default::default() };
    Some(render_camera::fit(&cam, min, max, 8.))
}

/// A frame cropped to its visible pixels: its offset in the full frame, and its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trim {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// What the data files describe besides the frames: where they went on sheets, how
/// they were trimmed, and how far a followed bone moved the camera.
#[derive(Clone, Copy, Default)]
pub struct Layout<'a> {
    pub sheets: Option<&'a SheetPlan>,
    /// per planned frame
    pub trims: Option<&'a [Trim]>,
    /// per planned frame: the camera's movement in output pixels (x right, y down)
    pub motion: Option<&'a [[f32; 2]]>,
}

/// The visible part of `img` (alpha above 0). An empty frame keeps one pixel.
pub fn trim_bounds(img: &image::RgbaImage) -> Trim {
    let (w, h) = img.dimensions();
    let raw = img.as_raw();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        let row = &raw[(y * w * 4) as usize..((y + 1) * w * 4) as usize];
        let mut first = None;
        let mut last = 0;
        for (x, p) in row.chunks_exact(4).enumerate() {
            if p[3] > 0 {
                first.get_or_insert(x as u32);
                last = x as u32;
            }
        }
        if let Some(f) = first {
            x0 = x0.min(f);
            x1 = x1.max(last);
            y0 = y0.min(y);
            y1 = y;
        }
    }
    if x0 > x1 || y0 > y1 {
        return Trim { x: 0, y: 0, w: 1, h: 1 };
    }
    Trim { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 }
}

/// `img` cropped to `t`.
pub fn crop(img: &image::RgbaImage, t: Trim) -> image::RgbaImage {
    let mut out = image::RgbaImage::new(t.w, t.h);
    let (w, raw) = (img.width() as usize, img.as_raw());
    let dst: &mut [u8] = &mut out;
    for r in 0..t.h as usize {
        let s = ((t.y as usize + r) * w + t.x as usize) * 4;
        dst[r * t.w as usize * 4..(r + 1) * t.w as usize * 4].copy_from_slice(&raw[s..s + t.w as usize * 4]);
    }
    out
}

/// The manifest (§7): sizes and pivot in output pixels, and every frame's file (or, for
/// spritesheets, its sheet and rectangle), trim and root motion.
pub fn manifest(rig: &str, armature: &Armature, cam: &RenderCamera, opts: &SequenceOptions, frames: &[PlannedFrame], layout: &Layout) -> Value {
    let (factor, (w, h)) = output_size(cam, opts);
    let pivot = cam.pivot_px() * factor;
    let sheets = layout.sheets;
    let mut anims = vec![];
    for (a, anim) in armature.animations.iter().enumerate() {
        let list: Vec<Value> = frames
            .iter()
            .enumerate()
            .filter(|(_, f)| f.anim == a)
            .map(|(i, f)| {
                let mut e = match sheets {
                    Some(s) => {
                        let (sheet, x, y) = s.placements[i];
                        let (rw, rh) = s.rects[i];
                        json!({ "name": f.key, "sheet": sheet, "rect": [x, y, rw, rh], "frame": f.frame, "duration": round4(f.duration) })
                    }
                    None => json!({ "file": f.path, "frame": f.frame, "duration": round4(f.duration) }),
                };
                if let Some(t) = layout.trims.map(|t| t[i]) {
                    e["offset"] = json!([t.x, t.y]);
                    e["trimmed_size"] = json!([t.w, t.h]);
                }
                if let Some(m) = layout.motion.map(|m| m[i]) {
                    e["root_motion"] = json!([round4(m[0]), round4(m[1])]);
                }
                e
            })
            .collect();
        if list.is_empty() {
            continue;
        }
        anims.push(json!({ "name": anim.name, "fps": anim.fps, "loop": true, "frames": list }));
    }
    let mut m = json!({
        "rig": rig,
        "camera": cam.name,
        "scale": round4(cam.scale * factor),
        "size": [w, h],
        "pivot": [round4(pivot.x), round4(pivot.y)],
        "anchor": [round4(pivot.x / w as f32), round4(pivot.y / h as f32)],
        "animations": anims,
    });
    if let Some(s) = sheets {
        let list: Vec<Value> = s.files.iter().zip(&s.sizes).map(|(f, (w, h))| json!({ "file": f, "size": [w, h] })).collect();
        m["sheets"] = Value::Array(list);
        m["padding"] = json!(s.padding);
    }
    m
}

/// The Phaser 3 animations file (§7.1), read by `this.anims.fromJSON`: one image key
/// per frame, or with `atlas`, that texture's frames. Stepped frames play at
/// `fps / step`; a shorter last frame gets a (negative) `duration` offset, which Phaser
/// adds to each frame's time.
pub fn phaser_anims(armature: &Armature, opts: &SequenceOptions, frames: &[PlannedFrame], atlas: Option<&str>) -> Value {
    let mut anims = vec![];
    for (a, anim) in armature.animations.iter().enumerate() {
        let mine: Vec<&PlannedFrame> = frames.iter().filter(|f| f.anim == a).collect();
        if mine.is_empty() {
            continue;
        }
        let rate = anim.fps.max(1) as f32 / opts.step.max(1) as f32;
        let ms_per_frame = 1000. / rate;
        let list: Vec<Value> = mine
            .iter()
            .map(|f| {
                let extra = f.duration * 1000. - ms_per_frame;
                let mut frame = match atlas {
                    Some(texture) => json!({ "key": texture, "frame": f.key }),
                    None => json!({ "key": f.key }),
                };
                if extra.abs() > 0.5 {
                    frame["duration"] = json!(extra.round());
                }
                frame
            })
            .collect();
        anims.push(json!({ "key": anim.name, "frameRate": round4(rate), "repeat": -1, "frames": list }));
    }
    json!({ "anims": anims })
}

/// The Phaser 3 loader pack (§7.1), loaded with `this.load.pack`: every frame image (or
/// the atlas), and the animations file.
pub fn phaser_pack(rig: &str, frames: &[PlannedFrame], sheets: Option<&SheetPlan>) -> Value {
    let mut files: Vec<Value> = match sheets {
        Some(s) if s.files.len() == 1 => {
            vec![json!({ "type": "atlas", "key": rig, "textureURL": s.files[0], "atlasURL": format!("{rig}_atlas.json") })]
        }
        Some(_) => vec![json!({ "type": "multiatlas", "key": rig, "atlasURL": format!("{rig}_atlas.json") })],
        None => frames.iter().map(|f| json!({ "type": "image", "key": f.key, "url": f.path })).collect(),
    };
    files.push(json!({ "type": "json", "key": format!("{rig}_anims"), "url": format!("{rig}_anims.json") }));
    let mut pack = serde_json::Map::new();
    pack.insert(rig.to_string(), json!({ "files": files }));
    Value::Object(pack)
}

/// The extra files an export writes beside the frames or sheets: (name, contents).
pub fn extra_files(rig: &str, armature: &Armature, cam: &RenderCamera, opts: &SequenceOptions, frames: &[PlannedFrame], layout: &Layout) -> Vec<(String, String)> {
    let pretty = |v: Value| serde_json::to_string_pretty(&v).unwrap();
    let sheets = layout.sheets;
    let mut files = vec![(format!("{rig}_frames.json"), pretty(manifest(rig, armature, cam, opts, frames, layout)))];
    if let Some(s) = sheets {
        files.push((format!("{rig}_atlas.json"), pretty(atlas_json(cam, opts, frames, s, layout.trims))));
    }
    if opts.phaser {
        let atlas = sheets.map(|_| rig);
        files.push((format!("{rig}_anims.json"), pretty(phaser_anims(armature, opts, frames, atlas))));
        files.push((format!("{rig}_pack.json"), pretty(phaser_pack(rig, frames, sheets))));
    }
    if opts.godot {
        files.push((format!("{rig}.tres"), godot_tres(armature, cam, opts, frames, layout)));
    }
    files
}

/// `dir` as a Godot path prefix ending in "/": "res://" if empty.
pub fn godot_prefix(dir: &str) -> String {
    let d = dir.trim().replace('\\', "/");
    if d.is_empty() {
        return "res://".to_string();
    }
    if d.ends_with('/') { d } else { format!("{d}/") }
}

/// A Godot 4 SpriteFrames resource (§7.2) for an AnimatedSprite2D: one animation per
/// exported animation, at its FPS (divided by the step), each frame's relative
/// duration, and the frame textures: the images, or AtlasTexture regions of the sheets
/// (with a margin restoring a trimmed frame's offset).
pub fn godot_tres(armature: &Armature, cam: &RenderCamera, opts: &SequenceOptions, frames: &[PlannedFrame], layout: &Layout) -> String {
    let prefix = godot_prefix(&opts.godot_dir);
    let (_, (fw, fh)) = output_size(cam, opts);
    let mut ext = String::new();
    let mut sub = String::new();
    let mut count = 0;
    // the texture each frame shows
    let mut textures = vec![];
    match layout.sheets {
        Some(s) => {
            for (i, file) in s.files.iter().enumerate() {
                ext += &format!("[ext_resource type=\"Texture2D\" path=\"{prefix}{file}\" id=\"{}\"]\n", i + 1);
                count += 1;
            }
            for (i, _) in frames.iter().enumerate() {
                let (sheet, x, y) = s.placements[i];
                let (w, h) = s.rects[i];
                sub += &format!("\n[sub_resource type=\"AtlasTexture\" id=\"AtlasTexture_{i}\"]\natlas = ExtResource(\"{}\")\nregion = Rect2({x}, {y}, {w}, {h})\n", sheet + 1);
                if let Some(t) = layout.trims.map(|t| t[i]) {
                    sub += &format!("margin = Rect2({}, {}, {}, {})\n", t.x, t.y, fw.saturating_sub(t.w), fh.saturating_sub(t.h));
                }
                count += 1;
                textures.push(format!("SubResource(\"AtlasTexture_{i}\")"));
            }
        }
        None => {
            for (i, f) in frames.iter().enumerate() {
                ext += &format!("[ext_resource type=\"Texture2D\" path=\"{prefix}{}\" id=\"{}\"]\n", f.path, i + 1);
                count += 1;
                textures.push(format!("ExtResource(\"{}\")", i + 1));
            }
        }
    }

    let mut anims = vec![];
    for (a, anim) in armature.animations.iter().enumerate() {
        let speed = anim.fps.max(1) as f32 / opts.step.max(1) as f32;
        let list: Vec<String> = frames
            .iter()
            .enumerate()
            .filter(|(_, f)| f.anim == a)
            .map(|(i, f)| format!("{{\n\"duration\": {:?},\n\"texture\": {}\n}}", round4(f.duration * speed), textures[i]))
            .collect();
        if list.is_empty() {
            continue;
        }
        let name = anim.name.replace('\\', "\\\\").replace('"', "\\\"");
        anims.push(format!(
            "{{\n\"frames\": [{}],\n\"loop\": true,\n\"name\": &\"{name}\",\n\"speed\": {:?}\n}}",
            list.join(", "),
            round4(speed)
        ));
    }
    format!(
        "[gd_resource type=\"SpriteFrames\" load_steps={} format=3]\n\n{ext}{sub}\n[resource]\nanimations = [{}]\n",
        count + 1,
        anims.join(", ")
    )
}

/// Where each frame goes on the spritesheets (§6.2): a uniform grid of `cell`-sized
/// cells, `padding` apart.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetPlan {
    pub cell: (u32, u32),
    pub padding: u32,
    /// each sheet's file name and size
    pub files: Vec<String>,
    pub sizes: Vec<(u32, u32)>,
    /// for each planned frame: (sheet, x, y)
    pub placements: Vec<(usize, u32, u32)>,
    /// for each planned frame: its size on the sheet (`cell`, or trimmed)
    pub rects: Vec<(u32, u32)>,
}

/// Frames in sheet groups: one group, or one per animation.
fn sheet_groups(frames: &[PlannedFrame], per_anim: bool) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = vec![];
    for (i, f) in frames.iter().enumerate() {
        if groups.is_empty() || (per_anim && frames[groups.last().unwrap()[0]].anim != f.anim) {
            groups.push(vec![]);
        }
        groups.last_mut().unwrap().push(i);
    }
    groups
}

/// Name the sheets: "walk.png", or "walk_0.png", "walk_1.png" when there are several.
fn name_sheets(plan: &mut SheetPlan, ext: &str) {
    let bases = plan.files.clone();
    for (s, base) in bases.iter().enumerate() {
        let count = bases.iter().filter(|b| *b == base).count();
        let nth = bases[..s].iter().filter(|b| *b == base).count();
        plan.files[s] = if count == 1 { format!("{base}.{ext}") } else { format!("{base}_{nth}.{ext}") };
    }
}

/// Pack frames of different `sizes` (trimmed) tightly onto sheets: shelves of frames
/// sorted tallest first, each sheet cropped to what it holds.
pub fn pack_trimmed(rig: &str, armature: &Armature, frames: &[PlannedFrame], sizes: &[(u32, u32)], opts: &SequenceOptions) -> Result<SheetPlan, String> {
    let pad = opts.sheet_padding;
    let max = opts.sheet_max.max(64);
    if let Some((w, h)) = sizes.iter().find(|(w, h)| *w > max || *h > max) {
        return Err(format!("A frame is {w}×{h} px, larger than the {max} px sheet limit. Use a smaller output size or a larger sheet size."));
    }
    let mut plan = SheetPlan {
        cell: (0, 0),
        padding: pad,
        files: vec![],
        sizes: vec![],
        placements: vec![(0, 0, 0); frames.len()],
        rects: sizes.to_vec(),
    };
    for group in sheet_groups(frames, opts.sheet_per_anim) {
        let mut order = group.clone();
        order.sort_by(|a, b| sizes[*b].1.cmp(&sizes[*a].1));
        let base = if opts.sheet_per_anim { sanitize(&armature.animations[frames[group[0]].anim].name) } else { rig.to_string() };
        // shelves: x along the current one, its top and height
        let (mut x, mut top, mut shelf, mut used) = (0u32, 0u32, 0u32, (0u32, 0u32));
        let mut sheet = plan.files.len();
        plan.files.push(base.clone());
        for i in order {
            let (w, h) = sizes[i];
            if x > 0 && x + w > max {
                top += shelf + pad;
                (x, shelf) = (0, 0);
            }
            if top + h > max {
                plan.sizes.push(used);
                sheet = plan.files.len();
                plan.files.push(base.clone());
                (x, top, shelf, used) = (0, 0, 0, (0, 0));
            }
            plan.placements[i] = (sheet, x, top);
            used = (used.0.max(x + w), used.1.max(top + h));
            shelf = shelf.max(h);
            x += w + pad;
        }
        plan.sizes.push(used);
    }
    name_sheets(&mut plan, opts.format.ext());
    Ok(plan)
}

/// Lay `frames` of size `cell` out on sheets. Errors if a frame is bigger than the
/// largest sheet allowed.
pub fn pack_sheets(rig: &str, armature: &Armature, frames: &[PlannedFrame], cell: (u32, u32), opts: &SequenceOptions) -> Result<SheetPlan, String> {
    let pad = opts.sheet_padding;
    let max = opts.sheet_max.max(64);
    let fit = |c: u32| if c > max { 0 } else { (max + pad) / (c + pad) };
    let (max_cols, max_rows) = (fit(cell.0), fit(cell.1));
    if max_cols == 0 || max_rows == 0 {
        return Err(format!(
            "Each frame is {}×{} px, larger than the {max} px sheet limit. Use a smaller output size or a larger sheet size.",
            cell.0, cell.1
        ));
    }
    let max_cols = if opts.sheet_columns > 0 { opts.sheet_columns.min(max_cols) } else { max_cols };
    let per_sheet = (max_cols * max_rows) as usize;

    let mut plan = SheetPlan {
        cell,
        padding: pad,
        files: vec![],
        sizes: vec![],
        placements: vec![(0, 0, 0); frames.len()],
        rects: vec![cell; frames.len()],
    };
    for group in sheet_groups(frames, opts.sheet_per_anim) {
        for chunk in group.chunks(per_sheet) {
            let n = chunk.len() as u32;
            let mut cols = if opts.sheet_columns > 0 {
                max_cols
            } else {
                // the most square sheet
                (((n * cell.1) as f32 / cell.0 as f32).sqrt().ceil() as u32).clamp(1, max_cols)
            };
            if (n + cols - 1) / cols > max_rows {
                cols = max_cols;
            }
            let cols = cols.min(n);
            let rows = (n + cols - 1) / cols;
            let sheet = plan.files.len();
            for (k, &i) in chunk.iter().enumerate() {
                let (c, r) = (k as u32 % cols, k as u32 / cols);
                plan.placements[i] = (sheet, c * (cell.0 + pad), r * (cell.1 + pad));
            }
            plan.sizes.push((cols * cell.0 + (cols - 1) * pad, rows * cell.1 + (rows - 1) * pad));
            let base = if opts.sheet_per_anim { sanitize(&armature.animations[frames[chunk[0]].anim].name) } else { rig.to_string() };
            plan.files.push(base);
        }
    }
    name_sheets(&mut plan, opts.format.ext());
    Ok(plan)
}

/// The atlas (§7.1): TexturePacker "JSON Hash" for one sheet (Phaser `load.atlas`,
/// PixiJS), Phaser's multiatlas format for several (`load.multiatlas`). The pivot is
/// given both as `anchor` (Phaser) and `pivot` (TexturePacker), normalised.
pub fn atlas_json(cam: &RenderCamera, opts: &SequenceOptions, frames: &[PlannedFrame], sheets: &SheetPlan, trims: Option<&[Trim]>) -> Value {
    let (factor, (w, h)) = output_size(cam, opts);
    let pivot = cam.pivot_px() * factor;
    let anchor = json!({ "x": round4(pivot.x / w as f32), "y": round4(pivot.y / h as f32) });
    let entry = |i: usize| {
        let (_, x, y) = sheets.placements[i];
        let t = trims.map_or(Trim { x: 0, y: 0, w, h }, |t| t[i]);
        json!({
            "frame": { "x": x, "y": y, "w": t.w, "h": t.h },
            "rotated": false,
            "trimmed": t.w != w || t.h != h,
            "spriteSourceSize": { "x": t.x, "y": t.y, "w": t.w, "h": t.h },
            "sourceSize": { "w": w, "h": h },
            "anchor": anchor,
            "pivot": anchor,
        })
    };
    let format = if opts.format == ImageFormat::Jpg { "RGB888" } else { "RGBA8888" };
    let meta = |s: usize| {
        let (sw, sh) = sheets.sizes[s];
        json!({ "image": sheets.files[s], "format": format, "size": { "w": sw, "h": sh }, "scale": 1 })
    };
    let app = json!({ "app": "Keyternity", "version": env!("CARGO_PKG_VERSION") });
    if sheets.files.len() == 1 {
        let mut map = serde_json::Map::new();
        for (i, f) in frames.iter().enumerate() {
            map.insert(f.key.clone(), entry(i));
        }
        let mut m = meta(0);
        m["app"] = app["app"].clone();
        m["version"] = app["version"].clone();
        return json!({ "frames": map, "meta": m });
    }
    let textures: Vec<Value> = (0..sheets.files.len())
        .map(|s| {
            let list: Vec<Value> = (0..frames.len())
                .filter(|&i| sheets.placements[i].0 == s)
                .map(|i| {
                    let mut e = entry(i);
                    e["filename"] = json!(frames[i].key);
                    e
                })
                .collect();
            let mut t = meta(s);
            t["frames"] = Value::Array(list);
            t
        })
        .collect();
    json!({ "textures": textures, "meta": app })
}

/// How far the camera moves for each planned frame (world units): by its followed
/// bone's movement from the rest pose.
pub fn follow_offsets(armature: &Armature, frames: &[PlannedFrame], cam: &RenderCamera) -> Vec<crate::shared::Vec2> {
    if cam.follow.is_none() {
        return vec![crate::shared::Vec2::ZERO; frames.len()];
    }
    let mut arm = Armature { bones: armature.bones.clone(), animations: armature.animations.clone(), ..Default::default() };
    frames
        .iter()
        .map(|f| {
            let posed = arm.animate(f.anim, f.frame, Some(&armature.bones));
            render_camera::follow_offset(cam, &armature.bones, &posed)
        })
        .collect()
}

/// `follow_offsets` in output pixels (x right, y down): the root motion an engine can
/// apply to move the in-place sprite. None if the camera follows nothing.
pub fn root_motion(cam: &RenderCamera, opts: &SequenceOptions, offsets: &[crate::shared::Vec2]) -> Option<Vec<[f32; 2]>> {
    cam.follow?;
    let (factor, _) = output_size(cam, opts);
    let s = cam.scale * factor;
    Some(offsets.iter().map(|o| [o.x * s, -o.y * s]).collect())
}

/// The chosen frames that go outside `cam`, as "walk: frames 8–11" per animation.
/// `tolerance` is in output pixels at scale 1.
pub fn outside_frames(armature: &Armature, frames: &[PlannedFrame], cam: &RenderCamera) -> Vec<String> {
    let tolerance = 0.5 / cam.scale.max(1e-6);
    let (min, max) = (cam.min(), cam.max());
    let mut arm = Armature { bones: armature.bones.clone(), animations: armature.animations.clone(), ..Default::default() };
    let mut out: Vec<(usize, Vec<i32>)> = vec![];
    for f in frames {
        let posed = arm.animate(f.anim, f.frame, Some(&armature.bones));
        let shift = render_camera::follow_offset(cam, &armature.bones, &posed);
        let Some((lo, hi)) = render_camera::pose_bounds(armature, &posed).map(|(lo, hi)| (lo - shift, hi - shift)) else { continue };
        let outside = lo.x < min.x - tolerance || lo.y < min.y - tolerance || hi.x > max.x + tolerance || hi.y > max.y + tolerance;
        if outside {
            match out.last_mut() {
                Some((a, list)) if *a == f.anim => list.push(f.frame),
                _ => out.push((f.anim, vec![f.frame])),
            }
        }
    }
    out.into_iter()
        .map(|(a, list)| {
            let word = if list.len() == 1 { "frame" } else { "frames" };
            format!("{}: {word} {}", armature.animations[a].name, compress_runs(&list))
        })
        .collect()
}

/// "8–11, 14" from [8, 9, 10, 11, 14] (consecutive planned frames, whatever the step).
fn compress_runs(list: &[i32]) -> String {
    let step = list.windows(2).map(|w| w[1] - w[0]).min().unwrap_or(1).max(1);
    let mut runs = vec![];
    let mut start = 0;
    for i in 1..=list.len() {
        if i == list.len() || list[i] - list[i - 1] != step {
            runs.push(if i - 1 == start { list[start].to_string() } else { format!("{}–{}", list[start], list[i - 1]) });
            start = i;
        }
    }
    runs.join(", ")
}

/// The values each engine needs, for the summary after an export (§7.5), as Markdown.
/// `sheets`: how many spritesheets were written, for the spritesheet export.
pub fn engine_notes(rig: &str, cam: &RenderCamera, opts: &SequenceOptions, sheets: Option<usize>) -> String {
    let (factor, (w, h)) = output_size(cam, opts);
    let p = cam.pivot_px() * factor;
    let r = |v: f32| (v * 100.).round() / 100.;
    let mut notes = String::new();
    if opts.phaser {
        notes += &format!("**Phaser** (the pack loads everything):\n```\n{}\n```\n", phaser_snippet(rig));
        if sheets == Some(1) {
            notes += &format!(
                "Or as a plain grid: `this.load.spritesheet('{rig}', '{rig}.png', {{ frameWidth: {w}, frameHeight: {h}, spacing: {} }})`\n\n",
                opts.sheet_padding
            );
        }
    }
    notes += &format!(
        "**Godot 4**: AnimatedSprite2D, Centered on, Offset ({}, {}), so the pivot sits on the node.\n\n",
        r(w as f32 / 2. - p.x),
        r(h as f32 / 2. - p.y)
    );
    notes += &format!("**Unity**: Pivot Custom ({}, {})", r(p.x / w as f32), r(1. - p.y / h as f32));
    if sheets.is_some() {
        notes += &format!(", Sprite Mode Multiple, Slice Grid By Cell Size {w} × {h}, Padding {}", opts.sheet_padding);
    }
    notes += ".\n\n";
    notes += &format!("Frame size {w} × {h} px, pivot at ({}, {}) px from the top left. All of it is in `{rig}_frames.json`.", r(p.x), r(p.y));
    notes
}

/// The Phaser code that loads an export, for the summary.
pub fn phaser_snippet(rig: &str) -> String {
    format!(
        "// preload\nthis.load.setPath('<folder>/');\nthis.load.pack('{rig}', '{rig}_pack.json');\n// create\nthis.anims.fromJSON(this.cache.json.get('{rig}_anims'));"
    )
}

fn round4(v: f32) -> f64 {
    ((v as f64) * 10000.).round() / 10000.
}

/// The size to render at for an output of `out` from a camera of `full` pixels:
/// larger outputs render straight at their size; smaller ones at twice the output
/// size (at most the camera's own), then shrink, which antialiases without the cost
/// of always rendering full size.
pub fn render_size(full: (u32, u32), out: (u32, u32)) -> (u32, u32) {
    if out.0 >= full.0 && out.1 >= full.1 {
        return out;
    }
    let k = (full.0 as f32 / out.0 as f32).min(2.);
    if k <= 1. {
        return out;
    }
    let side = |s: u32| ((s as f32 * k).round() as u32).max(1);
    (side(out.0), side(out.1))
}

/// Resize in premultiplied alpha, so transparent edges don't pick up dark fringes.
pub fn resize_premultiplied(img: &image::RgbaImage, w: u32, h: u32) -> image::RgbaImage {
    resize_pixels(img.as_raw(), (img.width(), img.height()), (w, h), false)
}

/// RGBA (or BGRA, swapped as it goes) `pixels` of `size`, resized to `to` in
/// premultiplied alpha.
pub fn resize_pixels(pixels: &[u8], size: (u32, u32), to: (u32, u32), bgra: bool) -> image::RgbaImage {
    let (r, b) = if bgra { (2, 0) } else { (0, 2) };
    let mut pre = Vec::with_capacity(pixels.len());
    for p in pixels.chunks_exact(4) {
        let a = p[3] as f32 / 255.;
        let k = a / 255.;
        pre.extend_from_slice(&[p[r] as f32 * k, p[1] as f32 * k, p[b] as f32 * k, a]);
    }
    let pre = image::Rgba32FImage::from_raw(size.0, size.1, pre).unwrap();
    // through DynamicImage: `imageops::resize` is generic, so called directly it's
    // compiled with this crate's settings, and unoptimized it's ~25x slower
    let resized = image::DynamicImage::ImageRgba32F(pre)
        .resize_exact(to.0, to.1, image::imageops::FilterType::Lanczos3)
        .into_rgba32f();
    let (w, h) = (resized.width(), resized.height());
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for p in resized.as_raw().chunks_exact(4) {
        let a = p[3].clamp(0., 1.);
        let c = |v: f32| if a > 0. { ((v / a).clamp(0., 1.) * 255.).round() as u8 } else { 0 };
        out.extend_from_slice(&[c(p[0]), c(p[1]), c(p[2]), (a * 255.).round() as u8]);
    }
    image::RgbaImage::from_raw(w, h, out).unwrap()
}

/// Copy `src` into `dst` at (`x`, `y`), a row at a time (clipped to `dst`).
pub fn copy_into(dst: &mut image::RgbaImage, src: &image::RgbaImage, x: u32, y: u32) {
    let (dw, dh) = dst.dimensions();
    if x >= dw || y >= dh {
        return;
    }
    let w = src.width().min(dw - x) as usize * 4;
    let rows = src.height().min(dh - y);
    let (sw, dstride) = (src.width() as usize * 4, dw as usize * 4);
    let (src, dst) = (src.as_raw(), &mut **dst);
    for r in 0..rows as usize {
        let d = (y as usize + r) * dstride + x as usize * 4;
        dst[d..d + w].copy_from_slice(&src[r * sw..r * sw + w]);
    }
}

/// BGRA to RGBA in place, a pixel (u32) at a time.
fn swap_red_blue(pixels: &mut [u8]) {
    for p in pixels.chunks_exact_mut(4) {
        let v = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        let v = (v & 0xFF00_FF00) | ((v >> 16) & 0xFF) | ((v & 0xFF) << 16);
        p.copy_from_slice(&v.to_le_bytes());
    }
}

/// Where the files go.
pub enum Target {
    Folder(std::path::PathBuf),
    #[cfg(not(target_arch = "wasm32"))]
    Zip(zip::ZipWriter<std::fs::File>),
}

impl Target {
    /// Write one file, relative to the output.
    pub fn write(&mut self, rel: &str, bytes: &[u8]) -> Result<(), String> {
        match self {
            Target::Folder(dir) => {
                let path = dir.join(rel);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("couldn't create {}: {e}", parent.display()))?;
                }
                std::fs::write(&path, bytes).map_err(|e| format!("couldn't write {}: {e}", path.display()))
            }
            #[cfg(not(target_arch = "wasm32"))]
            Target::Zip(zip) => {
                use std::io::Write;
                let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
                zip.start_file(rel, options).map_err(|e| e.to_string())?;
                zip.write_all(bytes).map_err(|e| e.to_string())
            }
        }
    }

    fn close(self) -> Result<(), String> {
        match self {
            Target::Folder(_) => Ok(()),
            #[cfg(not(target_arch = "wasm32"))]
            Target::Zip(zip) => zip.finish().map(|_| ()).map_err(|e| e.to_string()),
        }
    }
}

/// A rendered frame for the writer: its pixels at the render size.
pub struct RawFrame {
    pub path: String,
    pub pixels: Vec<u8>,
    /// the pixels are BGRA (the GPU's order), not RGBA
    pub bgra: bool,
}

/// How the writer encodes images.
#[derive(Clone, Copy, Debug)]
pub struct Encoding {
    pub format: ImageFormat,
    pub jpg_quality: u8,
    /// what JPG's missing alpha is filled with
    pub background: [u8; 3],
}

impl Encoding {
    pub fn new(opts: &SequenceOptions, background: Option<Color>) -> Encoding {
        let bg = background.map_or([255, 255, 255], |c| [c.r, c.g, c.b]);
        Encoding { format: opts.format, jpg_quality: opts.jpg_quality.clamp(1, 100), background: bg }
    }

    pub fn encode(&self, img: &image::RgbaImage) -> Result<Vec<u8>, String> {
        use image::ImageEncoder;
        let (w, h) = img.dimensions();
        let mut buf = vec![];
        let result = match self.format {
            ImageFormat::Png => image::codecs::png::PngEncoder::new(&mut buf).write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8),
            ImageFormat::Webp => image::codecs::webp::WebPEncoder::new_lossless(&mut buf).write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8),
            ImageFormat::Jpg => {
                // over the background, without alpha
                let bg = self.background;
                let mut rgb = Vec::with_capacity((w * h * 3) as usize);
                for p in img.as_raw().chunks_exact(4) {
                    let a = p[3] as u32;
                    for c in 0..3 {
                        rgb.push(((p[c] as u32 * a + bg[c] as u32 * (255 - a) + 127) / 255) as u8);
                    }
                }
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, self.jpg_quality).write_image(&rgb, w, h, image::ExtendedColorType::Rgb8)
            }
        };
        result.map_err(|e| e.to_string())?;
        Ok(buf)
    }
}

/// Builds a packed layout once every trimmed frame's size is known.
pub type Packer = Box<dyn FnOnce(&[(u32, u32)]) -> Result<SheetPlan, String> + Send>;
/// Builds the data files (manifest, atlas, Phaser, Godot) from the final layout.
pub type Finisher = Box<dyn FnOnce(&Layout) -> Vec<(String, String)> + Send>;

/// How the writer lays frames out.
pub enum WriteMode {
    /// each frame its own file
    Files,
    /// onto spritesheets laid out beforehand: each written when the next starts
    Grid(SheetPlan),
    /// trimmed, kept until the end, then packed onto spritesheets
    Packed(Packer),
}

/// Frames the writer may hold at once; rendering waits while it's this far behind.
pub const MAX_QUEUED: usize = 3;

/// The background thread that shrinks, trims, encodes and writes frames, then the data
/// files, so the app stays responsive (§8.3).
pub struct Writer {
    tx: Option<std::sync::mpsc::Sender<RawFrame>>,
    /// sent but not yet written
    pub queued: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub written: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// spritesheets written (known for packed sheets only at the end)
    pub sheets: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    handle: Option<std::thread::JoinHandle<Result<(), String>>>,
}

impl Writer {
    pub fn spawn(mut target: Target, render: (u32, u32), out: (u32, u32), enc: Encoding, mode: WriteMode, finish: Finisher) -> Writer {
        use std::sync::atomic::Ordering;
        let (tx, rx) = std::sync::mpsc::channel::<RawFrame>();
        let queued = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let written = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sheets_written = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (q, w, sw) = (queued.clone(), written.clone(), sheets_written.clone());
        let handle = std::thread::spawn(move || -> Result<(), String> {
            let trimming = matches!(mode, WriteMode::Packed(_));
            // grid: the sheet being filled; packed: every trimmed frame
            let mut sheet: Option<(usize, image::RgbaImage)> = None;
            let mut kept: Vec<image::RgbaImage> = vec![];
            let mut trims: Vec<Trim> = vec![];
            let mut index = 0;
            for mut frame in rx {
                if frame.pixels.len() != (render.0 * render.1 * 4) as usize {
                    return Err("a frame came back from the GPU the wrong size".into());
                }
                let img = if out != render {
                    resize_pixels(&frame.pixels, render, out, frame.bgra)
                } else {
                    if frame.bgra {
                        swap_red_blue(&mut frame.pixels);
                    }
                    image::RgbaImage::from_raw(render.0, render.1, frame.pixels).unwrap()
                };
                match &mode {
                    WriteMode::Files => target.write(&frame.path, &enc.encode(&img)?)?,
                    WriteMode::Grid(plan) => {
                        let (s, x, y) = plan.placements[index];
                        if sheet.as_ref().map(|(i, _)| *i) != Some(s) {
                            if let Some((i, done)) = sheet.take() {
                                target.write(&plan.files[i], &enc.encode(&done)?)?;
                                sw.fetch_add(1, Ordering::SeqCst);
                            }
                            sheet = Some((s, blank(plan.sizes[s], &enc)));
                        }
                        copy_into(&mut sheet.as_mut().unwrap().1, &img, x, y);
                    }
                    WriteMode::Packed(_) => {
                        let t = trim_bounds(&img);
                        kept.push(crop(&img, t));
                        trims.push(t);
                    }
                }
                index += 1;
                q.fetch_sub(1, Ordering::SeqCst);
                w.fetch_add(1, Ordering::SeqCst);
            }

            let files = match mode {
                WriteMode::Files => finish(&Layout::default()),
                WriteMode::Grid(plan) => {
                    if let Some((i, done)) = sheet.take() {
                        target.write(&plan.files[i], &enc.encode(&done)?)?;
                        sw.fetch_add(1, Ordering::SeqCst);
                    }
                    finish(&Layout { sheets: Some(&plan), ..Default::default() })
                }
                WriteMode::Packed(packer) => {
                    let sizes: Vec<(u32, u32)> = kept.iter().map(|i| i.dimensions()).collect();
                    let plan = packer(&sizes)?;
                    for s in 0..plan.files.len() {
                        let mut img = blank(plan.sizes[s], &enc);
                        for (i, frame) in kept.iter().enumerate().filter(|(i, _)| plan.placements[*i].0 == s) {
                            let (_, x, y) = plan.placements[i];
                            copy_into(&mut img, frame, x, y);
                        }
                        target.write(&plan.files[s], &enc.encode(&img)?)?;
                        sw.fetch_add(1, Ordering::SeqCst);
                    }
                    finish(&Layout { sheets: Some(&plan), trims: trimming.then_some(&trims[..]), ..Default::default() })
                }
            };
            for (name, contents) in files {
                target.write(&name, contents.as_bytes())?;
            }
            target.close()
        });
        Writer { tx: Some(tx), queued, written, sheets: sheets_written, handle: Some(handle) }
    }

    /// Hand a frame over. False if the writer has stopped (it failed).
    pub fn send(&self, frame: RawFrame) -> bool {
        self.queued.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.tx.as_ref().map_or(false, |tx| tx.send(frame).is_ok())
    }

    pub fn has_room(&self) -> bool {
        self.queued.load(std::sync::atomic::Ordering::SeqCst) < MAX_QUEUED
    }

    /// No more frames: the writer finishes with the sheets and data files.
    pub fn close(&mut self) {
        self.tx = None;
    }

    /// The writer's result once it has stopped (finished, or failed).
    pub fn result(&mut self) -> Option<Result<(), String>> {
        if !self.handle.as_ref()?.is_finished() {
            return None;
        }
        let handle = self.handle.take()?;
        Some(handle.join().unwrap_or_else(|_| Err("the writer crashed".into())))
    }
}

/// An empty sheet: transparent, or for JPG the background.
fn blank((w, h): (u32, u32), enc: &Encoding) -> image::RgbaImage {
    if enc.format == ImageFormat::Jpg {
        let [r, g, b] = enc.background;
        image::RgbaImage::from_pixel(w, h, image::Rgba([r, g, b, 255]))
    } else {
        image::RgbaImage::new(w, h)
    }
}

/// An export in progress: `lib.rs` renders frames each app frame and hands them to the
/// `Writer`.
pub struct Job {
    /// the rest armature, animated frame by frame
    pub armature: Armature,
    pub rest_bones: Vec<crate::shared::Bone>,
    pub frames: Vec<PlannedFrame>,
    /// the next frame to render
    pub next: usize,
    /// the animation being rendered (its physics restarts with each animation)
    pub anim: usize,
    pub camera: RenderCamera,
    /// per planned frame: how far a followed bone moves the camera, in world units
    pub follow: Vec<crate::shared::Vec2>,
    /// rendered at this size, then resized to `out` if they differ
    pub render: (u32, u32),
    pub out: (u32, u32),
    pub clear: Color,
    pub writer: Writer,
    /// where it's going, for messages
    pub dest: std::path::PathBuf,
    pub rig: String,
    /// a spritesheet export
    pub sheets: bool,
}

/// How many of `paths` (relative) already exist in `dir`.
pub fn existing_files(dir: &std::path::Path, paths: &[String]) -> usize {
    paths.iter().filter(|p| dir.join(p).exists()).count()
}
