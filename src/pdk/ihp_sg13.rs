// SPDX-FileCopyrightText: 2026 aesc silicon
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Layer table and keep-out polygon generation for the IHP SG13 process family.
//!
//! Both ihp-sg13g2 and ihp-sg13cmos5l share the same keepout rules; they differ
//! only in the metal stack (SG13G2 has Metal5 and TopMetal2 on top).

use std::collections::HashMap;

use geo::{BoundingRect, Polygon, Rect, coord};

use super::{
    FillAlgorithm, OverlapParams, PdkConstants, PdkLayer, SquareParams, TrackOrientation,
    TrackParams,
};
use crate::{offset_polygons, LayerMap};

// Fill algorithm helpers

fn square(min_width: f64, max_width: f64, min_space: f64, max_space: f64) -> FillAlgorithm {
    FillAlgorithm::Square(SquareParams {
        min_width, max_width, min_space, max_space, clipping: true, origin_um: (0.0, 0.0),
    })
}

fn square_noclip(min_width: f64, max_width: f64, min_space: f64, max_space: f64) -> FillAlgorithm {
    FillAlgorithm::Square(SquareParams {
        min_width, max_width, min_space, max_space, clipping: false, origin_um: (0.0, 0.0),
    })
}

#[allow(clippy::too_many_arguments)]
fn track_v(
    min_width: f64, max_width: f64,
    min_space: f64, max_space: f64,
    gaps: f64, cell_height: f64,
    pass_fracs: &'static [f64],
    free_heights_um: &'static [f64],
) -> FillAlgorithm {
    FillAlgorithm::Track(TrackParams {
        min_width, max_width, min_space, max_space,
        orientation: TrackOrientation::Vertical,
        gaps, cell_height,
        aggressive_fill: false,
        pass_fracs,
        free_heights_um,
    })
}

#[allow(clippy::too_many_arguments)]
fn track_h(
    min_width: f64, max_width: f64,
    min_space: f64, max_space: f64,
    gaps: f64, cell_height: f64,
    pass_fracs: &'static [f64],
    free_heights_um: &'static [f64],
) -> FillAlgorithm {
    FillAlgorithm::Track(TrackParams {
        min_width, max_width, min_space, max_space,
        orientation: TrackOrientation::Horizontal,
        gaps, cell_height,
        aggressive_fill: false,
        pass_fracs,
        free_heights_um,
    })
}

fn overlap(min_width: f64, max_width: f64, min_extension: f64, min_space: f64) -> FillAlgorithm {
    FillAlgorithm::Overlap(OverlapParams {
        min_width, max_width, min_extension, min_space, ref_layer: "Activ", cover: false,
    })
}

macro_rules! ihp_layer {
    ($gds_layer:expr, [$($alg:expr),+ $(,)?], $density:expr, $deviation:expr, $tile_um:expr) => {
        PdkLayer {
            gds_layer:         $gds_layer,
            drawing_datatype:  0,
            fill_datatype:     22,
            nofill_datatype:   Some(23),
            max_depth:         10,
            algorithms:        vec![$($alg),+],
            default_density:   $density,
            default_deviation: $deviation,
            tile_width_um:     $tile_um,
            merge_for_density: true,
            merge_window_um:   None,
        }
    };
    // merge = <window_um>: tiled merge with the given sub-window size.
    // Use for dense layers with many small structures (Activ, GatPoly, Metal1).
    ($gds_layer:expr, [$($alg:expr),+ $(,)?], $density:expr, $deviation:expr, $tile_um:expr, merge = $window:expr) => {
        PdkLayer {
            merge_window_um:   Some($window),
            ..ihp_layer!($gds_layer, [$($alg),+], $density, $deviation, $tile_um)
        }
    };
}

// IHP SG13G2

