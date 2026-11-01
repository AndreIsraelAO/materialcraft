//! Meshes for the 3D view: OBJ and glTF/GLB (self-contained: embedded or data-URI buffers),
//! normalized to fit a unit sphere at the origin, with normals, tangents and (when the file has
//! none) generated UVs.

use materialcraft_geom::Mesh;
use materialcraft_geom::glam::{Vec2, Vec3};

use crate::{FormatError, Result};

/// Mesh file extensions [`load_mesh`] reads.
pub const MESH_EXTENSIONS: &[&str] = &["obj", "glb", "gltf"];

/// Largest mesh accepted (triangles).
const MAX_TRIANGLES: usize = 4_000_000;

fn bad(msg: impl Into<String>) -> FormatError {
    FormatError::BadPackage(msg.into())
}

/// Parse a mesh file by extension.
pub fn load_mesh(name: &str, bytes: &[u8]) -> Result<Mesh> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let mut mesh = match ext.as_str() {
        "obj" => obj(bytes)?,
        "glb" | "gltf" => gltf(bytes)?,
        other => return Err(bad(format!("unsupported mesh format `{other}` (OBJ, glTF or GLB)"))),
    };
    if mesh.indices.is_empty() || mesh.positions.is_empty() {
        return Err(bad("the mesh has no triangles"));
    }
    if mesh.triangle_count() > MAX_TRIANGLES {
        return Err(bad(format!("the mesh has {} triangles (limit {MAX_TRIANGLES})", mesh.triangle_count())));
    }
    if mesh.uvs.len() != mesh.positions.len() || mesh.uvs.iter().all(|u| *u == mesh.uvs[0]) {
        // No usable UVs: a box projection keeps materials tiling across the surface.
        mesh.uvs = box_uvs(&mesh);
    }
    mesh.normalize_to(1.0);
    if mesh.normals.len() != mesh.positions.len() {
        mesh.compute_normals();
    }
    mesh.compute_tangents();
    mesh.validate().map_err(|e| bad(e.to_string()))?;
    Ok(mesh)
}

fn obj(bytes: &[u8]) -> Result<Mesh> {
    let mut reader = std::io::BufReader::new(bytes);
    let opts = tobj::LoadOptions { triangulate: true, single_index: true, ..Default::default() };
    let (models, _) = tobj::load_obj_buf(&mut reader, &opts, |_| Ok((Vec::new(), Default::default()))).map_err(|e| bad(format!("OBJ: {e}")))?;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for m in models {
        let base = positions.len() as u32;
        let mesh = &m.mesh;
        let n = mesh.positions.len() / 3;
        for i in 0..n {
            positions.push(Vec3::new(mesh.positions[i * 3], mesh.positions[i * 3 + 1], mesh.positions[i * 3 + 2]));
            if mesh.normals.len() == mesh.positions.len() {
                normals.push(Vec3::new(mesh.normals[i * 3], mesh.normals[i * 3 + 1], mesh.normals[i * 3 + 2]));
            }
            // OBJ UVs have their origin at the bottom left.
            uvs.push(mesh.texcoords.get(i * 2..i * 2 + 2).map_or(Vec2::ZERO, |t| Vec2::new(t[0], 1.0 - t[1])));
        }
        indices.extend(mesh.indices.iter().filter(|i| (**i as usize) < n).map(|i| i + base));
        if indices.len() / 3 > MAX_TRIANGLES {
            return Err(bad("the mesh is too large"));
        }
    }
    indices.truncate(indices.len() / 3 * 3);
    let normals = (normals.len() == positions.len()).then_some(normals);
    Mesh::from_parts(positions, normals, uvs, indices, "material").map_err(|e| bad(e.to_string()))
}

fn gltf(bytes: &[u8]) -> Result<Mesh> {
    let (doc, buffers, _) = gltf::import_slice(bytes).map_err(|e| bad(format!("glTF: {e}")))?;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    let mut all_normals = true;
    for node in doc.nodes() {
        let Some(mesh) = node.mesh() else { continue };
        let m = materialcraft_geom::glam::Mat4::from_cols_array_2d(&node.transform().matrix());
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(|b| buffers.get(b.index()).map(|d| d.0.as_slice()));
            let Some(pos) = reader.read_positions() else { continue };
            let base = positions.len() as u32;
            let pos: Vec<Vec3> = pos.map(|p| m.transform_point3(Vec3::from(p))).collect();
            let n = pos.len();
            match reader.read_normals() {
                Some(ns) => normals.extend(ns.map(|q| m.transform_vector3(Vec3::from(q)).normalize_or_zero())),
                None => all_normals = false,
            }
            match reader.read_tex_coords(0) {
                Some(t) => uvs.extend(t.into_f32().map(Vec2::from)),
                None => uvs.extend(std::iter::repeat_n(Vec2::ZERO, n)),
            }
            match reader.read_indices() {
                Some(ix) => indices.extend(ix.into_u32().filter(|i| (*i as usize) < n).map(|i| i + base)),
                None => indices.extend((0..n as u32).map(|i| i + base)),
            }
            positions.extend(pos);
            if indices.len() / 3 > MAX_TRIANGLES {
                return Err(bad("the mesh is too large"));
            }
        }
    }
    indices.truncate(indices.len() / 3 * 3);
    let normals = (all_normals && normals.len() == positions.len()).then_some(normals);
    Mesh::from_parts(positions, normals, uvs, indices, "material").map_err(|e| bad(e.to_string()))
}

/// Box-projected UVs: each vertex takes the plane of its normal's largest axis.
fn box_uvs(mesh: &Mesh) -> Vec<Vec2> {
    let (lo, hi) = mesh.bounds();
    let size = (hi - lo).max_element().max(1e-6);
    mesh.positions
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let q = (*p - lo) / size;
            let n = mesh.normals.get(i).copied().unwrap_or(Vec3::Y).abs();
            if n.x >= n.y && n.x >= n.z {
                Vec2::new(q.z, 1.0 - q.y)
            } else if n.y >= n.z {
                Vec2::new(q.x, q.z)
            } else {
                Vec2::new(q.x, 1.0 - q.y)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obj_quad_loads_with_uvs_and_tangents() {
        let obj = b"v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nf 1/1 2/2 3/3 4/4\n";
        let m = load_mesh("quad.obj", obj).unwrap();
        assert_eq!(m.triangle_count(), 2);
        assert_eq!(m.tangents.len(), m.positions.len());
        assert!(m.radius() <= 1.0 + 1e-4);
        // A mesh without UVs gets projected ones.
        let m = load_mesh("tri.obj", b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
        assert!(m.uvs.iter().any(|u| *u != m.uvs[0]));
    }

    #[test]
    fn bad_meshes_are_errors() {
        assert!(load_mesh("x.obj", b"").is_err());
        assert!(load_mesh("x.glb", b"glTF garbage").is_err());
        assert!(load_mesh("x.fbx", b"").is_err());
        assert!(load_mesh("x.obj", b"v 0 0 0\nf 1 2 3\n").is_err());
    }
}
