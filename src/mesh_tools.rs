//! Canvas-side helpers for the topology tools (docs/TOPOLOGY_TOOLS.md §5, §6):
//! what's under the cursor, measured in screen pixels.

use crate::*;

/// How close (screen px) the cursor must be to a vertex to pick it.
pub const VERT_PICK_PX: f32 = 9.;
/// How close (screen px) the cursor must be to an edge to pick it.
pub const EDGE_PICK_PX: f32 = 6.;
/// How far (screen px) the cursor must travel in an Eraser stroke between erases,
/// so holding still over freshly made edges doesn't keep erasing them.
pub const ERASE_SPACING_PX: f32 = 8.;

/// A vertex or edge of a mesh, by vertex index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    Vertex(u32),
    Edge(u32, u32),
}

/// Renderer-space position (`renderer::world_vert` output: -1..1 on both axes)
/// to screen pixels.
pub fn to_screen(pos: Vec2, window: Vec2) -> Vec2 {
    Vec2::new((pos.x + 1.) / 2. * window.x, (1. - pos.y) / 2. * window.y)
}

/// The vertex or edge under `mouse` (renderer space), vertices first.
/// `verts` are the mesh's vertices in renderer space, in mesh order.
pub fn pick(verts: &[Vertex], indices: &[u32], mouse: Vec2, window: Vec2) -> Option<Pick> {
    let m = to_screen(mouse, window);
    let px = |i: u32| to_screen(verts[i as usize].pos, window);

    let mut best: Option<(f32, Pick)> = None;
    for i in 0..verts.len() as u32 {
        let d = (px(i) - m).mag();
        if d < VERT_PICK_PX && best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, Pick::Vertex(i)));
        }
    }
    if best.is_some() {
        return best.map(|(_, p)| p);
    }

    for (i, j) in topology::edges(indices) {
        let d = segment_distance(m, px(i), px(j));
        if d < EDGE_PICK_PX && best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, Pick::Edge(i, j)));
        }
    }
    best.map(|(_, p)| p)
}

fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    let len2 = d.x * d.x + d.y * d.y;
    if len2 == 0. {
        return (p - a).mag();
    }
    let t = (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / len2).clamp(0., 1.);
    (p - (a + d * t)).mag()
}

/// Apply one eraser hit to `bone`: dissolve keeps the surface, delete leaves a hole.
pub fn erase(bone: &mut Bone, pick: Pick, delete: bool) -> Result<(), topology::TopoError> {
    match (pick, delete) {
        (Pick::Vertex(v), false) => topology::dissolve_vertex(bone, v),
        (Pick::Vertex(v), true) => topology::delete_vertex(bone, v),
        (Pick::Edge(i, j), false) => topology::dissolve_edge(bone, i, j),
        (Pick::Edge(i, j), true) => topology::delete_edge(bone, i, j),
    }
}

/// Screen pixels back to renderer space (inverse of `to_screen`).
pub fn from_screen(px: Vec2, window: Vec2) -> Vec2 {
    Vec2::new(px.x / window.x * 2. - 1., 1. - px.y / window.y * 2.)
}

/// Maps between texture UV and renderer space for one drawn mesh: exact
/// (barycentric) inside its triangles, from an affine fit of the whole mesh
/// outside them.
pub struct UvMap<'a> {
    verts: &'a [Vertex],
    indices: &'a [u32],
    to_uv: Option<[f32; 6]>,
    to_pos: Option<[f32; 6]>,
}

impl<'a> UvMap<'a> {
    /// `verts` in renderer space, carrying their UVs.
    pub fn new(verts: &'a [Vertex], indices: &'a [u32]) -> Self {
        let pairs: Vec<(Vec2, Vec2)> = verts.iter().map(|v| (v.pos, v.uv)).collect();
        let flipped: Vec<(Vec2, Vec2)> = pairs.iter().map(|(p, uv)| (*uv, *p)).collect();
        UvMap { verts, indices, to_uv: pen::fit_affine(&pairs), to_pos: pen::fit_affine(&flipped) }
    }

    /// The UV under a renderer-space point.
    pub fn uv_at(&self, pos: Vec2) -> Option<Vec2> {
        let found = self.inside(pos, |v| v.pos, |v| v.uv);
        found.or_else(|| self.to_uv.map(|m| affine(m, pos)))
    }

    /// Where a UV is drawn, in renderer space.
    pub fn pos_of(&self, uv: Vec2) -> Option<Vec2> {
        let found = self.inside(uv, |v| v.uv, |v| v.pos);
        found.or_else(|| self.to_pos.map(|m| affine(m, uv)))
    }

