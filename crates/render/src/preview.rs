//! The 3D view's renderer: one ray per pixel (plus 2×2 supersampling when idle) against the
//! scene mesh's BVH, the material's maps sampled with tiling and parallax occlusion from the
//! height output, then metallic/roughness GGX shading under image-based light (SH diffuse,
//! prefiltered specular, an analytic environment BRDF), exposure and ACES tone mapping.

use std::sync::Arc;

use materialcraft_geom::glam::{Vec2, Vec3};
use materialcraft_geom::{Bvh, Mesh, OrbitCamera};
use materialcraft_image::{Image, linear_to_srgb};
use rayon::prelude::*;

use crate::ibl::{self, EnvImage};

/// The material's maps (any may be missing: defaults are mid gray, flat, rough 0.5, dielectric).
#[derive(Clone, Default)]
pub struct Maps {
    pub base_color: Option<Image>,
    pub normal: Option<Image>,
    pub roughness: Option<Image>,
    pub metallic: Option<Image>,
    pub height: Option<Image>,
    pub ao: Option<Image>,
    pub emissive: Option<Image>,
    pub opacity: Option<Image>,
}

impl Maps {
    /// Maps from `(usage id, image)` pairs (`baseColor`, `normal`, …).
    pub fn from_usages<'a>(items: impl IntoIterator<Item = (&'a str, &'a Image)>) -> Maps {
        let mut m = Maps::default();
        for (usage, img) in items {
            let slot = match usage {
                "baseColor" => &mut m.base_color,
                "normal" => &mut m.normal,
                "roughness" => &mut m.roughness,
                "metallic" => &mut m.metallic,
                "height" => &mut m.height,
                "ambientOcclusion" => &mut m.ao,
                "emissive" => &mut m.emissive,
                "opacity" => &mut m.opacity,
                _ => continue,
            };
            if slot.is_none() {
                *slot = Some(img.clone());
            }
        }
        m
    }
}

/// A mesh ready for ray casts.
pub struct Scene {
    pub mesh: Arc<Mesh>,
    pub bvh: Arc<Bvh>,
    pub radius: f32,
    /// Center of the mesh bounds (the camera orbits it).
    pub center: Vec3,
}

impl Scene {
    pub fn new(mut mesh: Mesh) -> Scene {
        if mesh.normals.len() != mesh.positions.len() {
            mesh.compute_normals();
        }
        if mesh.tangents.len() != mesh.positions.len() {
            mesh.compute_tangents();
        }
        let bvh = Bvh::new(&mesh);
        let (lo, hi) = mesh.bounds();
        let center = (lo + hi) * 0.5;
        let radius = ((hi - lo).length() * 0.5).max(1e-3);
        Scene { mesh: Arc::new(mesh), bvh: Arc::new(bvh), radius, center }
    }

    /// A built-in scene (`rounded_cube`, `sphere`, …); unknown ids give the rounded cube.
    pub fn builtin(id: &str) -> Scene {
        let mesh = materialcraft_geom::primitives::builtin(id).unwrap_or_else(|| materialcraft_geom::primitives::rounded_box(Vec3::ONE, 0.15));
        Scene::new(mesh)
    }
}

/// Prefiltered environment lighting.
pub struct Lighting {
    pub name: String,
    pub specular: Vec<EnvImage>,
    pub sh: [[f32; 3]; 9],
}

impl Lighting {
    pub fn new(id: &str) -> Lighting {
        let env = ibl::environment(ibl::by_id(id), 256);
        Lighting { name: id.into(), specular: ibl::prefilter(&env, 6), sh: ibl::sh9(&env) }
    }

    fn diffuse(&self, n: Vec3) -> Vec3 {
        let b = ibl::sh_basis(n);
        let mut c = Vec3::ZERO;
        for k in 0..9 {
            c += Vec3::from(self.sh[k]) * b[k];
        }
        c.max(Vec3::ZERO)
    }

    fn specular(&self, r: Vec3, rough: f32) -> Vec3 {
        let levels = self.specular.len();
        if levels == 0 {
            return Vec3::ZERO;
        }
        let l = rough.clamp(0.0, 1.0) * (levels - 1) as f32;
        let i = (l.floor() as usize).min(levels - 1);
        let j = (i + 1).min(levels - 1);
        let (a, b) = (self.specular[i].sample(r), self.specular[j].sample(r));
        a.lerp(b, l - i as f32)
    }
}

/// Camera and look.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Degrees.
    pub yaw: f32,
    pub pitch: f32,
    /// In scene radii.
    pub distance: f32,
    pub tiling: f32,
    /// Stops.
    pub exposure: f32,
    /// Degrees.
    pub env_rotation: f32,
    /// Parallax depth in UV units (0 = off).
    pub height_scale: f32,
    /// Show the (blurred) environment behind the mesh instead of the flat backdrop.
    pub show_env: bool,
    /// 1 = one ray per pixel, 2 = 2×2 supersampling.
    pub supersample: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { yaw: 35.0, pitch: 25.0, distance: 3.2, tiling: 1.0, exposure: 0.0, env_rotation: 0.0, height_scale: 0.03, show_env: false, supersample: 1 }
    }
}

