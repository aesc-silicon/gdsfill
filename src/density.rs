use std::collections::HashSet;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use geo::{Area, Polygon, Rect, coord};
use gds21::{GdsBoundary, GdsElement, GdsPoint};
use rayon::prelude::*;

use crate::{
    build_tile_index, clipped_area, get_target_layers, tiled_merge_area, read_gds, write_gds,
    RunContext, LayerMap, DEBUG_MERGED_DT
};

/// Per-tile density result, collected in parallel and sorted before printing.
struct TileResult {
    ix: usize,
    iy: usize,
    draw_area: f64,
    fill_area: f64,
    tile_area: f64,
}

/// Calculate metal density per layer and per tile for `gds_file`.
///
/// The reference area is the filled exterior of the boundary layer (EdgeSeal),
/// matching KLayout's `(edgeseal + edgeseal.holes()).area()`.
///
/// When `debug` is `true`, merged polygon shapes are written back to the GDS
/// file on datatype [`DEBUG_MERGED_DT`] for visual inspection.
pub fn run(gds_file: &Path, ctx: RunContext, debug: bool) -> Result<()> {
    let RunContext { ref process, config: _, ref pdk } = ctx;

    let mut needed: HashSet<(i16, i16)> = HashSet::new();

    let (bl_layer, bl_datatype) = pdk.boundary_layer
        .ok_or_else(|| anyhow!("No boundary layer defined for process '{}'", process))?;
    needed.insert((bl_layer, bl_datatype));

    let density_targets = get_target_layers(&ctx);
    for (_, layer) in &density_targets {
        needed.insert((layer.gds_layer, layer.drawing_datatype));
        needed.insert((layer.gds_layer, layer.fill_datatype));
    }

    let mut lib = read_gds(gds_file)
        .with_context(|| format!("Failed to read GDS file: {}", gds_file.display()))?;

    let all_cells: HashSet<&str> = lib.structs.iter()
        .flat_map(|s| s.elems.iter())
        .filter_map(|e| match e {
            GdsElement::GdsStructRef(r) => Some(r.name.as_str()),
            GdsElement::GdsArrayRef(r)  => Some(r.name.as_str()),
            _ => None,
        })
        .collect();

    let top_cells: Vec<&str> = lib.structs.iter()
        .filter(|s| !all_cells.contains(s.name.as_str()) && !s.name.starts_with("$$"))
        .map(|s| s.name.as_str())
        .collect();
    println!("GDS: {} cells total, {} top cell(s):", lib.structs.len(), top_cells.len());
    for name in &top_cells {
        println!("  {}", name);
    }

    let mut layer_map = LayerMap::build_for(&lib, Some(&needed));
    let bbox = layer_map.bbox(bl_layer, bl_datatype)
        .ok_or_else(|| anyhow!(
            "Chip boundary layer ({}, {}) not found in GDS",
            bl_layer, bl_datatype
        ))?;

    let dbu   = pdk.db_unit_um;
    let x_min = bbox.min().x;
    let y_min = bbox.min().y;
    let x_max = bbox.max().x;
    let y_max = bbox.max().y;

    layer_map.merge(bl_layer, bl_datatype);
    let density_area: f64 = layer_map.polygons(bl_layer, bl_datatype)
        .iter()
        .map(|p| Polygon::new(p.exterior().clone(), vec![]).unsigned_area())
        .sum();
    layer_map.drop(bl_layer, bl_datatype);

    for (_, layer) in &density_targets {
        layer_map.remove_contained(layer.gds_layer, layer.drawing_datatype);
    }

    println!("Density area: {:.1} µm²", density_area * dbu * dbu);

    let mut debug_boundaries: Vec<GdsBoundary> = Vec::new();

    for (name, layer) in &density_targets {
        println!("\nLayer {} (layer {}):", name, layer.gds_layer);

        let tile_size = pdk.tile_width_um / dbu;
        let nx = ((x_max - x_min) / tile_size).ceil() as usize;
        let ny = ((y_max - y_min) / tile_size).ceil() as usize;

        let drawing = layer_map.polygons(layer.gds_layer, layer.drawing_datatype);
        let fill    = layer_map.polygons(layer.gds_layer, layer.fill_datatype);

        if debug {
            for poly in drawing.iter().chain(fill.iter()) {
                let xy = poly.exterior().0.iter()
                    .map(|c| GdsPoint { x: c.x.round() as i32, y: c.y.round() as i32 })
                    .collect();
                debug_boundaries.push(GdsBoundary {
                    layer: layer.gds_layer,
                    datatype: DEBUG_MERGED_DT,
                    xy,
                    ..Default::default()
                });
            }
        }

        let merge_window_dbu = layer.merge_window_um.map(|w| w / dbu);
        let merge_for_density = layer.merge_for_density;

        let run_tiles = |draw_polys: &[Polygon<f64>]| -> (Vec<TileResult>, f64, f64) {
            let draw_idx = build_tile_index(draw_polys, x_min, y_min, tile_size, nx, ny);
            let fill_idx = build_tile_index(fill,       x_min, y_min, tile_size, nx, ny);
            let coords: Vec<(usize, usize)> = (0..ny)
                .flat_map(|iy| (0..nx).map(move |ix| (ix, iy)))
                .collect();
            let mut tiles: Vec<TileResult> = coords.par_iter()
                .map(|&(ix, iy)| {
                    let tx0 = x_min + ix as f64 * tile_size;
                    let tx1 = (tx0 + tile_size).min(x_max);
                    let ty0 = y_min + iy as f64 * tile_size;
                    let ty1 = (ty0 + tile_size).min(y_max);
                    let tile_rect = Rect::new(coord!(x: tx0, y: ty0), coord!(x: tx1, y: ty1));
                    let tile_poly = tile_rect.to_polygon();
                    let draw_candidates = &draw_idx[iy * nx + ix];
                    let fill_candidates = &fill_idx[iy * nx + ix];
                    let draw_area = if merge_for_density {
                        tiled_merge_area(draw_polys, draw_candidates, tile_rect, merge_window_dbu)
                    } else {
                        clipped_area(draw_polys, draw_candidates, &tile_poly)
                    };
                    TileResult {
                        ix, iy,
                        draw_area,
                        fill_area: clipped_area(fill, fill_candidates, &tile_poly),
                        tile_area: (tx1 - tx0) * (ty1 - ty0),
                    }
                })
                .collect();
            tiles.sort_unstable_by_key(|t| (t.iy, t.ix));
            let total_draw: f64 = tiles.iter().map(|t| t.draw_area).sum();
            let total_fill: f64 = tiles.iter().map(|t| t.fill_area).sum();
            (tiles, total_draw, total_fill)
        };

        let (tiles, total_draw, total_fill) = run_tiles(drawing);

        for t in &tiles {
            let tx0 = x_min + t.ix as f64 * tile_size;
            let tx1 = (tx0 + tile_size).min(x_max);
            let ty0 = y_min + t.iy as f64 * tile_size;
            let ty1 = (ty0 + tile_size).min(y_max);
            println!(
                "  Tile [{:2},{:2}] [({:7.1}, {:7.1}) - ({:7.1}, {:7.1}) µm]: \
                 draw {:5.1}%  fill {:5.1}%  total {:5.1}%",
                t.ix, t.iy,
                tx0 * dbu, ty0 * dbu, tx1 * dbu, ty1 * dbu,
                t.draw_area / t.tile_area * 100.0,
                t.fill_area / t.tile_area * 100.0,
                (t.draw_area + t.fill_area) / t.tile_area * 100.0,
            );
        }

        println!(
            "  Overall:  draw {:5.2}%  fill {:5.2}%  total {:5.2}%",
            (total_draw / density_area * 100.0).max(0.0),
            (total_fill / density_area * 100.0).max(0.0),
            ((total_draw + total_fill) / density_area * 100.0).max(0.0),
        );

        layer_map.drop(layer.gds_layer, layer.drawing_datatype);
        layer_map.drop(layer.gds_layer, layer.fill_datatype);
    }

    if debug {
        let referenced: HashSet<String> = lib.structs.iter()
            .flat_map(|s| s.elems.iter())
            .filter_map(|e| match e {
                GdsElement::GdsStructRef(r) => Some(r.name.clone()),
                GdsElement::GdsArrayRef(r)  => Some(r.name.clone()),
                _ => None,
            })
            .collect();
        for s in lib.structs.iter_mut().filter(|s| !referenced.contains(&s.name)) {
            s.elems.retain(|e| !matches!(e, GdsElement::GdsBoundary(b) if b.datatype == DEBUG_MERGED_DT));
            for b in &debug_boundaries {
                s.elems.push(GdsElement::GdsBoundary(b.clone()));
            }
        }
        write_gds(&lib, gds_file)
            .with_context(|| format!("Failed to write debug shapes to {}", gds_file.display()))?;
        println!("\nWrote debug shapes (datatype {DEBUG_MERGED_DT}) to {}", gds_file.display());
    }

    Ok(())
}
