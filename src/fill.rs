use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use geo::{Area, BoundingRect, Contains, Intersects, Point, Rect, coord};
use gds21::{GdsBoundary, GdsElement, GdsPoint};
use rayon::prelude::*;

use crate::pdk::FillAlgorithm;
use crate::{
    build_tile_index, clipped_area, get_target_layers, tiled_merge_area, read_gds, write_gds,
    RunContext, LayerMap, DEBUG_KEEPOUT_DT, DEBUG_MERGED_DT
};

pub fn run(gds_file: &Path, ctx: RunContext, debug: bool, dryrun: bool) -> Result<()> {
    let RunContext { ref process, ref config, ref pdk } = ctx;

    let mut needed: HashSet<(i16, i16)> = HashSet::new();

    let (bl_layer, bl_datatype) = pdk.boundary_layer
        .ok_or_else(|| anyhow!("No boundary layer defined for process '{}'", process))?;
    needed.insert((bl_layer, bl_datatype));

    let fill_targets = get_target_layers(&ctx);
    for (_, layer) in &fill_targets {
        needed.insert((layer.gds_layer, layer.drawing_datatype));
        needed.insert((layer.gds_layer, layer.fill_datatype));
    }

    // Keepout builders read additional layers (e.g. GatPoly, Cont, NWell for IHP).
    match process.as_str() {
        "ihp-sg13g2" | "ihp-sg13cmos5l" =>
            needed.extend(crate::pdk::ihp_sg13g2::needed_layers()),
        _ => {}
    }

    let mut lib = read_gds(gds_file)
        .with_context(|| format!("Failed to read GDS file: {}", gds_file.display()))?;

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
    let boundary_outer: Vec<geo::Polygon<f64>> = layer_map
        .polygons(bl_layer, bl_datatype)
        .iter()
        .map(|p| geo::Polygon::new(p.exterior().clone(), vec![]))
        .collect();
    let density_area: f64 = boundary_outer.iter().map(|p| p.unsigned_area()).sum();
    layer_map.drop(bl_layer, bl_datatype);

    for (_, layer) in &fill_targets {
        layer_map.remove_contained(layer.gds_layer, layer.drawing_datatype);
    }

    println!("Density area: {:.1} µm²", density_area * dbu * dbu);

    // Compute the digital core area used by Track fill (PDK-specific).
    let core_polys_all: Vec<geo::Polygon<f64>> = match process.as_str() {
        "ihp-sg13g2" | "ihp-sg13cmos5l" =>
            crate::pdk::ihp_sg13g2::compute_core_area(&layer_map, dbu),
        _ => vec![],
    };

    if !core_polys_all.is_empty()
        && let Some(b) = core_polys_all[0].bounding_rect() {
            println!("Core area: ({:.3}, {:.3}) .. ({:.3}, {:.3}) µm  ({:.1} x {:.1} µm)",
                b.min().x * dbu, b.min().y * dbu,
                b.max().x * dbu, b.max().y * dbu,
                (b.max().x - b.min().x) * dbu,
                (b.max().y - b.min().y) * dbu);
        }

    let mut all_new_boundaries: Vec<GdsBoundary> = Vec::new();
    let mut total_shapes = 0usize;

    // Placed fill rects keyed by (gds_layer, fill_datatype); populated after each
    // layer is processed so Overlap algorithms can reference previously placed shapes.
    let mut placed_rects: HashMap<(i16, i16), Vec<Rect<f64>>> = HashMap::new();

    'layer: for (name, layer) in &fill_targets {
        let tile_size = layer.tile_width_um / dbu;
        let nx = ((x_max - x_min) / tile_size).ceil() as usize;
        let ny = ((y_max - y_min) / tile_size).ceil() as usize;

        let layer_config = config.as_ref()
            .and_then(|c| c.layers.as_ref())
            .and_then(|m| m.get(*name));
        let target_density = layer_config
            .map(|lc| lc.density)
            .unwrap_or(layer.default_density);
        let deviation = layer_config
            .map(|lc| lc.deviation)
            .unwrap_or(layer.default_deviation);

        let has_fill_algo = layer.algorithms.iter().any(|a| matches!(
            a, FillAlgorithm::Square(_) | FillAlgorithm::Overlap(_) | FillAlgorithm::Track(_)
        ));
        if !has_fill_algo {
            eprintln!("Warning: layer '{}' has no supported fill algorithm, skipping", name);
            continue 'layer;
        }

        let layer_t = Instant::now();
        println!("Filling layer {} (layer {}, {}x{} tiles of {}µm)...",
            name, layer.gds_layer, nx, ny, layer.tile_width_um);

        let merge_window_dbu = layer.merge_window_um.map(|w| w / dbu);
        let merge_for_density = layer.merge_for_density;

        // Raw polygon slices -- no global merge; spatial index drives per-tile lookup.
        let t = Instant::now();
        let drawing_raw = layer_map.polygons(layer.gds_layer, layer.drawing_datatype);
        let fill_raw    = layer_map.polygons(layer.gds_layer, layer.fill_datatype);
        let drawing_idx = build_tile_index(drawing_raw, x_min, y_min, tile_size, nx, ny);
        let fill_idx    = build_tile_index(fill_raw,    x_min, y_min, tile_size, nx, ny);
        println!("  {:<18} {:>8.2?}  (drawing {}, fill {})",
            "index draw/fill:", t.elapsed(), drawing_raw.len(), fill_raw.len());

        // Keep-out is process-specific; store (bbox, polygon) for fast spatial queries.
        let t = Instant::now();
        let base_keepout: Vec<(Rect<f64>, geo::Polygon<f64>)> = {
            let polys: Vec<geo::Polygon<f64>> = match process.as_str() {
                "ihp-sg13g2" | "ihp-sg13cmos5l" => {
                    crate::pdk::ihp_sg13g2::build_keepout(&layer_map, name, layer, dbu)
                }
                _ => {
                    eprintln!("Warning: no keepout rules for process '{}', fill may overlap existing metal", process);
                    vec![]
                }
            };
            polys.into_iter()
                .filter_map(|p| p.bounding_rect().map(|bbox| (bbox, p)))
                .collect()
        };
        println!("  {:<18} {:>8.2?}  ({} polys)",
            "keepout:", t.elapsed(), base_keepout.len());

        if debug {
            for (_, p) in &base_keepout {
                all_new_boundaries.push(poly_to_boundary(p, layer.gds_layer, DEBUG_KEEPOUT_DT));
            }
            for p in drawing_raw {
                all_new_boundaries.push(poly_to_boundary(p, layer.gds_layer, DEBUG_MERGED_DT));
            }
        }

        // Spatial index for keepout polygons (reuses the same helper).
        let t = Instant::now();
        let keepout_polys_only: Vec<geo::Polygon<f64>> =
            base_keepout.iter().map(|(_, p)| p.clone()).collect();
        let tile_keepout_idx = build_tile_index(&keepout_polys_only, x_min, y_min, tile_size, nx, ny);
        let avg_ko = if nx * ny > 0 {
            tile_keepout_idx.iter().map(|v| v.len()).sum::<usize>() / (nx * ny)
        } else { 0 };
        println!("  {:<18} {:>8.2?}  ({} polys, avg {}/tile)",
            "index keepout:", t.elapsed(), base_keepout.len(), avg_ko);

        // Core polygon tile index for Track fill (empty if no Track algorithm on this layer).
        let has_track_algo = layer.algorithms.iter().any(|a| matches!(a, FillAlgorithm::Track(_)));
        let core_tile_idx: Vec<Vec<usize>> = if has_track_algo && !core_polys_all.is_empty() {
            build_tile_index(&core_polys_all, x_min, y_min, tile_size, nx, ny)
        } else {
            vec![vec![]; nx * ny]
        };

        // For Track fill: anchor the fill grid to the lower-left corner of the digital
        // core area.  Routing tracks in a standard-cell core start from the core origin,
        // so this gives a stable, deterministic phase that is identical for every tile.
        let (track_phase_x, track_phase_y): (f64, f64) = if has_track_algo {
            core_polys_all.first()
                .and_then(|p| p.bounding_rect())
                .map(|b| (b.min().x, b.min().y))
                .unwrap_or((x_min, y_min))
        } else {
            (x_min, y_min)
        };

        // For Overlap algorithms: build a tile index over the reference layer's
        // previously placed fill rects so each tile closure can look them up cheaply.
        let overlap_ref_key: Option<(i16, i16)> = layer.algorithms.iter().find_map(|a| {
            if let FillAlgorithm::Overlap(op) = a {
                pdk.layers.get(op.ref_layer).map(|l| (l.gds_layer, l.fill_datatype))
            } else { None }
        });
        let ref_rects_storage: Vec<Rect<f64>> = overlap_ref_key
            .and_then(|key| placed_rects.get(&key))
            .cloned()
            .unwrap_or_default();
        let ref_tile_idx: Vec<Vec<usize>> = if overlap_ref_key.is_some() {
            let ref_polys: Vec<geo::Polygon<f64>> = ref_rects_storage.iter()
                .map(|r| r.to_polygon()).collect();
            build_tile_index(&ref_polys, x_min, y_min, tile_size, nx, ny)
        } else {
            vec![vec![]; nx * ny]
        };

        // Process all tiles in parallel; each tile is independent (reads only shared refs)
        let coords: Vec<(usize, usize)> = (0..ny)
            .flat_map(|iy| (0..nx).map(move |ix| (ix, iy)))
            .collect();

        struct TileStat {
            ix: usize, iy: usize,
            draw_area: f64, old_fill_area: f64, new_fill_area: f64, tile_area: f64,
        }

        let t = Instant::now();
        let tile_results: Vec<(Vec<GdsBoundary>, Vec<Rect<f64>>, TileStat)> = coords.par_iter()
            .map(|&(ix, iy)| {
                let tx0 = x_min + ix as f64 * tile_size;
                let tx1 = (tx0 + tile_size).min(x_max);
                let ty0 = y_min + iy as f64 * tile_size;
                let ty1 = (ty0 + tile_size).min(y_max);
                let tile = Rect::new(coord!(x: tx0, y: ty0), coord!(x: tx1, y: ty1));
                let tile_area = (tx1 - tx0) * (ty1 - ty0);
                let empty_stat = TileStat { ix, iy, draw_area: 0.0, old_fill_area: 0.0, new_fill_area: 0.0, tile_area };
                if tile_area <= 0.0 { return (vec![], vec![], empty_stat); }

                let tile_poly = tile.to_polygon();

                let draw_candidates = &drawing_idx[iy * nx + ix];
                let fill_candidates = &fill_idx[iy * nx + ix];
                let draw_area = if merge_for_density {
                    tiled_merge_area(drawing_raw, draw_candidates, tile, merge_window_dbu)
                } else {
                    clipped_area(drawing_raw, draw_candidates, &tile_poly)
                };
                let old_fill_area = clipped_area(fill_raw, fill_candidates, &tile_poly);
                let existing_pct = (draw_area + old_fill_area) / tile_area * 100.0;

                let mut tile_keepout: Vec<(Rect<f64>, geo::Polygon<f64>)> =
                    tile_keepout_idx[iy * nx + ix]
                        .iter()
                        .map(|&ki| base_keepout[ki].clone())
                        .collect();

                let mut running_pct = existing_pct;
                let mut tile_boundaries: Vec<GdsBoundary> = Vec::new();
                let mut tile_fill_rects: Vec<Rect<f64>> = Vec::new();
                let mut new_fill_area = 0.0_f64;

                // Reject fills whose centre lies outside the chip boundary polygon.
                // Needed for non-rectangular boundaries where bounding-box tiles
                // extend into areas outside the actual sealring shape.
                let inside_boundary = |r: &Rect<f64>| -> bool {
                    if boundary_outer.is_empty() { return true; }
                    let center = Point::new(
                        (r.min().x + r.max().x) / 2.0,
                        (r.min().y + r.max().y) / 2.0,
                    );
                    boundary_outer.iter().any(|bp| bp.contains(&center))
                };

                for algo in &layer.algorithms {
                    let ctx = TileCtx {
                        running_pct, target_density, deviation,
                        dbu, tile_area, grid_dbu: pdk.grid_dbu,
                    };
                    match algo {
                        FillAlgorithm::Square(sq) => {
                            let new_rects: Vec<Rect<f64>> = fill_square_tile(&tile, &tile_keepout, sq, &ctx)
                                .into_iter().filter(|r| inside_boundary(r)).collect();
                            if !new_rects.is_empty() {
                                let min_space_dbu = sq.min_space / dbu;
                                let added_area: f64 = new_rects.iter()
                                    .map(|r| (r.max().x - r.min().x) * (r.max().y - r.min().y))
                                    .sum();
                                tile_keepout.extend(new_rects.iter().map(|r| {
                                    let e = Rect::new(
                                        coord!(x: r.min().x - min_space_dbu, y: r.min().y - min_space_dbu),
                                        coord!(x: r.max().x + min_space_dbu, y: r.max().y + min_space_dbu),
                                    );
                                    (e, e.to_polygon())
                                }));
                                running_pct  += added_area / tile_area * 100.0;
                                new_fill_area += added_area;
                                for r in new_rects {
                                    tile_boundaries.push(rect_to_boundary(r, layer.gds_layer, layer.fill_datatype));
                                    tile_fill_rects.push(r);
                                }
                            }
                        }

                        FillAlgorithm::Overlap(op) => {
                            let tile_ref_rects: Vec<Rect<f64>> = ref_tile_idx[iy * nx + ix]
                                .iter()
                                .map(|&i| ref_rects_storage[i])
                                .collect();
                            let new_rects: Vec<Rect<f64>> = fill_overlap_tile(&tile_keepout, op, &tile_ref_rects, &ctx)
                                .into_iter().filter(|r| inside_boundary(r)).collect();
                            if !new_rects.is_empty() {
                                let min_space_dbu = op.min_space / dbu;
                                let added_area: f64 = new_rects.iter()
                                    .map(|r| (r.max().x - r.min().x) * (r.max().y - r.min().y))
                                    .sum();
                                tile_keepout.extend(new_rects.iter().map(|r| {
                                    let e = Rect::new(
                                        coord!(x: r.min().x - min_space_dbu, y: r.min().y - min_space_dbu),
                                        coord!(x: r.max().x + min_space_dbu, y: r.max().y + min_space_dbu),
                                    );
                                    (e, e.to_polygon())
                                }));
                                new_fill_area += added_area;
                                for r in new_rects {
                                    tile_boundaries.push(rect_to_boundary(r, layer.gds_layer, layer.fill_datatype));
                                    tile_fill_rects.push(r);
                                }
                            }
                        }

                        FillAlgorithm::Track(tp) => {
                            let tile_core_polys: Vec<geo::Polygon<f64>> =
                                core_tile_idx[iy * nx + ix]
                                    .iter()
                                    .map(|&i| core_polys_all[i].clone())
                                    .collect();
                            let new_rects: Vec<Rect<f64>> = fill_track_tile(
                                &tile, &tile_keepout, tp, &tile_core_polys,
                                &ctx, track_phase_x, track_phase_y,
                            ).into_iter().filter(|r| inside_boundary(r)).collect();
                            if !new_rects.is_empty() {
                                let min_space_dbu = tp.min_space / dbu;
                                let added_area: f64 = new_rects.iter()
                                    .map(|r| (r.max().x - r.min().x) * (r.max().y - r.min().y))
                                    .sum();
                                tile_keepout.extend(new_rects.iter().map(|r| {
                                    let e = Rect::new(
                                        coord!(x: r.min().x - min_space_dbu, y: r.min().y - min_space_dbu),
                                        coord!(x: r.max().x + min_space_dbu, y: r.max().y + min_space_dbu),
                                    );
                                    (e, e.to_polygon())
                                }));
                                running_pct  += added_area / tile_area * 100.0;
                                new_fill_area += added_area;
                                for r in new_rects {
                                    tile_boundaries.push(rect_to_boundary(r, layer.gds_layer, layer.fill_datatype));
                                    tile_fill_rects.push(r);
                                }
                            }
                        }
                    }
                }

                let stat = TileStat { ix, iy, draw_area, old_fill_area, new_fill_area, tile_area };
                (tile_boundaries, tile_fill_rects, stat)
            })
            .collect();

        let fill_elapsed = t.elapsed();

        // Store placed rects for reference by subsequent layers.
        let layer_fill_rects: Vec<Rect<f64>> = tile_results.iter()
            .flat_map(|(_, r, _)| r.iter().copied()).collect();
        placed_rects.insert((layer.gds_layer, layer.fill_datatype), layer_fill_rects);

        let layer_shapes: usize = tile_results.iter().map(|(b, _, _)| b.len()).sum();

        // Per-tile density table
        for (_, _, s) in &tile_results {
            if s.tile_area <= 0.0 { continue; }
            let tx0 = x_min + s.ix as f64 * tile_size;
            let tx1 = (tx0 + tile_size).min(x_max);
            let ty0 = y_min + s.iy as f64 * tile_size;
            let ty1 = (ty0 + tile_size).min(y_max);
            let total = (s.draw_area + s.old_fill_area + s.new_fill_area) / s.tile_area * 100.0;
            println!(
                "  Tile [{:2},{:2}] [({:8.1}, {:8.1}) - ({:8.1}, {:8.1}) µm]: \
                 draw {:5.1}%  old {:5.1}%  new {:5.1}% -> {:5.1}%",
                s.ix, s.iy,
                tx0 * dbu, ty0 * dbu, tx1 * dbu, ty1 * dbu,
                s.draw_area    / s.tile_area * 100.0,
                s.old_fill_area / s.tile_area * 100.0,
                s.new_fill_area / s.tile_area * 100.0,
                total,
            );
        }

        // Under-density tile summary
        let min_required = target_density - deviation;
        let under: Vec<_> = tile_results.iter()
            .filter(|(_, _, s)| {
                if s.tile_area <= 0.0 { return false; }
                let total = (s.draw_area + s.old_fill_area + s.new_fill_area) / s.tile_area * 100.0;
                total < min_required
            })
            .collect();
        if !under.is_empty() {
            println!("  Under-density tiles ({} below {:.1}%):", under.len(), min_required);
            for (_, _, s) in &under {
                let tx0 = x_min + s.ix as f64 * tile_size;
                let tx1 = (tx0 + tile_size).min(x_max);
                let ty0 = y_min + s.iy as f64 * tile_size;
                let ty1 = (ty0 + tile_size).min(y_max);
                let total = (s.draw_area + s.old_fill_area + s.new_fill_area) / s.tile_area * 100.0;
                println!(
                    "    Tile [{:2},{:2}] [({:8.1}, {:8.1}) - ({:8.1}, {:8.1}) µm]: \
                     {:.2}% (need {:.1}%, gap {:.3}%)",
                    s.ix, s.iy,
                    tx0 * dbu, ty0 * dbu, tx1 * dbu, ty1 * dbu,
                    total, min_required, min_required - total,
                );
            }
        }

        // Overall density for this layer
        let tot_draw  : f64 = tile_results.iter().map(|(_, _, s)| s.draw_area).sum();
        let tot_old   : f64 = tile_results.iter().map(|(_, _, s)| s.old_fill_area).sum();
        let tot_new   : f64 = tile_results.iter().map(|(_, _, s)| s.new_fill_area).sum();
        let da = density_area.max(1.0);  // avoid div-by-zero on tiny chips
        println!(
            "  Overall        draw {:5.2}%  old {:5.2}%  new {:5.2}%  -> {:5.2}%  \
             (target {:.0}% +/- {:.0}%)",
            (tot_draw  / da * 100.0).max(0.0),
            (tot_old   / da * 100.0).max(0.0),
            (tot_new   / da * 100.0).max(0.0),
            ((tot_draw + tot_old + tot_new) / da * 100.0).max(0.0),
            target_density, deviation,
        );

        // Timing summary
        println!("  {:<18} {:>8.2?}  ({} shapes)", "fill:", fill_elapsed, layer_shapes);
        println!("  {:<18} {:>8.2?}", "layer total:", layer_t.elapsed());

        all_new_boundaries.extend(tile_results.into_iter().flat_map(|(b, _, _)| b));
        total_shapes += layer_shapes;
    }

    if dryrun {
        println!("Fill in dry run. Skip writting metal dummy fill.");
        return Ok(());
    }

    // Append fill shapes to the top-level cell
    if !all_new_boundaries.is_empty() {
        let referenced: HashSet<String> = lib.structs.iter()
            .flat_map(|s| s.elems.iter())
            .filter_map(|e| match e {
                GdsElement::GdsStructRef(r) => Some(r.name.clone()),
                GdsElement::GdsArrayRef(r)  => Some(r.name.clone()),
                _ => None,
            })
            .collect();

        let top_name = lib.structs.iter()
            .find(|s| !referenced.contains(&s.name) && !s.name.starts_with("$$"))
            .map(|s| s.name.clone())
            .ok_or_else(|| anyhow!("No top-level cell found in GDS library"))?;

        println!("Writing {} shape(s) to top cell '{}'", all_new_boundaries.len(), top_name);

        if let Some(top) = lib.structs.iter_mut().find(|s| s.name == top_name) {
            for b in all_new_boundaries {
                top.elems.push(GdsElement::GdsBoundary(b));
            }
        }
    }

    println!("Total fill shapes added: {}", total_shapes);

    write_gds(&lib, gds_file)
        .with_context(|| format!("Failed to write GDS file: {}", gds_file.display()))?;

    Ok(())
}

