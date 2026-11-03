//! The 3D view's renderer: one ray per pixel (plus 2×2 supersampling when idle) against the
//! scene mesh's BVH, the material's maps sampled with tiling and parallax occlusion from the
//! height output, then metallic/roughness GGX shading under image-based light (SH diffuse,
//! prefiltered specular, an analytic environment BRDF), exposure and ACES tone mapping.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
        Scene::new(builtin_mesh(id))
    }

    /// `base` subdivided to at most `max_tris` triangles and pushed along its normals by
    /// `depth × (height − 0.5)` (the height map repeated `tiling` times): real silhouettes and
    /// self-occlusion for offline renders. Coincident vertices (UV seams, hard edges) move
    /// together, so the surface stays closed.
    pub fn displaced(base: &Mesh, height: &Image, tiling: f32, depth: f32, max_tris: usize) -> Scene {
        let mut mesh = base.clone();
        if mesh.normals.len() != mesh.positions.len() {
            mesh.compute_normals();
        }
        while mesh.triangle_count() > 0 && mesh.triangle_count() * 4 <= max_tris {
            mesh = mesh.subdivided();
        }
        // Pre-filtered so fine detail (left to the normal map) does not spike the silhouette.
        let height = height.resize(height.w.min(512), height.h.min(512), true);
        let key = |p: Vec3| (p * 1e4).round().as_ivec3().to_array();
        let mut welded: HashMap<[i32; 3], (Vec3, f32, f32)> = HashMap::new();
        for ((p, n), uv) in mesh.positions.iter().zip(&mesh.normals).zip(&mesh.uvs) {
            let e = welded.entry(key(*p)).or_default();
            e.0 += *n;
            e.1 += height.sample_gray(uv.x * tiling, uv.y * tiling, true);
            e.2 += 1.0;
        }
        for p in &mut mesh.positions {
            if let Some((n, h, count)) = welded.get(&key(*p)) {
                *p += n.normalize_or_zero() * depth * (h / count - 0.5);
            }
        }
        Scene::new(mesh)
    }
}

/// A built-in mesh by id; unknown ids give the rounded cube.
pub fn builtin_mesh(id: &str) -> Mesh {
    let mut m = materialcraft_geom::primitives::builtin(id).unwrap_or_else(|| materialcraft_geom::primitives::rounded_box(Vec3::ONE, 0.15));
    if id == "sphere" {
        // The texture wraps twice around, so texels are square at the equator.
        m.uvs.iter_mut().for_each(|uv| uv.x *= 2.0);
    }
    m
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
    /// Orbit target offset from the scene center, in scene radii (the caller bounds it).
    pub target: [f32; 3],
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
    /// Ray-traced ambient occlusion rays per sample (0 = off): darkens the crevices of a
    /// displaced scene.
    pub ao_rays: u32,
    /// Strength of a shadow-casting key light from the camera's upper left (0 = off).
    pub key_light: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            yaw: 35.0,
            pitch: 25.0,
            distance: 3.2,
            target: [0.0; 3],
            tiling: 1.0,
            exposure: 0.0,
            env_rotation: 0.0,
            height_scale: 0.03,
            show_env: false,
            supersample: 1,
            ao_rays: 0,
            key_light: 0.0,
        }
    }
}

/// The orbit camera for `s` around `scene`.
pub fn camera(scene: &Scene, s: &Settings) -> OrbitCamera {
    let target = scene.center + Vec3::from(s.target) * scene.radius;
    OrbitCamera {
        target: if target.is_finite() { target } else { scene.center },
        yaw: s.yaw.to_radians(),
        pitch: s.pitch.clamp(-89.0, 89.0).to_radians(),
        distance: s.distance.clamp(1.05, 50.0) * scene.radius,
        fov: 35.0,
        scene_radius: scene.radius,
        ortho: false,
    }
}

/// `s.target` after panning the view by (dx, dy) viewport heights: the scene follows the pointer.
/// Unbounded; non-finite parts become 0.
pub fn pan(scene: &Scene, s: &Settings, dx: f32, dy: f32) -> [f32; 3] {
    let mut cam = camera(scene, s);
    cam.pan(dx, dy);
    let t = (cam.target - scene.center) / scene.radius.max(f32::EPSILON);
    t.to_array().map(|v| if v.is_finite() { v } else { 0.0 })
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

/// Per-pixel inputs of [`shade`] beyond the camera ray.
struct Px {
    /// Backdrop color (seen through transparent areas).
    bg: Vec3,
    /// Direction toward the key light.
    key: Vec3,
    /// Random rotation of the ambient occlusion ray pattern.
    jitter: Vec2,
}

/// The fraction of `n`'s hemisphere at `p` that is open within `reach` (cosine-weighted).
fn ambient_occlusion(scene: &Scene, p: Vec3, n: Vec3, rays: u32, jitter: Vec2) -> f32 {
    let (t, b) = n.any_orthonormal_pair();
    let reach = scene.radius * 0.3;
    let open = (0..rays)
        .filter(|&i| {
            let u = (ibl::hammersley(i, rays) + jitter).fract();
            let (r, phi) = (u.x.sqrt(), u.y * std::f32::consts::TAU);
            let d = t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - u.x).max(0.0).sqrt();
            !scene.bvh.occluded(p, d, reach)
        })
        .count();
    open as f32 / rays as f32
}

