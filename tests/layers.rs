//! Animation layers (docs/TIMELINE.md §5): evaluation, keying through a layer,
//! flattening and merging.

use keyternity_lib::layers;
use keyternity_lib::shared::*;

fn key(element: AnimElement, frame: i32, value: f32) -> Keyframe {
    let (start_handle, end_handle) = keyternity_lib::utils::interp_preset(HandlePreset::Linear);
    Keyframe { frame, bone_id: 0, element, value, start_handle, end_handle, ..Default::default() }
}

fn setup() -> Vec<Bone> {
    vec![Bone { id: 0, parent_id: -1, name: "chest".into(), rot: 0.5, scale: Vec2::new(1., 2.), ..Default::default() }]
}

/// a walk on Base: rotation 0 → 1 over 10 frames
fn walk() -> Animation {
    Animation { fps: 24, keyframes: vec![key(AnimElement::Rotation, 0, 0.), key(AnimElement::Rotation, 10, 1.)], ..Default::default() }
}

fn rot(anim: &Animation, frame: i32) -> f32 {
    layers::pose(anim, &setup(), frame)[0].rot
}

use AnimElement as E;

#[test]
fn without_layers_it_plays_as_before() {
    let a = walk();
    assert_eq!(rot(&a, 0), 0.);
    assert!((rot(&a, 5) - 0.5).abs() < 1e-4);
    assert_eq!(layers::pose(&a, &setup(), 5)[0].scale.y, 2., "unkeyed channels keep the setup pose");
}

#[test]
fn override_and_additive() {
    let mut a = walk();
    let i = a.add_layer("Breathing", LayerMode::Additive);
    a.layers[i].keyframes = vec![key(E::Rotation, 0, 0.5 + 0.2)]; // setup 0.5: an offset of 0.2
    assert!((rot(&a, 5) - 0.7).abs() < 1e-4, "adds its offset from the setup pose");
    a.layers[i].weight = 0.5;
    assert!((rot(&a, 5) - 0.6).abs() < 1e-4, "by its weight");

    a.layers[i].mode = LayerMode::Override;
    a.layers[i].weight = 1.;
    assert!((rot(&a, 5) - 0.7).abs() < 1e-4, "replaces");
    a.layers[i].weight = 0.5;
    assert!((rot(&a, 5) - 0.6).abs() < 1e-4, "blends halfway: (0.5 + 0.7) / 2");

    a.layers[i].muted = true;
    assert!((rot(&a, 5) - 0.5).abs() < 1e-4, "muted layers don't play");
}

#[test]
fn additive_scale_multiplies() {
    let mut a = walk();
    let i = a.add_layer("Puff", LayerMode::Additive);
    a.layers[i].keyframes = vec![key(E::ScaleY, 0, 3.)]; // setup 2: ×1.5
    assert!((layers::pose(&a, &setup(), 0)[0].scale.y - 3.).abs() < 1e-4);
}

#[test]
fn solo_plays_only_soloed_layers() {
    let mut a = walk();
    let x = a.add_layer("x", LayerMode::Additive);
    let y = a.add_layer("y", LayerMode::Additive);
    a.layers[x].keyframes = vec![key(E::Rotation, 0, 0.6)];
    a.layers[y].keyframes = vec![key(E::Rotation, 0, 0.8)];
    a.layers[y].solo = true;
    assert!((rot(&a, 0) - 0.3).abs() < 1e-4, "Base 0 plus y's 0.3");
}

#[test]
fn the_active_layer_keeps_its_keys_in_keyframes() {
    let mut a = walk();
    let i = a.add_layer("Face", LayerMode::Additive);
    a.layers[i].keyframes = vec![key(E::Rotation, 0, 0.6)];
    let before = rot(&a, 5);
    a.set_active_layer(i);
    assert_eq!(a.keyframes.len(), 1, "the layer's keys are what tools edit");
    assert_eq!(a.layer_keys(0).len(), 2);
    assert_eq!(rot(&a, 5), before, "switching layers doesn't change the pose");
    let saved = a.normalized();
    assert_eq!(saved.active_layer, 0);
    assert_eq!(saved.keyframes.len(), 2, "files keep Base in keyframes");
}

