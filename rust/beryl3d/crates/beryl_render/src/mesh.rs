//! The Baseline tier's three meshes (docs/beryl-3d.md, "The Baseline tier"): the card, the
//! strip that softens its edge, and a unit quad. All three live in one vertex buffer and one
//! index buffer, and the indices count from the start of the vertex buffer, so a draw call
//! needs no base vertex (OpenGL ES 3.0 has none).

use beryl_scene::frame::Mesh;
use beryl_scene::table::{CARD_HEIGHT, CARD_WIDTH};

/// The radius of a card's rounded corners, in millimetres.
pub const CARD_RADIUS: f32 = 3.0;
/// Points along each rounded corner. Four corners of nine points and the centre make the card
/// 37 vertices.
const CORNER_POINTS: usize = 9;
/// Points around a card's outline.
const OUTLINE: usize = 4 * CORNER_POINTS;

/// One vertex of a mesh, in the mesh's own space: a card is its printed size in millimetres,
/// centred on the origin in the plane z = 0 with its face toward +z.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    /// Where the vertex falls on the card's face: u to the right, v down from the top.
    pub uv: [f32; 2],
    /// For the edge strip, the outline's outward direction at this point; zero elsewhere.
    pub outward: [f32; 2],
    /// For the edge strip, 0 on the card's outline and 1 on the ring one pixel outside it.
    pub edge: f32,
    pub unused: f32,
}

/// The meshes' vertices and indices, and where each mesh's indices are.
pub struct Meshes {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
    /// The range of `indices` of each mesh, by `Mesh` value.
    pub ranges: [std::ops::Range<u32>; 3],
}

/// Builds the three meshes.
#[must_use]
pub fn meshes() -> Meshes {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let card = card(&mut vertices, &mut indices);
    let edge = edge(&mut vertices, &mut indices);
    let quad = quad(&mut vertices, &mut indices);
    let mut ranges = [0..0, 0..0, 0..0];
    ranges[Mesh::Card as usize] = card;
    ranges[Mesh::CardEdge as usize] = edge;
    ranges[Mesh::Quad as usize] = quad;
    Meshes {
        vertices,
        indices,
        ranges,
    }
}

/// The card's outline, counterclockwise seen from the face: each point and the outward
/// direction there.
fn outline() -> impl Iterator<Item = ([f32; 2], [f32; 2])> {
    let half = [
        CARD_WIDTH / 2.0 - CARD_RADIUS,
        CARD_HEIGHT / 2.0 - CARD_RADIUS,
    ];
    let corners = [
        (1.0, 1.0, 0.0),
        (-1.0, 1.0, 90.0),
        (-1.0, -1.0, 180.0),
        (1.0, -1.0, 270.0),
    ];
    corners.into_iter().flat_map(move |(sx, sy, start)| {
        (0..CORNER_POINTS).map(move |i| {
            let angle = (start + 90.0 * i as f32 / (CORNER_POINTS - 1) as f32).to_radians();
            let (sin, cos) = angle.sin_cos();
            (
                [
                    sx * half[0] + CARD_RADIUS * cos,
                    sy * half[1] + CARD_RADIUS * sin,
                ],
                [cos, sin],
            )
        })
    })
}

fn uv(position: [f32; 2]) -> [f32; 2] {
    [
        position[0] / CARD_WIDTH + 0.5,
        0.5 - position[1] / CARD_HEIGHT,
    ]
}

fn vertex(position: [f32; 2], outward: [f32; 2], edge: f32) -> Vertex {
    Vertex {
        position,
        uv: uv(position),
        outward,
        edge,
        unused: 0.0,
    }
}

fn first(vertices: &[Vertex]) -> u16 {
    u16::try_from(vertices.len()).expect("the meshes have fewer than 65,536 vertices")
}

fn range(indices: &[u16], start: usize) -> std::ops::Range<u32> {
    start as u32..indices.len() as u32
}

/// The card: a fan of triangles around its centre, so its corners are geometry and every pixel
/// it covers is opaque.
fn card(vertices: &mut Vec<Vertex>, indices: &mut Vec<u16>) -> std::ops::Range<u32> {
    let start = indices.len();
    let centre = first(vertices);
    vertices.push(vertex([0.0, 0.0], [0.0, 0.0], 0.0));
    vertices.extend(outline().map(|(point, _)| vertex(point, [0.0, 0.0], 0.0)));
    let ring = OUTLINE as u16;
    for i in 0..ring {
        indices.extend_from_slice(&[centre, centre + 1 + i, centre + 1 + (i + 1) % ring]);
    }
    range(indices, start)
}

/// The edge strip: an inner ring on the card's outline and an outer ring at the same points,
/// which the vertex shader moves one physical pixel outward along `outward`.
fn edge(vertices: &mut Vec<Vertex>, indices: &mut Vec<u16>) -> std::ops::Range<u32> {
    let start = indices.len();
    let base = first(vertices);
    for (point, outward) in outline() {
        vertices.push(vertex(point, outward, 0.0));
        vertices.push(vertex(point, outward, 1.0));
    }
    let ring = OUTLINE as u16;
    for i in 0..ring {
        let inner = base + 2 * i;
        let next = base + 2 * ((i + 1) % ring);
        // Counterclockwise seen from the face, as the card's triangles are.
        indices.extend_from_slice(&[inner, inner + 1, next + 1, inner, next + 1, next]);
    }
    range(indices, start)
}

/// A square from -0.5 to 0.5, its texture coordinates running over the whole square.
fn quad(vertices: &mut Vec<Vertex>, indices: &mut Vec<u16>) -> std::ops::Range<u32> {
    let start = indices.len();
    let base = first(vertices);
    for (x, y) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
        vertices.push(Vertex {
            position: [x, y],
            uv: [x + 0.5, 0.5 - y],
            outward: [0.0, 0.0],
            edge: 0.0,
            unused: 0.0,
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    range(indices, start)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Twice the signed area of a triangle: positive when its points run counterclockwise.
    fn winding(m: &Meshes, triangle: &[u16]) -> f32 {
        let [a, b, c] = [0, 1, 2].map(|i| m.vertices[usize::from(triangle[i])].position);
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }

    #[test]
    fn every_triangle_faces_the_front() {
        let m = meshes();
        for (mesh, range) in m.ranges.iter().enumerate() {
            let indices = &m.indices[range.start as usize..range.end as usize];
            assert!(!indices.is_empty());
            for triangle in indices.chunks(3) {
                // The edge strip's rings coincide until the shader moves the outer one, so
                // move it here along `outward` to measure the winding.
                let mut moved = Meshes {
                    vertices: m.vertices.clone(),
                    indices: Vec::new(),
                    ranges: m.ranges.clone(),
                };
                for v in &mut moved.vertices {
                    v.position[0] += v.outward[0] * v.edge;
                    v.position[1] += v.outward[1] * v.edge;
                }
                assert!(winding(&moved, triangle) > 0.0, "mesh {mesh}: {triangle:?}");
            }
        }
    }
}
