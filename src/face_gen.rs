//! Generating the face from landmarks (docs/LIP_SYNC.md §6.7).
//!
//! The artist maps ~30 landmarks (the midline and one side; the other side is
//! derived), and each standard shape is generated from a template: how the landmarks
//! move, in face-relative units. The motion spreads to the mesh by a blend of the
//! landmarks' motions weighted by distance, limited to the feature's region, and
//! split by lip (and lid) so the upper and lower halves move apart. That's the
//! "helper bones + automatic weights + capture" of the design, computed directly.

use crate::shared::*;
use std::collections::HashMap;

/// Which part of the face a landmark belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Region {
    Mouth,
    Eye,
    Brow,
    Nose,
    Jaw,
}

/// Which half of a two-part feature a landmark is on (lips, lids), or neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Half {
    Upper,
    Lower,
    Both,
}

pub struct LandmarkDef {
    pub name: &'static str,
    pub region: Region,
    pub half: Half,
    /// the other side's landmark; `None` on the midline
    pub mirror: Option<&'static str>,
}

macro_rules! lm {
    ($name:expr, $region:ident, $half:ident) => {
        LandmarkDef { name: $name, region: Region::$region, half: Half::$half, mirror: None }
    };
    ($name:expr, $region:ident, $half:ident, $mirror:expr) => {
        LandmarkDef { name: $name, region: Region::$region, half: Half::$half, mirror: Some($mirror) }
    };
}

/// The landmark map (docs/LIP_SYNC.md §6.7). `_L` is the character's left, screen right.
pub const LANDMARKS: &[LandmarkDef] = &[
    // midline
    lm!("UpperLipMid", Mouth, Upper),
    lm!("LowerLipMid", Mouth, Lower),
    lm!("NoseTip", Nose, Both),
    lm!("Chin", Jaw, Lower),
    // mouth: corners, then 2 points per lip per side (1 nearer the middle)
    lm!("MouthCorner_L", Mouth, Both, "MouthCorner_R"),
    lm!("MouthCorner_R", Mouth, Both, "MouthCorner_L"),
    lm!("UpperLip_L1", Mouth, Upper, "UpperLip_R1"),
    lm!("UpperLip_L2", Mouth, Upper, "UpperLip_R2"),
    lm!("UpperLip_R1", Mouth, Upper, "UpperLip_L1"),
    lm!("UpperLip_R2", Mouth, Upper, "UpperLip_L2"),
    lm!("LowerLip_L1", Mouth, Lower, "LowerLip_R1"),
    lm!("LowerLip_L2", Mouth, Lower, "LowerLip_R2"),
    lm!("LowerLip_R1", Mouth, Lower, "LowerLip_L1"),
    lm!("LowerLip_R2", Mouth, Lower, "LowerLip_L2"),
    // eyes
    lm!("EyeIn_L", Eye, Both, "EyeIn_R"),
    lm!("EyeOut_L", Eye, Both, "EyeOut_R"),
    lm!("Lid_L_Up1", Eye, Upper, "Lid_R_Up1"),
    lm!("Lid_L_Up2", Eye, Upper, "Lid_R_Up2"),
    lm!("Lid_L_Low1", Eye, Lower, "Lid_R_Low1"),
    lm!("Lid_L_Low2", Eye, Lower, "Lid_R_Low2"),
    lm!("EyeIn_R", Eye, Both, "EyeIn_L"),
    lm!("EyeOut_R", Eye, Both, "EyeOut_L"),
    lm!("Lid_R_Up1", Eye, Upper, "Lid_L_Up1"),
    lm!("Lid_R_Up2", Eye, Upper, "Lid_L_Up2"),
    lm!("Lid_R_Low1", Eye, Lower, "Lid_L_Low1"),
    lm!("Lid_R_Low2", Eye, Lower, "Lid_L_Low2"),
    // brows
    lm!("Brow_L_In", Brow, Both, "Brow_R_In"),
    lm!("Brow_L_Mid", Brow, Both, "Brow_R_Mid"),
    lm!("Brow_L_Out", Brow, Both, "Brow_R_Out"),
    lm!("Brow_R_In", Brow, Both, "Brow_L_In"),
    lm!("Brow_R_Mid", Brow, Both, "Brow_L_Mid"),
    lm!("Brow_R_Out", Brow, Both, "Brow_L_Out"),
    // nose
    lm!("NoseWing_L", Nose, Both, "NoseWing_R"),
    lm!("NoseWing_R", Nose, Both, "NoseWing_L"),
];

pub fn def(name: &str) -> Option<&'static LandmarkDef> {
    LANDMARKS.iter().find(|l| l.name == name)
}

