//! Built-in sample materials and graph templates, built in code from the node library (no
//! third-party content).

use materialcraft_core::{Graph, NodeId, Package, Value};

/// Template ids and labels for File ▸ New graph.
pub const TEMPLATES: &[(&str, &str)] = &[
    ("empty", "Empty"),
    ("pbr", "PBR material (outputs only)"),
    ("bricks", "Brick wall"),
    ("stone_tiles", "Stone tiles"),
    ("wood", "Wood planks"),
    ("rusty_metal", "Rusty metal"),
    ("fabric", "Woven fabric"),
    ("ground", "Stone ground"),
];

struct B<'g> {
    g: &'g mut Graph,
}

impl B<'_> {
    fn n(&mut self, def: &str, x: f32, y: f32, params: &[(&str, Value)]) -> NodeId {
        // Node cards are about 100 × 135 with thumbnails: spread the layout to fit them.
        let id = self.g.add_node(def, [x * 1.1, y * 1.3]);
        if let Ok(n) = self.g.node_mut(id) {
            for (k, v) in params {
                n.params.insert((*k).into(), v.clone());
            }
        }
        id
    }
    fn c(&mut self, from: NodeId, to: NodeId, port: &str) {
        self.cp(from, "output", to, port);
    }
    fn cp(&mut self, from: NodeId, from_port: &str, to: NodeId, port: &str) {
        // Sample graphs are acyclic by construction; a failure would only drop a wire.
        let _ = self.g.connect(from, from_port, to, port);
    }
    /// The six PBR outputs at `x`, fed by the given nodes.
    fn outputs(&mut self, x: f32, feeds: &[(NodeId, &str, u32, &str)]) {
        for (i, (from, port, usage, ident)) in feeds.iter().enumerate() {
            let o = self.n("output", x, i as f32 * 125.0, &[("usage", Value::Enum(*usage)), ("identifier", Value::Text((*ident).into()))]);
            self.cp(*from, port, o, "input");
        }
    }
}

fn f(v: f32) -> Value {
    Value::Float(v)
}

fn e(v: u32) -> Value {
    Value::Enum(v)
}

fn i(v: i32) -> Value {
    Value::Int(v)
}

fn col(r: f32, g: f32, b: f32) -> Value {
    Value::Color([r, g, b, 1.0])
}

fn grad(keys: &[(f32, [f32; 3])]) -> Value {
    Value::Gradient(materialcraft_core::Gradient {
        keys: keys.iter().map(|(p, c)| materialcraft_core::GradientKey { pos: *p, color: [c[0], c[1], c[2], 1.0] }).collect(),
    })
}

/// A new graph from a template (unknown ids give an empty graph).
pub fn template(id: &str, name: &str, size_log2: u32) -> Graph {
    let mut g = Graph::new(name);
    g.size = [size_log2, size_log2];
    let mut b = B { g: &mut g };
    match id {
        "pbr" => {
            let base = b.n("pbr_base_material", 0.0, 220.0, &[]);
            b.outputs(
                320.0,
                &[
                    (base, "base_color", 0, "basecolor"),
                    (base, "normal", 1, "normal"),
                    (base, "roughness", 2, "roughness"),
                    (base, "metallic", 3, "metallic"),
                    (base, "height", 4, "height"),
                    (base, "ambient_occlusion", 5, "ambientocclusion"),
                ],
            );
        }
        "bricks" => bricks(&mut b),
        "stone_tiles" => stone_tiles(&mut b),
        "wood" => wood(&mut b),
        "rusty_metal" => rusty_metal(&mut b),
        "fabric" => fabric(&mut b),
        "ground" => ground(&mut b),
        _ => {}
    }
    g
}

