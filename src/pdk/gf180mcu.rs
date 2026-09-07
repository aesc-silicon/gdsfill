// SPDX-FileCopyrightText: 2026 aesc silicon
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Layer table and keep-out polygon generation for GlobalFoundries GF180MCU.
//!
//! Variants differ only in the metal stack: A = 3 metals (30K top), B = 4
//! (11K top), C = 5 (9K top), D = 5 (11K top).  The top metal, whichever
//! layer it is, takes the thick-metal dummy spacing (DM.2c).
//!
//! Rules follow the PDK's KLayout fill scripts (`tech/scripts/fill_*.rb`), the
//! DRC decks (`drc/rule_decks/dummy_*.rb`, `density.rb`) and design manual
//! section 13: dummy COMP (DCF.*), dummy Poly2 (DPF.*) and dummy metal (DM.*).

use std::collections::HashMap;

use anyhow::Result;
use geo::{Polygon, Rect, coord};

use super::{FillAlgorithm, OverlapParams, PdkConstants, PdkLayer, SquareParams};
use crate::{close_polygons_tiled, edge_bands_tiled, offset_polygons, LayerMap};

// Layers

/// PR_bndry: librelane writes DIEAREA here.  Sub-cell boundaries land on it too,
/// but the merged exterior is still the die outline.
const DIE_LAYER: (i16, i16) = (0, 0);

const COMP:  (i16, i16) = (22, 0);
const POLY2: (i16, i16) = (30, 0);

/// Wells whose *boundary* (both sides) is a keep-out (DCF.6, DPF.6).
const NWELL:    (i16, i16) = (21, 0);
const DNWELL:   (i16, i16) = (12, 0);
const LVPWELL:  (i16, i16) = (204, 0);
const DUALGATE: (i16, i16) = (55, 0);

const RES_MK: (i16, i16) = (110, 5);
const IND_MK: (i16, i16) = (151, 5);
const PAD:    (i16, i16) = (37, 0);
/// Dummy COMP exclude and dummy Poly2 exclude marker layers.
const NDMY:   (i16, i16) = (111, 5);
const PMNDMY: (i16, i16) = (152, 5);
const MTPMK:  (i16, i16) = (122, 5);

const METALS: &[(&str, i16)] = &[
    ("Metal1", 34),
    ("Metal2", 36),
    ("Metal3", 42),
    ("Metal4", 46),
    ("Metal5", 81),
];

/// DM.8 marker layers: FuseTop (MIM top plate), POLYFUSE, FUSEWINDOW_D,
/// PMNDMY, MTPMK, OTP_MK.
const DM8_MARKERS: &[(i16, i16)] = &[(75, 0), (220, 0), (96, 1), (152, 5), (122, 5), (173, 5)];

// Dummy COMP rule constants (µm)

/// DCF.1c: 5 x 5 squares; DCF.2a: 3.0 placement space.
const DCF_SIZE_UM: f64 = 5.0;
const DCF_SPACE_UM: f64 = 3.0;
/// DCF.1a: only gaps between circuit COMP of >= 20 µm receive dummies, i.e.
/// COMP closed by 10 µm per side is a keep-out.
const DCF_CLOSING_UM: f64 = 10.0;
/// Window size for the tiled closing computation.  Total work is roughly flat
/// in this value, but peak memory per window is not: every rayon thread holds
/// one window's dilation, so keep it small (30 um: ~40 MB/window on a dense
/// design, 100 um: ~700 MB/window).  Clamped to `grow + shrink` internally.
const CLOSING_WINDOW_UM: f64 = 30.0;
/// Window size for the tiled well-boundary computation.  Dualgate in particular
/// is drawn as tens of thousands of large overlapping rectangles, which a global
/// merge cannot handle.
const WELL_WINDOW_UM: f64 = 100.0;
/// DCF.4 space to circuit COMP, DCF.5 to Poly2.
const DCF_COMP_SPACE_UM: f64 = 3.5;
const DCF_POLY2_SPACE_UM: f64 = 1.5;
/// DCF.6a-d: Nwell / DNWELL / LVPWELL / Dualgate boundary.
const DCF_NWELL_UM: f64 = 1.3;
const DCF_DNWELL_UM: f64 = 4.0;
const DCF_LVPWELL_UM: f64 = 1.3;
const DCF_DUALGATE_UM: f64 = 1.3;
/// DCF.8a RES_MK, DCF.9 Pad, DCF.11a NDMY, DCF.12 IND_MK.
const DCF_RES_MK_UM: f64 = 3.5;
const DCF_PAD_UM: f64 = 7.0;
const DCF_NDMY_UM: f64 = 3.5;
const DCF_IND_MK_UM: f64 = 3.0;

