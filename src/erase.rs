use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use gds21::GdsLibrary;

use crate::{
    is_fill_element, get_target_layers, read_gds, write_gds,
    RunContext, DEBUG_KEEPOUT_DT, DEBUG_MERGED_DT
};

/// Erase dummy fill from `gds_file` in-place.
///
/// If `ctx.config` lists specific layers, only those are erased; otherwise all
/// PDK fill datatypes are removed. Debug shapes (datatypes 250/251) are always
/// erased alongside fill.
pub fn run(gds_file: &Path, ctx: RunContext) -> Result<()> {
    let erase_targets = get_target_layers(&ctx);

    println!("Erasing fill from {} layer(s):", erase_targets.len());
    for (name, layer) in &erase_targets {
        println!("  {:<12} (layer {:>3}, datatype {})", name, layer.gds_layer, layer.fill_datatype);
    }

    let mut lib: GdsLibrary = read_gds(gds_file)
        .with_context(|| format!("Failed to read GDS file: {}", gds_file.display()))?;

    let fill_only: HashSet<(i16, i16)> = erase_targets.iter()
        .map(|&(_, layer)| (layer.gds_layer, layer.fill_datatype))
        .collect();
    let debug_only: HashSet<(i16, i16)> = erase_targets.iter()
        .flat_map(|&(_, layer)| [
            (layer.gds_layer, DEBUG_KEEPOUT_DT),
            (layer.gds_layer, DEBUG_MERGED_DT),
        ])
        .collect();
    let fill_and_debug: HashSet<(i16, i16)> = fill_only.iter().copied()
        .chain(debug_only.iter().copied())
        .collect();

    let mut erased_fill  = 0usize;
    let mut erased_debug = 0usize;
    for gds_struct in &mut lib.structs {
        for elem in &gds_struct.elems {
            if is_fill_element(elem, &fill_only)  {
                erased_fill  += 1;
            }
            if is_fill_element(elem, &debug_only) {
                erased_debug += 1;
            }
        }
        gds_struct.elems.retain(|elem| !is_fill_element(elem, &fill_and_debug));
    }

    if erased_debug > 0 {
        println!("Erased {} debug element(s) from '{}'", erased_debug, gds_file.display());
    }
    if erased_fill > 0 {
        println!("Erased {} fill element(s) from '{}'",  erased_fill,  gds_file.display());
    }
    if erased_fill == 0 && erased_debug == 0 {
        println!("Nothing to erase in '{}'", gds_file.display());
    }

    write_gds(&lib, gds_file)
        .with_context(|| format!("Failed to write GDS file: {}", gds_file.display()))?;

    Ok(())
}