#[test]
fn keying_through_a_layer_shows_the_target() {
    let mut a = walk();
    let i = a.add_layer("Face", LayerMode::Additive);
    a.layers[i].weight = 0.5;
    a.set_active_layer(i);
    let k = layers::key_on_active(&a, &setup(), 0, &E::Rotation, 5, 0.9);
    a.keyframes = vec![key(E::Rotation, 5, k)];
    assert!((rot(&a, 5) - 0.9).abs() < 1e-4, "the pose shows what was set: {}", rot(&a, 5));

    a.layers[i].mode = LayerMode::Override;
    let k = layers::key_on_active(&a, &setup(), 0, &E::Rotation, 5, 0.2);
    a.keyframes = vec![key(E::Rotation, 5, k)];
    assert!((rot(&a, 5) - 0.2).abs() < 1e-4);
}

#[test]
fn flattening_matches_the_layers_frame_by_frame() {
    let mut a = walk();
    let i = a.add_layer("Wobble", LayerMode::Additive);
    a.layers[i].keyframes = vec![key(E::Rotation, 0, 0.5), key(E::Rotation, 4, 0.8), key(E::Rotation, 10, 0.5), key(E::ScaleX, 3, 1.5)];
    a.layers[i].weight = 0.7;
    let flat = layers::flatten(&a, &setup());
    assert!(flat.layers.is_empty());
    for f in 0..=10 {
        let (want, got) = (layers::pose(&a, &setup(), f), layers::pose(&flat, &setup(), f));
        assert!((want[0].rot - got[0].rot).abs() < 1e-4, "frame {f}: {} vs {}", want[0].rot, got[0].rot);
        assert!((want[0].scale.x - got[0].scale.x).abs() < 1e-4, "frame {f}");
    }
    assert!(flat.keyframes.len() < 22, "straight runs need no extra keys: {}", flat.keyframes.len());
}

#[test]
fn discrete_channels_come_from_the_top_layer() {
    let mut a = walk();
    a.keyframes.push(Keyframe { element: E::Hidden, value: 0., ..key(E::Hidden, 0, 0.) });
    let i = a.add_layer("Blink", LayerMode::Override);
    a.layers[i].keyframes = vec![key(E::Hidden, 4, 1.), key(E::Hidden, 6, 0.)];
    let hidden = |a: &Animation, f| layers::pose(a, &setup(), f)[0].hidden;
    assert_eq!((hidden(&a, 3), hidden(&a, 4), hidden(&a, 6)), (false, true, false));
    a.layers[i].weight = 0.4;
    assert!(!hidden(&a, 4), "below half weight, discrete keys don't apply");
    a.layers[i].weight = 1.;
    let flat = layers::flatten(&a, &setup());
    assert_eq!((hidden(&flat, 3), hidden(&flat, 4), hidden(&flat, 6)), (false, true, false));
}

#[test]
fn merging_down() {
    let mut a = walk();
    let x = a.add_layer("x", LayerMode::Additive);
    let y = a.add_layer("y", LayerMode::Override);
    a.layers[x].keyframes = vec![key(E::Rotation, 0, 0.6), key(E::Rotation, 10, 0.9)];
    a.layers[x].weight = 0.5;
    a.layers[y].keyframes = vec![key(E::Rotation, 5, 2.)];
    a.layers[y].weight = 0.25;
    let want: Vec<f32> = (0..=10).map(|f| rot(&a, f)).collect();

    // y into x (a layer): x's mode and weight are kept, and keyed so the pose holds
    let mut m = a.clone();
    assert!(layers::merge_down(&mut m, &setup(), y));
    assert_eq!(m.layers.len(), 2);
    for f in 0..=10 {
        assert!((rot(&m, f) - want[f as usize]).abs() < 1e-3, "frame {f}: {} vs {}", rot(&m, f), want[f as usize]);
    }
    // then x into Base: no layers left
    assert!(layers::merge_down(&mut m, &setup(), 1));
    assert!(m.layers.is_empty());
    for f in 0..=10 {
        assert!((rot(&m, f) - want[f as usize]).abs() < 1e-3, "frame {f}");
    }
}
