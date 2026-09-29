//! Bind Pose skinning tests (docs/BIND_POSE.md §9).

use skelform_lib::bind_pose;
use skelform_lib::renderer::{construction, is_facing_left};
use skelform_lib::shared::*;
use skelform_lib::utils;

const EPS: f32 = 1e-3;

fn bone(id: i32, parent_id: i32, name: &str, pos: Vec2, rot: f32) -> Bone {
    Bone {
        id,
        parent_id,
        name: name.to_string(),
        pos,
        rot,
        scale: Vec2::new(1., 1.),
        pivot_scale: Vec2::new(1., 1.),
        tint: TintColor::new(1., 1., 1., 1.),
        ik_target_id: -1,
        ik_family_id: -1,
        physics_id: -1,
        visuals_id: -1,
        ..Default::default()
    }
}

fn vertex(id: u32, x: f32, y: f32) -> Vertex {
    Vertex {
        id,
        pos: Vec2::new(x, y),
        uv: Vec2::new(0.5, 0.5),
        init_pos: Vec2::new(x, y),
        color: Color::new(0, 0, 0, 0),
        add_color: Color::new(0, 0, 0, 0),
        tint: TintColor::new(1., 1., 1., 1.),
        offset_rot: 0.,
    }
}

fn bind(bone_id: i32, verts: &[(i32, f32)]) -> BoneBind {
    BoneBind {
        bone_id,
        is_path: false,
        verts: verts
            .iter()
            .map(|(id, weight)| BoneBindVert {
                id: *id,
                weight: *weight,
            })
            .collect(),
    }
}

/// root → upper arm → forearm, plus a mesh bone under root with a textured pivot.
/// The mesh blends across all three frames, including a vertex in two binds.
fn arm_rig(with_texture: bool, mirrored: bool) -> Armature {
    let root = bone(0, -1, "root", Vec2::new(10., -5.), 0.1);
    let upper = bone(1, 0, "upper", Vec2::new(100., 0.), 0.2);
    let mut fore = bone(2, 1, "fore", Vec2::new(100., 0.), -0.3);
    if mirrored {
        fore.scale = Vec2::new(-1., 1.);
    }
    let mut mesh = bone(3, 0, "mesh", Vec2::new(150., 20.), 0.35);
    mesh.verts_edited = true;
    mesh.pivot_pos = Vec2::new(0.1, -0.2);
    mesh.pivot_rot = 0.2;
    mesh.pivot_scale = Vec2::new(1.1, 0.9);
    mesh.vertices = vec![
        vertex(0, -60., 15.),
        vertex(1, 0., 15.),
        vertex(2, 60., 15.),
        vertex(3, 60., -15.),
        vertex(4, 0., -15.),
        vertex(5, -60., -15.),
    ];
    mesh.indices = vec![0, 1, 4, 0, 4, 5, 1, 2, 3, 1, 3, 4];
    mesh.binds = vec![
        bind(1, &[(0, 1.), (5, 1.), (1, 0.5), (4, 0.5)]),
        bind(2, &[(2, 1.), (3, 1.), (1, 0.5), (4, 0.3)]),
    ];

    let mut arm = Armature::default();
    arm.bones = vec![root, upper, fore, mesh];
    if with_texture {
        mesh_texture(&mut arm);
    }
    arm
}

fn mesh_texture(arm: &mut Armature) {
    arm.bones[3].tex = "m".into();
    arm.styles.push(Style {
        name: "default".into(),
        active: true,
        textures: vec![Texture {
            name: "m".into(),
            size: Vec2::new(64., 32.),
            ..Default::default()
        }],
        ..Default::default()
    });
}

fn world(arm: &Armature) -> Vec<Bone> {
    let local = arm.bones.clone();
    let mut world = local.clone();
    construction(&mut world, &local);
    world
}

/// World-space vertices of the mesh bone as the renderer draws them (incl. pivot offset).
fn drawn(arm: &Armature, mesh_id: i32) -> Vec<Vec2> {
    let world = world(arm);
    let b = world.iter().find(|b| b.id == mesh_id).unwrap();
    let size = arm.tex_of(mesh_id).map(|t| t.size).unwrap_or(Vec2::ZERO);
    let left = if is_facing_left(b.scale) { -1. } else { 1. };
    let pivot = utils::rotate(&(size * b.pivot_pos), b.rot * left) * b.scale;
    b.vertices.iter().map(|v| v.pos + pivot).collect()
}

