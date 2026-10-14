//! Photoshop import: every visible layer becomes its own bone and texture, so
//! art an artist split into layers for rigging stays split. Groups become
//! untextured parent bones that hold the hierarchy.
//!
//! Marker layers (any name containing `$`) aren't art:
//! - `$pivot`: the group bone sits at this layer's center, so it rotates there.
//!   Without one, a group bone sits at the center of its art.
//! - `$ik_<n>`, `$ik_clockwise`, `$ik_counterclockwise`: the group bone joins
//!   IK chain n, or sets the chain's bend. The chain's last group gets the IK
//!   constraint, turning the others, with a target where it's drawn.
//!
//! A group named with `$"Style"` holds variants: each of its layers becomes a
//! texture in style "Style", replacing the same-named layer of the group it sits in.
//!
//! Reading the PSD (`tree_from_psd`) is separate from building the rig
//! (`build_armature`), so the rig-building rules can be tested without PSD files.

use crate::*;
use image::RgbaImage;
use wgpu::{BindGroupLayout, Device, Queue};

pub struct PsdTree {
    pub width: u32,
    pub height: u32,
    pub groups: Vec<TreeGroup>,
    /// Top layer first, as in Photoshop's layers panel.
    pub layers: Vec<TreeLayer>,
}

pub struct TreeGroup {
    pub id: u32,
    pub name: String,
    pub parent: Option<u32>,
}

pub struct TreeLayer {
    pub name: String,
    pub parent: Option<u32>,
    /// Top-left in canvas px (y down).
    pub tl: Vec2,
    pub size: Vec2,
    /// The layer's pixels, cropped to `tl`/`size`. `None` for marker layers.
    pub image: Option<RgbaImage>,
}

impl TreeLayer {
    pub fn is_marker(&self) -> bool {
        self.name.contains('$')
    }
    fn center(&self) -> Vec2 {
        self.tl + self.size / 2.
    }
}

/// GPU handles for the textures; all `None` in tests.
#[derive(Clone, Copy, Default)]
pub struct Gpu<'a> {
    pub queue: Option<&'a Queue>,
    pub device: Option<&'a Device>,
    pub layout: Option<&'a BindGroupLayout>,
    pub ctx: Option<&'a egui::Context>,
}

fn is_style_group(name: &str) -> bool {
    name.contains("$\"")
}

fn style_name(group_name: &str) -> Option<String> {
    group_name.split('"').nth(1).map(|s| s.to_string())
}

/// The visible groups and layers of `psd`. A layer or group inside a hidden
/// group is skipped too. Art layers are flattened one at a time, so memory
/// stays at about one canvas-sized buffer.
pub fn tree_from_psd(psd: &psd::Psd) -> PsdTree {
    let groups = psd.groups();
    let visible_group = |mut id: Option<u32>| {
        while let Some(g) = id.and_then(|i| groups.get(&i)) {
            if !g.visible() {
                return false;
            }
            id = g.parent_id();
        }
        true
    };

    let mut tree_groups: Vec<TreeGroup> = groups
        .values()
        .filter(|g| g.visible() && visible_group(g.parent_id()))
        .map(|g| TreeGroup { id: g.id(), name: g.name().to_string(), parent: g.parent_id() })
        .collect();
    tree_groups.sort_by_key(|g| g.id);

    let mut layers = vec![];
    for (idx, layer) in psd.layers().iter().enumerate() {
        if !layer.visible() || !visible_group(layer.parent_id()) {
            continue;
        }
        let name = layer.name().to_string();
        let mut tl = Vec2::new(layer.layer_left() as f32, layer.layer_top() as f32);
        let mut size = Vec2::new(layer.width() as f32, layer.height() as f32);
        let mut image = None;
        if !name.contains('$') {
            let Ok((px, w, h, x, y)) = psd.flatten_layers_rgba(&|(i, _)| i == idx) else {
                continue;
            };
            // empty, or entirely off the canvas
            if w == 0 || h == 0 {
                continue;
            }
            let Some(full) = RgbaImage::from_raw(w, h, px) else { continue };
            // exporters often save every layer canvas-sized: keep just the art
            let Some((cx, cy, cw, ch)) = opaque_bounds(&full) else { continue };
            tl = Vec2::new((x + cx) as f32, (y + cy) as f32);
            size = Vec2::new(cw as f32, ch as f32);
            image = Some(image::imageops::crop_imm(&full, cx, cy, cw, ch).to_image());
        }
        layers.push(TreeLayer { name, parent: layer.parent_id(), tl, size, image });
    }

    PsdTree { width: psd.width(), height: psd.height(), groups: tree_groups, layers }
}

/// The smallest rectangle (x, y, width, height) holding every pixel that isn't
/// fully transparent, or `None` if they all are.
pub fn opaque_bounds(img: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = img.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p.0[3] != 0 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
    }
    (x1 > x0 && y1 > y0).then(|| (x0, y0, x1 - x0, y1 - y0))
}

