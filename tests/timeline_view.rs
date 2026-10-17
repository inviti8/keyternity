//! The timeline's view range (docs/TIMELINE.md §2): mapping, zoom, fit, range bar, ruler.

use keyternity_lib::timeline_view::*;

const W: f32 = 1000.;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn frame_and_x_round_trip() {
    let v = TimelineView::new(10., 60.);
    let x = v.frame_to_x(35., 100., W);
    assert!(close(v.x_to_frame(x, 100., W), 35.));
    assert!(close(v.frame_to_x(10., 100., W), 100. + PAD), "the first visible frame sits after the pad");
    assert!(close(v.frame_to_x(60., 100., W), 100. + W - PAD));
    assert_eq!(v.nearest_frame(x + 0.4 * v.px_per_frame(W), 100., W), 35);
    assert_eq!(v.nearest_frame(-500., 100., W), 0, "never below frame 0");
}

#[test]
fn zoom_keeps_the_anchor_in_place() {
    let mut v = TimelineView::new(0., 100.);
    let x = v.frame_to_x(40., 0., W);
    v.zoom_at(40., 2., 1000.);
    assert!(close(v.span(), 50.));
    assert!(close(v.frame_to_x(40., 0., W), x), "the frame under the cursor doesn't move");
    v.zoom_at(40., 0.5, 1000.);
    assert!(close(v.span(), 100.));
}

#[test]
fn zoom_is_limited_and_stays_after_zero() {
    let mut v = TimelineView::new(0., 100.);
    v.zoom_at(2., 1000., 500.);
    assert!(close(v.span(), MIN_SPAN));
    v.zoom_at(2., 0.0001, 500.);
    assert!(close(v.span(), 500.), "no further out than the content");
    assert!(v.start >= 0.);
}

#[test]
fn pan_stops_at_zero() {
    let mut v = TimelineView::new(5., 25.);
    v.pan(-50.);
    assert_eq!((v.start, v.end), (0., 20.));
    v.pan(30.);
    assert_eq!((v.start, v.end), (30., 50.));
}

#[test]
fn fit_shows_the_range() {
    let mut v = TimelineView::new(0., 10.);
    v.fit(100., 200., 10_000.);
    assert!(v.start <= 100. && v.end >= 200.);
    assert!(v.span() < 120., "with only a little room: {:?}", v);
    // a single frame still fits with some context
    v.fit(50., 50., 10_000.);
    assert!(v.span() >= MIN_SPAN && v.start <= 50. && v.end >= 50.);
}

#[test]
fn opening_an_animation_shows_all_its_keys() {
    let v = initial_view(Some(480), 24);
    assert!(v.start == 0. && v.end >= 480., "{:?}", v);
    let v = initial_view(None, 24);
    assert!(v.end >= 48.);
    assert!(content_frames(Some(480), 24) >= 480. + 48.);
    assert_eq!(content_frames(None, 24), 120.);
}

#[test]
fn range_bar_handles() {
    let total = 200.;
    let mut v = TimelineView::new(50., 100.);
    let (a, b) = bar_thumb(&v, total, 0., W);
    assert!(close(a, 250.) && close(b, 500.));
    assert_eq!(bar_hit(&v, total, 0., W, a + 2.), Some(BarHandle::Start));
    assert_eq!(bar_hit(&v, total, 0., W, b - 2.), Some(BarHandle::End));
    assert_eq!(bar_hit(&v, total, 0., W, (a + b) / 2.), Some(BarHandle::Middle));
    assert_eq!(bar_hit(&v, total, 0., W, 900.), None);

    // the middle scrolls without zooming, and stops at the ends
    bar_drag(&mut v, BarHandle::Middle, 500., total);
    assert_eq!((v.start, v.end), (150., 200.));
    // the ends zoom, and can't cross
    bar_drag(&mut v, BarHandle::Start, -150., total);
    assert_eq!((v.start, v.end), (0., 200.));
    bar_drag(&mut v, BarHandle::End, -1000., total);
    assert_eq!((v.start, v.end), (0., MIN_SPAN));
}

#[test]
fn a_tiny_thumb_stays_grabbable() {
    let v = TimelineView::new(0., 4.);
    let (a, b) = bar_thumb(&v, 100_000., 0., W);
    assert!(b - a >= BAR_MIN_THUMB - 1e-3 && a >= 0.);
}

#[test]
fn ruler_steps_space_labels_out() {
    // 10 points per frame: every 5th frame
    assert_eq!(ruler_step(10., 24, false), 5);
    assert_eq!(ruler_step(100., 24, false), 1);
    assert_eq!(ruler_step(0.1, 24, false), 500);
    // seconds: fractions of a second that divide the fps, then whole seconds
    assert_eq!(ruler_step(10., 24, true), 6);
    assert_eq!(ruler_step(1., 24, true), 48);
    assert_eq!(ruler_label(36, 24, true), "1.5s");
    assert_eq!(ruler_label(48, 24, true), "2s");
    assert_eq!(ruler_label(48, 24, false), "48");
}
