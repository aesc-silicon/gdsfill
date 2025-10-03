"""
Tile preparation script for KLayout dummy fill flow.
"""

import sys
import hashlib
import random
import string
from pathlib import Path
import pya
import yaml

# Validate runtime arguments
# pylint: disable=duplicate-code
try:
    output_path  # pylint: disable=used-before-assignment
except NameError:
    print("Missing output_path argument. Please define '-rd output_path=<path-to-export>'")
    sys.exit(1)

try:
    layer_name  # pylint: disable=used-before-assignment
except NameError:
    print("Missing layer_name argument. Please define '-rd layer_name=<layer_name>'")
    sys.exit(1)

try:
    tile_width  # pylint: disable=used-before-assignment
except NameError:
    print("Missing tile_width argument. Please define '-rd tile_width=<tile_width>'")
    sys.exit(1)
tile_width = int(tile_width)  # noqa: F821  # pylint: disable=undefined-variable


# Load constants
script = Path(__file__).parent.resolve()
content = (script / "constants.yaml").read_text(encoding="utf-8")
constants = yaml.safe_load(content)
DB2NM = constants["DB2NM"]
layers = yaml.safe_load((script / "../library/layers.yaml").read_text(encoding="utf-8"))


def get_layer(layer: str) -> tuple[int, int]:
    """
    Return (layer, datatype) tuple for a given layer name.

    Args:
        layer (str): Name of the layer as defined in layers.yaml.

    Returns:
        tuple[int, int]: (layer index, datatype)
    """
    return (layers[layer]["index"], layers[layer]["type"])


def get_nofill(layout, cell, layer_number: int) -> pya.Region:
    """
    Collect no-fill shapes for a given layer.

    Args:
        layout (pya.Layout): The active layout.
        cell (pya.Cell): The design cell.
        layer_number (int): The layer index.

    Returns:
        pya.Region: Region containing no-fill shapes.
    """
    layer = layout.layer(layer_number, 23)
    return pya.Region(cell.begin_shapes_rec(layer))


def get_fill_area(layout, cell) -> pya.Region:
    """
    Compute the fillable area inside the sealring.

    Args:
        layout (pya.Layout): The active layout.
        cell (pya.Cell): The design cell.

    Returns:
        pya.Region: Fill area region.
    """
    layer = layout.layer(39, 0)
    ring = pya.Region(cell.begin_shapes_rec(layer)).merged()
    return (ring + ring.holes()).merge()


def get_die_size(layout, cell) -> tuple[int, int]:
    """
    Return die width and height in microns.
    """
    layer = layout.layer(39, 4)
    edgeseal = pya.Region(cell.begin_shapes_rec(layer))
    return (int(edgeseal.bbox().width() / DB2NM), int(edgeseal.bbox().height() / DB2NM))


def get_core_area(layout, cell):
    """
    Returns a Region of stdcells, expanded by 5 nm, restricted to the core area for fill algorithms.
    """
    stdcells = [
        poly
        for poly in pya.Region(cell.begin_shapes_rec(layout.layer(189, 4)))
        if poly.bbox().height() == (3.78 * DB2NM)
    ]
    return pya.Region(stdcells).merged().sized(5.0 * DB2NM).merged()


def generate_border(x: int, y: int, tile_width_: int, space: float):
    """
    Generate border boxes around a tile.

    Args:
        x (int): Tile X coordinate in microns.
        y (int): Tile Y coordinate in microns.
        tile_width_ (int): Width of the tile in microns.
        space (float): Margin for the border.

    Returns:
        tuple[pya.Box, pya.Box, pya.Box, pya.Box]: Four border boxes (bottom, top, left, right).
    """
    box_bottom = pya.Box(x * DB2NM, y * DB2NM,
                         (x + tile_width_) * DB2NM, (y + space) * DB2NM)
    box_top = pya.Box(x * DB2NM, (y + tile_width_ - space) * DB2NM,
                      (x + tile_width_) * DB2NM, (y + tile_width_) * DB2NM)
    box_left = pya.Box(x * DB2NM, y * DB2NM,
                       (x + space) * DB2NM, (y + tile_width_) * DB2NM)
    box_right = pya.Box((x + tile_width_ - space) * DB2NM, y * DB2NM,
                        (x + tile_width_) * DB2NM, (y + tile_width_) * DB2NM)
    return (box_bottom, box_top, box_left, box_right)


def get_activ_border(width, height, tile_width_):
    """Wrapper to generate border for the activ layer."""
    return generate_border(width, height, tile_width_, 0.42)


