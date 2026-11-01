//! # materialcraft-nodes
//!
//! The node library, L1: every node's definition (ports, parameters, library category) and its
//! CPU implementation. A cook is a pure function of its inputs, resolved parameters, size, seed
//! and tiling ([`Ctx`]), so every node is testable on its own and identical in the app, the CLI,
//! the MCP server and the browser. Noises tile seamlessly.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod atomic;
pub mod catalog;
pub mod filters;
pub mod function;
pub mod generators;
pub mod material;
pub mod noise;
pub mod ops;
pub mod patterns;

pub use catalog::{CookResult, Ctx, NodeDef, ParamDef, PortDef, PortKind, catalog, categories, find, resolve_params};

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use materialcraft_image::{Image, Kind};

    use super::*;

    fn cook(def: &NodeDef, inputs: &[Option<Image>], size: u32) -> CookResult {
        let params = resolve_params(def, &BTreeMap::new());
        let no_res = |_: &str| None;
        let vars = BTreeMap::new();
        let ctx = Ctx { w: size, h: size, seed: 7, tile: true, inputs, params: &params, resource: &no_res, vars: &vars };
        (def.cook)(&ctx)
    }

    /// Every node cooks with no inputs and with hostile-sized inputs, returns as many images as
    /// it has outputs, at the requested size, with finite values.
    #[test]
    fn every_node_cooks() {
        let ids: Vec<&str> = catalog().iter().map(|d| d.id).collect();
        let mut seen = std::collections::BTreeSet::new();
        for id in &ids {
            assert!(seen.insert(*id), "duplicate node id {id}");
        }
        let odd = Image::generate(5, 3, Kind::Color, |x, y| [x as f32 / 4.0, y as f32 / 2.0, 0.5, 1.0]);
        for def in catalog() {
            if def.id == "graph_instance" {
                continue;
            }
            for inputs in [vec![None; def.inputs.len()], vec![Some(odd.clone()); def.inputs.len()]] {
                let out = cook(def, &inputs, 32).unwrap_or_else(|e| panic!("{}: {e}", def.id));
                assert_eq!(out.len(), def.outputs.len().max(1), "{}", def.id);
                for img in &out {
                    assert_eq!((img.w, img.h), (32, 32), "{}", def.id);
                    assert!(img.data.iter().all(|v| v.is_finite()), "{} produced a non-finite value", def.id);
                }
            }
        }
        assert!(catalog().len() >= 100, "{} nodes", catalog().len());
    }

    #[test]
    fn noises_tile() {
        for id in ["perlin_noise", "clouds_1", "cells_1", "voronoi", "brick_generator", "tile_generator", "hexagons"] {
            let def = find(id).unwrap();
            let img = &cook(def, &[], 64).unwrap()[0];
            // The jump across the wrap is no larger than typical neighbouring differences.
            let mut seam = 0.0f32;
            let mut inner = 0.0f32;
            for y in 0..64 {
                seam += (img.get(63, y, false)[0] - img.get(0, y, false)[0]).abs();
                inner += (img.get(31, y, false)[0] - img.get(32, y, false)[0]).abs();
            }
            assert!(seam <= inner * 3.0 + 0.5, "{id}: seam {seam} vs inner {inner}");
        }
    }

    #[test]
    fn blend_and_levels_math() {
        let fg = Image::filled(4, 4, Kind::Gray, [0.25; 4]);
        let bg = Image::filled(4, 4, Kind::Gray, [0.5; 4]);
        let def = find("blend").unwrap();
        let mut params = resolve_params(def, &BTreeMap::new());
        params.insert("mode".into(), materialcraft_core::Value::Enum(3));
        let no_res = |_: &str| None;
        let vars = BTreeMap::new();
        let inputs = [Some(fg), Some(bg), None];
        let ctx = Ctx { w: 4, h: 4, seed: 0, tile: true, inputs: &inputs, params: &params, resource: &no_res, vars: &vars };
        let out = (def.cook)(&ctx).unwrap();
        assert!((out[0].data[0] - 0.125).abs() < 1e-6);
        assert!((atomic::levels_fn(0.5, 0.0, 0.5, 1.0, 0.0, 1.0) - 0.5).abs() < 1e-5);
        assert_eq!(atomic::levels_fn(0.2, 0.4, 0.5, 0.8, 0.0, 1.0), 0.0);
    }
}
