"""
Layer filler driver.

Coordinates filler insertion by selecting the appropriate algorithm
and applying it to a given tile and layer.
"""
import gdstk

from gdsfill.library.filler.helper import calculate_density
from gdsfill.library.filler.overlap import fill_overlap
from gdsfill.library.filler.square import fill_square
from gdsfill.library.filler.track import fill_track


ALGOS = {
    'Overlap': fill_overlap,
    'Square': fill_square,
    'Track': fill_track,
}


# pylint: disable=too-many-arguments, too-many-positional-arguments, too-many-locals
def fill_layer(pdk, inputfile, layer, queue, tiles, tile):
    """
    Fill a layout layer to meet density requirements.

    Reads a GDS file, checks density, and applies one or more filler
    algorithms (square or track) as defined in the PDK.

    Args:
        pdk (object): Provides layer rules and supported algorithms.
        inputfile (Path | str): Path to the input GDS file.
        layer (str): Target layer name.
        queue (Queue): Queue object to return results.
        tiles (dict): Tiling information for the layout.
        tile (object): Current tile instance.

    Returns:
        bool: True if successful, False if algorithm unsupported.
    """
    library = gdstk.read_gds(inputfile, unit=1e-6)
    annotated_cell = library.top_level()[0]

    metal_density = calculate_density(annotated_cell)
    desired_density = pdk.get_layer_density(layer)
    fill_algos = pdk.get_layer_algorithm(layer)

    for fill_algo in fill_algos:
        if fill_algo not in ALGOS:
            queue.put(("error", f"Unknown fill algorithm {fill_algo} for layer {layer}"))
            return False

    for fill_algo in fill_algos:
        if not pdk.has_fill_algorithm(layer, fill_algo):
            queue.put(("error", f"Unsupported fill algorithm {fill_algo} for layer {layer}"))
            return False

    msg = f"Metal density {metal_density: <5} % (target {desired_density} %)"
    fill_lib = gdstk.Library("fill")
    if metal_density < desired_density:
        fills = []
        for fill_algo in fill_algos:
            fill_cell, result = ALGOS[fill_algo](pdk, layer, tiles, tile, annotated_cell)
            fill_lib.add(fill_cell)
            fills.append((fill_algo, result))
        final_fill = round(metal_density + sum(fill for _, fill in fills), 2)
        queue.put(("success", f"{msg} - final density {final_fill} % "))
    else:
        queue.put(("skipped", f"{msg} - exceeds limit"))

    fill_lib.write_gds(str(inputfile).replace('modified', 'filled'))
    return True