fn bricks(b: &mut B) {
    let bricks = b.n(
        "brick_generator",
        0.0,
        0.0,
        &[("bricks_x", i(4)), ("bricks_y", i(10)), ("gap", f(0.06)), ("bevel", f(0.18)), ("height_random", f(0.25)), ("lum_random", f(0.0))],
    );
    let cells = b.n("cells_1", 0.0, 180.0, &[("scale", f(24.0))]);
    let grunge = b.n("grunge_map_1", 0.0, 360.0, &[("scale", f(4.0))]);
    let chip = b.n("blend", 180.0, 90.0, &[("mode", e(3)), ("opacity", f(0.35))]);
    b.c(cells, chip, "foreground");
    b.c(bricks, chip, "background");
    let erode = b.n("slope_blur", 360.0, 90.0, &[("intensity", f(1.2)), ("mode", e(1))]);
    b.c(chip, erode, "input");
    b.c(grunge, erode, "slope");
    let height = b.n("levels", 540.0, 90.0, &[("in_high", f(0.9))]);
    b.c(erode, height, "input");
    let normal = b.n("normal", 720.0, 0.0, &[("intensity", f(2.2))]);
    b.c(height, normal, "input");
    let ao = b.n("ambient_occlusion", 720.0, 120.0, &[("spread", f(0.05)), ("intensity", f(0.6))]);
    b.c(height, ao, "height");
    // Color: per-brick tint from cells, mortar from the height mask.
    let tint = b.n("cells_3", 360.0, 300.0, &[("scale", f(10.0))]);
    let mix = b.n("blend", 540.0, 300.0, &[("mode", e(3)), ("opacity", f(0.6))]);
    b.c(grunge, mix, "foreground");
    b.c(tint, mix, "background");
    let brick_col =
        b.n("gradient_map", 720.0, 300.0, &[("gradient", grad(&[(0.0, [0.18, 0.05, 0.03]), (0.5, [0.42, 0.12, 0.06]), (1.0, [0.62, 0.28, 0.16])]))]);
    b.c(mix, brick_col, "input");
    let mortar = b.n("uniform", 720.0, 440.0, &[("color", col(0.42, 0.40, 0.36))]);
    let mask = b.n("histogram_scan", 720.0, 560.0, &[("position", f(0.92)), ("contrast", f(0.85))]);
    b.c(height, mask, "input");
    let base = b.n("blend", 900.0, 360.0, &[("mode", e(0))]);
    b.c(brick_col, base, "foreground");
    b.c(mortar, base, "background");
    b.c(mask, base, "mask");
    let rough = b.n("levels", 900.0, 520.0, &[("out_low", f(0.65)), ("out_high", f(0.95))]);
    b.c(grunge, rough, "input");
    let metal = b.n("uniform", 900.0, 640.0, &[("mode", e(1)), ("color", col(0.0, 0.0, 0.0))]);
    b.outputs(
        1120.0,
        &[
            (base, "output", 0, "basecolor"),
            (normal, "output", 1, "normal"),
            (rough, "output", 2, "roughness"),
            (metal, "output", 3, "metallic"),
            (height, "output", 4, "height"),
            (ao, "output", 5, "ambientocclusion"),
        ],
    );
}

fn stone_tiles(b: &mut B) {
    let tiles = b.n(
        "tile_generator",
        0.0,
        0.0,
        &[("pattern", e(7)), ("x_amount", i(5)), ("y_amount", i(5)), ("scale", f(0.96)), ("offset", f(0.5)), ("lum_random", f(0.25))],
    );
    let clouds = b.n("clouds_2", 0.0, 180.0, &[("scale", f(6.0))]);
    let detail = b.n("blend", 200.0, 80.0, &[("mode", e(3)), ("opacity", f(0.5))]);
    b.c(clouds, detail, "foreground");
    b.c(tiles, detail, "background");
    let normal = b.n("normal", 420.0, 0.0, &[("intensity", f(1.8))]);
    b.c(detail, normal, "input");
    let ao = b.n("ambient_occlusion", 420.0, 120.0, &[]);
    b.c(detail, ao, "height");
    let cmap = b.n("gradient_map", 420.0, 240.0, &[("gradient", grad(&[(0.0, [0.12, 0.12, 0.11]), (0.6, [0.45, 0.43, 0.40]), (1.0, [0.70, 0.68, 0.62])]))]);
    b.c(detail, cmap, "input");
    let rough = b.n("invert", 420.0, 360.0, &[]);
    b.c(clouds, rough, "input");
    let metal = b.n("uniform", 420.0, 480.0, &[("mode", e(1)), ("color", col(0.0, 0.0, 0.0))]);
    b.outputs(
        700.0,
        &[
            (cmap, "output", 0, "basecolor"),
            (normal, "output", 1, "normal"),
            (rough, "output", 2, "roughness"),
            (metal, "output", 3, "metallic"),
            (detail, "output", 4, "height"),
            (ao, "output", 5, "ambientocclusion"),
        ],
    );
}