fn assert_same(a: &[Vec2], b: &[Vec2], what: &str) {
    assert_eq!(a.len(), b.len());
    for (i, (p, q)) in a.iter().zip(b).enumerate() {
        let d = (*p - *q).mag();
        assert!(d < EPS, "{what}: vertex {i} moved by {d} ({p} vs {q})");
    }
}

fn by_name<'a>(arm: &'a mut Armature, name: &str) -> &'a mut Bone {
    arm.bones.iter_mut().find(|b| b.name == name).unwrap()
}

fn helper_count(arm: &Armature) -> usize {
    arm.bones.iter().filter(|b| b.bind_owner.is_some()).count()
}

// ------------------------------------------------------------------ §9.1 no jump

#[test]
fn set_bind_pose_does_not_move_vertices() {
    for mirrored in [false, true] {
        let mut arm = arm_rig(true, mirrored);
        let before = drawn(&arm, 3);
        bind_pose::set_bind_pose(&mut arm, 3).unwrap();
        assert_same(&before, &drawn(&arm, 3), &format!("mirrored={mirrored}"));

        assert!(bind_pose::is_bind_posed(&arm, 3));
        // one helper per bound bone (upper, fore) for this mesh
        assert_eq!(helper_count(&arm), 2);
        let mesh = arm.bones.iter().find(|b| b.id == 3).unwrap();
        assert_eq!(mesh.pivot_pos, Vec2::ZERO);
        // pivot rotation/scale apply inside skinning, so they're kept
        assert_eq!(mesh.pivot_rot, 0.2);
    }
}

#[test]
fn set_bind_pose_is_idempotent() {
    let mut arm = arm_rig(false, false);
    bind_pose::set_bind_pose(&mut arm, 3).unwrap();
    let once = drawn(&arm, 3);
    let bones = arm.bones.len();
    bind_pose::set_bind_pose(&mut arm, 3).unwrap();
    assert_eq!(arm.bones.len(), bones);
    assert_same(&once, &drawn(&arm, 3), "second set");
}

#[test]
fn rejects_path_binds_and_non_meshes() {
    let mut arm = arm_rig(false, false);
    arm.bones[3].binds[0].is_path = true;
    assert!(bind_pose::set_bind_pose(&mut arm, 3).is_err());
    assert!(bind_pose::set_bind_pose(&mut arm, 1).is_err());
}

// ------------------------------------------------------------------ §9.2 standard skinning

/// Apply a SkelForm world transform to a point (same as `renderer::inherit_vert`).
fn apply(t: &Bone, p: Vec2) -> Vec2 {
    utils::rotate(&(p * t.scale), t.rot) + t.pos
}

fn apply_inverse(t: &Bone, p: Vec2) -> Vec2 {
    utils::rotate(&(p - t.pos), -t.rot) / t.scale
}

/// Linear blend skinning computed independently: Σ eⱼ · Wⱼ · inverse(Bⱼ) · v_rest,
/// with eⱼ the effective weights of the mesh's binds.
fn reference_lbs(
    bind_arm: &Armature,
    posed: &Vec<Bone>,
    setup: &Vec<Bone>,
    mesh_id: i32,
) -> Vec<Vec2> {
    let mesh = bind_arm.bones.iter().find(|b| b.id == mesh_id).unwrap();
    let parent_of = |helper: i32| {
        bind_arm
            .bones
            .iter()
            .find(|b| b.id == helper)
            .unwrap()
            .parent_id
    };
    mesh.vertices
        .iter()
        .map(|v| {
            // the owner starts at 1, each bind lerps
            let mut weights: Vec<(i32, f32)> = vec![(mesh_id, 1.)];
            for bind in &mesh.binds {
                if let Some(bv) = bind.verts.iter().find(|bv| bv.id == v.id as i32) {
                    for w in weights.iter_mut() {
                        w.1 *= 1. - bv.weight;
                    }
                    weights.push((parent_of(bind.bone_id), bv.weight));
                }
            }
            // rest position: the owner's setup frame, with the mesh's pivot rot/scale
            let owner = setup.iter().find(|b| b.id == mesh_id).unwrap();
            let rest = utils::rotate(
                &(v.pos * owner.scale * mesh.pivot_scale),
                owner.rot + mesh.pivot_rot,
            ) + owner.pos;
            let mut out = Vec2::ZERO;
            for (bone_id, w) in weights {
                let b = setup.iter().find(|b| b.id == bone_id).unwrap();
                let wb = posed.iter().find(|b| b.id == bone_id).unwrap();
                out += apply(wb, apply_inverse(b, rest)) * w;
            }
            out
        })
        .collect()
}