/// The landmarks' positions on the mesh (rest, mesh space), by name.
pub fn positions(bone: &Bone, map: &FaceMap) -> HashMap<String, Vec2> {
    map.points
        .iter()
        .filter_map(|p| bone.vertices.iter().find(|v| v.id == p.vert).map(|v| (p.name.clone(), v.pos)))
        .collect()
}

// ---------------------------------------------------------------- deriving a side

/// The face's symmetry axis from midline points: a point on it and its unit
/// direction (up the face), by least squares (so a tilted head gives a tilted axis).
/// With one point, the axis is vertical through it.
pub fn symmetry_axis(midline: &[Vec2]) -> Option<(Vec2, Vec2)> {
    if midline.is_empty() {
        return None;
    }
    let n = midline.len() as f32;
    let c = midline.iter().fold(Vec2::ZERO, |a, p| a + *p) / n;
    if midline.len() == 1 {
        return Some((c, Vec2::new(0., 1.)));
    }
    // principal direction of the points (x as a function of y: the axis is near vertical)
    let (mut syy, mut sxy) = (0., 0.);
    for p in midline {
        let d = *p - c;
        syy += d.y * d.y;
        sxy += d.x * d.y;
    }
    let dir = if syy < 1e-6 { Vec2::new(0., 1.) } else { Vec2::new(sxy / syy, 1.) };
    let len = dir.mag();
    Some((c, dir / len))
}

/// Reflect `p` across the axis through `origin` along `dir` (unit).
pub fn reflect(p: Vec2, origin: Vec2, dir: Vec2) -> Vec2 {
    let d = p - origin;
    let along = dir * (d.x * dir.x + d.y * dir.y);
    let across = d - along;
    origin + along - across
}

/// Three-quarter views (docs/LIP_SYNC.md §6.7): the head as a cylinder of radius `r`
/// seen at yaw `theta`, centred on screen at `centre_x` (along the axis's normal).
/// A point at angle φ shows at `centre + r·sin(φ + θ)`, and its mirror twin at −φ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cylinder {
    pub r: f32,
    pub theta: f32,
    pub centre: f32,
}

impl Cylinder {
    /// Where the twin of a point `x` (across the axis, in the axis's normal direction)
    /// shows: φ from the near point, then the twin at −φ.
    pub fn twin(&self, x: f32) -> f32 {
        let s = ((x - self.centre) / self.r).clamp(-1., 1.);
        let phi = s.asin() - self.theta;
        self.centre + self.r * (self.theta - phi).sin()
    }

    /// Solve the yaw from one known pair: a near point and its mapped far twin (both
    /// as offsets across the axis), with the midline at `mid` and radius `r`. The
    /// cylinder's centre follows from the midline: `mid = centre + r·sin θ`.
    pub fn from_pair(near: f32, far: f32, mid: f32, r: f32) -> Option<Cylinder> {
        // search θ: the twin of `near` should land on `far`
        let mut best: Option<(f32, Cylinder)> = None;
        for i in -80..=80 {
            let theta = (i as f32).to_radians();
            let c = Cylinder { r, theta, centre: mid - r * theta.sin() };
            let err = (c.twin(near) - far).abs();
            if best.as_ref().map_or(true, |(e, _)| err < *e) {
                best = Some((err, c));
            }
        }
        // refine around the best degree
        let (_, mut c) = best?;
        let mut step = 0.5f32.to_radians();
        for _ in 0..20 {
            for t in [c.theta - step, c.theta + step] {
                let cand = Cylinder { r, theta: t, centre: mid - r * t.sin() };
                if (cand.twin(near) - far).abs() < (c.twin(near) - far).abs() {
                    c = cand;
                }
            }
            step *= 0.5;
        }
        Some(c)
    }
}

/// The derived side's suggested positions (mesh space), for each mapped landmark whose
/// twin isn't mapped. Front views reflect across the midline axis; with a `Cylinder`
/// (three-quarter), offsets across the axis follow it instead.
pub fn derive_side(known: &HashMap<String, Vec2>, cylinder: Option<Cylinder>) -> HashMap<String, Vec2> {
    let midline: Vec<Vec2> = LANDMARKS.iter().filter(|l| l.mirror.is_none()).filter_map(|l| known.get(l.name).copied()).collect();
    let Some((origin, dir)) = symmetry_axis(&midline) else { return HashMap::new() };
    let normal = Vec2::new(dir.y, -dir.x);
    let mut out = HashMap::new();
    for l in LANDMARKS {
        let (Some(twin), Some(p)) = (l.mirror, known.get(l.name)) else { continue };
        if known.contains_key(twin) {
            continue;
        }
        let d = *p - origin;
        let along = d.x * dir.x + d.y * dir.y;
        let across = d.x * normal.x + d.y * normal.y;
        let twin_across = match cylinder {
            Some(c) => c.twin(across),
            None => -across,
        };
        out.insert(twin.to_string(), origin + dir * along + normal * twin_across);
    }
    out
}