/// GGX specular plus Lambert diffuse from a white directional light along `l`.
fn direct(n: Vec3, v: Vec3, l: Vec3, base: Vec3, f0: Vec3, metal: f32, rough: f32) -> Vec3 {
    let ndl = n.dot(l);
    if ndl <= 0.0 {
        return Vec3::ZERO;
    }
    let ndv = n.dot(v).max(1e-4);
    let h = (v + l).normalize_or_zero();
    let (ndh, vdh) = (n.dot(h).max(0.0), v.dot(h).max(0.0));
    let a2 = (rough * rough).powi(2).max(1e-4);
    let d = a2 / (std::f32::consts::PI * (ndh * ndh * (a2 - 1.0) + 1.0).powi(2));
    let k = (rough + 1.0).powi(2) / 8.0;
    let g = ndl / (ndl * (1.0 - k) + k) * ndv / (ndv * (1.0 - k) + k);
    let f = f0 + (Vec3::ONE - f0) * (1.0 - vdh).powi(5);
    let spec = f * (d * g / (4.0 * ndv * ndl));
    let diff = (Vec3::ONE - f) * base * (1.0 - metal) / std::f32::consts::PI;
    (diff + spec) * ndl
}

/// Shade one camera ray; `None` = missed the mesh.
fn shade(scene: &Scene, maps: &Maps, light: &Lighting, s: &Settings, origin: Vec3, dir: Vec3, px: &Px) -> Option<Vec3> {
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
    if s.ao_rays > 0 || s.key_light > 0.0 {
        // The true surface (a displaced scene's facets), offset to avoid self-hits.
        let (p0, p1, p2) = (*m.positions.get(i0)?, *m.positions.get(i1)?, *m.positions.get(i2)?);
        let face = (p1 - p0).cross(p2 - p0).normalize_or_zero();
        let face = if face.dot(nrm) < 0.0 { -face } else { face };
        let p = origin + dir * hit.t + face * scene.radius * 2e-4;
        if s.ao_rays > 0 {
            c *= ambient_occlusion(scene, p, face, s.ao_rays, px.jitter);
        }
        if s.key_light > 0.0 && face.dot(px.key) > 0.0 && !scene.bvh.occluded(p, px.key, f32::MAX) {
            c += direct(n, v, px.key, base, f0, metal, rough) * s.key_light * ao;
        }
    }
    if let Some(e) = tex(&maps.emissive, uv) {
        c += Vec3::new(e[0], e[1], e[2]);
    }
    if let Some(o) = &maps.opacity {
        let a = o.sample_gray(uv.x, uv.y, true).clamp(0.0, 1.0);
        c = c * a + px.bg * (1.0 - a);
    }
    Some(c)
}

/// The studio backdrop at screen position `p` (−1..1): a soft vignette (linear).
fn backdrop(p: Vec2) -> Vec3 {
    let t = (p.length() * 0.75).clamp(0.0, 1.0);
    Vec3::splat(0.04).lerp(Vec3::splat(0.012), t * t * (3.0 - 2.0 * t))
}

/// A cheap integer hash to 0..1 pairs (the per-sample AO pattern rotation).
fn jitter(x: u32, y: u32) -> Vec2 {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    Vec2::new((h & 0xFFFF) as f32 / 65536.0, (h >> 16) as f32 / 65536.0)
}

/// Render `w × h` pixels to sRGB RGBA8.
pub fn render(scene: &Scene, maps: &Maps, light: &Lighting, s: &Settings, w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w.clamp(1, 4096), h.clamp(1, 4096));
    render_cancellable(scene, maps, light, s, w, h, &AtomicBool::new(false)).unwrap_or_else(|| vec![0; (w * h * 4) as usize])
}