    /// Interpolate `to` at `p` (given in `from`'s space) in the triangle containing it.
    fn inside(&self, p: Vec2, from: impl Fn(&Vertex) -> Vec2, to: impl Fn(&Vertex) -> Vec2) -> Option<Vec2> {
        for t in self.indices.chunks_exact(3) {
            let v = [t[0], t[1], t[2]].map(|i| &self.verts[i as usize]);
            let (a, b, c) = (from(v[0]), from(v[1]), from(v[2]));
            if topology::cross(a, b, c).abs() < 1e-12 {
                continue;
            }
            let (wa, wb, wc) = topology::barycentric(p, a, b, c);
            if wa >= 0. && wb >= 0. && wc >= 0. {
                return Some(to(v[0]) * wa + to(v[1]) * wb + to(v[2]) * wc);
            }
        }
        None
    }
}

fn affine(m: [f32; 6], p: Vec2) -> Vec2 {
    Vec2::new(m[0] * p.x + m[1] * p.y + m[2], m[3] * p.x + m[4] * p.y + m[5])
}

/// What a Pen click at the cursor would do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PenTarget {
    /// Add this point (drawn at the renderer-space position).
    Place(pen::PenPoint, Vec2),
    /// Close the cut being drawn, on its first point.
    Close,
    /// End the cut being drawn, on its last point (a double-click lands here).
    Finish,
}

/// Pen snapping options for one frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct PenSnapping {
    /// Snap to vertices and edges (Shift held turns it off).
    pub snap: bool,
    /// On an edge, take its midpoint (Ctrl held).
    pub midpoint: bool,
    /// Keep the new segment at a multiple of 45°.
    pub angle_lock: bool,
}

/// Where a Pen click at `mouse` (renderer space) would go. `active` holds the
/// renderer-space positions of the cut being drawn, if any. None if the mesh has
/// no usable layout to place a free point with.
pub fn pen_target(
    verts: &[Vertex],
    indices: &[u32],
    map: &UvMap,
    mouse: Vec2,
    window: Vec2,
    active: &[Vec2],
    snapping: PenSnapping,
) -> Option<PenTarget> {
    let m = to_screen(mouse, window);
    let near = |p: Vec2| (to_screen(p, window) - m).mag() < VERT_PICK_PX;
    if let (Some(first), Some(last)) = (active.first(), active.last()) {
        if active.len() >= 3 && near(*first) {
            return Some(PenTarget::Close);
        }
        if near(*last) {
            return Some(PenTarget::Finish);
        }
    }

    if snapping.snap {
        match pick(verts, indices, mouse, window) {
            Some(Pick::Vertex(v)) => {
                let vert = &verts[v as usize];
                let point = pen::PenPoint { uv: vert.uv, snap: pen::Snap::Vertex(v) };
                return Some(PenTarget::Place(point, vert.pos));
            }
            Some(Pick::Edge(i, j)) => {
                let (a, b) = (&verts[i as usize], &verts[j as usize]);
                let t = if snapping.midpoint {
                    0.5
                } else {
                    let (sa, sb) = (to_screen(a.pos, window), to_screen(b.pos, window));
                    let d = sb - sa;
                    let len2 = d.x * d.x + d.y * d.y;
                    if len2 == 0. { 0.5 } else { (((m.x - sa.x) * d.x + (m.y - sa.y) * d.y) / len2).clamp(0., 1.) }
                };
                let uv = a.uv + (b.uv - a.uv) * t;
                let point = pen::PenPoint { uv, snap: pen::Snap::Edge };
                return Some(PenTarget::Place(point, a.pos + (b.pos - a.pos) * t));
            }
            None => {}
        }
    }

    // free point, optionally kept on a 45° line from the last one
    let mut pos = mouse;
    if let (true, Some(last)) = (snapping.angle_lock, active.last()) {
        let from = to_screen(*last, window);
        let d = m - from;
        let step = std::f32::consts::FRAC_PI_4;
        let angle = (d.y.atan2(d.x) / step).round() * step;
        let dir = Vec2::new(angle.cos(), angle.sin());
        let len = d.x * dir.x + d.y * dir.y;
        pos = from_screen(from + dir * len, window);
    }
    let uv = map.uv_at(pos)?;
    let clamped = Vec2::new(uv.x.clamp(0., 1.), uv.y.clamp(0., 1.));
    if clamped != uv {
        pos = map.pos_of(clamped)?;
    }
    let point = pen::PenPoint { uv: clamped, snap: pen::Snap::Free };
    Some(PenTarget::Place(point, pos))
}

/// Where a placed point is drawn, in renderer space.
pub fn pen_point_pos(point: &pen::PenPoint, verts: &[Vertex], map: &UvMap) -> Option<Vec2> {
    match point.snap {
        pen::Snap::Vertex(v) => verts.get(v as usize).map(|v| v.pos),
        _ => map.pos_of(point.uv),
    }
}