/// The orbit camera for `s` around `scene`.
pub fn camera(scene: &Scene, s: &Settings) -> OrbitCamera {
    OrbitCamera {
        target: scene.center,
        yaw: s.yaw.to_radians(),
        pitch: s.pitch.clamp(-89.0, 89.0).to_radians(),
        distance: s.distance.clamp(1.05, 50.0) * scene.radius,
        fov: 35.0,
        scene_radius: scene.radius,
        ortho: false,
    }
}

#[inline]
fn tex(img: &Option<Image>, uv: Vec2) -> Option<[f32; 4]> {
    img.as_ref().map(|i| i.sample(uv.x, uv.y, true))
}

#[inline]
fn tex_gray(img: &Option<Image>, uv: Vec2, default: f32) -> f32 {
    img.as_ref().map_or(default, |i| i.sample_gray(uv.x, uv.y, true))
}

/// Narkowicz's fit of the ACES filmic curve.
#[inline]
fn aces(x: f32) -> f32 {
    ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
}

/// Karis' analytic approximation of the split-sum environment BRDF: (scale, bias) for F0.
#[inline]
fn env_brdf(roughness: f32, ndv: f32) -> (f32, f32) {
    let c0 = [-1.0, -0.0275, -0.572, 0.022];
    let c1 = [1.0, 0.0425, 1.04, -0.04];
    let r = [roughness * c0[0] + c1[0], roughness * c0[1] + c1[1], roughness * c0[2] + c1[2], roughness * c0[3] + c1[3]];
    let a004 = (r[0] * r[0]).min((-9.28 * ndv).exp2()) * r[0] + r[1];
    (a004 * -1.04 + r[2], a004 * 1.04 + r[3])
}

fn rotate_y(v: Vec3, deg: f32) -> Vec3 {
    let (s, c) = deg.to_radians().sin_cos();
    Vec3::new(v.x * c + v.z * s, v.y, -v.x * s + v.z * c)
}

/// Shade one camera ray; `None` = missed the mesh.
fn shade(scene: &Scene, maps: &Maps, light: &Lighting, s: &Settings, origin: Vec3, dir: Vec3) -> Option<Vec3> {
    let hit = scene.bvh.closest(origin, dir, f32::MAX)?;
    let m = &scene.mesh;
    let [i0, i1, i2] = m.tri(hit.tri as usize)?;
    let (w1, w2) = (hit.u, hit.v);
    let w0 = 1.0 - w1 - w2;
    let lerp3 = |a: Vec3, b: Vec3, c: Vec3| a * w0 + b * w1 + c * w2;
    let nrm = lerp3(*m.normals.get(i0)?, *m.normals.get(i1)?, *m.normals.get(i2)?).normalize_or_zero();
    let (t0, t1, t2) = (*m.tangents.get(i0)?, *m.tangents.get(i1)?, *m.tangents.get(i2)?);
    let tan = (t0.truncate() * w0 + t1.truncate() * w1 + t2.truncate() * w2).normalize_or_zero();
    let sign = if t0.w < 0.0 { -1.0 } else { 1.0 };
    let tan = (tan - nrm * nrm.dot(tan)).normalize_or_zero();
    let bit = nrm.cross(tan) * sign;
    let uv0 = *m.uvs.get(i0)? * w0 + *m.uvs.get(i1)? * w1 + *m.uvs.get(i2)? * w2;
    let mut uv = uv0 * s.tiling;
    let v = -dir;
    // Parallax occlusion: march the height field along the view direction in tangent space.
    if s.height_scale > 0.0
        && let Some(h) = &maps.height
    {
        let vt = Vec3::new(v.dot(tan), v.dot(bit), v.dot(nrm).max(0.05));
        let steps = 32;
        let delta = Vec2::new(vt.x, -vt.y) / vt.z * s.height_scale * s.tiling / steps as f32;
        let mut layer = 1.0;
        let mut cur = uv;
        let mut prev_h = h.sample_gray(cur.x, cur.y, true);
        let mut prev_layer = layer;
        for _ in 0..steps {
            let hv = h.sample_gray(cur.x, cur.y, true);
            if hv >= layer {
                // Linear refinement between the last two samples.
                let a = prev_h - prev_layer;
                let b = hv - layer;
                let t = if (b - a).abs() > 1e-6 { a / (a - b) } else { 0.0 };
                cur -= delta * (1.0 - t.clamp(0.0, 1.0));
                break;
            }
            prev_h = hv;
            prev_layer = layer;
            layer -= 1.0 / steps as f32;
            cur -= delta;
        }
        uv = cur;
    }
    let base = tex(&maps.base_color, uv).map_or(Vec3::splat(0.5), |c| Vec3::new(c[0], c[1], c[2]));
    let rough = tex_gray(&maps.roughness, uv, 0.5).clamp(0.02, 1.0);
    let metal = tex_gray(&maps.metallic, uv, 0.0).clamp(0.0, 1.0);
    let ao = tex_gray(&maps.ao, uv, 1.0).clamp(0.0, 1.0);
    let n = match tex(&maps.normal, uv) {
        Some(c) => {
            let t = Vec3::new(c[0] * 2.0 - 1.0, c[1] * 2.0 - 1.0, c[2] * 2.0 - 1.0);
            // OpenGL convention: green points along +v in texture space, which is -bitangent
            // with top-left UVs.
            (tan * t.x - bit * t.y + nrm * t.z.max(0.05)).normalize_or_zero()
        }
        None => nrm,
    };
    let n = if n.dot(v) < 0.0 { nrm } else { n };
    let ndv = n.dot(v).clamp(1e-4, 1.0);
    let f0 = Vec3::splat(0.04).lerp(base, metal);
    let diffuse_col = base * (1.0 - metal);
    let rot = |d: Vec3| rotate_y(d, -s.env_rotation);
    let r = (n * (2.0 * n.dot(v)) - v).normalize_or_zero();
    let (sa, sb) = env_brdf(rough, ndv);
    let spec = light.specular(rot(r), rough) * (f0 * sa + Vec3::splat(sb));
    let diff = light.diffuse(rot(n)) * diffuse_col;
    let mut c = (diff + spec) * ao;
    if let Some(e) = tex(&maps.emissive, uv) {
        c += Vec3::new(e[0], e[1], e[2]);
    }
    if let Some(o) = &maps.opacity {
        let a = o.sample_gray(uv.x, uv.y, true).clamp(0.0, 1.0);
        c = c * a + backdrop(dir) * (1.0 - a);
    }
    Some(c)
}

