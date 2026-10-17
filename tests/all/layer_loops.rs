//! Play modes, linked layers and generators (docs/TIMELINE.md §5.7–5.8).

use keyternity_lib::editor;
use keyternity_lib::layers::{self, Ctx};
use keyternity_lib::shared::*;
use keyternity_lib::utils;

fn key(element: AnimElement, frame: i32, value: f32) -> Keyframe {
    let (start_handle, end_handle) = utils::interp_preset(HandlePreset::Linear);
    Keyframe { frame, bone_id: 0, element, value, start_handle, end_handle, ..Default::default() }
}

fn setup() -> Vec<Bone> {
    vec![Bone { id: 0, parent_id: -1, name: "chest".into(), scale: Vec2::new(1., 1.), ..Default::default() }]
}

use AnimElement as E;

/// a 10 fps animation, 4 s long, with an additive "breath" layer looping every second:
/// rotation 0 → 1 → 0 over frames 0, 5, 10
fn breathing() -> Animation {
    let mut a = Animation { id: 1, fps: 10, end_frame: 40, ..Default::default() };
    let i = a.add_layer("Breath", LayerMode::Additive);
    a.layers[i].play = PlayMode::Looped { length: 1., speed: 1., phase: 0., free_running: false };
    a.layers[i].keyframes = vec![key(E::Rotation, 0, 0.), key(E::Rotation, 5, 1.), key(E::Rotation, 10, 0.)];
    a
}

