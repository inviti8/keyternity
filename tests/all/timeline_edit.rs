//! Timeline edits through the editor's events: scaling and moving keys, deleting a
//! row's keys (docs/TIMELINE.md §3).

use keyternity_lib::editor;
use keyternity_lib::shared::*;
use keyternity_lib::timeline_rows::Group;

fn run(s: &mut Shared) {
    while !s.events.events.is_empty() {
        #[rustfmt::skip]
        editor::iterate_events(
            &s.input, &mut s.config, &mut s.events, &mut s.camera, &mut s.edit_mode, &mut s.selections,
            &mut s.undo_states, &mut s.armature, &mut s.psd_armature, &mut s.copy_buffer, &mut s.ui, &mut s.renderer,
        );
    }
}

fn key(element: AnimElement, frame: i32) -> Keyframe {
    Keyframe { frame, bone_id: 0, element, ..Default::default() }
}

fn rig(keys: Vec<Keyframe>) -> Shared {
    let mut s = Shared::default();
    s.armature.bones = vec![Bone { id: 0, parent_id: -1, name: "arm".into(), scale: Vec2::new(1., 1.), ..Default::default() }];
    s.armature.animations = vec![Animation { id: 0, name: "wave".into(), fps: 24, keyframes: keys, ..Default::default() }];
    s.armature.animations[0].sort_keyframes();
    s.edit_mode.anim_open = true;
    s.selections.anim = 0;
    s
}

fn frames(s: &Shared, el: AnimElement) -> Vec<i32> {
    s.armature.animations[0].keyframes.iter().filter(|k| k.element == el).map(|k| k.frame).collect()
}

use AnimElement as E;

#[test]
fn scaling_retimes_the_selection_around_the_pivot() {
    let mut s = rig(vec![key(E::Rotation, 0), key(E::Rotation, 10), key(E::Rotation, 20), key(E::PositionX, 20)]);
    s.ui.selected_keyframes = s.armature.animations[0].keyframes.iter().filter(|k| k.element == E::Rotation).cloned().collect();
    s.events.scale_selected_keyframes(10, 2.);
    run(&mut s);
    assert_eq!(frames(&s, E::Rotation), vec![0, 10, 30], "frame 0 would go to -10: it stays at 0");
    assert_eq!(frames(&s, E::PositionX), vec![20], "unselected keys stay");
    assert!(s.ui.selected_keyframes.iter().any(|k| k.frame == 30), "the selection follows the keys");

    s.events.undo();
    run(&mut s);
    assert_eq!(frames(&s, E::Rotation), vec![0, 10, 20]);
}

#[test]
fn moving_keys_keeps_them_selected() {
    let mut s = rig(vec![key(E::Rotation, 0), key(E::Rotation, 5)]);
    s.ui.selected_keyframes = vec![key(E::Rotation, 5)];
    s.ui.dragged_keyframe = Keyframe { frame: 5, bone_id: -2, ..Default::default() };
    s.events.move_selected_keyframes(12);
    run(&mut s);
    assert_eq!(frames(&s, E::Rotation), vec![0, 12]);
    assert_eq!(s.ui.selected_keyframes[0].frame, 12);
}

#[test]
fn deleting_a_group_row() {
    let mut s = rig(vec![key(E::PositionX, 0), key(E::PositionY, 4), key(E::Rotation, 4)]);
    s.events.delete_keyframe_group(0, Group::Position);
    run(&mut s);
    assert!(frames(&s, E::PositionX).is_empty() && frames(&s, E::PositionY).is_empty());
    assert_eq!(frames(&s, E::Rotation), vec![4]);
    s.events.undo();
    run(&mut s);
    assert_eq!(frames(&s, E::PositionY), vec![4]);
}