fn wood(b: &mut B) {
    let grain = b.n("anisotropic_noise", 0.0, 0.0, &[("scale", f(3.0)), ("scale_y", f(96.0)), ("smoothness", f(0.3))]);
    let planks = b.n(
        "brick_generator",
        0.0,
        180.0,
        &[("bricks_x", i(1)), ("bricks_y", i(6)), ("offset", f(0.0)), ("gap", f(0.015)), ("bevel", f(0.05)), ("height_random", f(0.1))],
    );
    let warp_src = b.n("perlin_noise", 0.0, 360.0, &[("scale", f(4.0))]);
    let warped = b.n("directional_warp", 200.0, 0.0, &[("intensity", f(6.0)), ("angle", f(0.25))]);
    b.c(grain, warped, "input");
    b.c(warp_src, warped, "intensity_input");
    let rot = b.n("transformation", 380.0, 0.0, &[("scale", Value::Float2([1.0, 1.0]))]);
    b.c(warped, rot, "input");
    let h = b.n("blend", 560.0, 90.0, &[("mode", e(3))]);
    b.c(rot, h, "foreground");
    b.c(planks, h, "background");
    let normal = b.n("normal", 740.0, 0.0, &[("intensity", f(1.2))]);
    b.c(h, normal, "input");
    let ao = b.n("ambient_occlusion", 740.0, 120.0, &[("spread", f(0.03))]);
    b.c(h, ao, "height");
    let cmap = b.n("gradient_map", 740.0, 240.0, &[("gradient", grad(&[(0.0, [0.10, 0.04, 0.015]), (0.45, [0.33, 0.15, 0.05]), (1.0, [0.62, 0.36, 0.15])]))]);
    b.c(h, cmap, "input");
    let rough = b.n("levels", 740.0, 360.0, &[("out_low", f(0.45)), ("out_high", f(0.75))]);
    b.c(rot, rough, "input");
    let metal = b.n("uniform", 740.0, 480.0, &[("mode", e(1)), ("color", col(0.0, 0.0, 0.0))]);
    b.outputs(
        1000.0,
        &[
            (cmap, "output", 0, "basecolor"),
            (normal, "output", 1, "normal"),
            (rough, "output", 2, "roughness"),
            (metal, "output", 3, "metallic"),
            (h, "output", 4, "height"),
            (ao, "output", 5, "ambientocclusion"),
        ],
    );
}

fn rusty_metal(b: &mut B) {
    let rust_mask = b.n("grunge_map_1", 0.0, 0.0, &[("scale", f(3.0)), ("contrast", f(0.7))]);
    let scan = b.n("histogram_scan", 200.0, 0.0, &[("position", f(0.55)), ("contrast", f(0.7))]);
    b.c(rust_mask, scan, "input");
    let fine = b.n("clouds_3", 0.0, 180.0, &[("scale", f(12.0))]);
    let brushed = b.n("anisotropic_noise", 0.0, 360.0, &[("scale", f(2.0)), ("scale_y", f(256.0))]);
    let h = b.n("blend", 400.0, 90.0, &[("mode", e(1)), ("opacity", f(0.4))]);
    b.c(fine, h, "foreground");
    b.c(scan, h, "background");
    let normal = b.n("normal", 600.0, 0.0, &[("intensity", f(1.4))]);
    b.c(h, normal, "input");
    let rust_col = b.n("gradient_map", 400.0, 240.0, &[("gradient", grad(&[(0.0, [0.12, 0.04, 0.01]), (0.5, [0.38, 0.12, 0.03]), (1.0, [0.55, 0.25, 0.08])]))]);
    b.c(fine, rust_col, "input");
    let metal_col = b.n("uniform", 400.0, 380.0, &[("color", col(0.55, 0.56, 0.58))]);
    let base = b.n("blend", 600.0, 300.0, &[]);
    b.c(rust_col, base, "foreground");
    b.c(metal_col, base, "background");
    b.c(scan, base, "mask");
    let metal = b.n("invert", 600.0, 440.0, &[]);
    b.c(scan, metal, "input");
    let rough_metal = b.n("levels", 400.0, 520.0, &[("out_low", f(0.25)), ("out_high", f(0.4))]);
    b.c(brushed, rough_metal, "input");
    let rough = b.n("blend", 600.0, 560.0, &[]);
    let rough_rust = b.n("uniform", 400.0, 640.0, &[("mode", e(1)), ("color", col(0.85, 0.85, 0.85))]);
    b.c(rough_rust, rough, "foreground");
    b.c(rough_metal, rough, "background");
    b.c(scan, rough, "mask");
    let ao = b.n("ambient_occlusion", 600.0, 680.0, &[]);
    b.c(h, ao, "height");
    b.outputs(
        860.0,
        &[
            (base, "output", 0, "basecolor"),
            (normal, "output", 1, "normal"),
            (rough, "output", 2, "roughness"),
            (metal, "output", 3, "metallic"),
            (h, "output", 4, "height"),
            (ao, "output", 5, "ambientocclusion"),
        ],
    );
}

