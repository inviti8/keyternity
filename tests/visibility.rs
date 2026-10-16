//! Keying visibility: what the Visible row and the eye toggles send (`edit_bone` with
//! `AnimElement::Hidden`), through the editor's event path.

use keyternity_lib::editor;
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

fn talking() -> Shared {
    let mut s = Shared::default();
    s.armature.bones = vec![Bone { id: 0, parent_id: -1, name: "mouth".into(), scale: Vec2::new(1., 1.), ..Default::default() }];
    s.armature.animations = vec![Animation { id: 0, name: "talk".into(), fps: 24, ..Default::default() }];
    s.edit_mode.anim_open = true;
    s.selections.anim = 0;
    s.selections.bone_idx = 0;
    s
}

fn hide(s: &mut Shared, frame: i32) {
    s.events.save_edited_bone(0);
    s.events.edit_bone(0, &AnimElement::Hidden, 1., "", 0, frame);
    run(s);
}

fn hidden_keys(s: &Shared) -> Vec<(i32, f32)> {
    s.armature.animations[0].keyframes.iter().filter(|k| k.element == AnimElement::Hidden).map(|k| (k.frame, k.value)).collect()
}

#[test]
fn keys_visibility_at_the_frame() {
    let mut s = talking();
    hide(&mut s, 5);
    // a frame-0 key keeps the earlier frames visible
    assert_eq!(hidden_keys(&s), vec![(0, 0.), (5, 1.)]);
    assert!(!s.armature.bones[0].hidden, "the setup pose is unchanged");
    let mut at = |f| s.armature.animate(0, f, None)[0].hidden;
    assert_eq!((at(0), at(4), at(5), at(9)), (false, false, true, true), "visibility steps, it doesn't blend");
}

#[test]
fn without_a_frame_or_when_locked_it_changes_the_setup_pose() {
    let mut s = talking();
    hide(&mut s, -1);
    assert!(hidden_keys(&s).is_empty());
    assert!(s.armature.bones[0].hidden);

    let mut s = talking();
    s.armature.bones[0].locked = true;
    hide(&mut s, 5);
    assert!(hidden_keys(&s).is_empty());
    assert!(s.armature.bones[0].hidden);
}

#[test]
fn undo_removes_the_key() {
    let mut s = talking();
    hide(&mut s, 5);
    s.events.undo();
    run(&mut s);
    assert!(hidden_keys(&s).iter().all(|(f, _)| *f != 5), "{:?}", hidden_keys(&s));
}
