// SPDX-FileCopyrightText: 2026 aesc silicon
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;

use anyhow::{anyhow, Result};

pub mod gf180mcu;
pub mod ihp_sg13;

// Algorithm parameter structs

/// Parameters for the square (checkerboard) fill algorithm.
pub struct SquareParams {
    pub min_width: f64,
    pub max_width: f64,
    pub min_space: f64,
    pub max_space: f64,
    /// Whether fill squares may be clipped at tile edges (default true).
    pub clipping: bool,
    /// Lattice origin offset from the chip bbox corner (in µm).  Use distinct
    /// offsets per layer so consecutive layers do not replicate the pattern.
    pub origin_um: (f64, f64),
}

/// Orientation of fill tracks (rows vs columns).
pub enum TrackOrientation {
    Horizontal,
    Vertical,
}

/// Parameters for the track-based fill algorithm.
///
/// Tracks are parallel stripes whose pitch is determined by `min_width`,
/// `min_space`, and `gaps` (the routing track pitch to align to).
pub struct TrackParams {
    pub min_width: f64,
    pub max_width: f64,
    pub min_space: f64,
    pub max_space: f64,
    pub orientation: TrackOrientation,
    /// Routing track pitch to snap fill stripes to (in µm).
    pub gaps: f64,
    /// Standard-cell row height used to align fill (in µm).
    pub cell_height: f64,
    /// When `true`, allow fill up to the tile edge even if `max_space` is exceeded.
    pub aggressive_fill: bool,
    /// Fractions of `n_max = floor(max_width / gaps)` that determine the
    /// perpendicular fill sizes (across routing tracks).  Each entry `f` produces
    /// size `floor(f * n_max) * gaps`, snapped to the manufacturing grid.
    /// `min_width` is always appended as the final perpendicular size.
    /// Example: `&[1.0, 0.6, 0.5, 0.4, 0.3]` for M2/M4 (gaps=0.48 µm, max=5 µm).
    pub pass_fracs: &'static [f64],
    /// Explicit free-direction sizes in µm (along the routing direction).
    /// Tried from largest to smallest; each is combined with every perpendicular
    /// size from `pass_fracs`.  Values are snapped to the manufacturing grid
    /// and clamped to `[min_width, max_width]`.
    /// Example: `&[5.0, 4.0, 3.0, 2.0, 1.5, 1.0]`.
    pub free_heights_um: &'static [f64],
}

/// Parameters for the overlap fill algorithm used on GatPoly.
///
/// Fill rectangles extend `min_extension` beyond existing geometry to keep
/// gate-poly fill electrically equivalent to drawn gates.
pub struct OverlapParams {
    pub min_width: f64,
    pub max_width: f64,
    /// Minimum extension of fill past existing geometry (in µm).
    pub min_extension: f64,
    pub min_space: f64,
    /// Name of the PDK layer whose placed fill shapes are used as placement
    /// anchors (e.g. `"Activ"` for GatPoly overlap fill).
    pub ref_layer: &'static str,
    /// `true`: the fill is the reference rect grown by `min_extension` on all
    /// four sides, fully covering it (GF180 dummy Poly2 over dummy COMP).
    /// `false`: a stripe `min_extension` wider than the reference but shorter
    /// than it, with the height chosen from the density budget (IHP GatPoly).
    pub cover: bool,
}

// Algorithm enum

/// Fill strategy for a single PDK layer.
///
/// Multiple algorithms may be listed in [`PdkLayer::algorithms`]; they are
/// applied in order, each operating on the space left by the previous pass.
pub enum FillAlgorithm {
    Square(SquareParams),
    Track(TrackParams),
    Overlap(OverlapParams),
}

// Layer and process constants

/// PDK-level description of a single physical layer.
pub struct PdkLayer {
    /// GDS layer number.
    pub gds_layer: i16,
    /// GDS datatype for drawing shapes (typically 0).
    pub drawing_datatype: i16,
    /// GDS datatype written for generated fill shapes.
    pub fill_datatype: i16,
    /// GDS datatype of no-fill keep-out markers on the same layer, if any.
    pub nofill_datatype: Option<i16>,
    /// Maximum cell hierarchy depth traversed when collecting shapes.
    pub max_depth: u32,
    /// Fill algorithms applied in order (e.g. Track first, then Square for remainder).
    pub algorithms: Vec<FillAlgorithm>,
    /// Default target fill density in percent.
    pub default_density: f64,
    /// Default acceptable deviation from the target in percent.
    pub default_deviation: f64,
    /// Tile width used during fill for this layer, in micrometres.
    pub tile_width_um: f64,
    /// Merge drawing polygons before density calculation.
    /// Required for layers where IO-filler cells produce intentionally overlapping
    /// or self-touching shapes (Metal3 and above).
    pub merge_for_density: bool,
    /// Maximum merge window size in µm used by the tiled merge in density
    /// calculation.  `None` means the density tile itself is the window (safe
    /// for sparse layers like TopMetal).  Set to a small value (e.g. 50 µm)
    /// for dense layers (Activ, GatPoly, Metal1) to bound peak memory.
    pub merge_window_um: Option<f64>,
}

