//! Envelope tool geometry (docs/ENVELOPES.md §4.1): capsule outlines and the radius
//! handles.

use keyternity_lib::envelope::Envelope;
use keyternity_lib::renderer::{capsule_outline, envelope_handles};
use keyternity_lib::shared::*;

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// Distance from `p` to the segment a..b.
fn to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.x * ab.x + ab.y * ab.y;
    let t = if len2 > 0. { (((p - a).x * ab.x + (p - a).y * ab.y) / len2).clamp(0., 1.) } else { 0. };
    (p - (a + ab * t)).mag()
}

fn is_convex(poly: &[Vec2]) -> bool {
    let n = poly.len();
    let mut sign = 0.;
    for i in 0..n {
        let (a, b, c) = (poly[i], poly[(i + 1) % n], poly[(i + 2) % n]);
        let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
        if cross.abs() < 1e-4 {
            continue;
        }
        if sign == 0. {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

#[test]
fn equal_radii_make_a_stadium() {
    let (a, b) = (v(10., 20.), v(110., 60.));
    let outline = capsule_outline(a, 15., b, 15.);
    assert!(outline.len() >= 32);
    assert!(is_convex(&outline));
    for p in &outline {
        assert!((to_segment(*p, a, b) - 15.).abs() < 0.5, "{p:?} is {} from the bone", to_segment(*p, a, b));
    }
}

#[test]
fn tapered_radii_keep_each_end() {
    let outline = capsule_outline(v(0., 0.), 10., v(100., 0.), 40.);
    assert!(is_convex(&outline));
    let max_x = outline.iter().map(|p| p.x).fold(f32::MIN, f32::max);
    let min_x = outline.iter().map(|p| p.x).fold(f32::MAX, f32::min);
    let max_y = outline.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    assert!((max_x - 140.).abs() < 0.1 && (min_x + 10.).abs() < 0.1 && (max_y - 40.).abs() < 0.5);
}

#[test]
fn a_circle_inside_the_other_is_just_the_bigger_circle() {
    let outline = capsule_outline(v(0., 0.), 50., v(5., 0.), 5.);
    for p in &outline {
        assert!((p.mag() - 50.).abs() < 1e-3, "{p:?}");
    }
}

#[test]
fn handles_sit_on_either_side_at_their_radius() {
    let bone = Bone { pos: v(10., 10.), rot: std::f32::consts::FRAC_PI_2, length: 100., scale: v(1., 1.), ..Default::default() };
    let e = Envelope { head_inner: 5., head_outer: 20., tail_inner: 8., tail_outer: 30., ..Envelope::for_length(100.) };
    let handles = envelope_handles(&bone, &e);
    let at = |f: EnvelopeField| handles.iter().find(|h| h.0 == f).unwrap().1;
    // the bone points up (+y); inner handles on its left (−x), outer on its right
    assert!((at(EnvelopeField::HeadInner) - v(5., 10.)).mag() < 1e-3);
    assert!((at(EnvelopeField::HeadOuter) - v(30., 10.)).mag() < 1e-3);
    assert!((at(EnvelopeField::TailInner) - v(2., 110.)).mag() < 1e-3);
    assert!((at(EnvelopeField::TailOuter) - v(40., 110.)).mag() < 1e-3);
}

#[test]
fn point_bones_have_only_head_handles() {
    let bone = Bone { pos: v(0., 0.), scale: v(1., 1.), ..Default::default() };
    let handles = envelope_handles(&bone, &Envelope::for_length(0.));
    let fields: Vec<EnvelopeField> = handles.iter().map(|h| h.0).collect();
    assert_eq!(fields, vec![EnvelopeField::HeadInner, EnvelopeField::HeadOuter]);
}
