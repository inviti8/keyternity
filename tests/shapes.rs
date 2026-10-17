//! Shapes (morph targets) on meshes (docs/LIP_SYNC.md §4).

use keyternity_lib::editor;
use keyternity_lib::layers;
use keyternity_lib::renderer;
use keyternity_lib::shapes;
use keyternity_lib::shared::*;
use keyternity_lib::topology::{self, VertSite};
use keyternity_lib::utils;

fn vertex(id: u32, x: f32, y: f32) -> Vertex {
    Vertex { id, pos: Vec2::new(x, y), uv: Vec2::new(x / 10., y / 10.), init_pos: Vec2::new(x, y), ..Default::default() }
}

/// a 10×10 quad under the root, with a "smile" shape lifting its top corners
fn face() -> Armature {
    let mut mouth = Bone {
        id: 1,
        parent_id: -1,
        name: "mouth".into(),
        scale: Vec2::new(1., 1.),
        pivot_scale: Vec2::new(1., 1.),
        pos: Vec2::new(100., 50.),
        tex: "mouth".into(),
        vertices: vec![vertex(10, 0., 0.), vertex(11, 10., 0.), vertex(12, 10., 10.), vertex(13, 0., 10.)],
        indices: vec![0, 1, 2, 0, 2, 3],
        verts_edited: true,
        ..Default::default()
    };
    mouth.shapes = vec![MeshShape {
        id: 1,
        name: "smile".into(),
        deltas: vec![ShapeDelta { vert: 12, delta: Vec2::new(0., 4.) }, ShapeDelta { vert: 13, delta: Vec2::new(0., 2.) }],
        ..Default::default()
    }];
    Armature { bones: vec![mouth], ..Default::default() }
}

/// world positions of the mesh's vertices, posed as the renderer does
fn posed(bones: &[Bone]) -> Vec<Vec2> {
    let mut work = bones.to_vec();
    renderer::construction(&mut work, &bones.to_vec());
    work[0].vertices.iter().map(|v| v.pos).collect()
}

fn run(s: &mut Shared) {
    while !s.events.events.is_empty() {
        #[rustfmt::skip]
        editor::iterate_events(
            &s.input, &mut s.config, &mut s.events, &mut s.camera, &mut s.edit_mode, &mut s.selections,
            &mut s.undo_states, &mut s.armature, &mut s.psd_armature, &mut s.copy_buffer, &mut s.ui, &mut s.renderer,
        );
    }
}

fn shared_with_anim() -> Shared {
    let mut s = Shared::default();
    s.armature = face();
    s.armature.animations = vec![Animation { id: 0, name: "talk".into(), fps: 24, ..Default::default() }];
    s.edit_mode.anim_open = true;
    s.selections.anim = 0;
    s
}

#[test]
fn weights_blend_offsets_before_skinning() {
    let arm = face();
    let rest = posed(&arm.bones);
    let mut half = arm.bones.clone();
    half[0].shapes[0].weight = 0.5;
    let moved = posed(&half);
    assert_eq!(moved[0], rest[0], "vertices without offsets stay");
    assert!((moved[2].y - rest[2].y - 2.).abs() < 1e-4 && (moved[3].y - rest[3].y - 1.).abs() < 1e-4);

    // the offsets are in the mesh's space: they turn with the bone
    let mut turned = half.clone();
    turned[0].rot = std::f32::consts::FRAC_PI_2;
    let t_rest = { let mut b = arm.bones.clone(); b[0].rot = turned[0].rot; posed(&b) };
    let t = posed(&turned);
    let d = t[2] - t_rest[2];
    assert!((d.x.abs() - 2.).abs() < 1e-3 && d.y.abs() < 1e-3, "{:?}", d);
}

#[test]
fn keyed_weights_animate_and_undo() {
    let mut s = shared_with_anim();
    s.events.edit_shape_weight(1, 1, 1., 0, 10, true);
    run(&mut s);
    let keys: Vec<(i32, f32)> = s.armature.animations[0].keyframes.iter().map(|k| (k.frame, k.value)).collect();
    assert_eq!(keys, vec![(0, 0.), (10, 1.)], "a first key with the setup weight, then the new one");
    assert!(s.armature.animations[0].keyframes.iter().all(|k| k.element == AnimElement::ShapeWeight && k.shape == 1));
    let w = |s: &mut Shared, f| s.armature.animate(0, f, None)[0].shapes[0].weight;
    assert!((w(&mut s, 5) - 0.5).abs() < 1e-4);
    assert_eq!(s.armature.bones[0].shapes[0].weight, 0., "the setup weight is unchanged");

    s.events.undo();
    run(&mut s);
    assert!(s.armature.animations[0].keyframes.is_empty());
}

#[test]
fn without_a_frame_it_sets_the_setup_weight() {
    let mut s = shared_with_anim();
    s.events.edit_shape_weight(1, 1, 0.3, usize::MAX, -1, true);
    run(&mut s);
    assert_eq!(s.armature.bones[0].shapes[0].weight, 0.3);
    assert!(s.armature.animations[0].keyframes.is_empty());
}