/// Suggest the view from the mapped landmarks (docs/LIP_SYNC.md §6.7): with both mouth
/// corners, compare their distances from the midline axis.
pub fn suggest_view(known: &HashMap<String, Vec2>) -> FaceView {
    let midline: Vec<Vec2> = LANDMARKS.iter().filter(|l| l.mirror.is_none()).filter_map(|l| known.get(l.name).copied()).collect();
    let (Some((origin, dir)), Some(l), Some(r)) = (symmetry_axis(&midline), known.get("MouthCorner_L"), known.get("MouthCorner_R")) else {
        return FaceView::Front;
    };
    let normal = Vec2::new(dir.y, -dir.x);
    let across = |p: &Vec2| ((*p - origin).x * normal.x + (*p - origin).y * normal.y).abs();
    let (a, b) = (across(l), across(r));
    let ratio = a.min(b) / a.max(b).max(1e-4);
    if ratio > 0.9 {
        FaceView::Front
    } else {
        FaceView::ThreeQuarter
    }
}

/// The vertex to snap a derived landmark to: the nearest, preferring vertices on the
/// mesh's edge (lips and lids sit on seams) when the landmark's source does.
pub fn snap(bone: &Bone, target: Vec2, prefer_edge: bool) -> Option<u32> {
    let edge = edge_vertices(bone);
    let pick = |only_edge: bool| {
        bone.vertices
            .iter()
            .enumerate()
            .filter(|(i, _)| !only_edge || edge.contains(&(*i as u32)))
            .min_by(|a, b| (a.1.pos - target).mag().total_cmp(&(b.1.pos - target).mag()))
            .map(|(_, v)| v.id)
    };
    if prefer_edge {
        pick(true).or_else(|| pick(false))
    } else {
        pick(false)
    }
}

