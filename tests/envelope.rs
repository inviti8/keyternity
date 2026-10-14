//! Envelope maths (docs/ENVELOPES.md §3.2–3.3, §7): influence, falloff, tapering,
//! absolute mode, vertex weights, and shares ↔ ordered bind weights.

use keyternity_lib::envelope::{self, Envelope, Falloff, Skinner};
use keyternity_lib::renderer::construct_verts;
use keyternity_lib::shared::*;

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn env(inner: f32, outer: f32) -> Envelope {
    Envelope { head_inner: inner, head_outer: outer, tail_inner: inner, tail_outer: outer, ..Envelope::for_length(0.) }
}

const EPS: f32 = 1e-5;

#[test]
fn inside_inner_is_full_and_outside_outer_is_none() {
    let e = env(10., 30.);
    let (head, tail) = (v(0., 0.), v(100., 0.));
    for p in [v(0., 0.), v(50., 9.), v(100., -10.), v(-10., 0.)] {
        assert_eq!(e.influence(head, tail, p), 1., "at {p:?}");
    }
    for p in [v(50., 30.), v(50., -45.), v(-31., 0.), v(135., 0.)] {
        assert_eq!(e.influence(head, tail, p), 0., "at {p:?}");
    }
}

#[test]
fn falloff_curves_at_the_midpoint() {
    // halfway between inner (10) and outer (30), off the middle of the segment
    let p = v(50., 20.);
    let at = |falloff| Envelope { falloff, ..env(10., 30.) }.influence(v(0., 0.), v(100., 0.), p);
    assert!((at(Falloff::Linear) - 0.5).abs() < EPS);
    assert!((at(Falloff::Smooth) - 0.5).abs() < EPS);
    assert!((at(Falloff::FastOut) - 0.25).abs() < EPS);
    assert!((at(Falloff::SlowOut) - 0.75).abs() < EPS);

    // a quarter of the way in from the outer radius
    let q = v(50., 25.);
    let at = |falloff| Envelope { falloff, ..env(10., 30.) }.influence(v(0., 0.), v(100., 0.), q);
    assert!((at(Falloff::Linear) - 0.25).abs() < EPS);
    assert!((at(Falloff::Smooth) - 0.15625).abs() < EPS);
    for f in [Falloff::Linear, Falloff::Smooth, Falloff::FastOut, Falloff::SlowOut] {
        assert!(at(f) > 0. && at(f) < 1., "{f:?}");
    }
}

#[test]
fn radii_taper_along_the_segment() {
    let e = Envelope { head_inner: 0., head_outer: 10., tail_inner: 0., tail_outer: 50., falloff: Falloff::Linear, ..env(0., 0.) };
    let (head, tail) = (v(0., 0.), v(100., 0.));
    // outer radius 30 at the middle, 10 at the head
    assert!((e.influence(head, tail, v(50., 15.)) - 0.5).abs() < EPS);
    assert_eq!(e.influence(head, tail, v(0., 15.)), 0.);
    assert!(e.influence(head, tail, v(100., 15.)) > 0.6);
}

#[test]
fn point_bone_envelope_is_a_circle() {
    let e = Envelope { falloff: Falloff::Linear, ..env(10., 30.) };
    let c = v(5., 5.);
    for angle in [0., 1., 2., 3., 4., 5.] {
        let p = c + Vec2::new(f32::cos(angle), f32::sin(angle)) * 20.;
        assert!((e.influence(c, c, p) - 0.5).abs() < EPS, "angle {angle}");
    }
}

#[test]
fn absolute_is_full_anywhere_inside_the_outer_radius() {
    let e = Envelope { absolute: true, ..env(10., 30.) };
    assert_eq!(e.influence(v(0., 0.), v(100., 0.), v(50., 29.)), 1.);
    assert_eq!(e.influence(v(0., 0.), v(100., 0.), v(50., 31.)), 0.);
}

fn skinner(id: i32, head: Vec2, tail: Vec2, e: Envelope) -> Skinner {
    Skinner { id, head, tail, envelope: e }
}

