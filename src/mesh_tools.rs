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