fn fabric(b: &mut B) {
    let weave = b.n("weave", 0.0, 0.0, &[("tiling", i(16)), ("width", f(0.85))]);
    let fuzz = b.n("white_noise", 0.0, 180.0, &[]);
    let soft = b.n("blur", 200.0, 180.0, &[("intensity", f(0.4))]);
    b.c(fuzz, soft, "input");
    let h = b.n("blend", 400.0, 60.0, &[("mode", e(3)), ("opacity", f(0.25))]);
    b.c(soft, h, "foreground");
    b.c(weave, h, "background");
    let normal = b.n("normal", 600.0, 0.0, &[("intensity", f(1.0))]);
    b.c(h, normal, "input");
    let ao = b.n("ambient_occlusion", 600.0, 120.0, &[("spread", f(0.02))]);
    b.c(h, ao, "height");
    let cmap = b.n("gradient_map", 600.0, 240.0, &[("gradient", grad(&[(0.0, [0.02, 0.05, 0.12]), (1.0, [0.12, 0.22, 0.45])]))]);
    b.c(h, cmap, "input");
    let rough = b.n("uniform", 600.0, 360.0, &[("mode", e(1)), ("color", col(0.9, 0.9, 0.9))]);
    let metal = b.n("uniform", 600.0, 480.0, &[("mode", e(1)), ("color", col(0.0, 0.0, 0.0))]);
    b.outputs(
        860.0,
        &[
            (cmap, "output", 0, "basecolor"),
            (normal, "output", 1, "normal"),
            (rough, "output", 2, "roughness"),
            (metal, "output", 3, "metallic"),
            (h, "output", 4, "height"),
            (ao, "output", 5, "ambientocclusion"),
        ],
    );
}

fn ground(b: &mut B) {
    let stones = b.n("cells_4", 0.0, 0.0, &[("scale", f(6.0))]);
    let pebbles = b.n("cells_1", 0.0, 180.0, &[("scale", f(18.0))]);
    let dirt = b.n("dirt", 0.0, 360.0, &[("scale", f(5.0))]);
    let h1 = b.n("height_blend", 220.0, 90.0, &[("offset", f(0.45)), ("contrast", f(0.85))]);
    b.c(stones, h1, "top");
    b.c(pebbles, h1, "bottom");
    let h = b.n("blend", 420.0, 160.0, &[("mode", e(6)), ("opacity", f(0.8))]);
    b.c(dirt, h, "foreground");
    b.cp(h1, "height", h, "background");
    let normal = b.n("normal", 620.0, 0.0, &[("intensity", f(2.0))]);
    b.c(h, normal, "input");
    let ao = b.n("ambient_occlusion", 620.0, 120.0, &[]);
    b.c(h, ao, "height");
    let cmap = b.n("gradient_map", 620.0, 240.0, &[("gradient", grad(&[(0.0, [0.08, 0.06, 0.04]), (0.5, [0.27, 0.22, 0.17]), (1.0, [0.55, 0.52, 0.47])]))]);
    b.c(h, cmap, "input");
    let rough = b.n("levels", 620.0, 360.0, &[("out_low", f(0.6)), ("out_high", f(0.95))]);
    b.c(dirt, rough, "input");
    let metal = b.n("uniform", 620.0, 480.0, &[("mode", e(1)), ("color", col(0.0, 0.0, 0.0))]);
    b.outputs(
        880.0,
        &[
            (cmap, "output", 0, "basecolor"),
            (normal, "output", 1, "normal"),
            (rough, "output", 2, "roughness"),
            (metal, "output", 3, "metallic"),
            (h, "output", 4, "height"),
            (ao, "output", 5, "ambientocclusion"),
        ],
    );
}

/// The package the app opens with: the brick wall sample.
pub fn sample_package() -> Package {
    let mut p = Package::new("Untitled package");
    p.graphs.push(template("bricks", "Brick_wall", 10));
    p
}