/// Canvas px (y down, origin top-left) to armature space (y up, origin at the
/// canvas center).
fn to_world(p: Vec2, tree: &PsdTree) -> Vec2 {
    Vec2::new(p.x - tree.width as f32 / 2., tree.height as f32 / 2. - p.y)
}

/// Replace `arm`'s bones with a rig built from `tree`, and add its textures to
/// `arm`'s styles (replacing same-named textures there). Bones are created as
/// Root, the group bones, a bone per art layer from the bottom layer up (so
/// higher layers get higher z-indices), then IK targets.
pub fn build_armature(tree: &PsdTree, arm: &mut Armature, default_style: &str, gpu: Gpu) {
    arm.bones = vec![];
    let root = arm.new_bone(-1).0.id;
    {
        let r = arm.find_bone_mut(root).unwrap();
        r.name = "Root".to_string();
        r.folded = true;
    }

    let style_id = |arm: &mut Armature, name: &str| -> i32 {
        let low = name.to_lowercase().trim().to_string();
        if let Some(s) = arm.styles.iter().find(|s| s.name.to_lowercase().trim() == low) {
            return s.id;
        }
        let ids = arm.styles.iter().map(|s| s.id).collect();
        let id = generate_id(ids);
        arm.styles.push(Style { id, name: name.to_string(), textures: vec![], active: true });
        id
    };
    let default_id = style_id(arm, default_style);

    // group bones, parents first; style groups don't get bones
    let group = |id: u32| tree.groups.iter().find(|g| g.id == id);
    let mut group_bone: std::collections::HashMap<u32, i32> = Default::default();
    let mut world: std::collections::HashMap<i32, Vec2> = Default::default();
    world.insert(root, Vec2::ZERO);
    let mut pending: Vec<&TreeGroup> = tree.groups.iter().filter(|g| !is_style_group(&g.name)).collect();
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|g| {
            let parent_bone = match g.parent {
                None => root,
                Some(p) => match (group(p), group_bone.get(&p)) {
                    (Some(_), Some(&b)) => b,
                    // parent is hidden, a style group, or missing: attach to Root
                    (None, _) => root,
                    (Some(pg), None) if is_style_group(&pg.name) => root,
                    (Some(_), None) => return true,
                },
            };
            let id = arm.new_bone(-1).0.id;
            let b = arm.find_bone_mut(id).unwrap();
            b.name = g.name.clone();
            b.parent_id = parent_bone;
            b.folded = true;
            group_bone.insert(g.id, id);
            false
        });
        if pending.len() == before {
            break;
        }
    }

    // group positions: the $pivot marker, else the center of the group's art
    let in_group = |mut parent: Option<u32>, gid: u32| {
        while let Some(p) = parent {
            if p == gid {
                return true;
            }
            parent = group(p).and_then(|g| g.parent);
        }
        false
    };
    for (&gid, &bone) in &group_bone {
        let pivot = tree.layers.iter().find(|l| l.parent == Some(gid) && l.name.contains("$pivot"));
        let pos = match pivot {
            Some(p) => p.center(),
            None => {
                let art: Vec<&TreeLayer> = tree.layers.iter().filter(|l| l.image.is_some() && in_group(l.parent, gid)).collect();
                if art.is_empty() {
                    Vec2::new(tree.width as f32 / 2., tree.height as f32 / 2.)
                } else {
                    let min = art.iter().fold(Vec2::new(f32::MAX, f32::MAX), |m, l| Vec2::new(m.x.min(l.tl.x), m.y.min(l.tl.y)));
                    let max = art.iter().fold(Vec2::new(f32::MIN, f32::MIN), |m, l| {
                        let br = l.tl + l.size;
                        Vec2::new(m.x.max(br.x), m.y.max(br.y))
                    });
                    (min + max) / 2.
                }
            }
        };
        world.insert(bone, to_world(pos, tree));
    }

    // IK markers: `$ik_N` puts the group's bone in chain N (groups in tree order),
    // `$ik_clockwise` / `$ik_counterclockwise` set the chain's bend
    let mut ik_chains: Vec<(i32, Vec<i32>)> = vec![];
    let mut ik_bends: std::collections::HashMap<i32, IkBend> = Default::default();
    let mut gids: Vec<u32> = group_bone.keys().copied().collect();
    gids.sort();
    for gid in gids {
        let bone_id = group_bone[&gid];
        for l in tree.layers.iter().filter(|l| l.parent == Some(gid) && l.name.contains("$ik_")) {
            if l.name.contains("counterclockwise") {
                ik_bends.insert(bone_id, IkBend::CounterClockwise);
            } else if l.name.contains("clockwise") {
                ik_bends.insert(bone_id, IkBend::Clockwise);
            } else if let Ok(n) = utils::without_unicode(utils::after_underscore(&l.name)).parse::<i32>() {
                match ik_chains.iter_mut().find(|c| c.0 == n) {
                    Some(chain) => chain.1.push(bone_id),
                    None => ik_chains.push((n, vec![bone_id])),
                }
            }
        }
    }

    // a bone per art layer, bottom layer first
    let mut used: Vec<(i32, String)> = vec![];
    let mut layer_tex: std::collections::HashMap<(Option<u32>, String), String> = Default::default();
    for l in tree.layers.iter().rev() {
        let Some(img) = &l.image else { continue };
        let style_group = l.parent.and_then(group).filter(|g| is_style_group(&g.name));
        if style_group.is_some() {
            continue;
        }

        // reuse an identical texture; otherwise add one, named after the layer
        let existing = arm.styles.iter().find(|s| s.id == default_id).and_then(|s| {
            s.textures.iter().find(|t| {
                arm.tex_data.iter().find(|d| d.id == t.data_id).map_or(false, |d| d.image.to_rgba8().as_raw() == img.as_raw())
            })
        });
        let tex_name = match existing {
            Some(t) => t.name.clone(),
            None => {
                let name = unique_name(&utils::without_unicode(&l.name), default_id, &used);
                add_layer_texture(arm, default_id, &name, img, gpu);
                used.push((default_id, name.clone()));
                name
            }
        };
        layer_tex.insert((l.parent, l.name.clone()), tex_name.clone());

        let id = arm.new_bone(-1).0.id;
        let parent = l.parent.and_then(|p| group_bone.get(&p).copied()).unwrap_or(root);
        let b = arm.find_bone_mut(id).unwrap();
        b.name = l.name.clone();
        b.parent_id = parent;
        b.tex = tex_name.clone();
        b.pivot_scale = Vec2::new(1., 1.);
        arm.set_bone_tex(id, tex_name, usize::MAX, -1);
        world.insert(id, to_world(l.center(), tree));
    }

    // style variants: each layer replaces the same-named layer of the group
    // that holds the style group
    for l in tree.layers.iter().rev() {
        let Some(img) = &l.image else { continue };
        let Some(sg) = l.parent.and_then(group).filter(|g| is_style_group(&g.name)) else { continue };
        let Some(style) = style_name(&sg.name) else { continue };
        let sid = style_id(arm, &style);
        let name = layer_tex
            .get(&(sg.parent, l.name.clone()))
            .cloned()
            .unwrap_or_else(|| utils::without_unicode(&l.name).to_string());
        if let Some(s) = arm.styles.iter_mut().find(|s| s.id == sid) {
            s.textures.retain(|t| t.name != name);
        }
        add_layer_texture(arm, sid, &name, img, gpu);
    }

    // world positions to local (no rotation or scale at import, so a subtraction)
    let parents: Vec<(i32, i32)> = arm.bones.iter().map(|b| (b.id, b.parent_id)).collect();
    for (id, parent) in parents {
        let w = world.get(&id).copied().unwrap_or_default();
        let pw = if parent == -1 { Vec2::ZERO } else { world.get(&parent).copied().unwrap_or_default() };
        arm.find_bone_mut(id).unwrap().pos = w - pw;
    }

    add_ik(arm, &ik_chains, &ik_bends, &world);
}

