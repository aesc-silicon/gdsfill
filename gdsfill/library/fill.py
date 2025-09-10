"""
Layer filler driver.

Coordinates filler insertion by selecting the appropriate algorithm
and applying it to a given tile and layer.
"""
import gdstk

from gdsfill.library.filler.helper import calculate_density
from gdsfill.library.filler.square import fill_square
from gdsfill.library.filler.track import fill_track


ALGOS = {
    'Square': fill_square,
    'Track': fill_track,
}


def fill_layer(pdk, inputfile, layer, tiles, tile):
    """
    Fill a layout layer to meet density requirements.

    Reads a GDS file, checks density, and applies one or more filler
    algorithms (square or track) as defined in the PDK.

    Args:
        pdk (object): Provides layer rules and supported algorithms.
        inputfile (Path | str): Path to the input GDS file.
        layer (str): Target layer name.
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

    print(f"Filling Tile {tile.x}x{tile.y}")
    print(f"Metal density: {metal_density} %")
    print(f"Desired density: {desired_density} % with {pdk.get_layer_deviation(layer)} % deviation")
    print(f"Fill algorithm: {', '.join(fill_algos)}")

    for fill_algo in fill_algos:
        if fill_algo not in ALGOS:
            print(f"Unknown fill algorithm {fill_algo} for layer {layer}")
            return False

    for fill_algo in fill_algos:
        if not pdk.has_fill_algorithm(layer, fill_algo):
            print(f"Unsupported fill algorithm {fill_algo} for layer {layer}")
            return False

    fill_lib = gdstk.Library("fill")
    if metal_density < desired_density:
        for fill_algo in fill_algos:
            fill_cell = ALGOS[fill_algo](pdk, layer, tiles, tile, annotated_cell)
            fill_lib.add(fill_cell)

    fill_lib.write_gds(str(inputfile).replace('modified', 'filled'))
    return True