// Dummy Poly2 rule constants (µm)

/// DPF.1: dummy COMP sized up by 0.3 per side (5.6 x 5.6); DPF.2a: 2.4 space.
const DPF_EXTENSION_UM: f64 = 0.3;
const DPF_SIZE_UM: f64 = DCF_SIZE_UM + 2.0 * DPF_EXTENSION_UM;
const DPF_SPACE_UM: f64 = 2.4;
/// DPF.4 space to COMP (applied to the 20 µm closing like fill_poly2.rb), DPF.5 to Poly2.
const DPF_COMP_SPACE_UM: f64 = 3.2;
const DPF_POLY2_SPACE_UM: f64 = 5.0;
/// DPF.6a-d.
const DPF_NWELL_UM: f64 = 1.0;
const DPF_DNWELL_UM: f64 = 2.0;
const DPF_LVPWELL_UM: f64 = 1.0;
const DPF_DUALGATE_UM: f64 = 1.0;
/// DPF.8 RES_MK, DPF.9 Pad, DPF.11 NDMY, DPF.14 IND_MK, DPF.16 MTPMK, DPF.19 PMNDMY.
const DPF_RES_MK_UM: f64 = 19.7;
const DPF_PAD_UM: f64 = 6.7;
const DPF_NDMY_UM: f64 = 29.7;
const DPF_IND_MK_UM: f64 = 3.0;
const DPF_MTPMK_UM: f64 = 3.0;
const DPF_PMNDMY_UM: f64 = 8.0;
/// DPF.12 / DPF.13: space to circuit Metal1 / Metal2.  Applied by fill_poly2.rb;
/// commented out in the DRC deck.  `None` disables.
const DPF_METAL_SPACE_UM: Option<f64> = Some(2.0);

// Dummy metal rule constants (µm)

/// DM.1: dummy metal is exactly 2.0 x 2.0.
const DM_SIZE_UM: f64 = 2.0;
/// DM.2a: dummy-to-dummy space (layout); DM.2c: thick top metal.
const DM_SPACE_UM: f64 = 1.2;
const DM_SPACE_TOP_UM: f64 = 2.0;
/// DM.3: space to circuit metal on the same layer.
const DM_CIRCUIT_SPACE_UM: f64 = 2.0;
/// DM.4-DM.7: space to previous/subsequent metal.  `None` disables it, matching
/// the PDK DRC deck (rules commented out) and the "ignore_active" mode of
/// fill_metal.rb; `Some(2.0)` reproduces the script's default keep-out.
const DM_ADJACENT_SPACE_UM: Option<f64> = None;
/// DM.8: space to fuse/OTP/MIM marker layers.
const DM_MARKER_SPACE_UM: f64 = 6.0;
/// DM.9: consecutive layers must not replicate the pattern; shift by 0.5 per layer.
const DM_LAYER_OFFSET_UM: f64 = 0.5;
/// DCF.7a / DPF.7 / fill_metal.rb: keep-out from the scribe line (die edge).
const DIE_EDGE_SPACE_UM: f64 = 26.0;

// Layer table