// Shared tile context

/// Common parameters passed to every per-tile fill function.
struct TileCtx {
    running_pct:    f64,
    target_density: f64,
    deviation:      f64,
    dbu:            f64,
    tile_area:      f64,
    grid_dbu:       f64,
}

// Square fill

fn fill_square_tile(
    tile: &Rect<f64>,
    keepout: &[(Rect<f64>, geo::Polygon<f64>)],
    sq: &crate::pdk::SquareParams,
    ctx: &TileCtx,
) -> Vec<Rect<f64>> {
    let TileCtx { running_pct, target_density, deviation, dbu, tile_area, grid_dbu } = *ctx;
    let fill_target_pct = target_density - running_pct;
    if fill_target_pct <= deviation / 2.0 {
        return vec![];
    }

    let min_width_dbu = sq.min_width / dbu;
    let max_width_dbu = sq.max_width / dbu;
    let min_space_dbu = sq.min_space / dbu;
    let max_space_dbu = sq.max_space / dbu;

    // Tile-edge keepout: inset all four sides by min_space so that fill shapes
    // cannot violate the minimum-space rule against shapes in adjacent tiles.
    let b = min_space_dbu;
    let (x0, y0, x1, y1) = (tile.min().x, tile.min().y, tile.max().x, tile.max().y);
    let edge_rects = [
        Rect::new(coord!(x: x0,     y: y0), coord!(x: x0 + b, y: y1)),
        Rect::new(coord!(x: x1 - b, y: y0), coord!(x: x1,     y: y1)),
        Rect::new(coord!(x: x0,     y: y0), coord!(x: x1, y: y0 + b)),
        Rect::new(coord!(x: x0,     y: y1 - b), coord!(x: x1, y: y1)),
    ];
    let mut local_keepout: Vec<(Rect<f64>, geo::Polygon<f64>)> = keepout.to_vec();
    for r in &edge_rects {
        local_keepout.push((*r, r.to_polygon()));
    }

    // Upper bound: densest grid achievable with given constraints.
    let max_theoretical = (max_width_dbu / (max_width_dbu + min_space_dbu)).powi(2) * 100.0;

    // Binary-search on `effective_target`.  First iteration tries `fill_target_pct`
    // directly (analytical approximation without keepout correction) -- this is
    // often already within tolerance.  Subsequent iterations bisect [lo, hi].
    let mut lo = fill_target_pct;
    let mut hi = max_theoretical;

    let mut best_rects: Vec<Rect<f64>> = Vec::new();
    let mut best_diff = f64::INFINITY;

    for i in 0..6 {
        let effective_target = if i == 0 { fill_target_pct } else { (lo + hi) / 2.0 };
        let (raw_size, raw_space) = analytical_params(
            min_width_dbu, max_width_dbu,
            min_space_dbu, max_space_dbu,
            effective_target,
        );
        // Snap size and space to multiples of 2*grid_dbu so that:
        //   half = size/2  is a multiple of grid_dbu
        //   pitch = size+space is a multiple of 2*grid_dbu
        //   drift = pitch/2 is a multiple of grid_dbu
        // This ensures every shape corner lands on the manufacturing grid.
        let g2 = 2.0 * grid_dbu;
        let min_size_snapped  = (min_width_dbu / g2).ceil() * g2;
        let min_space_snapped = (min_space_dbu  / g2).ceil() * g2;
        let size  = ((raw_size  / g2).round() * g2).max(min_size_snapped);
        let space = ((raw_space / g2).round() * g2).max(min_space_snapped);

        let rects = generate_fill(tile, &local_keepout, size, space, min_width_dbu, sq.clipping);

        let actual_pct = rects.iter()
            .map(|r| (r.max().x - r.min().x) * (r.max().y - r.min().y))
            .sum::<f64>() / tile_area * 100.0;

        let diff = (actual_pct - fill_target_pct).abs();
        if diff < best_diff {
            best_diff = diff;
            best_rects = rects;
        }

        if actual_pct < fill_target_pct - deviation / 2.0 {
            lo = effective_target; // too sparse -> denser grid needed
        } else if actual_pct > fill_target_pct + deviation / 2.0 {
            hi = effective_target; // overshot -> sparser grid
        } else {
            break; // within tolerance
        }
    }

    best_rects
}

