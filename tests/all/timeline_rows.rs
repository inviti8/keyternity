//! Timeline rows and retiming (docs/TIMELINE.md §3).

use keyternity_lib::shared::*;
use keyternity_lib::timeline_rows::*;

fn key(bone_id: i32, element: AnimElement, frame: i32) -> Keyframe {
    Keyframe { frame, bone_id, element, ..Default::default() }
}

fn bone(id: i32, name: &str) -> Bone {
    Bone { id, parent_id: -1, name: name.into(), ..Default::default() }
}

fn anim(keys: Vec<Keyframe>) -> Animation {
    let mut a = Animation { fps: 24, keyframes: keys, ..Default::default() };
    a.sort_keyframes();
    a
}

use AnimElement as E;

#[test]
fn rows_group_channels_per_property() {
    let a = anim(vec![
        key(1, E::PositionX, 0),
        key(1, E::PositionY, 4),
        key(1, E::Rotation, 8),
        key(0, E::TintR, 2),
    ]);
    let bones = vec![bone(0, "head"), bone(1, "arm"), bone(2, "unkeyed")];
    let rows = build_rows(&a, &bones, &[]);
    assert_eq!(
        rows,
        vec![
            Row::Summary,
            Row::Bone(0),
            Row::Group(0, Group::Tint),
            Row::Bone(1),
            Row::Group(1, Group::Position),
            Row::Group(1, Group::Rotation),
        ],
        "bones in hierarchy order, unkeyed bones left out"
    );

    // opening a group with several channels shows them; one-channel groups don't open
    let rows = build_rows(&a, &bones, &[(1, Group::Position), (0, Group::Tint)]);
    assert!(rows.contains(&Row::Channel(1, E::PositionX)) && rows.contains(&Row::Channel(1, E::PositionY)));
    assert!(!rows.iter().any(|r| matches!(r, Row::Channel(0, _))), "Tint has only R keyed");
}

#[test]
fn folded_bones_keep_their_summary_row() {
    let a = anim(vec![key(0, E::Rotation, 0), key(0, E::ScaleX, 5)]);
    let mut b = bone(0, "head");
    b.anim_folded = true;
    let rows = build_rows(&a, &[b], &[]);
    assert_eq!(rows, vec![Row::Summary, Row::Bone(0)]);
    assert_eq!(row_frames(&a, &Row::Bone(0)), vec![0, 5]);
    assert!(build_rows(&anim(vec![]), &[bone(0, "head")], &[]).is_empty());
}

#[test]
fn row_frames_merge_channels() {
    let a = anim(vec![key(0, E::PositionX, 3), key(0, E::PositionY, 3), key(0, E::PositionY, 7), key(1, E::Rotation, 5)]);
    assert_eq!(row_frames(&a, &Row::Group(0, Group::Position)), vec![3, 7]);
    assert_eq!(row_frames(&a, &Row::Summary), vec![3, 5, 7]);
    assert_eq!(keys_at(&a, &Row::Group(0, Group::Position), 3).len(), 2);
}

#[test]
fn stepped_keys() {
    assert!(is_stepped_key(&key(0, E::Texture, 0)));
    assert!(is_stepped_key(&key(0, E::Hidden, 0)));
    assert!(!is_stepped_key(&key(0, E::Rotation, 0)));
    let snap = Keyframe { handle_preset: HandlePreset::Snap, ..key(0, E::Rotation, 0) };
    assert!(is_stepped_key(&snap));
}

#[test]
fn moving_replaces_what_it_lands_on() {
    let keys = vec![key(0, E::Rotation, 0), key(0, E::Rotation, 5), key(0, E::Rotation, 10)];
    let (out, sel, merged) = retime(&keys, &[keys[0].clone()], |f| f + 5);
    assert_eq!(out.iter().map(|k| k.frame).collect::<Vec<_>>(), vec![5, 10]);
    assert_eq!(sel[0].frame, 5);
    assert_eq!(merged, 0);
    // never below 0
    let (out, _, _) = retime(&keys, &[keys[1].clone()], |f| f - 50);
    assert_eq!(out.iter().map(|k| k.frame).collect::<Vec<_>>(), vec![0, 10], "lands on (and replaces) frame 0");
}

#[test]
fn scaling_around_the_playhead() {
    let keys: Vec<Keyframe> = [0, 4, 8, 12].iter().map(|f| key(0, E::Rotation, *f)).collect();
    // twice as fast, around frame 0
    let (out, _, merged) = retime(&keys, &keys, |f| scale_frame(f, 0, 0.5));
    assert_eq!(out.iter().map(|k| k.frame).collect::<Vec<_>>(), vec![0, 2, 4, 6]);
    assert_eq!(merged, 0);
    // around frame 12: keys on the pivot stay put
    let (out, _, _) = retime(&keys, &keys, |f| scale_frame(f, 12, 2.));
    assert_eq!(out.iter().map(|k| k.frame).collect::<Vec<_>>(), vec![0, 4, 12], "frames 0 and 4 land below 0, clamp to it and merge");
    // squeezing merges, keeping the key that moved farther (the outer one)
    let mut keys = keys.clone();
    keys[3].value = 99.;
    let (out, _, merged) = retime(&keys, &keys, |f| scale_frame(f, 0, 0.01));
    assert_eq!(merged, 3);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].value, 99., "the key from frame 12 survives");
}

#[test]
fn reversing_past_the_pivot() {
    let keys: Vec<Keyframe> = [10, 14].iter().map(|f| key(0, E::Rotation, *f)).collect();
    let (out, _, _) = retime(&keys, &keys, |f| scale_frame(f, 10, -1.));
    assert_eq!(out.iter().map(|k| k.frame).collect::<Vec<_>>(), vec![6, 10]);
}

#[test]
fn scale_stops_before_frame_zero() {
    // keys before the pivot can't stretch below 0
    assert_eq!(clamp_scale(&[0, 10], 10, 3.), 1.);
    assert_eq!(clamp_scale(&[5, 10], 10, 3.), 2.);
    // reversing: keys after the pivot can't flip below 0
    assert_eq!(clamp_scale(&[10, 20], 10, -3.), -1.);
    assert_eq!(clamp_scale(&[10, 20], 10, 0.5), 0.5);
}