fn metal_layer(gds_layer: i16, space_um: f64, origin_um: f64) -> PdkLayer {
    PdkLayer {
        gds_layer,
        drawing_datatype: 0,
        fill_datatype: 4,
        nofill_datatype: None,
        max_depth: 10,
        algorithms: vec![FillAlgorithm::Square(SquareParams {
            min_width: DM_SIZE_UM,
            max_width: DM_SIZE_UM,
            min_space: space_um,
            max_space: 10.0,
            clipping: true,
            origin_um: (origin_um, origin_um),
        })],
        // M1.4-M5.4: > 30 % over the die; aim slightly above the floor.
        default_density: 35.0,
        default_deviation: 5.0,
        tile_width_um: 100.0,
        merge_for_density: true,
        merge_window_um: Some(50.0),
    }
}

fn comp_layer() -> PdkLayer {
    PdkLayer {
        gds_layer: COMP.0,
        drawing_datatype: 0,
        fill_datatype: 4,
        nofill_datatype: None,
        max_depth: 10,
        algorithms: vec![FillAlgorithm::Square(SquareParams {
            min_width: DCF_SIZE_UM,
            max_width: DCF_SIZE_UM,
            min_space: DCF_SPACE_UM,
            max_space: 10.0,
            clipping: true,
            origin_um: (0.0, 0.0),
        })],
        // Dummy COMP is mandatory wherever allowed (STI dishing); the 5/3 lattice
        // tops out at 39 %, so a 40 % target fills every legal cell.  DCF.1b needs
        // >= 25 % globally, hence the wide deviation for reporting.
        default_density: 40.0,
        default_deviation: 15.0,
        tile_width_um: 100.0,
        merge_for_density: true,
        merge_window_um: Some(50.0),
    }
}

fn poly2_layer() -> PdkLayer {
    PdkLayer {
        gds_layer: POLY2.0,
        drawing_datatype: 0,
        fill_datatype: 4,
        nofill_datatype: None,
        max_depth: 10,
        algorithms: vec![FillAlgorithm::Overlap(OverlapParams {
            min_width: DPF_SIZE_UM,
            max_width: DPF_SIZE_UM,
            min_extension: DPF_EXTENSION_UM,
            min_space: DPF_SPACE_UM,
            ref_layer: "COMP",
            cover: true,
        })],
        // PL.8: >= 14 % globally.
        default_density: 20.0,
        default_deviation: 6.0,
        tile_width_um: 100.0,
        merge_for_density: true,
        merge_window_um: Some(50.0),
    }
}

pub fn for_variant(variant: char) -> Option<PdkConstants> {
    let n_metals = match variant {
        'A' => 3,
        'B' => 4,
        'C' | 'D' => 5,
        _ => return None,
    };
    Some(gf180mcu(n_metals))
}

fn gf180mcu(n_metals: usize) -> PdkConstants {
    let mut layers = HashMap::new();
    layers.insert("COMP", comp_layer());
    layers.insert("Poly2", poly2_layer());
    for (i, &(name, gds)) in METALS.iter().take(n_metals).enumerate() {
        let top = i == n_metals - 1;
        let space = if top { DM_SPACE_TOP_UM } else { DM_SPACE_UM };
        layers.insert(name, metal_layer(gds, space, i as f64 * DM_LAYER_OFFSET_UM));
    }
    PdkConstants {
        layers,
        db_unit_um: 0.005,
        // Design manual: density window 200 x 200 µm at 100 µm step.
        tile_width_um: 200.0,
        fill_boundary_layer: Some(DIE_LAYER),
        density_boundary_layer: Some(DIE_LAYER),
        // 5 nm manufacturing grid = 1 DBU.
        grid_dbu: 1.0,
    }
}

// Keep-out

/// All `(layer, datatype)` pairs the keep-out rules read besides the target layers.
pub fn needed_layers() -> Vec<(i16, i16)> {
    let mut v = vec![
        DIE_LAYER, COMP, POLY2, NWELL, DNWELL, LVPWELL, DUALGATE,
        RES_MK, IND_MK, PAD, NDMY, PMNDMY, MTPMK,
    ];
    v.extend_from_slice(DM8_MARKERS);
    if DM_ADJACENT_SPACE_UM.is_some() || DPF_METAL_SPACE_UM.is_some() {
        v.extend(METALS.iter().map(|&(_, gds)| (gds, 0)));
    }
    v
}