# pylint: disable=unused-argument
def get_activ(layout, design_cell, tmp_cell, layer_number: int):
    """
    Collect polygons for the activ layer and insert them into the temporary cell.
    """
    metal = pya.Region(design_cell.begin_shapes_rec(layout.layer(layer_number, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("drawing"))).insert(metal.merged())
    trans = pya.Region(design_cell.begin_shapes_rec(layout.layer(26, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_0"))).insert(trans)
    gatpoly = pya.Region(design_cell.begin_shapes_rec(layout.layer(5, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_1"))).insert(gatpoly)
    cont = pya.Region(design_cell.begin_shapes_rec(layout.layer(6, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_2"))).insert(cont)
    nwell = pya.Region(design_cell.begin_shapes_rec(layout.layer(31, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_3"))).insert(nwell)
    nbulay = pya.Region(design_cell.begin_shapes_rec(layout.layer(32, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_4"))).insert(nbulay)
    pwell_block = pya.Region(design_cell.begin_shapes_rec(layout.layer(46, 21)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_5"))).insert(pwell_block)

    nofill = get_nofill(layout, design_cell, layer_number)
    tmp_cell.shapes(layout.layer(*get_layer("nofill_area"))).insert(nofill)


def get_gatpoly_border(width, height, tile_width_):
    """Wrapper to generate border for the gatpoly layer."""
    return generate_border(width, height, tile_width_, 0.8)


# pylint: disable=unused-argument, too-many-locals
def get_gatpoly(layout, design_cell, tmp_cell, layer_number: int):
    """
    Collect polygons for the gatpoly layer and insert them into the temporary cell.
    """

    metal = pya.Region(design_cell.begin_shapes_rec(layout.layer(layer_number, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("drawing"))).insert(metal.merged())
    activ_filler = pya.Region(design_cell.begin_shapes_rec(layout.layer(1, 22)))
    tmp_cell.shapes(layout.layer(*get_layer("reference"))).insert(activ_filler)
    trans = pya.Region(design_cell.begin_shapes_rec(layout.layer(26, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_0"))).insert(trans)
    gatpoly = pya.Region(design_cell.begin_shapes_rec(layout.layer(5, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_1"))).insert(gatpoly)
    cont = pya.Region(design_cell.begin_shapes_rec(layout.layer(6, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_2"))).insert(cont)
    nwell = pya.Region(design_cell.begin_shapes_rec(layout.layer(31, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_3"))).insert(nwell)
    nbulay = pya.Region(design_cell.begin_shapes_rec(layout.layer(32, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_4"))).insert(nbulay)

    activ = pya.Region(design_cell.begin_shapes_rec(layout.layer(1, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_5"))).insert(activ)
    psd = pya.Region(design_cell.begin_shapes_rec(layout.layer(14, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_6"))).insert(psd)
    nsd_block = pya.Region(design_cell.begin_shapes_rec(layout.layer(7, 21)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_7"))).insert(nsd_block)
    salblock = pya.Region(design_cell.begin_shapes_rec(layout.layer(28, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_8"))).insert(salblock)

    nofill = get_nofill(layout, design_cell, layer_number)
    tmp_cell.shapes(layout.layer(*get_layer("nofill_area"))).insert(nofill)


def get_metal_border(x: int, y: int, tile_width_: int):
    """Wrapper to generate border for standard metal layers."""
    return generate_border(x, y, tile_width_, 0.42)


def get_metal(layout, design_cell, tmp_cell, layer_number: int) -> None:
    """
    Collect polygons for metal layers and insert them into the temporary cell.
    """
    metal = pya.Region(design_cell.begin_shapes_rec(layout.layer(layer_number, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("drawing"))).insert(metal.merged())
    trans = pya.Region(design_cell.begin_shapes_rec(layout.layer(26, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_0"))).insert(trans)

    placement_cell = get_core_area(layout, design_cell)
    tmp_cell.shapes(layout.layer(*get_layer("placement_core"))).insert(placement_cell)

    nofill = get_nofill(layout, design_cell, layer_number)
    tmp_cell.shapes(layout.layer(*get_layer("nofill_area"))).insert(nofill)


def get_topmetal_border(x: int, y: int, tile_width_: int):
    """Wrapper to generate border for top-metal layers."""
    return generate_border(x, y, tile_width_, 3.0)


def get_topmetal(layout, design_cell, tmp_cell, layer_number: int) -> None:
    """
    Collect polygons for top-metal layers and insert them into the temporary cell.
    """
    top_metal = pya.Region(design_cell.begin_shapes_rec(layout.layer(layer_number, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("drawing"))).insert(top_metal.merged())
    trans = pya.Region(design_cell.begin_shapes_rec(layout.layer(26, 0)))
    tmp_cell.shapes(layout.layer(*get_layer("keep_away_0"))).insert(trans)

    placement_cell = get_core_area(layout, design_cell)
    tmp_cell.shapes(layout.layer(*get_layer("placement_core"))).insert(placement_cell)

    nofill = get_nofill(layout, design_cell, layer_number)
    tmp_cell.shapes(layout.layer(*get_layer("nofill_area"))).insert(nofill)


FUNC_MAPPING = {
    "Activ": get_activ,
    "GatPoly": get_gatpoly,
    "Metal1": get_metal,
    "Metal2": get_metal,
    "Metal3": get_metal,
    "Metal4": get_metal,
    "Metal5": get_metal,
    "TopMetal1": get_topmetal,
    "TopMetal2": get_topmetal,
}

FUNC_BORDER_MAPPING = {
    "Activ": get_activ_border,
    "GatPoly": get_gatpoly_border,
    "Metal1": get_metal_border,
    "Metal2": get_metal_border,
    "Metal3": get_metal_border,
    "Metal4": get_metal_border,
    "Metal5": get_metal_border,
    "TopMetal1": get_topmetal_border,
    "TopMetal2": get_topmetal_border,
}


# pylint: disable=too-many-locals
def export_tiles(output_dir: Path, layer_name: str, tile_width_: int):
    """
    Generate tiled GDSII files and metadata for a specified metal or top-metal layer.

    This function splits the design into tiles of the given width, applies
    layer-specific processing (e.g., inserting borders and keep-out regions),
    and exports each tile as a separate GDSII file. It also collects metadata
    about die size, core size, core origin, and a checksum of the source design.

    Args:
        output_dir (Path): Directory where the generated `raw` tiles and
            `tiles.yaml` metadata should be written.
        layer_name (str): The name of the metal or top-metal layer
            (must be defined in `constants["layers"]` and `FUNC_MAPPING`).
        tile_width_ (int): Width of each square tile in design units.

    Returns:
        dict: A dictionary containing die dimensions, core dimensions and origin,
        per-tile metadata (coordinates, size), and an MD5 checksum of the source GDS.

    Raises:
        KeyError: If the given `layer_name` is not present in the configuration.
        FileNotFoundError: If the active layout or its source GDS file cannot be read.
        RuntimeError: If no active layout or top cell is available in KLayout.
    """
    # Temporary filler cell
    tmp_filler_top_name = "".join(random.choices(string.ascii_letters, k=30))

    layout = pya.CellView.active().layout()
    design_cell = layout.top_cell()
    tmp_cell = layout.create_cell(tmp_filler_top_name)

    layer_index = constants["layers"][layer_name]["index"]
    FUNC_MAPPING[layer_name](layout, design_cell, tmp_cell, layer_index)

    sealring = get_fill_area(layout, design_cell)
    tmp_cell.shapes(layout.layer(*get_layer("placement_chip"))).insert(sealring)

    die_width, die_height = get_die_size(layout, design_cell)

    # Compute checksum
    with open(pya.CellView.active().filename(), "rb") as gds:
        file_hash = hashlib.md5()
        while chunk := gds.read(8192):
            file_hash.update(chunk)

    data = {
        "die": {"width": die_width, "height": die_height},
        "tiles": {},
        "checksum": file_hash.hexdigest(),
    }

    for x in range(0, die_width, tile_width_):
        for y in range(0, die_height, tile_width_):
            tile_name = f"{x}_{y}"
            file_name = f"tile_{tile_name}.gds"
            data["tiles"][tile_name] = {
                "x": x,
                "y": y,
                "width": min(tile_width_, die_width - x),
                "height": min(tile_width_, die_height - y),
            }

            box_bottom, box_top, box_left, box_right = FUNC_BORDER_MAPPING[layer_name](
                x, y, tile_width_
            )
            tmp_cell.shapes(layout.layer(*get_layer("tile_border"))).insert(box_bottom)
            tmp_cell.shapes(layout.layer(*get_layer("tile_border"))).insert(box_top)
            tmp_cell.shapes(layout.layer(*get_layer("tile_border"))).insert(box_left)
            tmp_cell.shapes(layout.layer(*get_layer("tile_border"))).insert(box_right)

            clip_rect = pya.Box(x * DB2NM, y * DB2NM,
                                (x + tile_width_) * DB2NM, (y + tile_width_) * DB2NM)
            layout.clip(layout.cell(tmp_filler_top_name), clip_rect).write(
                output_dir / "raw" / file_name)

    return data


# pylint: disable=undefined-variable
outputdir = Path(output_path)  # noqa: F821
tile_data = export_tiles(outputdir, layer_name, tile_width)  # noqa: F821

# Write metadata YAML
try:
    with (outputdir / "tiles.yaml").open("w") as f:
        yaml.safe_dump(tile_data, f, default_flow_style=False)
except (OSError, yaml.YAMLError) as e:
    print(f"Failed to write YAML file: {e}")