// Overlap fill

/// Place one GatPoly-style fill rectangle centred on each reference (Activ) fill rect,
/// choosing the gate height analytically to reach the target density.
///
/// **Algorithm**
/// 1. Evaluate each candidate at `max_h` (the tallest legally allowed shape).
///    A rect that passes keepout at `max_h` will also pass at any `h <= max_h`
///    (same centre, strictly smaller).
/// 2. Compute the `h` needed so that `sum w_i * h ~ fill_target * tile_area`.
/// 3. Snap `h` to the manufacturing grid, clamp to `[min_width, min(max_h_i)]`.
/// 4. Place all surviving candidates at that uniform `h`.
fn fill_overlap_tile(
    keepout: &[(Rect<f64>, geo::Polygon<f64>)],
    op: &crate::pdk::OverlapParams,
    ref_rects: &[Rect<f64>],
    ctx: &TileCtx,
) -> Vec<Rect<f64>> {
    let TileCtx { running_pct, target_density, deviation, dbu, tile_area, grid_dbu } = *ctx;
    if ref_rects.is_empty() { return vec![]; }

    let fill_target_pct = target_density - running_pct;
    if fill_target_pct <= deviation / 2.0 { return vec![]; }

    let min_width_dbu = op.min_width     / dbu;
    let max_width_dbu = op.max_width     / dbu;
    let min_ext_dbu   = op.min_extension / dbu;
    let g2 = 2.0 * grid_dbu;
    let min_h_grid = (min_width_dbu / g2).ceil() * g2;
    let max_w_grid = (max_width_dbu / g2).floor() * g2;

    // Pass 1: find candidates that survive keepout at their individual max_h.
    struct Candidate { cx: f64, cy: f64, w: f64 }
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut sum_w  = 0.0_f64;
    let mut cap_h  = f64::INFINITY; // minimum of all max_h_i -> uniform h cap

    for &activ in ref_rects {
        let activ_w = activ.max().x - activ.min().x;
        let activ_h = activ.max().y - activ.min().y;
        let cx = activ.min().x + activ_w / 2.0;
        let cy = activ.min().y + activ_h / 2.0;

        let raw_w = activ_w + 2.0 * min_ext_dbu;
        let w = ((raw_w / g2).round() * g2)
            .clamp((min_width_dbu / g2).ceil() * g2, max_w_grid);

        let max_h_grid = ((activ_h - 2.0 * min_ext_dbu).min(max_width_dbu) / g2).floor() * g2;
        if max_h_grid < min_h_grid { continue; }

        // Test keepout at max_h -- conservative: any smaller h will also pass.
        let half_w = w / 2.0;
        let half_h = max_h_grid / 2.0;
        let test = Rect::new(
            coord!(x: cx - half_w, y: cy - half_h),
            coord!(x: cx + half_w, y: cy + half_h),
        );
        let test_poly = test.to_polygon();
        let ok = !keepout.iter().any(|(ko_bbox, ko_poly)| {
            test.intersects(ko_bbox) && test_poly.intersects(ko_poly)
        });
        if !ok { continue; }

        sum_w += w;
        cap_h = cap_h.min(max_h_grid);
        candidates.push(Candidate { cx, cy, w });
    }

    if candidates.is_empty() { return vec![]; }

    // Pass 2: compute uniform h analytically, snap to grid.
    let required_area = fill_target_pct / 100.0 * tile_area;
    let h_needed = required_area / sum_w;
    let h = ((h_needed / g2).round() * g2).clamp(min_h_grid, cap_h);

    candidates.iter().map(|c| {
        let half_w = c.w / 2.0;
        let half_h = h / 2.0;
        Rect::new(
            coord!(x: c.cx - half_w, y: c.cy - half_h),
            coord!(x: c.cx + half_w, y: c.cy + half_h),
        )
    }).collect()
}