/// `chip` is the die outline bbox (the boundary layer itself is no longer in `map`).
pub fn build_keepout(
    map: &LayerMap,
    chip: Rect<f64>,
    layer_name: &str,
    layer: &PdkLayer,
    dbu: f64,
) -> Result<Vec<Polygon<f64>>> {
    match METALS.iter().position(|&(n, _)| n == layer_name) {
        Some(idx) => Ok(keepout_metal(map, chip, idx, layer, dbu)),
        None if layer_name == "COMP"  => keepout_comp(map, chip, layer, dbu),
        None if layer_name == "Poly2" => keepout_poly2(map, chip, layer, dbu),
        None => {
            eprintln!("Warning: no GF180MCU keepout rule for layer '{}', skipping", layer_name);
            Ok(vec![])
        }
    }
}

/// Circuit COMP closed by `DCF_CLOSING_UM`, then grown by `space_um`
/// (`fill_comp.rb`: `COMP_20um_spacing.sized(space)`).  Computed as the
/// dilated COMP eroded by `closing - space`, which equals the script's region
/// except at concave corners, where it is slightly larger (conservative).
fn comp_closing_keepout(map: &LayerMap, space_um: f64, dbu: f64) -> Result<Vec<Polygon<f64>>> {
    debug_assert!(space_um < DCF_CLOSING_UM);
    let window = CLOSING_WINDOW_UM / dbu;
    close_polygons_tiled(
        map.polygons(COMP.0, COMP.1),
        DCF_CLOSING_UM / dbu,
        (DCF_CLOSING_UM - space_um) / dbu,
        window,
    )
}

/// Both-sided boundary ring of a (merged) well layer.
fn well_ring(map: &LayerMap, well: (i16, i16), space_um: f64, dbu: f64) -> Vec<Polygon<f64>> {
    edge_bands_tiled(map.polygons(well.0, well.1), space_um / dbu, WELL_WINDOW_UM / dbu)
}

fn keepout_comp(map: &LayerMap, chip: Rect<f64>, layer: &PdkLayer, dbu: f64) -> Result<Vec<Polygon<f64>>> {
    let mut ko = comp_closing_keepout(map, DCF_COMP_SPACE_UM, dbu)?;
    ko.extend(offset_polygons(map.polygons(layer.gds_layer, layer.fill_datatype), DCF_SPACE_UM / dbu));
    ko.extend(offset_polygons(map.polygons(POLY2.0, POLY2.1), DCF_POLY2_SPACE_UM / dbu));
    ko.extend(well_ring(map, NWELL,    DCF_NWELL_UM,    dbu));
    ko.extend(well_ring(map, DNWELL,   DCF_DNWELL_UM,   dbu));
    ko.extend(well_ring(map, LVPWELL,  DCF_LVPWELL_UM,  dbu));
    ko.extend(well_ring(map, DUALGATE, DCF_DUALGATE_UM, dbu));
    ko.extend(offset_polygons(map.polygons(RES_MK.0, RES_MK.1), DCF_RES_MK_UM / dbu));
    ko.extend(offset_polygons(map.polygons(PAD.0,    PAD.1),    DCF_PAD_UM    / dbu));
    ko.extend(offset_polygons(map.polygons(IND_MK.0, IND_MK.1), DCF_IND_MK_UM / dbu));
    ko.extend(offset_polygons(map.polygons(NDMY.0,   NDMY.1),   DCF_NDMY_UM   / dbu));
    ko.extend(die_edge_ring(chip, DIE_EDGE_SPACE_UM / dbu));
    Ok(ko)
}

