"""
Helper functions for filler cell generation.

Provides geometry utilities and density calculations used by filler modules,
including polygon validation, size checks, and edge manipulation.
"""
import math
from collections import Counter
from pathlib import Path
import yaml
import gdstk


script = Path(__file__).parent.resolve()
layers = yaml.safe_load((script / "../layers.yaml").read_text(encoding='utf-8'))


def calculate_core_density(top_cell):
    """
    Calculate metal density within the core placement region.

    Args:
        top_cell (gdstk.Cell): Layout cell.

    Returns:
        float: Core density percentage (0–100).
    """
    total_area = sum(polygon.area() for polygon in get_polygons(top_cell, 'placement_core'))
    if total_area == 0:
        return 0
    valid_metal = gdstk.boolean(get_polygons(top_cell, 'placement_core'),
                                get_polygons(top_cell, 'drawing'), operation='and')
    total_metal_area = sum(polygon.area() for polygon in valid_metal)
    if total_metal_area == 0:
        return 0
    return round((total_metal_area / total_area) * 100, 2)


def calculate_core_fill_density(top_cell, cell):
    """
    Calculate filler density within the core placement region.

    Args:
        top_cell (gdstk.Cell): Layout cell.
        cell (gdstk.Cell): Candidate filler cell.

    Returns:
        float: Core fill density percentage (0–100).
    """
    total_area = sum(polygon.area() for polygon in get_polygons(top_cell, 'placement_core'))
    if total_area == 0:
        return 0
    valid_fill = gdstk.boolean(get_polygons(top_cell, 'placement_core'),
                               cell.get_polygons(), operation='and')
    total_fill_area = sum(polygon.area() for polygon in valid_fill)
    if total_fill_area == 0:
        return 0
    return round((total_fill_area / total_area) * 100, 2)


def calculate_density(top_cell):
    """
    Calculate total drawing density over the chip placement region.

    Args:
        top_cell (gdstk.Cell): Layout cell.

    Returns:
        float: Density percentage (0–100).
    """
    total_area = sum(polygon.area() for polygon in get_polygons(top_cell, 'placement_chip'))
    if total_area == 0:
        return 0
    total_metal_area = sum(polygon.area() for polygon in get_polygons(top_cell, 'drawing'))
    if total_metal_area == 0:
        return 0
    return round((total_metal_area / total_area) * 100, 2)


def calculate_fill_density(top_cell, cell):
    """
    Calculate fill density contributed by a filler cell.

    Args:
        top_cell (gdstk.Cell): Layout cell with placement region.
        cell (gdstk.Cell): Candidate filler cell.

    Returns:
        float: Fill density percentage (0–100).
    """
    total_area = sum(polygon.area() for polygon in get_polygons(top_cell, 'placement_chip'))
    if total_area == 0:
        return 0
    total_fill_area = sum(polygon.area() for polygon in cell.get_polygons())
    if total_fill_area == 0:
        return 0
    return round((total_fill_area / total_area) * 100, 2)


def check_is_square(polygon, min_width=None):
    """
    Verify if a polygon is an axis-aligned square.

    Args:
        polygon (list[tuple[float, float]]): Four vertices.
        min_width (float, optional): Minimum side length.

    Returns:
        bool: True if square and valid.
    """
    p0, p1, p2, p3 = polygon

    height1 = p0[0] - p1[0]
    height2 = p3[0] - p2[0]
    width1 = p0[1] - p3[1]
    width2 = p1[1] - p2[1]

    if width1 != width2:
        return False
    if height1 != height2:
        return False
    if min_width is not None and (width1 < min_width or height1 < min_width):
        return False
    return True


def check_min_size(polygon, min_width=None, min_height=None):
    """
    Check if a polygon meets minimum width/height.

    Args:
        polygon (list[tuple[float, float]]): Polygon vertices.
        min_width (float, optional): Minimum width.
        min_height (float, optional): Minimum height.

    Returns:
        bool: True if requirements are met.
    """
    xs = [p[0] for p in polygon]
    ys = [p[1] for p in polygon]
    width = max(xs) - min(xs)
    height = max(ys) - min(ys)

    if min_width is not None and width < min_width:
        return False
    if min_height is not None and height < min_height:
        return False
    return True


def create_polygon(center: tuple[float, float], width: float, height: float):
    """Create a rectangular polygon centered at a point.

    The polygon is aligned to a 0.005 grid and defined by its width and height.

    Args:
        center (tuple[float, float]): Center coordinates (x, y).
        width (float): Rectangle width.
        height (float): Rectangle height.

    Returns:
        list[tuple[float, float]]: List of four corner points (clockwise).
    """
    cx = round(math.ceil(center[0] / 0.005) * 0.005, 3)
    cy = round(math.ceil(center[1] / 0.005) * 0.005, 3)
    hh = round(math.ceil((height / 2) / 0.005) * 0.005, 3)
    hw = round(math.ceil((width / 2) / 0.005) * 0.005, 3)
    return [
        (cx - hw, cy - hh),
        (cx + hw, cy - hh),
        (cx + hw, cy + hh),
        (cx - hw, cy + hh),
    ]


