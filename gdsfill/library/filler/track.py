"""
Track-based filler cell generation.

Provides functions to insert filler geometries into annotated cells while
respecting layer rules, spacing, and keep-out regions.
"""
# pylint: disable=too-many-locals, too-many-arguments, too-many-positional-arguments
import gdstk
from gdsfill.library.filler.helper import (
    calculate_core_density,
    calculate_core_fill_density,
    calculate_fill_density,
    check_min_size,
    get_layer,
    get_track_offset,
    get_polygons,
    remove_shortest_edge
)


def add_filler_cells(annotated_cell, filler_cells, gaps: float):
    """
    Add filler cells to the annotated cell with a spacing offset.

    This function offsets the polygons in the filler cell by the given gap
    and adds them to the target annotated cell.

    Args:
        annotated_cell (gdstk.Cell): The cell into which filler polygons are inserted.
        filler_cells (gdstk.Cell): The cell containing candidate filler polygons.
        gaps (float): The spacing offset applied to the filler polygons before insertion.
    """
    poly_with_offset = gdstk.offset(filler_cells.get_polygons(), gaps, **get_layer('keep_out'))
    annotated_cell.add(*(poly for poly in poly_with_offset))


def fill_track(pdk, layer: str, tile, annotated_cell, fill_density: float):
    """
    Fill a layout region with filler cells based on track rules.

    This function computes and places filler cells along the tracks defined
    by the PDK to increase the density of a given region until the minimum
    required density is reached or exceeded.

    Args:
        pdk (object): Process design kit providing fill rules and layer information.
        layer (str): Layer name for which filler is generated.
        tile (object): Current tile with position and size attributes.
        annotated_cell (gdstk.Cell): Target cell where filler polygons are inserted.
        fill_density (float): Density of the existing filler cells in this tile.

    Returns:
        tuple[gdstk.Cell, str]: Cell containing filler polygons and fill result.
    """
    fill_rules = pdk.get_fill_rules(layer, 'Track')
    density = calculate_core_density(annotated_cell) + fill_density
    min_fill = pdk.get_layer_density(layer) - pdk.get_layer_deviation(layer)

    if not get_polygons(annotated_cell, 'placement_core'):
        return (gdstk.Cell(name='FILLER_CELL_TRACK_EMPTY'), 0.0)

    valid_tracks = gdstk.boolean(get_polygons(annotated_cell, 'drawing'),
                                 get_polygons(annotated_cell, 'placement_core'),
                                 operation='and')
    offset_x = get_track_offset(valid_tracks, tile.x, fill_rules['gaps'])
    if offset_x is None:
        return (gdstk.Cell(name='FILLER_CELL_TRACK_EMPTY'), 0.0)
    offset_y = 0

    filler_cells = gdstk.Cell(name='FILLER_CELL_TRACK')
    for step in range(0, 4):
        offsets = (offset_x + step * fill_rules['gaps'], offset_y + fill_rules['gaps'])
        for width in range(50, 10, -5):
            _fill_track_logic(pdk, layer, tile, annotated_cell, filler_cells, width / 10, offsets)
            fill_density = density + calculate_core_fill_density(annotated_cell, filler_cells)
            if fill_density > min_fill:
                tile_fill_density = calculate_fill_density(annotated_cell, filler_cells)
                add_filler_cells(annotated_cell, filler_cells, fill_rules['gaps'])
                return (filler_cells, round(tile_fill_density, 2))

    tile_fill_density = calculate_fill_density(annotated_cell, filler_cells)
    add_filler_cells(annotated_cell, filler_cells, fill_rules['gaps'])
    return (filler_cells, round(tile_fill_density, 2))


def _fill_track_logic(pdk, layer: str, tile, annotated_cell, filler_cells, width: int,
                      offsets: tuple[float, float]):
    """
    Generate filler polygons for a single track iteration.

    This helper function creates candidate filler rectangles aligned to the track
    grid, clips them against keep-out and placement regions, ensures compliance with
    minimum size rules, and adds valid polygons to the filler cell.

    Args:
        pdk (object): Process design kit providing layer indices, datatypes, and rules.
        layer (str): Layer name being filled.
        tile (object): Current tile with x/y position and size attributes.
        annotated_cell (gdstk.Cell): The target cell used for density calculations.
        filler_cells (gdstk.Cell): Cell where valid filler polygons are accumulated.
        width (int): Candidate filler width in track units.
        offsets (tuple[float, float]): (x, y) offsets applied when placing filler cells.
    """
    layerindex = pdk.get_layer_index(layer)
    datatype = pdk.get_layer_fill_datatype(layer)
    fill_rules = pdk.get_fill_rules(layer, 'Track')
    min_width = fill_rules['min_width']
    orientation = fill_rules['orientation']
    if orientation == 'horizontal':
        cell_width = width
        cell_height = fill_rules['cell_height']
    else:
        cell_width = fill_rules['cell_height']
        cell_height = width
    offset_x = cell_width + fill_rules['gaps']
    offset_y = cell_height + fill_rules['gaps']

    lib = gdstk.Library(name="filler")
    rect = gdstk.rectangle((0, 0), (cell_width, cell_height), layer=layerindex, datatype=datatype)
    reference = lib.new_cell('REFERENCE')
    cell_ref = reference.add(rect)

    tile_width = pdk.get_layer_tile_width(layer)
    filler = lib.new_cell('FILLER')

    for x in range(0, int(tile_width / offset_x)):
        for y in range(0, int(tile_width / offset_y)):
            ref_x = tile.x + offsets[0] + x * offset_x
            ref_y = tile.y + offsets[1] + y * offset_y
            filler.add(gdstk.Reference(cell_ref, origin=(ref_x, ref_y)))

    existing_filler = gdstk.offset(filler_cells.get_polygons(), fill_rules['gaps'])
    valid_fills = gdstk.boolean(filler.get_polygons(),
                                get_polygons(annotated_cell, 'placement_core'),
                                operation='and', layer=layerindex, datatype=datatype)

    valid_fills2 = gdstk.boolean(valid_fills, existing_filler,
                                 operation='not', layer=layerindex, datatype=datatype)
    final = gdstk.boolean(valid_fills2, get_polygons(annotated_cell, 'keep_out'),
                          operation='not', layer=layerindex, datatype=datatype)

    aggressive_fill = fill_rules.get('aggressive_fill', False)
    for poly in final:
        if aggressive_fill and poly.size == 8:
            poly = remove_shortest_edge(poly.points, layerindex, datatype)
            poly = remove_shortest_edge(poly.points, layerindex, datatype)
            if check_min_size(poly.points, min_width, min_width):
                filler_cells.add(poly)
        if poly.size == 6:
            poly = remove_shortest_edge(poly.points, layerindex, datatype)
            if check_min_size(poly.points, min_width, min_width):
                filler_cells.add(poly)
        if poly.size == 4 and check_min_size(poly.points, min_width, min_width):
            filler_cells.add(poly)