/// Vertex indices on the mesh's boundary (edges used by one triangle).
pub fn edge_vertices(bone: &Bone) -> Vec<u32> {
    let mut count: HashMap<(u32, u32), u32> = HashMap::new();
    for t in bone.indices.chunks(3) {
        if t.len() < 3 {
            continue;
        }
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let mut out: Vec<u32> = count.into_iter().filter(|(_, n)| *n == 1).flat_map(|((a, b), _)| [a, b]).collect();
    out.sort();
    out.dedup();
    out
}

// ---------------------------------------------------------------- templates

/// How one landmark moves in a template: across (+ toward the character's own side
/// for sided landmarks, i.e. outward) in mouth widths, and up in nose-to-chin lengths.
#[derive(Clone, Copy, Debug)]
pub struct Move {
    pub landmark: &'static str,
    pub out: f32,
    pub up: f32,
}

const fn mv(landmark: &'static str, out: f32, up: f32) -> Move {
    Move { landmark, out, up }
}

/// A standard shape's template (docs/LIP_SYNC.md §6.7).
pub struct Template {
    pub shape: &'static str,
    pub moves: &'static [Move],
}

macro_rules! tpl {
    ($shape:expr, [$($m:expr),* $(,)?]) => {
        Template { shape: $shape, moves: &[$($m),*] }
    };
}

/// The default templates (front view). Sided shapes are written for `_L` and mirrored
/// for `_R`. Units: across in mouth widths (corner to corner), up in nose-to-chin.
pub const TEMPLATES: &[Template] = &[
    tpl!("Smile_L", [mv("MouthCorner_L", 0.12, 0.10), mv("UpperLip_L2", 0.05, 0.05), mv("LowerLip_L2", 0.05, 0.06)]),
    tpl!("Frown_L", [mv("MouthCorner_L", 0.02, -0.10), mv("LowerLip_L2", 0., -0.04)]),
    tpl!("MouthNarrow_L", [mv("MouthCorner_L", -0.14, 0.), mv("UpperLip_L2", -0.06, 0.01), mv("LowerLip_L2", -0.06, -0.01)]),
    tpl!("MouthWhistle_L", [mv("MouthCorner_L", -0.10, 0.02), mv("UpperLip_L2", -0.05, 0.03), mv("UpperLip_L1", -0.02, 0.03), mv("LowerLip_L2", -0.05, -0.03), mv("LowerLip_L1", -0.02, -0.03)]),
    tpl!("MidMouth_L", [mv("MouthCorner_L", 0.10, 0.), mv("MouthCorner_R", -0.10, 0.), mv("UpperLip_L1", 0.10, 0.), mv("UpperLip_L2", 0.10, 0.), mv("UpperLip_R1", -0.10, 0.), mv("UpperLip_R2", -0.10, 0.), mv("LowerLip_L1", 0.10, 0.), mv("LowerLip_L2", 0.10, 0.), mv("LowerLip_R1", -0.10, 0.), mv("LowerLip_R2", -0.10, 0.), mv("UpperLipMid", 0., 0.), mv("LowerLipMid", 0., 0.)]),
    tpl!("UpperLipUp_L", [mv("UpperLip_L1", 0., 0.08), mv("UpperLip_L2", 0., 0.07)]),
    tpl!("LowerLipDown_L", [mv("LowerLip_L1", 0., -0.08), mv("LowerLip_L2", 0., -0.07)]),
    tpl!("MouthOpen", [mv("LowerLipMid", 0., -0.25), mv("LowerLip_L1", 0., -0.23), mv("LowerLip_R1", 0., -0.23), mv("LowerLip_L2", 0., -0.17), mv("LowerLip_R2", 0., -0.17), mv("MouthCorner_L", -0.02, -0.05), mv("MouthCorner_R", -0.02, -0.05), mv("Chin", 0., -0.20)]),
    tpl!("MouthUp", [mv("UpperLipMid", 0., 0.06), mv("LowerLipMid", 0., 0.06), mv("MouthCorner_L", 0., 0.06), mv("MouthCorner_R", 0., 0.06), mv("UpperLip_L1", 0., 0.06), mv("UpperLip_R1", 0., 0.06), mv("UpperLip_L2", 0., 0.06), mv("UpperLip_R2", 0., 0.06), mv("LowerLip_L1", 0., 0.06), mv("LowerLip_R1", 0., 0.06), mv("LowerLip_L2", 0., 0.06), mv("LowerLip_R2", 0., 0.06)]),
    tpl!("MouthDown", [mv("UpperLipMid", 0., -0.06), mv("LowerLipMid", 0., -0.06), mv("MouthCorner_L", 0., -0.06), mv("MouthCorner_R", 0., -0.06), mv("UpperLip_L1", 0., -0.06), mv("UpperLip_R1", 0., -0.06), mv("UpperLip_L2", 0., -0.06), mv("UpperLip_R2", 0., -0.06), mv("LowerLip_L1", 0., -0.06), mv("LowerLip_R1", 0., -0.06), mv("LowerLip_L2", 0., -0.06), mv("LowerLip_R2", 0., -0.06)]),
    tpl!("UpperLipIn", [mv("UpperLipMid", 0., -0.04), mv("UpperLip_L1", 0., -0.04), mv("UpperLip_R1", 0., -0.04), mv("UpperLip_L2", 0., -0.02), mv("UpperLip_R2", 0., -0.02)]),
    tpl!("UpperLipOut", [mv("UpperLipMid", 0., 0.04), mv("UpperLip_L1", 0., 0.04), mv("UpperLip_R1", 0., 0.04), mv("UpperLip_L2", 0., 0.02), mv("UpperLip_R2", 0., 0.02)]),
    tpl!("LowerLipIn", [mv("LowerLipMid", 0., 0.05), mv("LowerLip_L1", 0., 0.05), mv("LowerLip_R1", 0., 0.05), mv("LowerLip_L2", 0., 0.03), mv("LowerLip_R2", 0., 0.03)]),
    tpl!("LowerLipOut", [mv("LowerLipMid", 0., -0.05), mv("LowerLip_L1", 0., -0.05), mv("LowerLip_R1", 0., -0.05), mv("LowerLip_L2", 0., -0.03), mv("LowerLip_R2", 0., -0.03)]),
    tpl!("JawSide_L", [mv("Chin", 0.10, 0.), mv("LowerLipMid", 0.10, 0.), mv("LowerLip_L1", 0.10, 0.), mv("LowerLip_R1", -0.10, 0.), mv("LowerLip_L2", 0.08, 0.), mv("LowerLip_R2", -0.08, 0.)]),
    tpl!("JawTwist_L", [mv("Chin", 0.06, 0.02), mv("LowerLipMid", 0.05, 0.), mv("LowerLip_L2", 0.03, 0.02), mv("LowerLip_R2", -0.03, -0.02)]),
    tpl!("JawForward", [mv("Chin", 0., -0.04), mv("LowerLipMid", 0., -0.02)]),
    tpl!("JawBackward", [mv("Chin", 0., 0.04), mv("LowerLipMid", 0., 0.02)]),
    tpl!("TongueUp", []),
    tpl!("Blink_L", [mv("Lid_L_Up1", 0., -0.13), mv("Lid_L_Up2", 0., -0.12), mv("Lid_L_Low1", 0., 0.02), mv("Lid_L_Low2", 0., 0.02)]),
    tpl!("BrowsUp_L", [mv("Brow_L_In", 0., 0.10), mv("Brow_L_Mid", 0., 0.11), mv("Brow_L_Out", 0., 0.09)]),
    tpl!("BrowsDown_L", [mv("Brow_L_In", 0., -0.08), mv("Brow_L_Mid", 0., -0.08), mv("Brow_L_Out", 0., -0.07)]),
    tpl!("BrowsIn_L", [mv("Brow_L_In", -0.08, -0.03), mv("Brow_L_Mid", -0.04, -0.01)]),
    tpl!("BrowsOuterLower_L", [mv("Brow_L_Out", 0., -0.09), mv("Brow_L_Mid", 0., -0.03)]),
    tpl!("NoseScrunch_L", [mv("NoseWing_L", 0.02, 0.05), mv("UpperLip_L2", 0., 0.02)]),
    // Preston Blair visemes, from the same landmarks
    tpl!("AI", [mv("LowerLipMid", 0., -0.22), mv("LowerLip_L1", 0., -0.20), mv("LowerLip_R1", 0., -0.20), mv("LowerLip_L2", 0., -0.14), mv("LowerLip_R2", 0., -0.14), mv("UpperLipMid", 0., 0.03), mv("MouthCorner_L", 0.03, 0.), mv("MouthCorner_R", 0.03, 0.), mv("Chin", 0., -0.18)]),
    tpl!("E", [mv("LowerLipMid", 0., -0.08), mv("LowerLip_L1", 0., -0.07), mv("LowerLip_R1", 0., -0.07), mv("UpperLipMid", 0., 0.03), mv("MouthCorner_L", 0.08, 0.03), mv("MouthCorner_R", 0.08, 0.03), mv("Chin", 0., -0.06)]),
    tpl!("O", [mv("LowerLipMid", 0., -0.16), mv("LowerLip_L1", -0.03, -0.14), mv("LowerLip_R1", -0.03, -0.14), mv("UpperLipMid", 0., 0.04), mv("UpperLip_L1", -0.03, 0.04), mv("UpperLip_R1", -0.03, 0.04), mv("MouthCorner_L", -0.14, -0.04), mv("MouthCorner_R", -0.14, -0.04), mv("Chin", 0., -0.12)]),
    tpl!("U", [mv("LowerLipMid", 0., -0.05), mv("UpperLipMid", 0., 0.02), mv("MouthCorner_L", -0.18, -0.01), mv("MouthCorner_R", -0.18, -0.01), mv("UpperLip_L2", -0.08, 0.02), mv("UpperLip_R2", -0.08, 0.02), mv("LowerLip_L2", -0.08, -0.02), mv("LowerLip_R2", -0.08, -0.02), mv("Chin", 0., -0.04)]),
    tpl!("etc", [mv("LowerLipMid", 0., -0.09), mv("LowerLip_L1", 0., -0.08), mv("LowerLip_R1", 0., -0.08), mv("MouthCorner_L", 0.04, 0.01), mv("MouthCorner_R", 0.04, 0.01), mv("Chin", 0., -0.07)]),
    tpl!("WQ", [mv("LowerLipMid", 0., -0.04), mv("UpperLipMid", 0., 0.02), mv("MouthCorner_L", -0.22, 0.), mv("MouthCorner_R", -0.22, 0.), mv("UpperLip_L2", -0.10, 0.02), mv("UpperLip_R2", -0.10, 0.02), mv("LowerLip_L2", -0.10, -0.02), mv("LowerLip_R2", -0.10, -0.02)]),
    tpl!("MBP", [mv("LowerLipMid", 0., 0.02), mv("LowerLip_L1", 0., 0.02), mv("LowerLip_R1", 0., 0.02), mv("UpperLipMid", 0., -0.01), mv("UpperLip_L1", 0., -0.01), mv("UpperLip_R1", 0., -0.01)]),
    tpl!("L", [mv("LowerLipMid", 0., -0.17), mv("LowerLip_L1", 0., -0.15), mv("LowerLip_R1", 0., -0.15), mv("UpperLipMid", 0., 0.03), mv("Chin", 0., -0.13)]),
    tpl!("FV", [mv("LowerLipMid", 0., 0.04), mv("LowerLip_L1", 0., 0.04), mv("LowerLip_R1", 0., 0.04), mv("UpperLipMid", 0., 0.02), mv("MouthCorner_L", 0.02, 0.)]),
    tpl!("rest", []),
];

/// A template's moves for a shape, `_R` shapes mirrored from their `_L` template.
pub fn template_for(shape: &str) -> Option<Vec<Move>> {
    if let Some(t) = TEMPLATES.iter().find(|t| t.shape == shape) {
        return Some(t.moves.to_vec());
    }
    let left = shape.strip_suffix("_R").map(|s| format!("{s}_L"))?;
    let t = TEMPLATES.iter().find(|t| t.shape == left)?;
    Some(
        t.moves
            .iter()
            .map(|m| {
                // the mirrored landmark; "out" stays outward from its own side
                let name = def(m.landmark).and_then(|d| d.mirror).unwrap_or(m.landmark);
                Move { landmark: name, out: m.out, up: m.up }
            })
            .collect(),
    )
}

/// Units for templates: the mouth width, and the nose-to-chin length (each falls back
/// to the other).
pub fn units(pos: &HashMap<String, Vec2>) -> Option<(f32, f32)> {
    let w = match (pos.get("MouthCorner_L"), pos.get("MouthCorner_R")) {
        (Some(a), Some(b)) => Some((*a - *b).mag()),
        _ => None,
    };
    let h = match (pos.get("NoseTip"), pos.get("Chin")) {
        (Some(a), Some(b)) => Some((*a - *b).mag()),
        _ => None,
    };
    match (w, h) {
        (Some(w), Some(h)) => Some((w, h)),
        (Some(w), None) => Some((w, w * 1.3)),
        (None, Some(h)) => Some((h / 1.3, h)),
        _ => None,
    }
}

/// Each landmark's motion for a template, in mesh space. "Out" points away from the
/// midline axis on the landmark's own side (midline landmarks use +x for out).
pub fn landmark_motion(moves: &[Move], pos: &HashMap<String, Vec2>, strength: f32) -> HashMap<String, Vec2> {
    let Some((w, h)) = units(pos) else { return HashMap::new() };
    let midline: Vec<Vec2> = LANDMARKS.iter().filter(|l| l.mirror.is_none()).filter_map(|l| pos.get(l.name).copied()).collect();
    let (origin, dir) = symmetry_axis(&midline).unwrap_or((Vec2::ZERO, Vec2::new(0., 1.)));
    let normal = Vec2::new(dir.y, -dir.x); // screen right on an upright face
    let mut out = HashMap::new();
    for m in moves {
        let Some(p) = pos.get(m.landmark) else { continue };
        let side = ((*p - origin).x * normal.x + (*p - origin).y * normal.y).signum();
        let side = if side == 0. { 1. } else { side };
        out.insert(m.landmark.to_string(), (normal * (m.out * w * side) + dir * (m.up * h)) * strength);
    }
    out
}

// ---------------------------------------------------------------- spreading to the mesh

/// Which half a vertex belongs to, for a feature line: above or below the line through
/// the feature's "Both" landmarks (corners), judged by its triangles' centres so
/// vertices on a closed seam still pick their own lip.
fn vertex_half(bone: &Bone, idx: usize, line: (Vec2, Vec2)) -> Half {
    let (a, b) = line;
    let side = |p: Vec2| {
        let d = b - a;
        d.x * (p.y - a.y) - d.y * (p.x - a.x)
    };
    let mut sum = 0.;
    let mut n = 0;
    for t in bone.indices.chunks(3).filter(|t| t.len() == 3 && t.contains(&(idx as u32))) {
        let c = (bone.vertices[t[0] as usize].pos + bone.vertices[t[1] as usize].pos + bone.vertices[t[2] as usize].pos) / 3.;
        sum += side(c);
        n += 1;
    }
    if n == 0 {
        sum = side(bone.vertices[idx].pos);
    }
    // the line runs from the screen-left corner to the screen-right one
    if sum >= 0. {
        Half::Upper
    } else {
        Half::Lower
    }
}

/// The offsets a template's landmark motion gives each vertex: within each moving
/// region, a blend of the region's landmark motions (unmoved landmarks pull toward
/// zero) weighted by inverse squared distance, faded out by distance from the moving
/// landmarks, and, for lips and lids, only from landmarks on the vertex's own half.
pub fn spread(bone: &Bone, pos: &HashMap<String, Vec2>, motion: &HashMap<String, Vec2>) -> Vec<ShapeDelta> {
    let Some((w, _)) = units(pos) else { return vec![] };
    let mut regions: Vec<Region> = motion.keys().filter_map(|n| def(n)).map(|d| d.region).collect();
    regions.sort_by_key(|r| *r as i32);
    regions.dedup();
    let mut total: Vec<Vec2> = vec![Vec2::ZERO; bone.vertices.len()];
    for region in regions {
        // the region's landmarks (Jaw joins the mouth: the chin moves with the lips)
        let members: Vec<(&LandmarkDef, Vec2)> = LANDMARKS
            .iter()
            .filter(|d| d.region == region || (region == Region::Mouth && d.region == Region::Jaw) || (region == Region::Jaw && d.region == Region::Mouth))
            .filter_map(|d| pos.get(d.name).map(|p| (d, *p)))
            .collect();
        let moving: Vec<Vec2> = members.iter().filter(|(d, _)| motion.get(d.name).is_some_and(|m| m.mag() > 1e-6)).map(|(_, p)| *p).collect();
        if moving.is_empty() {
            continue;
        }
        // reach: how far from the moving landmarks the motion fades out
        let reach = match region {
            Region::Mouth | Region::Jaw => 0.45 * w,
            Region::Eye => 0.25 * w,
            Region::Brow => 0.35 * w,
            Region::Nose => 0.25 * w,
        };
        // the feature line for halves: the corners
        let line = match region {
            Region::Mouth | Region::Jaw => pos.get("MouthCorner_R").zip(pos.get("MouthCorner_L")).map(|(a, b)| (*a, *b)),
            Region::Eye => None,
            _ => None,
        };
        let lid_lines = [("EyeOut_R", "EyeIn_R"), ("EyeIn_L", "EyeOut_L")];
        for (i, v) in bone.vertices.iter().enumerate() {
            let near = moving.iter().map(|p| (*p - v.pos).mag()).fold(f32::MAX, f32::min);
            if near >= reach {
                continue;
            }
            let t = 1. - near / reach;
            let fade = t * t * (3. - 2. * t);
            // the vertex's half, for lips (mouth line) or lids (its eye's line)
            let half = match region {
                Region::Mouth | Region::Jaw => line.map(|l| vertex_half(bone, i, l)),
                Region::Eye => lid_lines
                    .iter()
                    .filter_map(|(a, b)| pos.get(*a).zip(pos.get(*b)).map(|(a, b)| (*a, *b)))
                    .min_by(|x, y| ((x.0 + x.1) / 2. - v.pos).mag().total_cmp(&((y.0 + y.1) / 2. - v.pos).mag()))
                    .map(|l| vertex_half(bone, i, l)),
                _ => None,
            };
            let (mut num, mut den) = (Vec2::ZERO, 0.);
            for (d, p) in &members {
                // below the mouth, the chin belongs to the lower half
                let ok = match (half, d.half) {
                    (Some(h), lh) => lh == Half::Both || lh == h,
                    (None, _) => true,
                };
                if !ok {
                    continue;
                }
                let r2 = (*p - v.pos).mag().powi(2) + (0.02 * w).powi(2);
                let wgt = 1. / r2;
                num += motion.get(d.name).copied().unwrap_or(Vec2::ZERO) * wgt;
                den += wgt;
            }
            if den > 0. {
                total[i] += num / den * fade;
            }
        }
    }
    bone.vertices
        .iter()
        .zip(total)
        .filter(|(_, d)| d.mag() > 1e-5)
        .map(|(v, d)| ShapeDelta { vert: v.id, delta: d })
        .collect()
}

/// Generate a shape's offsets from the landmark map, or `None` without a template or
/// enough landmarks.
pub fn generate(bone: &Bone, map: &FaceMap, shape: &str, strength: f32) -> Option<Vec<ShapeDelta>> {
    let canonical = crate::face::canonical(shape)?;
    let moves = template_for(canonical)?;
    let pos = positions(bone, map);
    units(&pos)?;
    let motion = landmark_motion(&moves, &pos, strength);
    Some(spread(bone, &pos, &motion))
}

/// Regenerate a shape, keeping the artist's corrections (docs/LIP_SYNC.md §6.7): the
/// shape's offsets become the new generated ones plus whatever the artist had added on
/// top of the old generated ones.
pub fn regenerate(shape: &mut MeshShape, generated: Vec<ShapeDelta>) {
    let mut corrections: HashMap<u32, Vec2> = shape.deltas.iter().map(|d| (d.vert, d.delta)).collect();
    for g in &shape.generated {
        *corrections.entry(g.vert).or_insert(Vec2::ZERO) -= g.delta;
    }
    let mut total: HashMap<u32, Vec2> = corrections;
    for g in &generated {
        *total.entry(g.vert).or_insert(Vec2::ZERO) += g.delta;
    }
    let mut deltas: Vec<ShapeDelta> = total.into_iter().filter(|(_, d)| d.mag() > 1e-5).map(|(vert, delta)| ShapeDelta { vert, delta }).collect();
    deltas.sort_by_key(|d| d.vert);
    shape.deltas = deltas;
    shape.generated = generated;
}

/// Generate Face: create any missing standard shapes (and visemes, when `visemes`) and
/// (re)generate every one with a template, keeping corrections. Returns how many shapes
/// were generated.
pub fn generate_face(bone: &mut Bone, visemes: bool, strength: f32) -> usize {
    let Some(map) = bone.face_map.clone() else { return 0 };
    crate::face::add_standard_shapes(bone, visemes);
    let mut n = 0;
    let names: Vec<(i32, String)> = bone.shapes.iter().map(|s| (s.id, s.name.clone())).collect();
    for (id, name) in names {
        let Some(generated) = generate(bone, &map, &name, strength) else { continue };
        if let Some(shape) = bone.shapes.iter_mut().find(|s| s.id == id) {
            regenerate(shape, generated);
            n += 1;
        }
    }
    n
}

/// What generating needs that the mesh lacks (docs/LIP_SYNC.md §6.7): a lip seam
/// (upper and lower lip middles must be separate vertices on the mesh's edge), and
/// enough landmarks for units.
pub fn checks(bone: &Bone, map: &FaceMap) -> Vec<&'static str> {
    let mut out = vec![];
    let pos = positions(bone, map);
    if units(&pos).is_none() {
        out.push("units");
    }
    let vert = |n: &str| map.points.iter().find(|p| p.name == n).map(|p| p.vert);
    if let (Some(u), Some(l)) = (vert("UpperLipMid"), vert("LowerLipMid")) {
        let edge = edge_vertices(bone);
        let idx = |id: u32| bone.vertices.iter().position(|v| v.id == id).map(|i| i as u32);
        let on_edge = |id: u32| idx(id).is_some_and(|i| edge.contains(&i));
        if u == l || !on_edge(u) || !on_edge(l) {
            out.push("lip_seam");
        }
    }
    out
}