/// [`render`] that gives up once `cancel` is set (rows not yet started are skipped) and then
/// returns `None`: a frame the viewer no longer needs (the camera moved, the view was resized)
/// stops holding the CPU.
pub fn render_cancellable(scene: &Scene, maps: &Maps, light: &Lighting, s: &Settings, w: u32, h: u32, cancel: &AtomicBool) -> Option<Vec<u8>> {
    let (w, h) = (w.clamp(1, 4096), h.clamp(1, 4096));
    let cam = camera(scene, s);
    let aspect = w as f32 / h as f32;
    let ss = s.supersample.clamp(1, 4);
    let k = 2f32.powf(s.exposure);
    let bg_env = light.specular.get(3).or(light.specular.last());
    // Key light from the camera's upper left, a little in front of the subject.
    let fwd = cam.ray(Vec2::ZERO, aspect).dir;
    let right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
    let key = (right * -0.6 + right.cross(fwd) * 0.75 - fwd * 0.5).normalize_or_zero();
    let mut out = vec![0u8; (w * h * 4) as usize];
    out.par_chunks_mut(w as usize * 4).enumerate().for_each(|(y, row)| {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        for x in 0..w as usize {
            let mut acc = Vec3::ZERO;
            for sy in 0..ss {
                for sx in 0..ss {
                    let px = (x as f32 + (sx as f32 + 0.5) / ss as f32) / w as f32 * 2.0 - 1.0;
                    let py = 1.0 - (y as f32 + (sy as f32 + 0.5) / ss as f32) / h as f32 * 2.0;
                    let ray = cam.ray(Vec2::new(px, py), aspect);
                    let bg = backdrop(Vec2::new(px, py));
                    let p = Px { bg, key, jitter: jitter(x as u32 * ss + sx, y as u32 * ss + sy) };
                    acc += match shade(scene, maps, light, s, ray.origin, ray.dir, &p) {
                        Some(c) => c * k,
                        None => match (s.show_env, bg_env) {
                            (true, Some(e)) => e.sample(rotate_y(ray.dir, -s.env_rotation)) * k,
                            _ => bg,
                        },
                    };
                }
            }
            let c = acc / (ss * ss) as f32;
            let q = |v: f32| (linear_to_srgb(aces(v)) * 255.0 + 0.5) as u8;
            if let Some(px) = row.get_mut(x * 4..x * 4 + 4) {
                px.copy_from_slice(&[q(c.x), q(c.y), q(c.z), 255]);
            }
        }
    });
    if cancel.load(Ordering::Relaxed) { None } else { Some(out) }
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

    #[test]
    fn a_cancelled_render_gives_up_and_an_uncancelled_one_matches_render() {
        let (scene, light, maps, s) = (Scene::builtin("sphere"), Lighting::new("studio"), Maps::default(), Settings::default());
        assert!(render_cancellable(&scene, &maps, &light, &s, 32, 24, &AtomicBool::new(true)).is_none());
        let px = render_cancellable(&scene, &maps, &light, &s, 32, 24, &AtomicBool::new(false));
        assert_eq!(px, Some(render(&scene, &maps, &light, &s, 32, 24)));
    }

    #[test]
    fn panning_moves_the_scene_with_the_pointer() {
        let scene = Scene::builtin("sphere");
        let s = Settings::default();
        let at = |s: &Settings| camera(&scene, s).project(scene.center, 1.0).unwrap();
        // Dragging right and down by a tenth of the view moves the center right and down by
        // the same fraction (NDC spans two view heights, y up).
        let right = Settings { target: pan(&scene, &s, 0.1, 0.0), ..s };
        let p = at(&right);
        assert!((p.x - 0.2).abs() < 1e-3 && p.y.abs() < 1e-3, "{p:?}");
        let down = Settings { target: pan(&scene, &s, 0.0, 0.1), ..s };
        let p = at(&down);
        assert!(p.x.abs() < 1e-3 && (p.y + 0.2).abs() < 1e-3, "{p:?}");
        // Orbiting turns around the panned target, not the scene center.
        let turned = Settings { yaw: s.yaw + 90.0, ..right };
        let cam = camera(&scene, &turned);
        assert!((cam.target - camera(&scene, &right).target).length() < 1e-5);
        // Panning back returns home; hostile inputs stay finite.
        let back = pan(&scene, &right, -0.1, 0.0);
        assert!(back.iter().all(|v| v.abs() < 1e-4), "{back:?}");
        let far = pan(&scene, &s, 1e9, f32::NAN);
        assert!(far.iter().all(|v| v.is_finite()), "{far:?}");
        for target in [[f32::NAN, 0.0, 0.0], [0.0, f32::INFINITY, 0.0], [0.0, 0.0, f32::MAX]] {
            let cam = camera(&scene, &Settings { target, ..s });
            assert_eq!(cam.target, scene.center, "{target:?}");
            assert!(cam.eye().is_finite());
        }
    }
}
