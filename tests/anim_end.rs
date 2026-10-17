//! An animation's End (`end_frame`): its last frame, set in the timeline.

use keyternity_lib::editor;
use keyternity_lib::export_sequence;
use keyternity_lib::shared::*;

fn run(s: &mut Shared) {
    while !s.events.events.is_empty() {
        #[rustfmt::skip]
        editor::iterate_events(
            &s.input, &mut s.config, &mut s.events, &mut s.camera, &mut s.edit_mode, &mut s.selections,
            &mut s.undo_states, &mut s.armature, &mut s.psd_armature, &mut s.copy_buffer, &mut s.ui, &mut s.renderer,
        );
    }
}

fn walk() -> Shared {
    let mut s = Shared::default();
    s.armature.bones = vec![Bone { id: 0, parent_id: -1, name: "hip".into(), scale: Vec2::new(1., 1.), ..Default::default() }];
    let key = |frame| Keyframe { frame, bone_id: 0, element: AnimElement::Rotation, ..Default::default() };
    s.armature.animations = vec![Animation { id: 0, name: "walk".into(), fps: 24, keyframes: vec![key(0), key(12)], ..Default::default() }];
    s.edit_mode.anim_open = true;
    s.selections.anim = 0;
    s
}

#[test]
fn follows_the_last_key_until_set() {
    let mut s = walk();
    assert_eq!(s.armature.animations[0].last_frame(), 12);
    s.events.set_anim_end(48);
    run(&mut s);
    assert_eq!(s.armature.animations[0].last_frame(), 48, "longer than its keys: a hold at the end");
    s.events.set_anim_end(6);
    run(&mut s);
    assert_eq!(s.armature.animations[0].last_frame(), 6, "shorter: later keys are kept but not played");
    assert_eq!(s.armature.animations[0].keyframes.len(), 2);
    s.events.set_anim_end(0);
    run(&mut s);
    assert_eq!(s.armature.animations[0].last_frame(), 12, "0 follows the last key again");
    assert_eq!(Animation::default().last_frame(), 0);
}

#[test]
fn exports_stop_at_the_end() {
    let mut s = walk();
    s.armature.animations[0].end_frame = 30;
    assert_eq!(export_sequence::last_frame(&s.armature, 0), 30);
}

#[test]
fn undo_restores_the_end() {
    let mut s = walk();
    s.events.set_anim_end(40);
    run(&mut s);
    s.events.undo();
    run(&mut s);
    assert_eq!(s.armature.animations[0].end_frame, 0);
}

#[test]
fn saved_files_without_it_follow_the_last_key() {
    let anim: Animation = serde_json::from_str(r#"{"name":"old","id":0,"fps":24,"keyframes":[]}"#).unwrap();
    assert_eq!(anim.end_frame, 0);
}