fn rot(a: &Animation, frame: i32) -> f32 {
    layers::pose(a, &setup(), frame)[0].rot
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn a_loop_repeats_for_as_long_as_the_animation_runs() {
    let a = breathing();
    for (f, want) in [(0, 0.), (5, 1.), (10, 0.), (15, 1.), (25, 1.), (32, 0.4)] {
        assert!(close(rot(&a, f), want), "frame {f}: {}", rot(&a, f));
    }
}

#[test]
fn speed_and_phase() {
    let mut a = breathing();
    a.layers[1].play = PlayMode::Looped { length: 1., speed: 2., phase: 0., free_running: false };
    assert!(close(rot(&a, 0), 0.) && close(rot(&a, 2), 0.8) && close(rot(&a, 5), 0.), "twice as fast");
    a.layers[1].play = PlayMode::Looped { length: 1., speed: 1., phase: 0.5, free_running: false };
    assert!(close(rot(&a, 0), 1.), "half a loop ahead");
    // half speed lands between frames: interpolated, not rounded
    a.layers[1].play = PlayMode::Looped { length: 1., speed: 0.5, phase: 0., free_running: false };
    assert!(close(rot(&a, 1), 0.1), "{}", rot(&a, 1));
}

#[test]
fn free_running_loops_follow_the_clock_while_playing() {
    let mut a = breathing();
    a.layers[1].play = PlayMode::Looped { length: 1., speed: 1., phase: 0., free_running: true };
    let anims = vec![a.clone()];
    let at = |clock: Option<f32>, frame| layers::pose_in(&a, &Ctx { anims: &anims, clock }, &setup(), frame)[0].rot;
    assert!(close(at(Some(0.5), 0), 1.), "the clock, not the frame");
    assert!(close(at(Some(0.5), 3), 1.), "whatever frame the animation is on");
    assert!(close(at(None, 3), 0.6), "without a clock (scrubbing, exports): the animation's time");
}

#[test]
fn linked_layers_play_another_animation() {
    // "Breathing", its own animation (20 fps): rotation 0 → 2 over a second
    let breath = Animation { id: 7, fps: 20, keyframes: vec![key(E::Rotation, 0, 0.), key(E::Rotation, 20, 2.)], ..Default::default() };
    let mut walk = Animation { id: 1, fps: 10, ..Default::default() };
    let i = walk.add_layer("Breathing", LayerMode::Additive);
    walk.layers[i].link = Some(7);
    walk.layers[i].play = PlayMode::Looped { length: 1., speed: 1., phase: 0., free_running: false };
    let anims = vec![walk.clone(), breath];
    let at = |frame| layers::pose_in(&walk, &Ctx { anims: &anims, clock: None }, &setup(), frame)[0].rot;
    assert!(close(at(5), 1.), "half a second in, at its own fps: {}", at(5));
    assert!(close(at(15), 1.), "looping");
    assert!(close(rot(&walk, 5), 0.), "without the other animations, nothing to play");
}

#[test]
fn generators_add_waves() {
    let mut a = Animation { id: 1, fps: 10, end_frame: 30, ..Default::default() };
    let i = a.add_layer("Sway", LayerMode::Additive);
    a.layers[i].generators = vec![Generator { bone_id: 0, element: E::Rotation, amplitude: 0.5, period: 2., ..Default::default() }];
    assert!(close(rot(&a, 0), 0.) && close(rot(&a, 5), 0.5) && close(rot(&a, 15), -0.5), "a sine, 2 s per cycle");
    a.layers[i].generators[0].wave = Wave::Square;
    assert!(close(rot(&a, 5), 0.5) && close(rot(&a, 15), -0.5));
    a.layers[i].generators[0].wave = Wave::Triangle;
    assert!(close(rot(&a, 5), 0.5) && close(rot(&a, 10), 0.));
}

#[test]
fn on_a_loop_generators_always_loop() {
    let mut a = Animation { id: 1, fps: 10, end_frame: 60, ..Default::default() };
    let i = a.add_layer("Idle", LayerMode::Additive);
    a.layers[i].play = PlayMode::Looped { length: 3., speed: 1., phase: 0., free_running: false };
    // 1.4 s doesn't divide 3 s: it snaps to 1.5 s
    a.layers[i].generators = vec![
        Generator { bone_id: 0, element: E::Rotation, amplitude: 1., period: 1.4, ..Default::default() },
        Generator { bone_id: 0, element: E::ScaleX, wave: Wave::Noise, amplitude: 0.3, period: 0.7, seed: 4, ..Default::default() },
    ];
    let p = |f| layers::pose(&a, &setup(), f)[0].clone();
    for f in 0..30 {
        assert!(close(p(f).rot, p(f + 30).rot), "frame {f}: the wave loops");
        assert!(close(p(f).scale.x, p(f + 30).scale.x), "frame {f}: the noise loops");
    }
    let noise: Vec<f32> = (0..30).map(|f| p(f).scale.x - 1.).collect();
    assert!(noise.iter().all(|n| n.abs() <= 0.3 + 1e-4), "within its amplitude");
    assert!(noise.iter().any(|n| n.abs() > 0.02), "and it moves");
}

#[test]
fn close_loop_and_seams() {
    let mut a = breathing();
    a.layers[1].keyframes = vec![key(E::Rotation, 0, 0.2), key(E::Rotation, 5, 1.)];
    assert_eq!(layers::close_loop(&mut a, 1), 1);
    assert!(a.layers[1].keyframes.iter().any(|k| k.frame == 10 && close(k.value, 0.2)), "the first key, at the loop's end");

    assert!(layers::seams(&a).is_empty(), "4 s is four 1 s loops");
    a.end_frame = 35;
    assert_eq!(layers::seams(&a).len(), 1, "3.5 s isn't");
}

#[test]
fn baking_a_generator_into_keys() {
    let mut a = Animation { id: 1, fps: 10, ..Default::default() };
    let i = a.add_layer("Bob", LayerMode::Additive);
    a.layers[i].play = PlayMode::Looped { length: 2., speed: 1., phase: 0., free_running: false };
    a.layers[i].generators = vec![Generator { bone_id: 0, element: E::Rotation, amplitude: 0.4, period: 2., ..Default::default() }];
    let before: Vec<f32> = (0..20).map(|f| rot(&a, f)).collect();
    layers::bake_generator(&mut a, i, 0, 1);
    assert!(a.layers[i].generators.is_empty());
    assert_eq!(a.layers[i].keyframes.len(), 21, "a key per frame over the loop");
    for f in 0..20 {
        assert!((rot(&a, f) - before[f as usize]).abs() < 0.02, "frame {f}");
    }
}

#[test]
fn flattening_bakes_loops_in_the_animations_time() {
    let a = breathing();
    let flat = layers::flatten(&a, &setup());
    for f in 0..=40 {
        assert!(close(rot(&a, f), rot(&flat, f)), "frame {f}");
    }
}

#[test]
fn editing_a_loop_keys_its_own_frame() {
    let mut s = Shared::default();
    s.armature.bones = setup();
    s.armature.animations = vec![breathing()];
    s.armature.animations[0].set_active_layer(1);
    s.edit_mode.anim_open = true;
    s.selections.anim = 0;
    s.selections.bone_idx = 0;
    // frame 17 is the loop's frame 7
    s.events.save_edited_bone(0);
    s.events.edit_bone(0, &E::Rotation, 0.9, "", 0, 17);
    while !s.events.events.is_empty() {
        #[rustfmt::skip]
        editor::iterate_events(
            &s.input, &mut s.config, &mut s.events, &mut s.camera, &mut s.edit_mode, &mut s.selections,
            &mut s.undo_states, &mut s.armature, &mut s.psd_armature, &mut s.copy_buffer, &mut s.ui, &mut s.renderer,
        );
    }
    let a = &s.armature.animations[0];
    assert!(a.keyframes.iter().any(|k| k.frame == 7), "{:?}", a.keyframes.iter().map(|k| k.frame).collect::<Vec<_>>());
    assert!(close(rot(a, 17), 0.9) && close(rot(a, 7), 0.9) && close(rot(a, 27), 0.9), "and every repeat shows it");
}