/// The flat studio backdrop: a soft vertical gradient (linear).
fn backdrop(dir: Vec3) -> Vec3 {
    let t = (dir.y * 0.5 + 0.5).clamp(0.0, 1.0);
    Vec3::splat(0.012).lerp(Vec3::splat(0.03), t)
}

/// Render `w × h` pixels to sRGB RGBA8.
pub fn render(scene: &Scene, maps: &Maps, light: &Lighting, s: &Settings, w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w.clamp(1, 4096), h.clamp(1, 4096));
    let cam = camera(scene, s);
    let aspect = w as f32 / h as f32;
    let ss = s.supersample.clamp(1, 4);
    let k = 2f32.powf(s.exposure);
    let bg_env = light.specular.get(3).or(light.specular.last());
    let mut out = vec![0u8; (w * h * 4) as usize];
    out.par_chunks_mut(w as usize * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w as usize {
            let mut acc = Vec3::ZERO;
            let mut cover = 0.0;
            for sy in 0..ss {
                for sx in 0..ss {
                    let px = (x as f32 + (sx as f32 + 0.5) / ss as f32) / w as f32 * 2.0 - 1.0;
                    let py = 1.0 - (y as f32 + (sy as f32 + 0.5) / ss as f32) / h as f32 * 2.0;
                    let ray = cam.ray(Vec2::new(px, py), aspect);
                    match shade(scene, maps, light, s, ray.origin, ray.dir) {
                        Some(c) => {
                            acc += c * k;
                            cover += 1.0;
                        }
                        None => {
                            acc += match (s.show_env, bg_env) {
                                (true, Some(e)) => e.sample(rotate_y(ray.dir, -s.env_rotation)) * k,
                                _ => backdrop(ray.dir),
                            };
                        }
                    }
                }
            }
            let _ = cover;
            let c = acc / (ss * ss) as f32;
            let q = |v: f32| (linear_to_srgb(aces(v)) * 255.0 + 0.5) as u8;
            if let Some(px) = row.get_mut(x * 4..x * 4 + 4) {
                px.copy_from_slice(&[q(c.x), q(c.y), q(c.z), 255]);
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use materialcraft_image::Kind;

    #[test]
    fn renders_a_material_on_every_scene() {
        let light = Lighting::new("studio");
        let maps = Maps {
            base_color: Some(Image::filled(16, 16, Kind::Color, [0.8, 0.1, 0.1, 1.0])),
            roughness: Some(Image::filled(16, 16, Kind::Gray, [0.3; 4])),
            height: Some(Image::gray(16, 16, |x, _| (x % 4) as f32 / 4.0)),
            ..Maps::default()
        };
        for id in ["rounded_cube", "sphere", "plane", "cylinder", "torus", "material_ball", "cube"] {
            let scene = Scene::builtin(id);
            let px = render(&scene, &maps, &light, &Settings::default(), 64, 48);
            assert_eq!(px.len(), 64 * 48 * 4);
            // The center hits the mesh (the torus' hole is left of it): reddish.
            let c = if id == "torus" { ((24 * 64 + 20) * 4) as usize } else { ((24 * 64 + 32) * 4) as usize };
            assert!(px[c] > px[c + 1], "{id}: center is not red ({:?})", &px[c..c + 3]);
        }
    }
}
