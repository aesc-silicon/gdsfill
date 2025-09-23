"""
Tile preparation utilities for PDK fill flows.

This module prepares activ, gatpoly, metal and top-metal tiles for
dummy fill insertion. It parses layer definitions from a YAML
configuration file, applies geometric transformations using KLayout's
internal API, and produces modified GDS files that contain blocking
regions for fill algorithms.
"""
# pylint: disable=too-many-locals
import sys
from pathlib import Path
import pya
import yaml


try:
    layer_name  # pylint: disable=used-before-assignment
except NameError:
    print("Missing layer_name argument. Please define '-rd layer_name=<layer_name>'")
    sys.exit(1)


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


def prepare_activ(top_cell):
    """
    Prepare blocking geometries for the activ layer.

    Steps:
        - Collect polygons for no-fill, tile border, filler, drawing,
          and keep-away areas.
        - Expand filler and drawing polygons by a fixed margin.
        - Combine with keep-away polygons to form exclusion regions.
        - Add the resulting blocking polygons to the top cell.

    Args:
        top_cell: The top-level cell of the tile GDS.
    """
    gatpoly = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_1"))))
    cont = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_2"))))
    AFil_c = gatpoly.sized(1.1 * DB2NM) + cont.sized(1.1 * DB2NM)
    del gatpoly
    del cont

    drawing = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("drawing"))))
    AFil_c1 = drawing.sized(0.42 * DB2NM)
    del drawing

    nwell = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_3"))))
    AFil_d_a = nwell.sized(1.0 * DB2NM) - nwell.sized(-1.0 * DB2NM)
    del nwell

    nblulay = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_4"))))
    AFil_d_b = nblulay.sized(1.0 * DB2NM) - nblulay.sized(-1.0 * DB2NM)
    del nblulay

    AFil_d = AFil_d_a + AFil_d_b

    trans = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_0"))))
    AFil_e = trans.sized(1.0 * DB2NM)
    del trans

    pwell_block = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_5"))))
    AFil_i = pwell_block.sized(1.5 * DB2NM) - pwell_block.sized(-1.5 * DB2NM)
    del pwell_block

    nofill = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("nofill_area"))))
    tile_border = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("tile_border"))))

    keep_out = nofill + tile_border + AFil_c + AFil_c1 + AFil_d + AFil_e + AFil_i
    top_cell.shapes(layout.layer(*get_layer("keep_out"))).insert(keep_out.merged())


def prepare_gatpoly(top_cell):
    """
    Prepare blocking geometries for the gatpoly layer.

    Steps:
        - Collect polygons for no-fill, tile border, filler, drawing,
          and keep-away areas.
        - Expand filler and drawing polygons by a fixed margin.
        - Combine with keep-away polygons to form exclusion regions.
        - Add the resulting blocking polygons to the top cell.

    Args:
        top_cell: The top-level cell of the tile GDS.
    """
    gatpoly = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_1"))))
    cont = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_2"))))
    activ = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_5"))))
    psd = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_6"))))
    nsd_block = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_7"))))
    salblock = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_8"))))
    GFil_d = gatpoly.sized(1.1 * DB2NM) + cont.sized(1.1 * DB2NM) + activ.sized(1.1 * DB2NM) + \
        psd.sized(1.1 * DB2NM) + nsd_block.sized(1.1 * DB2NM) + salblock.sized(1.1 * DB2NM)
    del activ
    del gatpoly
    del cont
    del psd
    del nsd_block
    del salblock

    nwell = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_3"))))
    GFil_e_a = nwell.sized(1.1 * DB2NM) - nwell.sized(-1.1 * DB2NM)
    del nwell
    nblulay = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_4"))))
    GFil_e_b = nblulay.sized(1.1 * DB2NM) - nblulay.sized(-1.1 * DB2NM)
    del nblulay
    GFil_e = GFil_e_a + GFil_e_b

    trans = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_0"))))
    GFil_f = trans.sized(1.1 * DB2NM)
    del trans

    nofill = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("nofill_area"))))
    tile_border = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("tile_border"))))

    keep_out = nofill + tile_border + GFil_d + GFil_e + GFil_f
    top_cell.shapes(layout.layer(*get_layer("keep_out"))).insert(keep_out.merged())


def prepare_metal(top_cell):
    """
    Prepare blocking geometries for standard metal layers.

    Steps:
        - Collect polygons for no-fill, tile border, filler, drawing,
          and keep-away areas.
        - Expand filler and drawing polygons by a fixed margin.
        - Combine with keep-away polygons to form exclusion regions.
        - Add the resulting blocking polygons to the top cell.

    Args:
        top_cell: The top-level cell of the tile GDS.
    """
    filler = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("filler"))))
    MxFil_b = filler.sized(0.42 * DB2NM)
    del filler

    drawing = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("drawing"))))
    MxFil_c = drawing.sized(0.42 * DB2NM)
    del drawing

    trans = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_0"))))
    MxFil_d = trans.sized(1.0 * DB2NM)
    del trans

    nofill = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("nofill_area"))))
    tile_border = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("tile_border"))))

    keep_out = nofill + tile_border + MxFil_b + MxFil_c + MxFil_d
    top_cell.shapes(layout.layer(*get_layer("keep_out"))).insert(keep_out.merged())


def prepare_topmetal(top_cell):
    """
    Prepare blocking geometries for top-metal layers.

    Top-metal rules generally require larger spacing than lower metal
    layers. This function applies top-metal specific expansion margins.

    Steps:
        - Collect polygons for no-fill, tile border, filler, drawing,
          and keep-away areas.
        - Expand filler and drawing polygons with larger margins than
          standard metals.
        - Combine with keep-away polygons to form exclusion regions.
        - Add the resulting blocking polygons to the top cell.

    Args:
        top_cell: The top-level cell of the tile GDS.
    """
    filler = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("filler"))))
    TMxFil_b = filler.sized(3.0 * DB2NM)
    del filler

    drawing = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("drawing"))))
    TMxFil_c = drawing.sized(3.0 * DB2NM)
    del drawing

    trans = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("keep_away_0"))))
    TMxFil_d = trans.sized(4.9 * DB2NM)
    del trans

    nofill = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("nofill_area"))))
    tile_border = pya.Region(top_cell.begin_shapes_rec(layout.layer(*get_layer("tile_border"))))

    keep_out = nofill + tile_border + TMxFil_b + TMxFil_c + TMxFil_d
    top_cell.shapes(layout.layer(*get_layer("keep_out"))).insert(keep_out.merged())


FUNC_MAPPING = {
  'Activ': prepare_activ,
  'GatPoly': prepare_gatpoly,
  'Metal1': prepare_metal,
  'Metal2': prepare_metal,
  'Metal3': prepare_metal,
  'Metal4': prepare_metal,
  'Metal5': prepare_metal,
  'TopMetal1': prepare_topmetal,
  'TopMetal2': prepare_topmetal,
}


layout = pya.CellView.active().layout()
design_cell = layout.top_cell()
FUNC_MAPPING[layer_name](design_cell)  # noqa: F821  # pylint: disable=undefined-variable
layout.write(pya.CellView.active().filename().replace('raw', 'modified'))
