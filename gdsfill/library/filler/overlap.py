"""
Module for generating overlap filler polygons in GDS layouts.

This module provides functionality to insert filler polygons into an
annotated GDS cell based on overlap fill rules from the PDK.
The algorithm iteratively adjusts filler dimensions until the target
density is reached or the maximum recursion depth is exceeded.
"""
# pylint: disable=too-many-locals, too-many-arguments, too-many-positional-arguments
import math
import itertools
from dataclasses import dataclass
from typing import Tuple
import gdstk
from gdsfill.library.filler.helper import (
    calculate_density,
    calculate_fill_density,
    create_polygon,
    get_box_dimension,
    get_cell_distance,
    get_center_point,
    get_polygons
)


Point = Tuple[float, float]


# pylint: disable=duplicate-code
@dataclass(frozen=True)
class OverlapParameter:
    """Immutable parameters for overlap fill iteration.

    Attributes:
        width (Point): Current min and max width values.
        height (Point): Current min and max height values.
        max_height (float): Maximum allowed filler height.
        step (float): Increment step for width and height during iteration.
        density (float): Initial tile density before filler insertion.
        max_depth (int): Maximum recursion depth.
        fill_density (float): Accumulated filler density, defaults to 0.0.
    """
    width: Point
    height: Point
    max_height: float
    step: float
    density: float
    max_depth: int
    fill_density: float = 0.0

    def __post_init__(self):
        if self.max_depth < 0:
            raise ValueError("max_depth must be >= 0")
        if self.step < 0:
            raise ValueError("step must be >= 0")
        if any(v < 0 for v in self.width):
            raise ValueError("width values must be positive")
        if any(v < 0 for v in self.height):
            raise ValueError("height values must be positive")

    def next(self, width: Point, height: Point, fill_density: float):
        """Create a new OverlapParameter with updated values.

        Args:
            width (Point): New width range.
            height (Point): New height range.
            fill_density (float): Updated fill density.

        Returns:
            OverlapParameter: New parameter instance with decremented
            max_depth.
        """
        return OverlapParameter(width=width, height=height, max_height=self.max_height,
                                step=self.step, density=self.density, max_depth=self.max_depth - 1,
                                fill_density=fill_density)


# pylint: disable=unused-argument
def fill_overlap(pdk, layer: str, tiles, tile, annotated_cell):
    """Generate overlap filler polygons for a given layer.

    Args:
        pdk: Process design kit providing fill rules and layer info.
        layer (str): Target layer name.
        tiles: Collection of tiles (unused placeholder).
        tile: Current tile being processed (unused placeholder).
        annotated_cell: GDS cell with annotations for placement and keep-out.

    Returns:
        tuple[gdstk.Cell, str]: Cell containing filler polygons and fill result.
    """
    fill_rules = pdk.get_fill_rules(layer, 'Overlap')
    max_depth = pdk.get_layer_max_depth(layer)
    max_size = fill_rules['max_width']

    density = calculate_density(annotated_cell)

    references = []
    for poly in get_polygons(annotated_cell, 'reference'):
        references.append((get_center_point(poly.points), get_box_dimension(poly.points)))

    if not references:
        return (gdstk.Cell(name='FILLER_CELL_OVERLAP_EMPTY'), 0.0)

    for index in range(0, 10):
        if not (cell_distance := get_cell_distance(references, index)):
            return (gdstk.Cell(name='FILLER_CELL_OVERLAP_EMPTY'), 0.0)

        if cell_distance < 10:
            break

    if cell_distance >= 10:
        return (gdstk.Cell(name='FILLER_CELL_OVERLAP_EMPTY'), 0.0)

    cell_distance = round(math.ceil(((cell_distance / 2) - 0.81) / 0.005) * 0.005, 3)

    step = max_size / max_depth
    step = 0.5
    parameter = OverlapParameter(width=(0.0, step), height=(0.0, step), max_height=cell_distance,
                                 step=step, density=density, max_depth=max_depth)

    return _fill_overlap(pdk, layer, annotated_cell, references, parameter)


def _fill_overlap(pdk, layer: str, annotated_cell, references, parameter):
    """Recursively adjust filler dimensions to reach target density.

    Args:
        pdk: Process design kit providing density targets and rules.
        layer (str): Target layer name.
        annotated_cell: GDS cell with chip and keep-out annotations.
        references (list[tuple]): List of reference centers and dimensions.
        parameter (OverlapParameter): Current overlap parameters.

    Returns:
        gdstk.Cell: Cell containing filler polygons with density as
        close as possible to the target.
    """
    values = list(itertools.product(parameter.width, parameter.height))

    results = []
    for (width_, height_) in values:
        filler_grid = _fill_overlap_logic(pdk, layer, annotated_cell, references, width_, height_,
                                          parameter.max_height)
        fill_density = calculate_fill_density(annotated_cell, filler_grid)
        tile_density = round(parameter.density + fill_density, 2)
        results.append((tile_density, filler_grid, width_, height_))

    closest = min(results, key=lambda x: abs(x[0] - pdk.get_layer_density(layer)))
    del results

    max_depth = parameter.max_depth - 1
    if max_depth == 0:
        return (closest[1], round(closest[0] - parameter.density, 2))
    if parameter.fill_density >= closest[0]:
        return (closest[1], round(closest[0] - parameter.density, 2))

    width = (closest[2], closest[2] + parameter.step)
    height = (closest[3], closest[3] + parameter.step)
    parameter = parameter.next(width, height, closest[0])

    return _fill_overlap(pdk, layer, annotated_cell, references, parameter)


def _fill_overlap_logic(pdk, layer: str, annotated_cell, references, width, height, max_height):
    """Construct filler polygons for given parameters and apply filtering.

    Args:
        pdk: Process design kit providing fill rules and layer info.
        layer (str): Target layer name.
        annotated_cell: GDS cell with placement and keep-out annotations.
        references (list[tuple]): Reference centers and dimensions.
        width (float): Additional filler width to apply.
        height (float): Additional filler height to apply.
        max_height (float): Maximum filler height allowed.

    Returns:
        gdstk.Cell: Cell containing filtered filler polygons.
    """
    layerindex = pdk.get_layer_index(layer)
    datatype = pdk.get_layer_fill_datatype(layer)
    fill_rules = pdk.get_fill_rules(layer, 'Overlap')
    min_ext = fill_rules['min_extension']
    min_ext_both = min_ext * 2 + 0.01

    lib = gdstk.Library(name="filler")
    filler = lib.new_cell('FILLER')
    for center, (ref_width, ref_height) in references:
        height_ = fill_rules['min_width'] + height
        height_ = min(height_, max_height)
        height_ = min(height_, ref_height - min_ext_both)
        height_ = min(height_, fill_rules['max_width'])
        width_ = ref_width + min_ext_both + width
        width_ = min(width_, fill_rules['max_width'])
        polygon = gdstk.Polygon(create_polygon(center, width_, height_), layer=layerindex,
                                datatype=datatype)
        filler.add(polygon)

    filler_cell = gdstk.Cell(name='FILLER_CELL_OVERLAP')
    chip_placement = get_polygons(annotated_cell, 'placement_chip')
    keep_out = get_polygons(annotated_cell, 'keep_out')
    for fill in filler.get_polygons():
        in_placement = gdstk.inside(fill.points, chip_placement)
        outside_keep_out = gdstk.inside(fill.points, keep_out)
        if any(in_placement) and not any(outside_keep_out):
            filler_cell.add(fill)

    del filler
    del lib

    return filler_cell