// Track fill

/// Place a regular grid of `fw * fh` rectangles inside each core polygon,
/// restricted to the tile and filtered against keepout.
///
/// `fw` is the fixed perpendicular dimension (`cell_height`); `fh` is computed
/// analytically to achieve `target_density` within the track grid:
///
///   density = fw*fh / ((fw+gaps)*(fh+gaps))
///   -> fh = density*(fw+gaps)*gaps / (fw - density*(fw+gaps))
/// Fill a tile with track-aligned rectangles, using multiple passes to maximise density.
///
/// # Pass sizes
/// Both dimensions of each fill are equal (square fills) at sizes derived from the track
/// pitch: `n_max * gaps`, `floor(6/10 * n_max) * gaps`, `floor(3/10 * n_max) * gaps`, `min_width`,
/// where `n_max = floor(max_width / gaps)`.  For M2/M4 (gaps = 0.48 µm, max = 5.0 µm) this
/// gives **4.80, 2.88, 1.44, 1.00 µm** exactly as requested.
///
/// # Grid alignment
/// The perpendicular pitch for each size is the minimum multiple of `track_pitch` that
/// satisfies `min_space`: `pitch_perp = ceil((size + min_space) / gaps) * gaps`.  Fill centres
/// therefore always land on routing-track positions.  The number of track offsets tried per
/// size equals the number of tracks in one pitch period (`pitch_perp / gaps`), covering
/// every unique routing-track position.
///
/// # Tile-boundary safety
/// In the free (along-routing) direction, a `min_space` margin is kept at the upper/right
/// tile edge so that fills from different size passes in adjacent tiles never violate DRC.
fn fill_track_tile(
    tile: &Rect<f64>,
    keepout: &[(Rect<f64>, geo::Polygon<f64>)],
    tp: &crate::pdk::TrackParams,
    core_polys: &[geo::Polygon<f64>],
    ctx: &TileCtx,
    track_phase_x: f64,
    track_phase_y: f64,
) -> Vec<Rect<f64>> {
    use crate::pdk::TrackOrientation;
    let TileCtx { running_pct, target_density, deviation, dbu, tile_area, grid_dbu } = *ctx;

    if core_polys.is_empty() { return vec![]; }

    let fill_target_pct = target_density - running_pct;
    if fill_target_pct <= deviation / 2.0 { return vec![]; }

    let gaps_dbu      = tp.gaps      / dbu;
    let min_space_dbu = tp.min_space / dbu;
    let min_size      = (tp.min_width / dbu / (2.0 * grid_dbu)).ceil() * (2.0 * grid_dbu);
    let max_size      = (tp.max_width / dbu / (2.0 * grid_dbu)).floor() * (2.0 * grid_dbu);
    let g2            = 2.0 * grid_dbu;

    if min_size <= 0.0 || gaps_dbu <= 0.0 { return vec![]; }

    // Perpendicular sizes (across routing tracks): from pass_fracs in pdk/mod.rs.
    // Each fraction f -> floor(f * n_max) * gaps, snapped to mfg grid.
    // min_width is always appended as the final perpendicular size.
    let n_max = (tp.max_width / tp.gaps).floor() as i64;
    let perp_sizes: Vec<f64> = {
        let mut v: Vec<f64> = Vec::new();
        for &frac in tp.pass_fracs {
            let n = (frac * n_max as f64).floor() as i64;
            if n >= 1 {
                let s = ((n as f64 * gaps_dbu) / g2).round() * g2;
                let s = s.clamp(min_size, max_size);
                if v.last().is_none_or(|&last| (last - s).abs() > g2 * 0.5) {
                    v.push(s);
                }
            }
        }
        if v.last().is_none_or(|&last| (last - min_size).abs() > g2 * 0.5) {
            v.push(min_size);
        }
        v
    };

    // Free-direction sizes (along routing): from free_heights_um in pdk/mod.rs.
    // Snapped to mfg grid and clamped to [min_size, max_size]; duplicates dropped.
    let free_sizes: Vec<f64> = {
        let mut v: Vec<f64> = Vec::new();
        for &h in tp.free_heights_um {
            let s = ((h / dbu) / g2).round() * g2;
            let s = s.clamp(min_size, max_size);
            if s > 0.0 && v.last().is_none_or(|&last| (last - s).abs() > g2 * 0.5) {
                v.push(s);
            }
        }
        v
    };

    if perp_sizes.is_empty() || free_sizes.is_empty() { return vec![]; }

    // local_keepout accumulates placed fills so min_space is enforced across all passes.
    let mut local_keepout: Vec<(Rect<f64>, geo::Polygon<f64>)> = keepout.to_vec();
    let mut results: Vec<Rect<f64>> = Vec::new();
    let mut placed_area = 0.0_f64;
    let n_perp_sizes = perp_sizes.len();

    // Outer loop: free-direction height (large -> small).
    // Inner loop: perpendicular size (large -> small).
    // For the smallest perp size, also try half-track offsets to fill between tracks.
    'passes: for free_size in &free_sizes {
        let free_size = *free_size;
        let half_free = free_size / 2.0;
        // Free-direction pitch: just needs min_space between shapes.
        let pitch_free = free_size + min_space_dbu;

        for (perp_idx, perp_size) in perp_sizes.iter().enumerate() {
            let perp_size = *perp_size;
            let is_last_perp = perp_idx == n_perp_sizes - 1;
            let half_perp = perp_size / 2.0;

            // Perpendicular pitch: minimum multiple of track_pitch satisfying min_space.
            let n_perp     = ((perp_size + min_space_dbu) / gaps_dbu).ceil() as i64;
            let pitch_perp = n_perp as f64 * gaps_dbu;

            // Assign X/Y roles based on routing orientation.
            let (pitch_x, pitch_y, half_x, half_y) = match tp.orientation {
                TrackOrientation::Vertical   => (pitch_perp, pitch_free, half_perp, half_free),
                TrackOrientation::Horizontal => (pitch_free, pitch_perp, half_free, half_perp),
            };

            // For the smallest perp size, also try half-track offsets so fills can
            // land between existing track-aligned shapes.
            let offsets_perp: Vec<f64> = if is_last_perp {
                (0..n_perp as usize).flat_map(|i| {
                    let base = i as f64 * gaps_dbu;
                    [base, base + gaps_dbu / 2.0]
                }).collect()
            } else {
                (0..n_perp as usize).map(|i| i as f64 * gaps_dbu).collect()
            };

            for offset_perp in offsets_perp {
                for core_poly in core_polys {
                    let core_bbox = match core_poly.bounding_rect() { Some(b) => b, None => continue };
                    let bx0 = core_bbox.min().x.max(tile.min().x);
                    let by0 = core_bbox.min().y.max(tile.min().y);
                    let bx1 = core_bbox.max().x.min(tile.max().x);
                    let by1 = core_bbox.max().y.min(tile.max().y);
                    if bx1 - bx0 < half_x * 2.0 || by1 - by0 < half_y * 2.0 { continue; }

                    // Global anchors anchored to core lower-left (stable across tiles).
                    let (anchor_x, anchor_y) = match tp.orientation {
                        TrackOrientation::Vertical   => (track_phase_x + offset_perp, track_phase_y + half_free),
                        TrackOrientation::Horizontal => (track_phase_x + half_free,   track_phase_y + offset_perp),
                    };
                    let n_x = ((bx0 + half_x - anchor_x) / pitch_x).ceil() as i64;
                    let n_y = ((by0 + half_y - anchor_y) / pitch_y).ceil() as i64;
                    let cx_start = anchor_x + n_x as f64 * pitch_x;
                    let cy_start = anchor_y + n_y as f64 * pitch_y;

                    // Guard at the upper/right tile edge in the free direction: fills from
                    // different free_size passes in adjacent tiles keep >= min_space.
                    let (x_limit, y_limit) = match tp.orientation {
                        TrackOrientation::Vertical   => (bx1,                    by1 - min_space_dbu),
                        TrackOrientation::Horizontal => (bx1 - min_space_dbu,    by1),
                    };

                    let mut cx = cx_start;
                    while cx + half_x <= x_limit {
                        let mut cy = cy_start;
                        while cy + half_y <= y_limit {
                            let r = Rect::new(
                                coord!(x: cx - half_x, y: cy - half_y),
                                coord!(x: cx + half_x, y: cy + half_y),
                            );
                            let r_poly = r.to_polygon();
                            if !local_keepout.iter().any(|(ko_bbox, ko_poly)| {
                                r.intersects(ko_bbox) && r_poly.intersects(ko_poly)
                            }) {
                                let expanded = Rect::new(
                                    coord!(x: r.min().x - min_space_dbu, y: r.min().y - min_space_dbu),
                                    coord!(x: r.max().x + min_space_dbu, y: r.max().y + min_space_dbu),
                                );
                                local_keepout.push((expanded, expanded.to_polygon()));
                                placed_area += perp_size * free_size;
                                results.push(r);
                            }
                            cy += pitch_y;
                        }
                        cx += pitch_x;
                    }
                }
            }
        }

        // Stop early once target density is met (checked after each free_size pass).
        if tile_area > 0.0
            && running_pct + placed_area / tile_area * 100.0 >= target_density - deviation / 2.0
        {
            break 'passes;
        }
    }

    results
}