fn keepout_poly2(map: &LayerMap, chip: Rect<f64>, layer: &PdkLayer, dbu: f64) -> Result<Vec<Polygon<f64>>> {
    let mut ko = comp_closing_keepout(map, DPF_COMP_SPACE_UM, dbu)?;
    ko.extend(offset_polygons(map.polygons(layer.gds_layer, layer.fill_datatype), DPF_SPACE_UM / dbu));
    ko.extend(offset_polygons(map.polygons(POLY2.0, POLY2.1), DPF_POLY2_SPACE_UM / dbu));
    ko.extend(well_ring(map, NWELL,    DPF_NWELL_UM,    dbu));
    ko.extend(well_ring(map, DNWELL,   DPF_DNWELL_UM,   dbu));
    ko.extend(well_ring(map, LVPWELL,  DPF_LVPWELL_UM,  dbu));
    ko.extend(well_ring(map, DUALGATE, DPF_DUALGATE_UM, dbu));
    ko.extend(offset_polygons(map.polygons(RES_MK.0, RES_MK.1), DPF_RES_MK_UM / dbu));
    ko.extend(offset_polygons(map.polygons(PAD.0,    PAD.1),    DPF_PAD_UM    / dbu));
    ko.extend(offset_polygons(map.polygons(IND_MK.0, IND_MK.1), DPF_IND_MK_UM / dbu));
    ko.extend(offset_polygons(map.polygons(MTPMK.0,  MTPMK.1),  DPF_MTPMK_UM  / dbu));
    ko.extend(offset_polygons(map.polygons(NDMY.0,   NDMY.1),   DPF_NDMY_UM   / dbu));
    ko.extend(offset_polygons(map.polygons(PMNDMY.0, PMNDMY.1), DPF_PMNDMY_UM / dbu));
    if let Some(s) = DPF_METAL_SPACE_UM {
        for &(_, gds) in &METALS[..2] {
            ko.extend(offset_polygons(map.polygons(gds, 0), s / dbu));
        }
    }
    ko.extend(die_edge_ring(chip, DIE_EDGE_SPACE_UM / dbu));
    Ok(ko)
}

fn keepout_metal(map: &LayerMap, chip: Rect<f64>, idx: usize, layer: &PdkLayer, dbu: f64) -> Vec<Polygon<f64>> {
    // DM.2a / DM.2c: the layer's own Square spacing already encodes whether it is the top metal.
    let dummy_space = layer.algorithms.iter().find_map(|a| match a {
        FillAlgorithm::Square(sq) => Some(sq.min_space),
        _ => None,
    }).unwrap_or(DM_SPACE_UM);
    let mut ko = offset_polygons(
        map.polygons(layer.gds_layer, layer.drawing_datatype), DM_CIRCUIT_SPACE_UM / dbu);
    ko.extend(offset_polygons(
        map.polygons(layer.gds_layer, layer.fill_datatype), dummy_space / dbu));
    for &(l, dt) in DM8_MARKERS {
        ko.extend(offset_polygons(map.polygons(l, dt), DM_MARKER_SPACE_UM / dbu));
    }
    if let Some(s) = DM_ADJACENT_SPACE_UM {
        for adj in [idx.checked_sub(1), Some(idx + 1)].into_iter().flatten() {
            if let Some(&(_, gds)) = METALS.get(adj) {
                ko.extend(offset_polygons(map.polygons(gds, 0), s / dbu));
            }
        }
    }
    ko.extend(die_edge_ring(chip, DIE_EDGE_SPACE_UM / dbu));
    ko
}

/// Four rectangles covering the band `space_dbu` wide inside the die outline.
fn die_edge_ring(chip: Rect<f64>, space_dbu: f64) -> Vec<Polygon<f64>> {
    let (x0, y0, x1, y1) = (chip.min().x, chip.min().y, chip.max().x, chip.max().y);
    [
        (x0, y0, x0 + space_dbu, y1),
        (x1 - space_dbu, y0, x1, y1),
        (x0, y0, x1, y0 + space_dbu),
        (x0, y1 - space_dbu, x1, y1),
    ]
    .into_iter()
    .map(|(ax, ay, bx, by)| Rect::new(coord!(x: ax, y: ay), coord!(x: bx, y: by)).to_polygon())
    .collect()
}
