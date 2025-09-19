"""
Square-based filler cell generation.

Provides functions to insert square filler geometries into annotated cells
based on layer rules, spacing, and density targets.
"""
# pylint: disable=too-many-locals, too-many-arguments, too-many-positional-arguments
from dataclasses import dataclass
from typing import Tuple
import itertools
import gdstk
from gdsfill.library.filler.helper import (
    calculate_density,
    calculate_fill_density,
    check_is_square,
    get_polygons,
    get_box_dimension,
    midpoint_snapped
)


Point = Tuple[float, float]


# pylint: disable=duplicate-code
@dataclass(frozen=True)
class SquareParameter:
    """Immutable container for geometric square parameters used in recursive
    subdivision or fill generation algorithms.

    Attributes:
        size (Point): The (width, height) of the square. Both values must be positive.
        space (Point): The spacing between adjacent squares. Both values must be non-negative.
        position (Point): The (x, y) coordinates of the lower-left corner of the square.
            Both values must be non-negative.
        density (float): The target fill density, typically expressed as a fraction
            between 0.0 and 1.0.
        max_depth (int): Maximum recursion depth allowed. Must be zero or positive.
            Each call to `next()` decreases this by 1.
        fill_density (float): Actual fill density achieved for this square.
            Defaults to 0.0.
    """
    size: Point
    space: Point
    position: Point
    density: float
    max_depth: int
    fill_density: float = 0.0

    def __post_init__(self):
        if self.max_depth < 0:
            raise ValueError("max_depth must be >= 0")
        if any(v <= 0 for v in self.size):
            raise ValueError("size values must be positive")
        if any(v < 0 for v in self.space):
            raise ValueError("space values must be positive")
        if any(v < 0 for v in self.position):
            raise ValueError("position values must be positive")

    def next(self, size: Point, space: Point, position: Point, fill_density: float):
        """Create the next-level SquareParameter with updated geometry and one less
        recursion depth.

        Args:
            size (Point): New (width, height) for the subdivided square. Must be positive values.
            space (Point): New spacing between adjacent subdivided squares. Must be non-negative.
            position (Point): New (x, y) coordinates for the subdivided square
                              Must be non-negative.
            fill_density (float): Fill density achieved for this subdivided square.

        Returns:
            SquareParameter: A new SquareParameter instance with updated geometry,
            preserved global density, decreased `max_depth`, and the given `fill_density`.

        Raises:
            ValueError: If the provided size, space, or position values are invalid.
        """
        return SquareParameter(size=size, space=space, position=position, density=self.density,
                               max_depth=self.max_depth - 1, fill_density=fill_density)


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
    start_size = midpoint_snapped(min_size, max_size)
    start_space = midpoint_snapped(min_space, max_space)
    size = (min_size, max_size)
    space = (min_space, max_space)
    position = (start_size, start_space)
    density = calculate_density(annotated_cell)
    max_depth = pdk.get_layer_max_depth(layer)

    parameter = SquareParameter(size=size, space=space, position=position, density=density,
                                max_depth=max_depth)

    return _fill_square(pdk, layer, tile, annotated_cell, parameter)


def _fill_square(pdk, layer: str, tile, annotated_cell, parameter: SquareParameter):
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
        tuple[gdstk.Cell, str]: Cell containing filler polygons and fill result.
    """
    values = list(itertools.product(parameter.size, parameter.space))

    results = []
    for (size_, space_) in values:
        filler_grid = _fill_square_logic(pdk, layer, tile, annotated_cell, size_, space_)
        fill_density = calculate_fill_density(annotated_cell, filler_grid)
        tile_density = round(parameter.density + fill_density, 2)
        results.append((tile_density, filler_grid, size_, space_))

    closest = min(results, key=lambda x: abs(x[0] - pdk.get_layer_density(layer)))
    min_fill = pdk.get_layer_density(layer) - pdk.get_layer_deviation(layer)
    max_fill = pdk.get_layer_density(layer) + pdk.get_layer_deviation(layer)

    if parameter.max_depth == 0:
        return (closest[1], round(closest[0] - parameter.density, 2))
    if closest[0] > min_fill and closest[0] < max_fill:
        return (closest[1], round(closest[0] - parameter.density, 2))

    min_size = min(parameter.position[0], closest[2])
    max_size = max(parameter.position[0], closest[2])
    min_space = min(parameter.position[1], closest[3])
    max_space = max(parameter.position[1], closest[3])
    start_size = midpoint_snapped(min_size, max_size)
    start_space = midpoint_snapped(min_space, max_space)

    size = (min_size, max_size)
    space = (min_space, max_space)
    position = (start_size, start_space)
    parameter = parameter.next(size, space, position, closest[0])

    return _fill_square(pdk, layer, tile, annotated_cell, parameter)


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
    drift = round(((square_size + space) / 2) / 0.005) * 0.005
    for x in range(0, int(tile_width / offset)):
        for y in range(0, int(tile_width / offset)):
            square_offset = drift if x % 2 else 0
            filler.add(gdstk.Reference(cell_ref,
                       origin=(tile.x + x * offset, tile.y + square_offset + y * offset)))

    filler_cell = gdstk.Cell(name='FILLER_CELL_SQUARE')
    valid_fills = gdstk.boolean(filler.get_polygons(),
                                get_polygons(annotated_cell, 'placement_chip'),
                                operation='and', layer=layerindex, datatype=datatype)
    final = gdstk.boolean(valid_fills, get_polygons(annotated_cell, 'keep_out'),
                          operation='not', layer=layerindex, datatype=datatype)

    clipping_disabled = not fill_rules.get('clipping', True)
    for poly in final:
        if poly.size != 4:
            continue
        if clipping_disabled and (square_size, square_size) != get_box_dimension(poly.points):
            continue
        if check_is_square(poly.points, min_width):
            filler_cell.add(poly)
    return filler_cell