// GDS output helpers

fn rect_to_boundary(r: Rect<f64>, layer: i16, datatype: i16) -> GdsBoundary {
    let x0 = r.min().x.round() as i32;
    let y0 = r.min().y.round() as i32;
    let x1 = r.max().x.round() as i32;
    let y1 = r.max().y.round() as i32;
    GdsBoundary {
        layer,
        datatype,
        xy: vec![
            GdsPoint::new(x0, y0),
            GdsPoint::new(x1, y0),
            GdsPoint::new(x1, y1),
            GdsPoint::new(x0, y1),
            GdsPoint::new(x0, y0),
        ],
        ..Default::default()
    }
}

fn poly_to_boundary(poly: &geo::Polygon<f64>, layer: i16, datatype: i16) -> GdsBoundary {
    let xy: Vec<GdsPoint> = poly.exterior().0.iter()
        .map(|c| GdsPoint::new(c.x.round() as i32, c.y.round() as i32))
        .collect();
    GdsBoundary { layer, datatype, xy, ..Default::default() }
}

// Geometry helpers

fn clip_to_tile(r: Rect<f64>, tile: &Rect<f64>) -> Rect<f64> {
    Rect::new(
        coord!(x: r.min().x.max(tile.min().x), y: r.min().y.max(tile.min().y)),
        coord!(x: r.max().x.min(tile.max().x), y: r.max().y.min(tile.max().y)),
    )
}

