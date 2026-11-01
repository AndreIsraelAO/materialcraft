# File format parity

> **Last reviewed:** 2026-11-02 · **Last updated:** 2026-11-02 · **Change:** major (first version) · **Target:** the professional node-based material workflow

MaterialCraft does not read or write the reference app's proprietary package or archive formats
(a non-goal of the clean room). It has its own package format and reads and writes the open image
formats material artists exchange.

| Format | Read | Write | Fidelity | Tests |
|---|---|---|---|---|
| `.mcraft` package (zip: `package.json` + `resources/`) | Yes | Yes | Lossless (graphs, parameters, frames, comments, pins, exposed inputs, embedded bitmaps) | `formats::tests::package_round_trip`, engine `package_save_and_open_round_trip` |
| PNG | Yes (8/16-bit, gray and color) | Yes (8/16-bit) | 16-bit gray round-trips within 1/65535 | `formats::tests::export_every_format` |
| JPEG | Yes | Yes (8-bit) | Lossy | same |
| TGA | Yes | Yes (8-bit) | Lossless | same |
| TIFF | Yes | Yes (8/16-bit) | Lossless | same |
| BMP | Yes | Yes (8-bit) | Lossless | same |
| WEBP | Yes | Yes (8-bit lossless) | Lossless | same |
| OpenEXR | Yes | Yes (32-bit float RGB) | Float | same |
| Radiance HDR | Yes | Yes | Float (RGBE) | same |
| SVG, fonts (resources) | No | — | — | gap |
| Meshes for the 3D view (OBJ, glTF, FBX) | No | — | — | gap |
| Published material archives | — | No | — | gap |

Exports write base color, emissive and specular through the sRGB curve and data maps (normal,
roughness, metallic, height, ambient occlusion, opacity) linear.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-02 | major | First version |