#[test]
fn weights_are_normalized_and_strongest_first() {
    let lin = |i, o| Envelope { falloff: Falloff::Linear, ..env(i, o) };
    let bones = [
        skinner(1, v(0., 0.), v(100., 0.), lin(0., 40.)),
        skinner(2, v(100., 0.), v(200., 0.), lin(0., 40.)),
    ];
    // on bone 2 (influence 1), 10 from bone 1's tail (influence 0.75)
    let (w, outside) = envelope::vertex_weights(v(110., 0.), &bones, 4);
    assert!(!outside);
    assert_eq!(w.iter().map(|x| x.0).collect::<Vec<_>>(), vec![2, 1]);
    let sum: f32 = w.iter().map(|x| x.1).sum();
    assert!((sum - 1.).abs() < EPS);
    assert!((w[0].1 - 1. / 1.75).abs() < EPS, "{w:?}");
}

#[test]
fn max_influences_keeps_the_strongest() {
    let bones: Vec<Skinner> = (0..6)
        .map(|i| skinner(i, v(i as f32 * 10., 0.), v(i as f32 * 10., 0.), Envelope { falloff: Falloff::Linear, ..env(0., 100.) }))
        .collect();
    let (w, _) = envelope::vertex_weights(v(0., 0.), &bones, 3);
    assert_eq!(w.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0, 1, 2]);
    let sum: f32 = w.iter().map(|x| x.1).sum();
    assert!((sum - 1.).abs() < EPS);
}

#[test]
fn outside_every_envelope_goes_to_the_nearest_bone() {
    let bones = [
        skinner(1, v(0., 0.), v(10., 0.), env(5., 10.)),
        skinner(2, v(0., 100.), v(10., 100.), env(5., 50.)),
    ];
    // 200 below bone 1 (190 past its outer radius), 300 below bone 2 (250 past)
    let (w, outside) = envelope::vertex_weights(v(5., -200.), &bones, 4);
    assert!(outside);
    assert_eq!(w, vec![(1, 1.)]);
    // 40 from bone 2, inside its outer radius of 50
    let (w, outside) = envelope::vertex_weights(v(5., 60.), &bones, 4);
    assert!(!outside, "inside bone 2's outer radius");
    assert_eq!(w[0].0, 2);
    assert!(envelope::vertex_weights(v(0., 0.), &[], 4).0.is_empty());
}

/// A deterministic pseudo-random sequence in 0..1.
fn rng(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    (*seed >> 8) as f32 / (1 << 24) as f32
}

#[test]
fn shares_and_bind_weights_convert_both_ways() {
    let mut seed = 7;
    for n in 1..6 {
        for owner_zero in [false, true] {
            let mut raw: Vec<f32> = (0..=n).map(|_| rng(&mut seed) + 0.01).collect();
            if owner_zero {
                raw[0] = 0.;
            }
            let sum: f32 = raw.iter().sum();
            let shares: Vec<f32> = raw.iter().map(|x| x / sum).collect();
            let weights = envelope::bind_weights(shares[0], &shares[1..]);
            let (owner, back) = envelope::shares(&weights);
            assert!((owner - shares[0]).abs() < EPS, "owner {owner} vs {}", shares[0]);
            for (a, b) in back.iter().zip(&shares[1..]) {
                assert!((a - b).abs() < EPS, "{back:?} vs {shares:?}");
            }
        }
    }
}

#[test]
fn bind_weights_give_the_renderer_each_share() {
    // one vertex at the owner's origin; bind bones only translated, so bind k
    // places it at that bone's position, and the blend should be the weighted sum
    let positions = [v(0., 0.), v(40., 0.), v(0., 30.), v(-20., -10.)];
    let shares = [0.1, 0.2, 0.3, 0.4];
    let weights = envelope::bind_weights(shares[0], &shares[1..]);

    let plain = |id: i32, pos: Vec2| Bone {
        id,
        parent_id: -1,
        pos,
        scale: v(1., 1.),
        pivot_scale: v(1., 1.),
        ..Default::default()
    };
    let mut owner = plain(0, positions[0]);
    owner.vertices = vec![Vertex { id: 0, pos: v(0., 0.), ..Default::default() }];
    owner.binds = (1..4)
        .map(|k| BoneBind { bone_id: k as i32, is_path: false, verts: vec![BoneBindVert { id: 0, weight: weights[k - 1] }] })
        .collect();
    let mut bones = vec![owner];
    for k in 1..4 {
        bones.push(plain(k as i32, positions[k]));
    }
    construct_verts(&mut bones);

    let expected = positions.iter().zip(&shares).fold(v(0., 0.), |acc, (p, s)| acc + *p * *s);
    let got = bones[0].vertices[0].pos;
    assert!((got - expected).mag() < 1e-3, "{got:?} vs {expected:?}");
}