fn pose(arm: &mut Armature) {
    by_name(arm, "root").pos += Vec2::new(5., 3.);
    by_name(arm, "upper").rot += 0.7;
    by_name(arm, "fore").rot -= 0.5;
    by_name(arm, "fore").scale *= 1.2;
    by_name(arm, "mesh").rot += 0.25;
}

#[test]
fn posed_mesh_matches_standard_skinning() {
    for mirrored in [false, true] {
        let mut arm = arm_rig(true, mirrored);
        bind_pose::set_bind_pose(&mut arm, 3).unwrap();
        let setup = world(&arm);

        // posing = changing bones without re-syncing helpers (like an animation frame)
        let mut posed_arm = arm.clone();
        pose(&mut posed_arm);
        let actual = drawn(&posed_arm, 3);
        let expected = reference_lbs(&arm, &world(&posed_arm), &setup, 3);
        assert_same(&expected, &actual, &format!("posed, mirrored={mirrored}"));
    }
}

// ------------------------------------------------------------------ §9.3 weight edits

#[test]
fn weight_edits_do_not_move_vertices() {
    let mut arm = arm_rig(true, false);
    bind_pose::set_bind_pose(&mut arm, 3).unwrap();
    let before = drawn(&arm, 3);
    for bind in arm
        .bones
        .iter_mut()
        .find(|b| b.id == 3)
        .unwrap()
        .binds
        .iter_mut()
    {
        for v in &mut bind.verts {
            v.weight = (v.weight * 0.37 + 0.2).min(1.);
        }
    }
    assert_same(&before, &drawn(&arm, 3), "weights edited");
}

// ------------------------------------------------------------------ §9.4 setup edits

#[test]
fn setup_edits_keep_mesh_in_place() {
    for mirrored in [false, true] {
        let mut arm = arm_rig(true, mirrored);
        bind_pose::set_bind_pose(&mut arm, 3).unwrap();
        let before = drawn(&arm, 3);

        // edit the bound bones (and the forearm's ancestor) in the setup pose, then re-sync
        by_name(&mut arm, "upper").rot += 0.4;
        by_name(&mut arm, "fore").scale *= 1.3;
        by_name(&mut arm, "fore").pos += Vec2::new(4., -9.);
        bind_pose::sync_helpers(&mut arm);
        assert_same(
            &before,
            &drawn(&arm, 3),
            &format!("setup edit, mirrored={mirrored}"),
        );

        // reparent the forearm onto the root
        by_name(&mut arm, "fore").parent_id = 0;
        bind_pose::sync_helpers(&mut arm);
        assert_same(
            &before,
            &drawn(&arm, 3),
            &format!("reparent, mirrored={mirrored}"),
        );

        // moving the mesh's own ancestor moves the mesh with it, rigidly (as always)
        by_name(&mut arm, "root").pos += Vec2::new(-7., 12.);
        bind_pose::sync_helpers(&mut arm);
        let shifted: Vec<Vec2> = before.iter().map(|p| *p + Vec2::new(-7., 12.)).collect();
        assert_same(
            &shifted,
            &drawn(&arm, 3),
            &format!("owner ancestor, mirrored={mirrored}"),
        );
    }
}

// ------------------------------------------------------------------ §9.5 round trip