#[rustfmt::skip]
pub fn sg13g2() -> PdkConstants {
    let mut layers = HashMap::new();

    layers.insert("Activ", ihp_layer!(
        1, [square_noclip(1.08, 4.63, 1.8, 10.0)],
        50.0, 5.0, 100.0, merge = 50.0));

    layers.insert("GatPoly", ihp_layer!(
        5, [overlap(0.7, 5.0, 0.18, 0.8)],
        30.0, 3.0, 100.0, merge = 50.0));

    layers.insert("Metal1", ihp_layer!(
        8, [square(1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0, merge = 50.0));

    layers.insert("Metal2", ihp_layer!(
        10, [track_v(1.0, 5.0, 0.42, 10.0, 0.48, 1.44,
                     &[1.0, 0.6, 0.5, 0.4, 0.3], &[5.0, 4.0, 3.0, 2.0, 1.5, 1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0));

    layers.insert("Metal3", ihp_layer!(
        30, [track_h(1.0, 5.0, 0.42, 10.0, 0.42, 1.26,
                     &[1.0, 0.6, 0.5, 0.4, 0.3], &[5.0, 4.0, 3.0, 2.0, 1.5, 1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0));

    layers.insert("Metal4", ihp_layer!(
        50, [track_v(1.0, 5.0, 0.42, 10.0, 0.48, 1.44,
                     &[1.0, 0.6, 0.5, 0.4, 0.3], &[5.0, 4.0, 3.0, 2.0, 1.5, 1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0));

    layers.insert("Metal5", ihp_layer!(
        67, [track_h(1.0, 5.0, 0.42, 10.0, 0.42, 1.26,
                     &[1.0, 0.6, 0.5, 0.4, 0.3], &[5.0, 4.0, 3.0, 2.0, 1.5, 1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0));

    layers.insert("TopMetal1", ihp_layer!(
        126, [square(5.0, 10.0, 3.0, 10.0)],
        40.0, 10.0, 800.0));

    layers.insert("TopMetal2", ihp_layer!(
        134, [square(5.0, 10.0, 3.0, 10.0)],
        40.0, 10.0, 800.0));

    PdkConstants {
        layers, db_unit_um: 0.001, tile_width_um: 800.0,
        fill_boundary_layer: Some((39, 0)), density_boundary_layer: Some((39, 4)),
        grid_dbu: 5.0,
    }
}

// IHP SG13CMOS5L

#[rustfmt::skip]
pub fn sg13cmos5l() -> PdkConstants {
    let mut layers = HashMap::new();

    layers.insert("Activ", ihp_layer!(
        1, [square_noclip(1.08, 4.63, 1.8, 10.0)],
        50.0, 5.0, 100.0, merge = 50.0));

    layers.insert("GatPoly", ihp_layer!(
        5, [overlap(0.7, 5.0, 0.18, 0.8)],
        30.0, 3.0, 100.0, merge = 50.0));

    layers.insert("Metal1", ihp_layer!(
        8, [square(1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0, merge = 50.0));

    layers.insert("Metal2", ihp_layer!(
        10, [track_v(1.0, 5.0, 0.42, 10.0, 0.48, 1.44,
                     &[1.0, 0.6, 0.5, 0.4, 0.3], &[5.0, 4.0, 3.0, 2.0, 1.5, 1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0));

    layers.insert("Metal3", ihp_layer!(
        30, [track_h(1.0, 5.0, 0.42, 10.0, 0.42, 1.26,
                     &[1.0, 0.6, 0.5, 0.4, 0.3], &[5.0, 4.0, 3.0, 2.0, 1.5, 1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0));

    layers.insert("Metal4", ihp_layer!(
        50, [track_v(1.0, 5.0, 0.42, 10.0, 0.48, 1.44,
                     &[1.0, 0.6, 0.5, 0.4, 0.3], &[5.0, 4.0, 3.0, 2.0, 1.5, 1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0));

    layers.insert("TopMetal1", ihp_layer!(
        126, [square(5.0, 10.0, 3.0, 10.0)],
        40.0, 10.0, 800.0));

    PdkConstants {
        layers, db_unit_um: 0.001, tile_width_um: 800.0,
        fill_boundary_layer: Some((39, 0)), density_boundary_layer: Some((39, 4)),
        grid_dbu: 5.0,
    }
}

// Keepout spacing constants (µm)

const ACTIV_SPACE_UM:    f64 = 0.42;
const METAL_SPACE_UM:    f64 = 0.42;
const TOPMETAL_SPACE_UM: f64 = 3.0;

/// Minimum space from GatPoly **filler** to any of the layers listed in the
/// "GatPoly:filler" DRC rule (Activ, GatPoly, Cont, pSD, nSD:block, SalBlock,
/// NWell, nBuLay, Trans).
const GATPOLY_FILL_SPACE_UM: f64 = 1.10;

// Activ layer (used as keepout reference for GatPoly filler)
const ACTIV_LAYER: i16 = 1;    const ACTIV_DATATYPE: i16 = 0;

// Activ.filler keepout -- additional layers
const GATPOLY_LAYER: i16 = 5;  const GATPOLY_DATATYPE: i16 = 0;
const CONT_LAYER:    i16 = 6;  const CONT_DATATYPE:    i16 = 0;
const NSD_BLK_LAYER: i16 = 7;  const NSD_BLK_DATATYPE: i16 = 21;
const NWELL_LAYER:   i16 = 31; const NWELL_DATATYPE:   i16 = 0;
const NBULAY_LAYER:  i16 = 32; const NBULAY_DATATYPE:  i16 = 0;
const PWBLK_LAYER:   i16 = 46; const PWBLK_DATATYPE:   i16 = 21;
const PSD_LAYER:     i16 = 14; const PSD_DATATYPE:     i16 = 0;
const SALBLOCK_LAYER: i16 = 28; const SALBLOCK_DATATYPE: i16 = 0;

const ACTIV_GATPOLY_SPACE_UM: f64 = 1.10;
const ACTIV_CONT_SPACE_UM:    f64 = 1.10;
const ACTIV_NWELL_SPACE_UM:   f64 = 1.00;
const ACTIV_NBULAY_SPACE_UM:  f64 = 1.00;
const ACTIV_PWBLK_SPACE_UM:   f64 = 1.50;

const TRANS_LAYER:    i16 = 26;
const TRANS_DATATYPE: i16 = 0;
const TRANS_ACTIV_SPACE_UM: f64 = 1.0;
const TRANS_METAL_SPACE_UM: f64 = 1.0;
const TRANS_TOPMETAL_SPACE_UM: f64 = 4.9;

// Core boundary layer (prBoundary shapes marking the digital standard-cell area)
const CORE_LAYER:      i16 = 189;
const CORE_DATATYPE:   i16 = 4;
const CELL_HEIGHT_UM:  f64 = 3.78;

// Public entry point

/// Return all `(layer, datatype)` pairs that the IHP keepout rules read from
/// the layout.  These must be present in the `LayerMap` before calling
/// [`build_keepout`].
pub fn needed_layers() -> &'static [(i16, i16)] {
    &[
        (ACTIV_LAYER,    ACTIV_DATATYPE),
        (GATPOLY_LAYER,  GATPOLY_DATATYPE),
        (CONT_LAYER,     CONT_DATATYPE),
        (NSD_BLK_LAYER,  NSD_BLK_DATATYPE),
        (PSD_LAYER,      PSD_DATATYPE),
        (SALBLOCK_LAYER, SALBLOCK_DATATYPE),
        (NWELL_LAYER,    NWELL_DATATYPE),
        (NBULAY_LAYER,   NBULAY_DATATYPE),
        (PWBLK_LAYER,    PWBLK_DATATYPE),
        (TRANS_LAYER,    TRANS_DATATYPE),
        (CORE_LAYER,     CORE_DATATYPE),
    ]
}

/// Compute the digital core area bounding rectangle from prBoundary shapes.
///
/// Scans all shapes on the core boundary layer `(189, 4)` whose height equals
/// one standard-cell row (~ 3.78 µm) and returns a single bounding-rectangle
/// polygon covering the entire digital core.  Returns an empty vec if no
/// qualifying shapes are found.
pub fn compute_core_area(map: &LayerMap, dbu: f64) -> Vec<geo::Polygon<f64>> {
    let cell_h_dbu = CELL_HEIGHT_UM / dbu;
    let tol = cell_h_dbu * 0.02;
    let raw = map.polygons(CORE_LAYER, CORE_DATATYPE);
    let mut cx0 = f64::INFINITY;
    let mut cy0 = f64::INFINITY;
    let mut cx1 = f64::NEG_INFINITY;
    let mut cy1 = f64::NEG_INFINITY;
    for p in raw {
        if let Some(b) = p.bounding_rect()
            && (b.max().y - b.min().y - cell_h_dbu).abs() < tol {
                cx0 = cx0.min(b.min().x);
                cy0 = cy0.min(b.min().y);
                cx1 = cx1.max(b.max().x);
                cy1 = cy1.max(b.max().y);
            }
    }
    if cx1 > cx0 && cy1 > cy0 {
        vec![Rect::new(coord!(x: cx0, y: cy0), coord!(x: cx1, y: cy1)).to_polygon()]
    } else {
        vec![]
    }
}

/// Build the keep-out polygon set for `layer_name` from shapes in `lib`.
pub fn build_keepout(
    map: &LayerMap,
    layer_name: &str,
    layer: &PdkLayer,
    dbu: f64,
) -> Vec<Polygon<f64>> {
    match layer_name {
        "Activ"   => keepout_activ(map, layer, dbu),
        "GatPoly" => keepout_gatpoly(map, layer, dbu),
        "Metal1" | "Metal2" | "Metal3" | "Metal4" | "Metal5" => keepout_metal(map, layer, dbu),
        "TopMetal1" | "TopMetal2" => keepout_topmetal(map, layer, dbu),
        _ => {
            eprintln!("Warning: no IHP keepout rule for layer '{}', skipping", layer_name);
            vec![]
        }
    }
}

// Per-family keep-out builders

fn keepout_activ(map: &LayerMap, layer: &PdkLayer, dbu: f64) -> Vec<Polygon<f64>> {
    let mut ko = base_keepout(map, layer, ACTIV_SPACE_UM / dbu);
    ko.extend(offset_polygons(map.polygons(TRANS_LAYER,   TRANS_DATATYPE),  TRANS_ACTIV_SPACE_UM  / dbu));
    ko.extend(offset_polygons(map.polygons(GATPOLY_LAYER, GATPOLY_DATATYPE), ACTIV_GATPOLY_SPACE_UM / dbu));
    ko.extend(offset_polygons(map.polygons(CONT_LAYER,    CONT_DATATYPE),    ACTIV_CONT_SPACE_UM    / dbu));
    ko.extend(offset_polygons(map.polygons(NWELL_LAYER,   NWELL_DATATYPE),   ACTIV_NWELL_SPACE_UM   / dbu));
    ko.extend(offset_polygons(map.polygons(NBULAY_LAYER,  NBULAY_DATATYPE),  ACTIV_NBULAY_SPACE_UM  / dbu));
    ko.extend(offset_polygons(map.polygons(PWBLK_LAYER,   PWBLK_DATATYPE),   ACTIV_PWBLK_SPACE_UM   / dbu));
    ko
}

fn keepout_gatpoly(map: &LayerMap, layer: &PdkLayer, dbu: f64) -> Vec<Polygon<f64>> {
    let s = GATPOLY_FILL_SPACE_UM / dbu;
    // GatPoly drawing/fill expanded by the filler-specific rule (1.10 µm > general 0.8 µm).
    let mut ko = offset_polygons(map.polygons(layer.gds_layer, layer.drawing_datatype), s);
    ko.extend(offset_polygons(map.polygons(layer.gds_layer, layer.fill_datatype),    s));
    if let Some(dt) = layer.nofill_datatype {
        ko.extend(map.polygons(layer.gds_layer, dt).iter().cloned());
    }
    // All layers with 1.10 µm filler spacing rule.
    ko.extend(offset_polygons(map.polygons(ACTIV_LAYER,    ACTIV_DATATYPE),    s));
    ko.extend(offset_polygons(map.polygons(CONT_LAYER,     CONT_DATATYPE),     s));
    ko.extend(offset_polygons(map.polygons(PSD_LAYER,      PSD_DATATYPE),      s));
    ko.extend(offset_polygons(map.polygons(NSD_BLK_LAYER,  NSD_BLK_DATATYPE),  s));
    ko.extend(offset_polygons(map.polygons(SALBLOCK_LAYER, SALBLOCK_DATATYPE), s));
    ko.extend(offset_polygons(map.polygons(NWELL_LAYER,    NWELL_DATATYPE),    s));
    ko.extend(offset_polygons(map.polygons(NBULAY_LAYER,   NBULAY_DATATYPE),   s));
    ko.extend(offset_polygons(map.polygons(TRANS_LAYER,    TRANS_DATATYPE),    s));
    ko
}

fn keepout_metal(map: &LayerMap, layer: &PdkLayer, dbu: f64) -> Vec<Polygon<f64>> {
    let mut ko = base_keepout(map, layer, METAL_SPACE_UM / dbu);
    ko.extend(offset_polygons(map.polygons(TRANS_LAYER, TRANS_DATATYPE), TRANS_METAL_SPACE_UM / dbu));
    ko
}

fn keepout_topmetal(map: &LayerMap, layer: &PdkLayer, dbu: f64) -> Vec<Polygon<f64>> {
    let mut ko = base_keepout(map, layer, TOPMETAL_SPACE_UM / dbu);
    ko.extend(offset_polygons(map.polygons(TRANS_LAYER, TRANS_DATATYPE), TRANS_TOPMETAL_SPACE_UM / dbu));
    ko
}

// Shared helper

/// Drawing + fill shapes expanded by `space_dbu`, nofill shapes as-is.
fn base_keepout(map: &LayerMap, layer: &PdkLayer, space_dbu: f64) -> Vec<Polygon<f64>> {
    let mut ko = offset_polygons(map.polygons(layer.gds_layer, layer.drawing_datatype), space_dbu);
    ko.extend(offset_polygons(map.polygons(layer.gds_layer, layer.fill_datatype), space_dbu));
    if let Some(dt) = layer.nofill_datatype {
        ko.extend(map.polygons(layer.gds_layer, dt).iter().cloned());
    }
    ko
}