/// An IK constraint on the last bone of each marked chain, turning the others,
/// with a target where that bone is drawn (docs/IK.md §5). Solving is relative to
/// the drawn pose, so nothing moves.
fn add_ik(
    arm: &mut Armature,
    chains: &[(i32, Vec<i32>)],
    bends: &std::collections::HashMap<i32, IkBend>,
    world: &std::collections::HashMap<i32, Vec2>,
) {
    for (_, ids) in chains {
        let Some(&tip_id) = ids.last() else { continue };
        if ids.len() < 2 {
            continue;
        }
        let bend = ids.iter().find_map(|id| bends.get(id)).copied().unwrap_or_default();

        let target_id = arm.new_bone(-1).0.id;
        let tip_name = arm.find_bone_mut(tip_id).unwrap().name.clone();
        let target = arm.find_bone_mut(target_id).unwrap();
        target.name = format!("{tip_name} Target");
        target.pos = world.get(&tip_id).copied().unwrap_or_default();
        target.zindex = 0;

        arm.find_bone_mut(tip_id).unwrap().ik = Some(IkConstraint {
            target_id,
            chain: ids.len() as u32 - 1,
            bend,
            ..Default::default()
        });
    }
}

/// `base`, or `base 2`, `base 3`… if this import already used it in `style`.
fn unique_name(base: &str, style: i32, used: &[(i32, String)]) -> String {
    let taken = |n: &str| used.iter().any(|(s, u)| *s == style && u == n);
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).unwrap()
}

/// Add `img` as texture `name` in `style`, replacing a same-named one there.
fn add_layer_texture(arm: &mut Armature, style: i32, name: &str, img: &RgbaImage, gpu: Gpu) {
    if let Some(s) = arm.styles.iter_mut().find(|s| s.id == style) {
        s.textures.retain(|t| t.name != name);
    }
    let dims = Vec2::new(img.width() as f32, img.height() as f32);
    let image = image::DynamicImage::ImageRgba8(img.clone());
    file_reader::add_texture(image, style, dims, name, arm, gpu.queue, gpu.device, gpu.layout, gpu.ctx);
}
