//! Deleting bones keeps the selection valid: bones deleted along with a hierarchy leave
//! the selection, and a surviving selected bone is re-found after indices shift.

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

fn bone(id: i32, parent_id: i32, name: &str) -> Bone {
    Bone { id, parent_id, name: name.into(), scale: Vec2::new(1., 1.), ..Default::default() }
}

fn select(s: &mut Shared, idx: usize) {
    s.selections.bone_idx = idx;
    s.selections.bone_ids = vec![s.armature.bones[idx].id];
    s.ui.last_selected = "bone".into();
}

#[test]
fn deleting_a_hierarchy_drops_selected_bones_inside_it() {
    // the original arm, with the pasted copy under it (selected)
    let mut s = Shared::default();
    s.armature.bones = vec![bone(0, -1, "arm"), bone(1, 0, "hand"), bone(2, 0, "arm copy"), bone(3, 2, "hand copy")];
    select(&mut s, 2);
    s.events.delete_bone(0);
    run(&mut s);
    assert!(s.armature.bones.is_empty());
    assert_eq!(s.selections.bone_idx, usize::MAX);
    assert!(s.selections.bone_ids.is_empty(), "{:?}", s.selections.bone_ids);
}

#[test]
fn a_surviving_selection_follows_its_bone() {
    let mut s = Shared::default();
    s.armature.bones = vec![bone(0, -1, "arm"), bone(1, 0, "hand"), bone(5, -1, "leg")];
    select(&mut s, 2);
    s.events.delete_bone(0);
    run(&mut s);
    assert_eq!(s.armature.bones.len(), 1);
    // "leg" moved from index 2 to 0; the selection moves with it
    assert_eq!(s.selections.bone_idx, 0);
    assert_eq!(s.selections.bone_ids, vec![5]);
}

#[test]
fn deleting_the_selected_bone_clears_the_selection() {
    let mut s = Shared::default();
    s.armature.bones = vec![bone(0, -1, "arm"), bone(1, 0, "hand"), bone(5, -1, "leg")];
    select(&mut s, 0);
    s.events.delete_bone(0);
    run(&mut s);
    assert_eq!(s.selections.bone_idx, usize::MAX);
    assert!(s.selections.bone_ids.is_empty());
    assert_eq!(s.armature.bones.iter().map(|b| b.id).collect::<Vec<_>>(), vec![5]);
}