#[test]
fn save_load_keeps_helpers_and_deformation() {
    use std::io::Write;

    let mut arm = arm_rig(true, false);
    arm.tex_data.push(TextureData {
        id: 0,
        image: image::DynamicImage::new_rgba8(64, 32),
        bind_group: None,
        ui_img: None,
    });
    bind_pose::set_bind_pose(&mut arm, 3).unwrap();
    let helper_names: Vec<String> = arm
        .bones
        .iter()
        .filter(|b| b.bind_owner.is_some())
        .map(|b| b.name.clone())
        .collect();
    let mut posed = arm.clone();
    pose(&mut posed);
    let expected = drawn(&posed, 3);

    let edit_mode = EditMode::default();
    let mut save_arm = arm.clone();
    let (atlases, sizes) = utils::create_tex_sheet(&mut save_arm, &edit_mode);
    let (armature_json, editor_json) =
        utils::prepare_files(&save_arm, Camera::default(), sizes, &edit_mode);
    let mut buf = std::io::Cursor::new(vec![]);
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::FullFileOptions::default();
        zip.start_file("armature.json", options.clone()).unwrap();
        zip.write_all(armature_json.as_bytes()).unwrap();
        zip.start_file("editor.json", options.clone()).unwrap();
        zip.write_all(editor_json.as_bytes()).unwrap();
        for (i, png) in atlases.iter().enumerate() {
            zip.start_file(format!("atlas{i}.png"), options.clone())
                .unwrap();
            zip.write_all(png).unwrap();
        }
        zip.finish().unwrap();
    }
    buf.set_position(0);

    let mut shared = Shared::default();
    let ctx = egui::Context::default();
    utils::import(buf, &mut shared, None, None, None, Some(&ctx));
    let mut loaded = shared.armature;

    let loaded_helpers: Vec<String> = loaded
        .bones
        .iter()
        .filter(|b| b.bind_owner.is_some())
        .map(|b| b.name.clone())
        .collect();
    assert_eq!(loaded_helpers, helper_names);

    // ids are re-indexed on save; find the mesh by name
    let mesh_id = loaded.bones.iter().find(|b| b.name == "mesh").unwrap().id;
    assert!(bind_pose::is_bind_posed(&loaded, mesh_id));
    pose(&mut loaded);
    assert_same(&expected, &drawn(&loaded, mesh_id), "after save/load");
}

// ------------------------------------------------------------------ clear

#[test]
fn clear_bind_pose_restores_classic_binds() {
    // single-bind vertices only, where classic binds are exact
    let mut arm = arm_rig(false, false);
    arm.bones[3].binds = vec![bind(1, &[(0, 1.), (5, 1.)]), bind(2, &[(2, 1.), (3, 1.)])];
    let before = drawn(&arm, 3);

    bind_pose::set_bind_pose(&mut arm, 3).unwrap();
    bind_pose::clear_bind_pose(&mut arm, 3);

    assert!(!bind_pose::is_bind_posed(&arm, 3));
    assert_eq!(helper_count(&arm), 0);
    assert_same(&before, &drawn(&arm, 3), "after clear");
}

// ------------------------------------------------------------------ editor upkeep

#[test]
fn bind_target_uses_helpers_on_bind_posed_meshes() {
    let mut arm = arm_rig(false, false);
    // classic mesh: picking a bone binds to the bone itself
    assert_eq!(bind_pose::bind_target(&mut arm, 1, 3), 1);

    bind_pose::set_bind_pose(&mut arm, 3).unwrap();
    let before = drawn(&arm, 3);
    // bind-posed: picking the root creates its helper for this mesh
    let target = bind_pose::bind_target(&mut arm, 0, 3);
    assert_eq!(bind_pose::helper_for(&arm, 0, 3), Some(target));
    let mesh = arm.bones.iter_mut().find(|b| b.id == 3).unwrap();
    mesh.binds.push(bind(target, &[(1, 0.4), (2, 0.6)]));
    bind_pose::maintain(&mut arm);
    assert_same(&before, &drawn(&arm, 3), "new bind on root");
}

#[test]
fn maintain_gives_a_pasted_mesh_its_own_helpers() {
    let mut arm = arm_rig(false, false);
    bind_pose::set_bind_pose(&mut arm, 3).unwrap();

    // simulate a paste: a copy of the mesh elsewhere, still bound to the original's helpers
    let mut copy = arm.bones.iter().find(|b| b.id == 3).unwrap().clone();
    copy.id = 50;
    copy.name = "mesh copy".into();
    copy.pos += Vec2::new(0., 80.);
    arm.bones.push(copy);

    bind_pose::maintain(&mut arm);
    assert!(bind_pose::is_bind_posed(&arm, 50));
    assert!(bind_pose::is_bind_posed(&arm, 3));
    assert_eq!(helper_count(&arm), 4);

    // the copy sits where the original is, shifted by its offset (in the root's frame)
    let offset = utils::rotate(&Vec2::new(0., 80.), by_name(&mut arm, "root").rot);
    let original = drawn(&arm, 3);
    let copied = drawn(&arm, 50);
    for (a, b) in original.iter().zip(&copied) {
        let d = *b - *a;
        assert!((d - offset).mag() < EPS, "copy offset {d}");
    }
}

#[test]
fn maintain_removes_helpers_of_deleted_meshes() {
    let mut arm = arm_rig(false, false);
    bind_pose::set_bind_pose(&mut arm, 3).unwrap();
    arm.bones.retain(|b| b.id != 3);
    bind_pose::maintain(&mut arm);
    assert_eq!(helper_count(&arm), 0);
}