#[test]
fn shapes_have_their_own_channels_and_layers() {
    let mut s = shared_with_anim();
    s.events.add_shape(1, "blink");
    run(&mut s);
    assert_eq!(s.armature.bones[0].shapes.iter().map(|s| (s.id, s.name.as_str())).collect::<Vec<_>>(), vec![(1, "smile"), (2, "blink")]);
    s.events.edit_shape_weight(1, 1, 1., 0, 0, true);
    s.events.edit_shape_weight(1, 2, 0.4, 0, 0, true);
    run(&mut s);
    let pose = s.armature.animate(0, 0, None);
    assert_eq!((pose[0].shapes[0].weight, pose[0].shapes[1].weight), (1., 0.4), "each shape keeps its own weight");

    // an additive layer adds to Base's weight
    let a = &mut s.armature.animations[0];
    let i = a.add_layer("Extra", LayerMode::Additive);
    a.layers[i].keyframes = vec![Keyframe { frame: 0, bone_id: 1, element: AnimElement::ShapeWeight, shape: 2, value: 0.3, ..Default::default() }];
    let pose = s.armature.animate(0, 0, None);
    assert!((pose[0].shapes[1].weight - 0.7).abs() < 1e-4);
    // and flattening keeps it
    let flat = layers::flatten(&s.armature.animations[0], &s.armature.bones);
    let k = flat.keyframes.iter().find(|k| k.shape == 2).unwrap();
    assert!((k.value - 0.7).abs() < 1e-4);
}

#[test]
fn deleting_a_shape_removes_its_keys() {
    let mut s = shared_with_anim();
    s.events.edit_shape_weight(1, 1, 1., 0, 4, true);
    run(&mut s);
    s.events.delete_shape(1, 1);
    run(&mut s);
    assert!(s.armature.bones[0].shapes.is_empty());
    assert!(s.armature.animations[0].keyframes.is_empty());
    s.events.undo();
    run(&mut s);
    assert_eq!(s.armature.bones[0].shapes.len(), 1, "undo brings it back");
    assert_eq!(s.armature.animations[0].keyframes.len(), 2, "with its keys");
}

#[test]
fn mesh_edits_keep_offsets_in_step() {
    let mut bone = face().bones[0].clone();
    // a new vertex on the top edge takes the blend of its neighbours' offsets
    let n = topology::add_vertex(&mut bone, vertex(20, 5., 10.), VertSite::Edge(2, 3)).unwrap();
    let id = bone.vertices[n as usize].id;
    let d = shapes::delta_of(&bone.shapes[0], id);
    assert!((d.y - 3.).abs() < 1e-3, "halfway between 4 and 2: {:?}", d);
    // a removed vertex takes its offsets with it
    bone.indices.retain(|i| *i != n);
    bone.indices.truncate(bone.indices.len() / 3 * 3);
    editor::cleanup_vertices(&mut bone);
    shapes::prune(&mut bone);
    assert!(bone.shapes[0].deltas.iter().all(|d| bone.vertices.iter().any(|v| v.id == d.vert)));
}

#[test]
fn mirroring_a_shape() {
    let mut bone = face().bones[0].clone();
    for v in &mut bone.vertices {
        v.pos.x -= 5.; // centred on the mesh's x = 0
    }
    let (m, missing) = shapes::mirrored(&bone, &bone.shapes[0], 0.01);
    assert_eq!(missing, 0);
    // vertex 12 (x 5) took vertex 13's offset (x -5), and the other way round
    assert_eq!(shapes::delta_of(&MeshShape { deltas: m.clone(), ..Default::default() }, 12), Vec2::new(0., 2.));
    assert_eq!(shapes::delta_of(&MeshShape { deltas: m, ..Default::default() }, 13), Vec2::new(0., 4.));
}

#[test]
fn saved_by_vertex_index() {
    let mut arm = face();
    // ids that aren't indices, as after editing
    for (i, v) in arm.bones[0].vertices.iter_mut().enumerate() {
        v.id = 40 + i as u32;
    }
    arm.bones[0].shapes[0].deltas = vec![ShapeDelta { vert: 42, delta: Vec2::new(0., 4.) }];
    let (json, _) = utils::prepare_files(&arm, Camera::default(), vec![], &EditMode::default(), None);
    let root: serde_json::Value = serde_json::from_str(&json).unwrap();
    let shape = &root["visuals"][0]["shapes"][0];
    assert_eq!(shape["name"], "smile");
    assert_eq!(shape["deltas"][0]["vert"], 2, "id 42 is the third vertex");
}

#[test]
fn duplicating_and_mirroring_through_events() {
    let mut s = shared_with_anim();
    for v in &mut s.armature.bones[0].vertices {
        v.pos.x -= 5.;
    }
    s.armature.bones[0].shapes[0].name = "Smile_L".into();
    s.events.duplicate_shape(1, 1, true);
    s.events.duplicate_shape(1, 1, false);
    run(&mut s);
    let names: Vec<&str> = s.armature.bones[0].shapes.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Smile_L", "Smile_R", "Smile_L 2"]);
    let r = &s.armature.bones[0].shapes[1];
    assert_eq!(shapes::delta_of(r, 12), Vec2::new(0., 2.), "flipped across x = 0");
}

#[test]
fn sculpting_shows_the_shape_alone() {
    let mut s = shared_with_anim();
    s.events.add_shape(1, "blink");
    s.events.edit_shape_weight(1, 2, 0.8, usize::MAX, -1, true);
    s.events.toggle_sculpt_shape(1, 1);
    run(&mut s);
    assert_eq!(s.edit_mode.sculpt_shape, Some((1, 1)));
    assert!(s.edit_mode.editing_mesh, "sculpting drags vertices");
    utils::animate_bones(&mut s.armature, &s.selections, &s.edit_mode);
    let w: Vec<f32> = s.armature.animated_bones[0].shapes.iter().map(|s| s.weight).collect();
    assert_eq!(w, vec![1., 0.]);
    s.events.toggle_sculpt_shape(1, 1);
    run(&mut s);
    assert_eq!(s.edit_mode.sculpt_shape, None);
}
