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
    ("metal_plates", "Painted metal plates"),
    ("rusty_metal", "Rusty metal"),
    ("lava", "Lava rock"),
    ("marble", "Polished marble"),
    ("wicker", "Wicker weave"),
    ("fabric", "Woven fabric"),
    ("ground", "Mossy ground"),
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
    /// The PBR outputs at `x`, fed by the given nodes.
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
        "metal_plates" => metal_plates(&mut b),
        "rusty_metal" => rusty_metal(&mut b),
        "lava" => lava(&mut b),
        "marble" => marble(&mut b),
        "wicker" => wicker(&mut b),
        "fabric" => fabric(&mut b),
        "ground" => ground(&mut b),
        _ => {}
    }
    g
}

fn bricks(b: &mut B) {
    let layout = b.n("brick_generator", 0.0, 0.0, &[("bricks_x", i(4)), ("bricks_y", i(12)), ("gap", f(0.05)), ("bevel", f(0.3)), ("height_random", f(0.12))]);
    let ids = b.n(
        "brick_generator",
        0.0,
        160.0,
        &[("bricks_x", i(4)), ("bricks_y", i(12)), ("gap", f(0.05)), ("bevel", f(0.0)), ("height_random", f(0.0)), ("lum_random", f(1.0))],
    );
    let pores = b.n("clouds_2", 0.0, 320.0, &[("scale", f(16.0))]);
    let grunge = b.n("grunge_map_1", 0.0, 480.0, &[("scale", f(4.0))]);
    let dirt = b.n("clouds_2", 0.0, 640.0, &[("scale", f(24.0))]);
    let chipped = b.n("edge_damage", 180.0, 0.0, &[("width", f(6.0)), ("amount", f(0.6)), ("scale", f(14.0))]);
    b.c(layout, chipped, "height");
    let porous = b.n("blend", 360.0, 0.0, &[("mode", e(3)), ("opacity", f(0.3))]);
    b.c(pores, porous, "foreground");
    b.c(chipped, porous, "background");
    let mortar_h = b.n("levels", 360.0, 160.0, &[("out_low", f(0.08)), ("out_high", f(0.16))]);
    b.c(dirt, mortar_h, "input");
    let height = b.n("blend", 540.0, 80.0, &[("mode", e(5))]);
    b.c(mortar_h, height, "foreground");
    b.c(porous, height, "background");
    let normal = b.n("normal", 720.0, 0.0, &[("intensity", f(2.5))]);
    b.c(height, normal, "input");
    let ao = b.n("ambient_occlusion", 720.0, 120.0, &[("spread", f(0.05)), ("intensity", f(0.7))]);
    b.c(height, ao, "height");
    let tone = b.n("blend", 360.0, 320.0, &[("mode", e(9)), ("opacity", f(0.6))]);
    b.c(grunge, tone, "foreground");
    b.c(ids, tone, "background");
    let brick_col = b.n(
        "gradient_map",
        540.0,
        320.0,
        &[("gradient", grad(&[(0.0, [0.09, 0.022, 0.012]), (0.5, [0.2, 0.05, 0.022]), (0.85, [0.3, 0.09, 0.04]), (1.0, [0.36, 0.15, 0.08])]))],
    );
    b.c(tone, brick_col, "input");
    let mortar_col = b.n("gradient_map", 540.0, 480.0, &[("gradient", grad(&[(0.0, [0.2, 0.19, 0.17]), (1.0, [0.3, 0.28, 0.25])]))]);
    b.c(dirt, mortar_col, "input");
    let mask = b.n("histogram_scan", 540.0, 640.0, &[("position", f(0.9)), ("contrast", f(0.9))]);
    b.c(chipped, mask, "input");
    let base = b.n("blend", 720.0, 400.0, &[]);
    b.c(brick_col, base, "foreground");
    b.c(mortar_col, base, "background");
    b.c(mask, base, "mask");
    let rough = b.n("levels", 720.0, 560.0, &[("out_low", f(0.7)), ("out_high", f(0.95))]);
    b.c(grunge, rough, "input");
    let metal = b.n("uniform", 720.0, 680.0, &[("mode", e(1)), ("color", col(0.0, 0.0, 0.0))]);
    b.outputs(
        940.0,
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

fn lava(b: &mut B) {
    let cells = b.n("voronoi", 0.0, 0.0, &[("scale", f(6.0)), ("style", e(2))]);
    let rock = b.n("clouds_2", 0.0, 160.0, &[("scale", f(12.0)), ("roughness", f(0.6))]);
    let fine = b.n("cracks", 0.0, 320.0, &[("scale", f(9.0)), ("width", f(1.0))]);
    let plates = b.n("levels", 180.0, 0.0, &[("in_low", f(0.02)), ("in_high", f(0.3))]);
    b.c(cells, plates, "input");
    let rough_h = b.n("blend", 360.0, 0.0, &[("mode", e(3)), ("opacity", f(0.4))]);
    b.c(rock, rough_h, "foreground");
    b.c(plates, rough_h, "background");
    let height = b.n("blend", 540.0, 0.0, &[("mode", e(2)), ("opacity", f(0.25))]);
    b.c(fine, height, "foreground");
    b.c(rough_h, height, "background");
    let normal = b.n("normal", 720.0, 0.0, &[("intensity", f(3.0))]);
    b.c(height, normal, "input");
    let ao = b.n("ambient_occlusion", 720.0, 120.0, &[("spread", f(0.06)), ("intensity", f(0.8))]);
    b.c(height, ao, "height");
    let seams = b.n("invert", 180.0, 160.0, &[]);
    b.c(plates, seams, "input");
    let seam_mask = b.n("levels", 360.0, 160.0, &[("in_low", f(0.6)), ("in_high", f(1.0))]);
    b.c(seams, seam_mask, "input");
    let glow = b.n("blur", 540.0, 160.0, &[("intensity", f(0.6))]);
    b.c(seam_mask, glow, "input");
    let emissive = b.n(
        "gradient_map",
        720.0,
        240.0,
        &[("gradient", grad(&[(0.0, [0.0, 0.0, 0.0]), (0.35, [0.35, 0.02, 0.0]), (0.7, [1.0, 0.28, 0.02]), (1.0, [1.0, 0.75, 0.3])]))],
    );
    b.c(glow, emissive, "input");
    let base =
        b.n("gradient_map", 720.0, 360.0, &[("gradient", grad(&[(0.0, [0.006, 0.005, 0.005]), (0.6, [0.018, 0.016, 0.016]), (1.0, [0.04, 0.036, 0.034])]))]);
    b.c(height, base, "input");
    let rough = b.n("levels", 720.0, 480.0, &[("out_low", f(0.72)), ("out_high", f(0.95))]);
    b.c(rock, rough, "input");
    let metal = b.n("uniform", 720.0, 600.0, &[("mode", e(1)), ("color", col(0.0, 0.0, 0.0))]);
    b.outputs(
        940.0,
        &[
            (base, "output", 0, "basecolor"),
            (normal, "output", 1, "normal"),
            (rough, "output", 2, "roughness"),
            (metal, "output", 3, "metallic"),
            (height, "output", 4, "height"),
            (ao, "output", 5, "ambientocclusion"),
            (emissive, "output", 6, "emissive"),
        ],
    );
}

fn metal_plates(b: &mut B) {
    let plates = b.n("tile_generator", 0.0, 0.0, &[("pattern", e(0)), ("x_amount", i(3)), ("y_amount", i(3)), ("scale", f(0.985))]);
    let inner = b.n("tile_generator", 0.0, 160.0, &[("pattern", e(0)), ("x_amount", i(3)), ("y_amount", i(3)), ("scale", f(0.88))]);
    let rivets = b.n("tile_generator", 0.0, 320.0, &[("pattern", e(2)), ("x_amount", i(27)), ("y_amount", i(27)), ("scale", f(0.42))]);
    let grunge = b.n("grunge_map_1", 0.0, 480.0, &[("scale", f(3.0)), ("contrast", f(0.45))]);
    let bevelled = b.n("bevel", 180.0, 0.0, &[("distance", f(0.02)), ("smoothing", f(0.5))]);
    b.c(plates, bevelled, "input");
    let band = b.n("blend", 180.0, 160.0, &[("mode", e(2))]);
    b.c(inner, band, "foreground");
    b.c(plates, band, "background");
    let rivet_band = b.n("blend", 360.0, 240.0, &[("mode", e(3))]);
    b.c(rivets, rivet_band, "foreground");
    b.c(band, rivet_band, "background");
    let plate_h = b.n("levels", 360.0, 0.0, &[("out_high", f(0.6))]);
    b.c(bevelled, plate_h, "input");
    let height = b.n("blend", 540.0, 0.0, &[("mode", e(1)), ("opacity", f(0.4))]);
    b.c(rivet_band, height, "foreground");
    b.c(plate_h, height, "background");
    let normal = b.n("normal", 720.0, 0.0, &[("intensity", f(2.5))]);
    b.c(height, normal, "input");
    let ao = b.n("ambient_occlusion", 720.0, 120.0, &[("spread", f(0.04)), ("intensity", f(0.7))]);
    b.c(height, ao, "height");
    let worn = b.n("edge_damage", 540.0, 160.0, &[("width", f(5.0)), ("amount", f(0.5)), ("scale", f(16.0))]);
    b.c(height, worn, "height");
    let scratches = b.n("scratches_generator", 360.0, 400.0, &[("count", i(70)), ("length", f(0.05)), ("width", f(0.4))]);
    let scratch_mask = b.n("levels", 540.0, 400.0, &[("in_low", f(0.5)), ("in_high", f(0.9)), ("out_high", f(0.5))]);
    b.c(scratches, scratch_mask, "input");
    let wear = b.n("blend", 720.0, 280.0, &[("mode", e(5))]);
    b.c(scratch_mask, wear, "foreground");
    b.cp(worn, "mask", wear, "background");
    let paint = b.n("gradient_map", 540.0, 520.0, &[("gradient", grad(&[(0.0, [0.012, 0.035, 0.08]), (1.0, [0.035, 0.085, 0.17])]))]);
    b.c(grunge, paint, "input");
    let steel = b.n("uniform", 540.0, 640.0, &[("color", col(0.56, 0.56, 0.57))]);
    let base = b.n("blend", 720.0, 480.0, &[]);
    b.c(steel, base, "foreground");
    b.c(paint, base, "background");
    b.c(wear, base, "mask");
    let rough_paint = b.n("uniform", 540.0, 760.0, &[("mode", e(1)), ("color", col(0.5, 0.5, 0.5))]);
    let rough_steel = b.n("uniform", 540.0, 880.0, &[("mode", e(1)), ("color", col(0.25, 0.25, 0.25))]);
    let rough = b.n("blend", 720.0, 640.0, &[]);
    b.c(rough_steel, rough, "foreground");
    b.c(rough_paint, rough, "background");
    b.c(wear, rough, "mask");
    b.outputs(
        940.0,
        &[
            (base, "output", 0, "basecolor"),
            (normal, "output", 1, "normal"),
            (rough, "output", 2, "roughness"),
            (wear, "output", 3, "metallic"),
            (height, "output", 4, "height"),
            (ao, "output", 5, "ambientocclusion"),
        ],
    );
}

fn wood(b: &mut B) {
    let planks = b.n(
        "brick_generator",
        0.0,
        0.0,
        &[("bricks_x", i(2)), ("bricks_y", i(7)), ("offset", f(0.37)), ("gap", f(0.012)), ("bevel", f(0.05)), ("height_random", f(0.06))],
    );
    let ids = b.n(
        "brick_generator",
        0.0,
        160.0,
        &[("bricks_x", i(2)), ("bricks_y", i(7)), ("offset", f(0.37)), ("gap", f(0.012)), ("bevel", f(0.0)), ("height_random", f(0.0)), ("lum_random", f(1.0))],
    );
    let grain = b.n("anisotropic_noise", 0.0, 320.0, &[("scale", f(3.0)), ("scale_y", f(110.0)), ("smoothness", f(0.25))]);
    let warp_src = b.n("perlin_noise", 0.0, 480.0, &[("scale", f(5.0))]);
    let warped = b.n("directional_warp", 180.0, 320.0, &[("intensity", f(5.0)), ("angle", f(0.25))]);
    b.c(grain, warped, "input");
    b.c(warp_src, warped, "intensity_input");
    let height = b.n("blend", 360.0, 80.0, &[("mode", e(3)), ("opacity", f(0.18))]);
    b.c(warped, height, "foreground");
    b.c(planks, height, "background");
    let normal = b.n("normal", 540.0, 0.0, &[("intensity", f(1.6))]);
    b.c(height, normal, "input");
    let ao = b.n("ambient_occlusion", 540.0, 120.0, &[("spread", f(0.03)), ("intensity", f(0.6))]);
    b.c(height, ao, "height");
    let tone = b.n("blend", 360.0, 240.0, &[("mode", e(9)), ("opacity", f(0.55))]);
    b.c(ids, tone, "foreground");
    b.c(warped, tone, "background");
    let wood_col = b.n(
        "gradient_map",
        540.0,
        240.0,
        &[("gradient", grad(&[(0.0, [0.045, 0.018, 0.007]), (0.4, [0.15, 0.065, 0.024]), (0.75, [0.3, 0.15, 0.06]), (1.0, [0.42, 0.24, 0.11])]))],
    );
    b.c(tone, wood_col, "input");
    let gaps = b.n("histogram_scan", 540.0, 400.0, &[("position", f(0.97)), ("contrast", f(0.95))]);
    b.c(planks, gaps, "input");
    let base = b.n("blend", 720.0, 300.0, &[("mode", e(3))]);
    b.c(gaps, base, "foreground");
    b.c(wood_col, base, "background");
    let rough = b.n("levels", 720.0, 440.0, &[("out_low", f(0.32)), ("out_high", f(0.55))]);
    b.c(warped, rough, "input");
    let metal = b.n("uniform", 720.0, 560.0, &[("mode", e(1)), ("color", col(0.0, 0.0, 0.0))]);
    b.outputs(
        940.0,
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

fn wicker(b: &mut B) {
    let weave = b.n("weave", 0.0, 0.0, &[("tiling", i(7)), ("width", f(0.92))]);
    let fibers = b.n("anisotropic_noise", 0.0, 160.0, &[("scale", f(2.0)), ("scale_y", f(160.0)), ("smoothness", f(0.2))]);
    let tint = b.n("clouds_2", 0.0, 320.0, &[("scale", f(10.0))]);
    let height = b.n("blend", 180.0, 0.0, &[("mode", e(3)), ("opacity", f(0.12))]);
    b.c(fibers, height, "foreground");
    b.c(weave, height, "background");
    let normal = b.n("normal", 360.0, 0.0, &[("intensity", f(2.2))]);
    b.c(height, normal, "input");
    let ao = b.n("ambient_occlusion", 360.0, 120.0, &[("spread", f(0.03)), ("intensity", f(0.9))]);
    b.c(height, ao, "height");
    let tone = b.n("blend", 180.0, 240.0, &[("mode", e(9)), ("opacity", f(0.35))]);
    b.c(tint, tone, "foreground");
    b.c(height, tone, "background");
    let base = b.n(
        "gradient_map",
        360.0,
        240.0,
        &[("gradient", grad(&[(0.0, [0.015, 0.008, 0.003]), (0.45, [0.14, 0.075, 0.025]), (0.8, [0.34, 0.21, 0.08]), (1.0, [0.46, 0.32, 0.14])]))],
    );
    b.c(tone, base, "input");
    let rough = b.n("levels", 360.0, 400.0, &[("out_low", f(0.6)), ("out_high", f(0.8))]);
    b.c(fibers, rough, "input");
    let metal = b.n("uniform", 360.0, 520.0, &[("mode", e(1)), ("color", col(0.0, 0.0, 0.0))]);
    b.outputs(
        600.0,
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

fn ground(b: &mut B) {
    let stones = b.n("stones", 0.0, 0.0, &[("scale", f(6.0)), ("gap", f(0.1)), ("bevel", f(0.45)), ("height_random", f(0.35)), ("detail", f(0.25))]);
    let dirt = b.n("dirt", 0.0, 160.0, &[("scale", f(6.0))]);
    let clouds = b.n("clouds_2", 0.0, 320.0, &[("scale", f(10.0))]);
    let hb = b.n("height_blend", 180.0, 80.0, &[("offset", f(0.22)), ("contrast", f(0.85))]);
    b.c(stones, hb, "top");
    b.c(dirt, hb, "bottom");
    let moss = b.n("moss", 360.0, 240.0, &[("amount", f(0.4)), ("scale", f(8.0)), ("height_bias", f(0.35))]);
    b.cp(hb, "height", moss, "height");
    let normal = b.n("normal", 360.0, 0.0, &[("intensity", f(2.0))]);
    b.cp(hb, "height", normal, "input");
    let ao = b.n("ambient_occlusion", 360.0, 120.0, &[("spread", f(0.07)), ("intensity", f(0.7))]);
    b.cp(hb, "height", ao, "height");
    let stone_h = b.n("blend", 180.0, 320.0, &[("mode", e(9)), ("opacity", f(0.6))]);
    b.c(clouds, stone_h, "foreground");
    b.c(stones, stone_h, "background");
    let stone_col =
        b.n("gradient_map", 360.0, 360.0, &[("gradient", grad(&[(0.0, [0.03, 0.03, 0.032]), (0.55, [0.11, 0.105, 0.1]), (1.0, [0.24, 0.22, 0.2])]))]);
    b.c(stone_h, stone_col, "input");
    let dirt_col = b.n("gradient_map", 360.0, 480.0, &[("gradient", grad(&[(0.0, [0.02, 0.013, 0.008]), (1.0, [0.1, 0.065, 0.04])]))]);
    b.c(dirt, dirt_col, "input");
    let moss_col = b.n("gradient_map", 360.0, 600.0, &[("gradient", grad(&[(0.0, [0.02, 0.04, 0.008]), (1.0, [0.07, 0.14, 0.02])]))]);
    b.c(clouds, moss_col, "input");
    let ground_col = b.n("blend", 540.0, 360.0, &[]);
    b.c(dirt_col, ground_col, "foreground");
    b.c(stone_col, ground_col, "background");
    b.cp(hb, "blend_mask", ground_col, "mask");
    let base = b.n("blend", 720.0, 360.0, &[]);
    b.c(moss_col, base, "foreground");
    b.c(ground_col, base, "background");
    b.c(moss, base, "mask");
    let rough = b.n("levels", 720.0, 480.0, &[("out_low", f(0.7)), ("out_high", f(0.95))]);
    b.c(clouds, rough, "input");
    let metal = b.n("uniform", 720.0, 600.0, &[("mode", e(1)), ("color", col(0.0, 0.0, 0.0))]);
    b.outputs(
        940.0,
        &[
            (base, "output", 0, "basecolor"),
            (normal, "output", 1, "normal"),
            (rough, "output", 2, "roughness"),
            (metal, "output", 3, "metallic"),
            (hb, "height", 4, "height"),
            (ao, "output", 5, "ambientocclusion"),
        ],
    );
}

fn stone_tiles(b: &mut B) {
    let layout = b.n(
        "brick_generator",
        0.0,
        0.0,
        &[("bricks_x", i(3)), ("bricks_y", i(3)), ("offset", f(0.5)), ("gap", f(0.025)), ("bevel", f(0.12)), ("height_random", f(0.2))],
    );
    let ids = b.n(
        "brick_generator",
        0.0,
        160.0,
        &[("bricks_x", i(3)), ("bricks_y", i(3)), ("offset", f(0.5)), ("gap", f(0.025)), ("bevel", f(0.0)), ("height_random", f(0.0)), ("lum_random", f(1.0))],
    );
    let cracks = b.n("cracks", 0.0, 320.0, &[("scale", f(4.0)), ("width", f(1.0))]);
    let clouds = b.n("clouds_2", 0.0, 480.0, &[("scale", f(12.0))]);
    let chipped = b.n("edge_damage", 180.0, 0.0, &[("width", f(10.0)), ("amount", f(0.5)), ("scale", f(10.0))]);
    b.c(layout, chipped, "height");
    let cracked = b.n("blend", 360.0, 0.0, &[("mode", e(2)), ("opacity", f(0.35))]);
    b.c(cracks, cracked, "foreground");
    b.c(chipped, cracked, "background");
    let height = b.n("blend", 540.0, 0.0, &[("mode", e(3)), ("opacity", f(0.2))]);
    b.c(clouds, height, "foreground");
    b.c(cracked, height, "background");
    let normal = b.n("normal", 720.0, 0.0, &[("intensity", f(2.0))]);
    b.c(height, normal, "input");
    let ao = b.n("ambient_occlusion", 720.0, 120.0, &[("spread", f(0.05)), ("intensity", f(0.7))]);
    b.c(height, ao, "height");
    let tone = b.n("blend", 360.0, 240.0, &[("mode", e(9)), ("opacity", f(0.45))]);
    b.c(clouds, tone, "foreground");
    b.c(ids, tone, "background");
    let stone_col =
        b.n("gradient_map", 540.0, 240.0, &[("gradient", grad(&[(0.0, [0.09, 0.08, 0.065]), (0.5, [0.2, 0.18, 0.14]), (1.0, [0.34, 0.31, 0.25])]))]);
    b.c(tone, stone_col, "input");
    let mask = b.n("histogram_scan", 540.0, 400.0, &[("position", f(0.9)), ("contrast", f(0.9))]);
    b.c(cracked, mask, "input");
    let grout = b.n("uniform", 540.0, 520.0, &[("color", col(0.04, 0.035, 0.03))]);
    let base = b.n("blend", 720.0, 320.0, &[]);
    b.c(stone_col, base, "foreground");
    b.c(grout, base, "background");
    b.c(mask, base, "mask");
    let rough = b.n("levels", 720.0, 460.0, &[("out_low", f(0.6)), ("out_high", f(0.9))]);
    b.c(clouds, rough, "input");
    let metal = b.n("uniform", 720.0, 580.0, &[("mode", e(1)), ("color", col(0.0, 0.0, 0.0))]);
    b.outputs(
        940.0,
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

fn marble(b: &mut B) {
    let veins = b.n("marble", 0.0, 0.0, &[("scale", f(3.0)), ("turbulence", f(5.0)), ("sharpness", f(14.0)), ("direction", e(2))]);
    let fine = b.n("ridged_fractal", 0.0, 160.0, &[("scale", f(6.0)), ("octaves", i(6)), ("roughness", f(0.55)), ("sharpness", f(6.0))]);
    let flow = b.n("fractal_sum_base", 0.0, 320.0, &[("scale", f(3.0)), ("octaves", i(6))]);
    let fine_mask = b.n("levels", 180.0, 160.0, &[("in_low", f(0.55)), ("in_high", f(1.0)), ("out_high", f(0.45))]);
    b.c(fine, fine_mask, "input");
    let vein_mask = b.n("blend", 360.0, 80.0, &[("mode", e(5))]);
    b.c(fine_mask, vein_mask, "foreground");
    b.c(veins, vein_mask, "background");
    let normal = b.n("normal", 720.0, 0.0, &[("intensity", f(0.25))]);
    b.c(flow, normal, "input");
    let ao = b.n("ambient_occlusion", 720.0, 120.0, &[("spread", f(0.02)), ("intensity", f(0.2))]);
    b.c(flow, ao, "height");
    let stone = b.n("gradient_map", 540.0, 240.0, &[("gradient", grad(&[(0.0, [0.8, 0.78, 0.74]), (0.5, [0.52, 0.48, 0.42]), (1.0, [0.2, 0.17, 0.13])]))]);
    b.c(vein_mask, stone, "input");
    let cloudy = b.n("blend", 720.0, 240.0, &[("mode", e(3)), ("opacity", f(0.3))]);
    b.c(flow, cloudy, "foreground");
    b.c(stone, cloudy, "background");
    let rough = b.n("uniform", 720.0, 400.0, &[("mode", e(1)), ("color", col(0.12, 0.12, 0.12))]);
    let metal = b.n("uniform", 720.0, 520.0, &[("mode", e(1)), ("color", col(0.0, 0.0, 0.0))]);
    b.outputs(
        940.0,
        &[
            (cloudy, "output", 0, "basecolor"),
            (normal, "output", 1, "normal"),
            (rough, "output", 2, "roughness"),
            (metal, "output", 3, "metallic"),
            (flow, "output", 4, "height"),
            (ao, "output", 5, "ambientocclusion"),
        ],
    );
}

fn rusty_metal(b: &mut B) {
    let rust_mask = b.n("grunge_map_1", 0.0, 0.0, &[("scale", f(3.0)), ("contrast", f(0.7))]);
    let scan = b.n("histogram_scan", 200.0, 0.0, &[("position", f(0.4)), ("contrast", f(0.8))]);
    b.c(rust_mask, scan, "input");
    let fine = b.n("clouds_3", 0.0, 180.0, &[("scale", f(12.0))]);
    let brushed = b.n("anisotropic_noise", 0.0, 360.0, &[("scale", f(2.0)), ("scale_y", f(256.0))]);
    let h = b.n("blend", 400.0, 90.0, &[("mode", e(3)), ("opacity", f(0.35))]);
    b.c(fine, h, "foreground");
    b.c(scan, h, "background");
    let normal = b.n("normal", 600.0, 0.0, &[("intensity", f(1.0))]);
    b.c(h, normal, "input");
    let rust_col =
        b.n("gradient_map", 400.0, 240.0, &[("gradient", grad(&[(0.0, [0.03, 0.01, 0.004]), (0.5, [0.11, 0.035, 0.01]), (1.0, [0.22, 0.08, 0.02])]))]);
    b.c(fine, rust_col, "input");
    let metal_col = b.n("uniform", 400.0, 380.0, &[("color", col(0.52, 0.53, 0.55))]);
    let base = b.n("blend", 600.0, 300.0, &[]);
    b.c(rust_col, base, "foreground");
    b.c(metal_col, base, "background");
    b.c(scan, base, "mask");
    let metal = b.n("invert", 600.0, 440.0, &[]);
    b.c(scan, metal, "input");
    let rough_metal = b.n("levels", 400.0, 520.0, &[("out_low", f(0.32)), ("out_high", f(0.48))]);
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

/// The package the app opens with: the brick wall sample.
pub fn sample_package() -> Package {
    let mut p = Package::new("Untitled package");
    p.graphs.push(template("bricks", "Brick_wall", 10));
    p
}