// Grid iteration

fn iter_fill_positions(
    tile: &Rect<f64>,
    size: f64,
    space: f64,
    clipping: bool,
    mut f: impl FnMut(Rect<f64>),
) {
    let pitch = size + space;
    if pitch <= 0.0 { return; }

    let half  = size / 2.0;
    let drift = (pitch / 2.0).ceil();
    let extra = if clipping { half } else { 0.0 };

    let mut ix: usize = 0;
    let mut cx = tile.min().x + half;
    while cx <= tile.max().x + extra {
        let y_offset = if ix % 2 == 1 { drift } else { 0.0 };
        let mut cy = tile.min().y + half + y_offset;
        while cy <= tile.max().y + extra {
            f(Rect::new(
                coord!(x: cx - half, y: cy - half),
                coord!(x: cx + half, y: cy + half),
            ));
            cy += pitch;
        }
        cx += pitch;
        ix += 1;
    }
}

// Placement validity

fn is_valid_placement(
    tile: &Rect<f64>,
    fill: &Rect<f64>,
    keepout: &[(Rect<f64>, geo::Polygon<f64>)],
    clipping: bool,
) -> bool {
    let cx = (fill.min().x + fill.max().x) / 2.0;
    let cy = (fill.min().y + fill.max().y) / 2.0;

    if cx < tile.min().x || cx > tile.max().x || cy < tile.min().y || cy > tile.max().y {
        return false;
    }
    if !clipping
        && (fill.min().x < tile.min().x || fill.max().x > tile.max().x
            || fill.min().y < tile.min().y || fill.max().y > tile.max().y)
    {
        return false;
    }
    // Bbox pre-filter (4 comparisons) before the full polygon intersection check.
    // Eliminates nearly all checks for tiny keepout shapes like Cont contacts.
    let fill_poly = fill.to_polygon();
    !keepout.iter().any(|(ko_bbox, ko_poly)| {
        fill.intersects(ko_bbox) && fill_poly.intersects(ko_poly)
    })
}