def get_box_dimension(polygon: list[tuple[float, float]]) -> tuple[float, float]:
    """Compute the width and height of a polygon's bounding box.

    Args:
        polygon (list[tuple[float, float]]): List of (x, y) points.

    Returns:
        tuple[float, float]: (width, height) of the bounding box.
    """
    xs = [p[0] for p in polygon]
    ys = [p[1] for p in polygon]
    width = round(max(xs) - min(xs), 3)
    height = round(max(ys) - min(ys), 3)
    return (width, height)


def get_cell_distance(references: list[tuple[tuple[float, float], tuple[float, float]]]) -> float:
    """Find the most common vertical distance between reference centers.

    The function filters references aligned on the same x-coordinate,
    computes vertical gaps, and returns the most frequent gap.

    Args:
        references (list[tuple[tuple[float, float], tuple[float, float]]]):
            List of (center, (width, height)) tuples.

    Returns:
        float: Most common vertical gap size.
    """
    ref = references[0]
    cells = sorted([r[0] for r in references if r[0][0] == ref[0][0]])
    gaps = [round(cells[i + 1][1] - cells[i][1], 3) for i in range(len(cells) - 1)]
    return Counter(gaps).most_common(1)[0][0]


def get_center_point(polygon: list[tuple[float, float]]) -> tuple[float, float]:
    """Calculate the geometric center of a rectangular polygon.

    Assumes the polygon has four points forming a rectangle.

    Args:
        polygon (list[tuple[float, float]]): List of four (x, y) points.

    Returns:
        tuple[float, float]: Center coordinates (x, y).
    """
    x = round(sum(p[0] for p in polygon) / 4, 3)
    y = round(sum(p[1] for p in polygon) / 4, 3)
    return (x, y)


def get_layer(layer: str):
    """
    Get layer definition for the internal layer map.

    Args:
        layer (str): Layer name.

    Returns:
        dict: Dictionary with 'layer' index and 'datatype'.
    """
    return {'layer': layers[layer]['index'], 'datatype': layers[layer]['type']}


def get_polygons(cell, layer):
    """
    Extract polygons from a cell for the given layer.

    Args:
        cell (gdstk.Cell): Source cell.
        layer (str): Layer name.

    Returns:
        list[gdstk.Polygon]: Polygons on the layer.
    """
    return cell.get_polygons(**get_layer(layer))


def edge_length(p1, p2):
    """
    Compute Euclidean distance between two points.

    Args:
        p1 (tuple[float, float]): First point.
        p2 (tuple[float, float]): Second point.

    Returns:
        float: Distance between points.
    """
    return math.hypot(p2[0] - p1[0], p2[1] - p1[1])


def midpoint_snapped(min_size: float, max_size: float, step: float = 0.005) -> float:
    """Calculate the midpoint between two values and snap it to a step grid.

    The midpoint is first computed, then snapped to the nearest multiple
    of `step`. The result is clamped to the range `[min_size, max_size]`.

    Args:
        min_size (float): Lower bound.
        max_size (float): Upper bound.
        step (float, optional): Grid step to snap to. Defaults to 0.005.

    Returns:
        float: Snapped midpoint rounded to three decimals.
    """
    mid = (min_size + max_size) / 2
    snapped = round(mid / step) * step
    snapped = max(min_size, min(snapped, max_size))
    return round(snapped, 3)


def remove_shortest_edge(polygon, layerindex, datatype):
    """
    Reduce a 6-vertex polygon to 4 vertices by removing the shortest edge.

    Args:
        polygon (list[tuple[float, float]]): Polygon vertices.
        layerindex (int): Layer index for result.
        datatype (int): Datatype for result.

    Returns:
        gdstk.Polygon: Adjusted 4-vertex polygon.
    """
    n = len(polygon)
    # Find the shortest edge
    edges = [(i, edge_length(polygon[i], polygon[(i+1) % n])) for i in range(n)]
    min_index, _ = min(edges, key=lambda x: x[1])

    # Remove the two vertices that form this edge
    new_poly = [polygon[i] for i in range(n) if i not in (min_index, (min_index+1) % n)]

    for idx in range(0, 4):
        if (new_poly[idx][0] != new_poly[(idx + 1) % 4][0] and
           new_poly[idx][1] != new_poly[(idx + 1) % 4][1]):

            edge_prev = edge_length(new_poly[idx], new_poly[(idx - 1) % 4])
            edge_past = edge_length(new_poly[(idx + 1) % 4], new_poly[(idx + 2) % 4])
            is_horizontal = new_poly[(idx - 1) % 4][0] == new_poly[(idx - 2) % 4][0]
            xy = 0 if is_horizontal else 1
            if edge_prev > edge_past:
                new_poly[idx][xy] = new_poly[(idx + 1) % 4][xy]
            else:
                new_poly[(idx + 1) % 4][xy] = new_poly[idx][xy]
            continue
    return gdstk.Polygon(new_poly, layer=layerindex, datatype=datatype)