/// Process-wide constants: layer table and global geometry parameters.
pub struct PdkConstants {
    /// All fillable layers keyed by their canonical name (e.g. `"Metal1"`).
    pub layers: HashMap<&'static str, PdkLayer>,
    /// Size of one database unit in micrometres (0.001 for IHP = 1 DBU -> 1 nm).
    pub db_unit_um: f64,
    /// Default tile width in micrometres.
    pub tile_width_um: f64,
    /// GDS (layer, datatype) that bounds where fill shapes may be placed
    /// (prBoundary, (39,0) for IHP).
    pub fill_boundary_layer: Option<(i16, i16)>,
    /// GDS (layer, datatype) whose filled exterior defines the reference area
    /// for density calculation (Edge.Seal, (39,4) for IHP).
    pub density_boundary_layer: Option<(i16, i16)>,
    /// Manufacturing grid in database units (e.g. 5 DBU = 5 nm for IHP).
    /// Fill shape sizes and spaces are snapped to multiples of `2 * grid_dbu`
    /// so that `half = size/2` always lands on a grid point.
    pub grid_dbu: f64,
}

impl PdkConstants {
    /// Return constants for the named process, or `None` if unknown.
    pub fn for_process(process: &str) -> Option<Self> {
        match process {
            "ihp-sg13g2"     => Some(ihp_sg13::sg13g2()),
            "ihp-sg13cmos5l" => Some(ihp_sg13::sg13cmos5l()),
            _ => process.strip_prefix("gf180mcu")
                .and_then(|v| v.chars().next())
                .filter(|_| process.len() == "gf180mcuX".len())
                .and_then(gf180mcu::for_variant),
        }
    }

    /// Tile width converted to database units.
    pub fn tile_width_dbu(&self) -> f64 {
        self.tile_width_um / self.db_unit_um
    }

    /// Check the layer table for values that would produce nonsensical geometry.
    ///
    /// Tile widths, track pitches and fill sizes are divisors and loop bounds
    /// deep inside the fill algorithms, where a zero or negative entry becomes
    /// an infinite count and then an unbounded allocation.  Catching it here
    /// costs nothing and names the offending layer.
    pub fn validate(&self, process: &str) -> Result<()> {
        fn positive(v: f64) -> bool {
            v.is_finite() && v > 0.0
        }
        let bad = |what: &str, value: f64| -> anyhow::Error {
            anyhow!("PDK '{}': {} must be positive and finite, got {}", process, what, value)
        };
        if !positive(self.db_unit_um) {
            return Err(bad("database unit", self.db_unit_um));
        }
        if !positive(self.tile_width_um) {
            return Err(bad("tile width", self.tile_width_um));
        }
        if !positive(self.grid_dbu) {
            return Err(bad("manufacturing grid", self.grid_dbu));
        }

        for (name, layer) in &self.layers {
            let bad = |what: &str, value: f64| -> anyhow::Error {
                anyhow!("PDK '{}', layer '{}': {} must be positive and finite, got {}",
                    process, name, what, value)
            };
            if !positive(layer.tile_width_um) {
                return Err(bad("tile width", layer.tile_width_um));
            }
            if let Some(w) = layer.merge_window_um
                && !positive(w) {
                    return Err(bad("merge window", w));
                }
            if layer.algorithms.is_empty() {
                return Err(anyhow!("PDK '{}', layer '{}': no fill algorithm", process, name));
            }
            for algorithm in &layer.algorithms {
                let (min_width, max_width) = match algorithm {
                    FillAlgorithm::Square(p) => (p.min_width, p.max_width),
                    FillAlgorithm::Overlap(p) => (p.min_width, p.max_width),
                    FillAlgorithm::Track(p) => {
                        if !positive(p.gaps) {
                            return Err(bad("track pitch", p.gaps));
                        }
                        if !positive(p.cell_height) {
                            return Err(bad("cell height", p.cell_height));
                        }
                        (p.min_width, p.max_width)
                    }
                };
                if !positive(min_width) {
                    return Err(bad("minimum fill width", min_width));
                }
                if !max_width.is_finite() || max_width < min_width {
                    return Err(anyhow!(
                        "PDK '{}', layer '{}': maximum fill width {} is below the minimum {}",
                        process, name, max_width, min_width));
                }
            }
        }
        Ok(())
    }
}
