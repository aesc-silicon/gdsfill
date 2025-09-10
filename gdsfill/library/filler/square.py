"""
Square-based filler cell generation.

Provides functions to insert square filler geometries into annotated cells
based on layer rules, spacing, and density targets.
"""
# pylint: disable=too-many-locals, too-many-arguments, too-many-positional-arguments
import itertools
import gdstk
from gdsfill.library.filler.helper import (
    calculate_density,
    calculate_fill_density,
    check_is_square,
    get_polygons
)


# pylint: disable=unused-argument
def fill_square(pdk, layer: str, tiles, tile, annotated_cell):
    """
    Place square fillers in a tile until density is within limits.

    Args:
        pdk (object): Provides layer rules.
        layer (str): Target layer.
        tiles (dict): Tiling information.
        tile (object): Current tile instance.
        annotated_cell (gdstk.Cell): Cell to update.

    Returns:
        gdstk.Cell: Cell containing inserted filler polygons.
    """
    fill_rules = pdk.get_fill_rules(layer, 'Square')
    min_size = fill_rules['min_width']
    max_size = fill_rules['max_width']
    min_space = fill_rules['min_space']
    max_space = fill_rules['max_space']
    start_size = round(round((((max_size - min_size) / 2) + min_size) / 0.005) * 0.005, 3)
    start_space = round(round((((max_space - min_space) / 2) + min_space) / 0.005) * 0.005, 3)
    size = (min_size, max_size)
    space = (min_space, max_space)
    position = (start_size, start_space)
    density = calculate_density(annotated_cell)
    max_depth = pdk.get_layer_max_depth(layer)

    return _fill_square(pdk, layer, tile, annotated_cell, size, space, position, density, max_depth)


def _fill_square(pdk, layer: str, tile, annotated_cell, size, space, position, density: float,
                 max_depth: int):
    """
    Iteratively refine square size and spacing to reach density targets.

    Args:
        pdk (object): Provides layer rules.
        layer (str): Target layer.
        tile (object): Current tile instance.
        annotated_cell (gdstk.Cell): Cell to update.
        size (tuple[float, float]): Min and max square sizes.
        space (tuple[float, float]): Min and max spacing.
        position (tuple[float, float]): Current size and spacing start values.
        density (float): Current density of annotated cell.
        max_depth (int): Remaining recursion depth.

    Returns:
        gdstk.Cell: Cell containing filler polygons.
    """
    values = list(itertools.product(size, space))

    results = []
    for (size_, space_) in values:
        filler_grid = _fill_square_logic(pdk, layer, tile, annotated_cell, size_, space_)
        fill_density = calculate_fill_density(annotated_cell, filler_grid)
        tile_density = round(density + fill_density, 3)
        results.append((tile_density, filler_grid, size_, space_))

    closest = min(results, key=lambda x: abs(x[0] - pdk.get_layer_density(layer)))
    min_fill = pdk.get_layer_density(layer) - pdk.get_layer_deviation(layer)
    max_fill = pdk.get_layer_density(layer) + pdk.get_layer_deviation(layer)

    max_depth = max_depth - 1
    if max_depth == 0:
        print(f"Final density {closest[0]} % - reached maximum depth")
        return closest[1]
    if closest[0] > min_fill and closest[0] < max_fill:
        print(f"Final density {closest[0]} %")
        return closest[1]

    min_size = min(position[0], closest[2])
    max_size = max(position[0], closest[2])
    min_space = min(position[1], closest[3])
    max_space = max(position[1], closest[3])
    start_size = round(round((((max_size - min_size) / 2) + min_size) / 0.005) * 0.005, 3)
    start_space = round(round((((max_space - min_space) / 2) + min_space) / 0.005) * 0.005, 3)

    size = (min_size, max_size)
    space = (min_space, max_space)
    position = (start_size, start_space)

    return _fill_square(pdk, layer, tile, annotated_cell, size, space, position, density, max_depth)


def _fill_square_logic(pdk, layer: str, tile, annotated_cell, square_size: float, space: float):
    """
    Generate and validate square filler polygons for a given size and spacing.

    Args:
        pdk (object): Provides layer rules.
        layer (str): Target layer.
        tile (object): Current tile instance.
        annotated_cell (gdstk.Cell): Cell used for overlap checks.
        square_size (float): Candidate square size.
        space (float): Spacing between squares.

    Returns:
        gdstk.Cell: Cell with valid filler polygons.
    """
    layerindex = pdk.get_layer_index(layer)
    datatype = pdk.get_layer_fill_datatype(layer)
    offset = square_size + space
    fill_rules = pdk.get_fill_rules(layer, 'Square')
    min_width = fill_rules['min_width']

    lib = gdstk.Library(name="filler")
    rect = gdstk.rectangle((0, 0), (square_size, square_size), layer=layerindex, datatype=datatype)
    reference = lib.new_cell('REFERENCE')
    cell_ref = reference.add(rect)

    tile_width = pdk.get_layer_tile_width(layer)
    filler = lib.new_cell('FILLER')
    for x in range(0, int(tile_width / offset)):
        for y in range(0, int(tile_width / offset)):
            filler.add(gdstk.Reference(cell_ref,
                       origin=(tile.x + x * offset, tile.y + y * offset)))

    filler_cell = gdstk.Cell(name='FILLER_CELL_SQUARE')
    valid_fills = gdstk.boolean(filler.get_polygons(),
                                get_polygons(annotated_cell, 'placement_chip'),
                                operation='and', layer=layerindex, datatype=datatype)
    final = gdstk.boolean(valid_fills, get_polygons(annotated_cell, 'keep_out'),
                          operation='not', layer=layerindex, datatype=datatype)

    for poly in final:
        if (poly.size == 4 and check_is_square(poly.points, min_width)):
            filler_cell.add(poly)
    return filler_cell