/// The order landmarks are mapped in (docs/LIP_SYNC.md §6.7): the midline, then the
/// character's left side. The right side is derived.
pub const MAP_ORDER: &[&str] = &[
    "UpperLipMid", "LowerLipMid", "NoseTip", "Chin",
    "MouthCorner_L", "UpperLip_L1", "UpperLip_L2", "LowerLip_L1", "LowerLip_L2",
    "EyeIn_L", "EyeOut_L", "Lid_L_Up1", "Lid_L_Up2", "Lid_L_Low1", "Lid_L_Low2",
    "Brow_L_In", "Brow_L_Mid", "Brow_L_Out", "NoseWing_L",
];

/// Set (or replace) a landmark, confirmed.
pub fn map_point(map: &mut FaceMap, name: &str, vert: u32) {
    map.points.retain(|p| p.name != name);
    map.points.push(LandmarkPoint { name: name.to_string(), vert, derived: false });
}

/// For three-quarter views: the cylinder from a mapped pair (a landmark and its twin,
/// both confirmed), with the radius taken from the mesh's half-width.
pub fn cylinder_for(bone: &Bone, known: &HashMap<String, Vec2>) -> Option<Cylinder> {
    let midline: Vec<Vec2> = LANDMARKS.iter().filter(|l| l.mirror.is_none()).filter_map(|l| known.get(l.name).copied()).collect();
    let (origin, dir) = symmetry_axis(&midline)?;
    let normal = Vec2::new(dir.y, -dir.x);
    let across = |p: Vec2| (p - origin).x * normal.x + (p - origin).y * normal.y;
    let (lo, hi) = bone.vertices.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(across(v.pos)), hi.max(across(v.pos))));
    let r = ((hi - lo) / 2.).max(1e-3);
    // prefer the pair farthest from the midline (the mouth corners, the eye corners)
    let pair = LANDMARKS
        .iter()
        .filter_map(|l| Some((known.get(l.name)?, known.get(l.mirror?)?)))
        .max_by(|a, b| across(*a.0).abs().total_cmp(&across(*b.0).abs()))?;
    Cylinder::from_pair(across(*pair.0), across(*pair.1), 0., r)
}

/// Derive Other Side: suggest each confirmed landmark's missing twin, snapped to a
/// vertex (on the mesh's edge when the source is), marked as derived. Earlier
/// suggestions are replaced; confirmed points are kept.
pub fn derive_into(bone: &Bone, map: &mut FaceMap) -> usize {
    map.points.retain(|p| !p.derived);
    let pos = positions(bone, map);
    let cylinder = if map.view == FaceView::ThreeQuarter { cylinder_for(bone, &pos) } else { None };
    let suggested = derive_side(&pos, cylinder);
    let edge = edge_vertices(bone);
    let on_edge = |id: u32| bone.vertices.iter().position(|v| v.id == id).is_some_and(|i| edge.contains(&(i as u32)));
    let mut n = 0;
    for (name, p) in suggested {
        let source = def(&name).and_then(|d| d.mirror).and_then(|m| map.points.iter().find(|q| q.name == m)).map(|q| q.vert);
        let prefer_edge = source.is_some_and(on_edge);
        if let Some(vert) = snap(bone, p, prefer_edge) {
            map.points.push(LandmarkPoint { name, vert, derived: true });
            n += 1;
        }
    }
    n
}