// Analytical parameter computation

/// Compute fill (size, space) analytically from the target density.
///
/// For the checkerboard grid in `iter_fill_positions`, each `pitch * pitch` cell
/// contains exactly one fill rect of area `size²`, giving:
///
///   density = (size / pitch)²   where pitch = size + space
///
/// Solving for the target density ratio `r = sqrt(target / 100)`:
///
///   size  = r * pitch
///   space = (1 - r) * pitch
///
/// The feasible pitch range is derived from the min/max size and space constraints.
/// The largest feasible pitch is chosen (fewer, larger shapes -> cleaner layout).
fn analytical_params(
    min_width: f64,
    max_width: f64,
    min_space: f64,
    max_space: f64,
    target_pct: f64,
) -> (f64, f64) {
    let r = (target_pct / 100.0).clamp(0.0, 1.0).sqrt();

    // Degenerate cases: no fill or maximum fill.
    if r <= 0.0 { return (min_width, max_space); }
    if r >= 1.0 { return (max_width, min_space); }

    // Derive feasible pitch range from all four constraints.
    let pitch_lo = (min_width / r).max(min_space / (1.0 - r));
    let pitch_hi = (max_width / r).min(max_space / (1.0 - r));

    // Pick pitch within [pitch_lo, pitch_hi]; clamp if infeasible.
    let pitch = pitch_hi.max(pitch_lo);

    let size  = (r * pitch).clamp(min_width, max_width);
    let space = ((1.0 - r) * pitch).clamp(min_space, max_space);
    (size, space)
}

// Fill shape generation

fn generate_fill(
    tile: &Rect<f64>,
    keepout: &[(Rect<f64>, geo::Polygon<f64>)],
    size: f64,
    space: f64,
    min_width: f64,
    clipping: bool,
) -> Vec<Rect<f64>> {
    let mut out = Vec::new();
    iter_fill_positions(tile, size, space, clipping, |fill_rect| {
        if is_valid_placement(tile, &fill_rect, keepout, clipping) {
            let r = if clipping { clip_to_tile(fill_rect, tile) } else { fill_rect };
            let w = r.max().x - r.min().x;
            let h = r.max().y - r.min().y;
            if w >= min_width && h >= min_width {
                out.push(r);
            }
        }
    });
    out
}
