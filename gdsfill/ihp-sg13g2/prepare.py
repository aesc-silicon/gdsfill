"""
Tile preparation utilities for PDK fill flows.

This module prepares metal and top-metal tiles for dummy fill insertion.
It parses layer definitions from a YAML configuration file, applies
geometric transformations (offsets and booleans) using `gdstk`, and
produces modified GDS files that contain blocking regions for fill
algorithms.
"""
# pylint: disable=too-many-locals
from pathlib import Path
import yaml
import gdstk

script = Path(__file__).parent.resolve()
layers = yaml.safe_load((script / "../library/layers.yaml").read_text(encoding='utf-8'))


def get_layer(layer):
    """
    Look up the GDS layer and datatype for a given layer name.

    Args:
        layer (str): Name of the layer (as defined in `layers.yaml`).

    Returns:
        dict: A dictionary with keys:
            - 'layer': Layer index (int).
            - 'datatype': Datatype index (int).
    """
    return {'layer': layers[layer]['index'], 'datatype': layers[layer]['type']}


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
        top_cell (gdstk.Cell): The top-level cell of the tile GDS.
    """
    nofill = top_cell.get_polygons(**get_layer('nofill_area'))
    tile_border = top_cell.get_polygons(**get_layer('tile_border'))
    drawing = top_cell.get_polygons(**get_layer('drawing'))
    keep_away0 = top_cell.get_polygons(**get_layer('keep_away_0'))  # TRANS
    keep_away1 = top_cell.get_polygons(**get_layer('keep_away_1'))  # GatPoly
    keep_away2 = top_cell.get_polygons(**get_layer('keep_away_2'))  # Cont
    keep_away3 = top_cell.get_polygons(**get_layer('keep_away_3'))  # NWell
    keep_away4 = top_cell.get_polygons(**get_layer('keep_away_4'))  # nBuLay
    keep_away5 = top_cell.get_polygons(**get_layer('keep_away_5'))  # PWell:block

    AFil_c_gp = gdstk.offset(keep_away1, 1.1, **get_layer('keep_out'))
    AFil_c_cont = gdstk.offset(keep_away2, 1.1, **get_layer('keep_out'))
    AFil_c = gdstk.boolean(AFil_c_gp, AFil_c_cont, operation='or', **get_layer('keep_out'))

    AFil_c1 = gdstk.offset(drawing, 0.42, **get_layer('keep_out'))

    AFil_d_a_out = gdstk.offset(keep_away3, 1.0, **get_layer('keep_out'))
    AFil_d_a_in = gdstk.offset(keep_away3, -1.0, **get_layer('keep_out'))
    AFil_d_a = gdstk.boolean(AFil_d_a_out, AFil_d_a_in, operation='not', **get_layer('keep_out'))

    AFil_d_b_out = gdstk.offset(keep_away4, 1.0, **get_layer('keep_out'))
    AFil_d_b_in = gdstk.offset(keep_away4, -1.0, **get_layer('keep_out'))
    AFil_d_b = gdstk.boolean(AFil_d_b_out, AFil_d_b_in, operation='not', **get_layer('keep_out'))
    AFil_d = gdstk.boolean(AFil_d_a, AFil_d_b, operation='or', **get_layer('keep_out'))

    AFil_e = gdstk.offset(keep_away0, 1.0, **get_layer('keep_out'))

    AFil_i_out = gdstk.offset(keep_away5, 1.5, **get_layer('keep_out'))
    AFil_i_in = gdstk.offset(keep_away5, -1.5, **get_layer('keep_out'))
    AFil_i = gdstk.boolean(AFil_i_out, AFil_i_in, operation='not', **get_layer('keep_out'))

    blocking_a = gdstk.boolean(nofill, tile_border, operation='or', **get_layer('keep_out'))
    blocking_b = gdstk.boolean(AFil_c, AFil_c1, operation='or', **get_layer('keep_out'))
    blocking_c = gdstk.boolean(AFil_d, AFil_e, operation='or', **get_layer('keep_out'))
    blocking0 = gdstk.boolean(blocking_a, blocking_b, operation='or', **get_layer('keep_out'))
    blocking1 = gdstk.boolean(blocking_c, AFil_i, operation='or', **get_layer('keep_out'))
    blocking = gdstk.boolean(blocking0, blocking1, operation='or', **get_layer('keep_out'))
    top_cell.add(*(poly for poly in blocking))


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
        top_cell (gdstk.Cell): The top-level cell of the tile GDS.
    """
    nofill = top_cell.get_polygons(**get_layer('nofill_area'))
    tile_border = top_cell.get_polygons(**get_layer('tile_border'))
    keep_away0 = top_cell.get_polygons(**get_layer('keep_away_0'))  # TRANS
    keep_away1 = top_cell.get_polygons(**get_layer('keep_away_1'))  # GatPoly
    keep_away2 = top_cell.get_polygons(**get_layer('keep_away_2'))  # Cont
    keep_away3 = top_cell.get_polygons(**get_layer('keep_away_3'))  # NWell
    keep_away4 = top_cell.get_polygons(**get_layer('keep_away_4'))  # nBuLay
    keep_away5 = top_cell.get_polygons(**get_layer('keep_away_5'))  # Activ
    keep_away6 = top_cell.get_polygons(**get_layer('keep_away_6'))  # pSD
    keep_away7 = top_cell.get_polygons(**get_layer('keep_away_7'))  # nSD:block
    keep_away8 = top_cell.get_polygons(**get_layer('keep_away_8'))  # SalBlock

    GFil_d_gp = gdstk.offset(keep_away1, 1.1, **get_layer('keep_out'))
    GFil_d_cont = gdstk.offset(keep_away2, 1.1, **get_layer('keep_out'))
    GFil_d_activ = gdstk.offset(keep_away5, 1.1, **get_layer('keep_out'))
    GFil_d_psd = gdstk.offset(keep_away6, 1.1, **get_layer('keep_out'))
    GFil_d_nsd_block = gdstk.offset(keep_away7, 1.1, **get_layer('keep_out'))
    GFil_d_salblock = gdstk.offset(keep_away8, 1.1, **get_layer('keep_out'))
    GFil_d_a = gdstk.boolean(GFil_d_gp, GFil_d_cont, operation='or', **get_layer('keep_out'))
    GFil_d_b = gdstk.boolean(GFil_d_activ, GFil_d_psd, operation='or', **get_layer('keep_out'))
    GFil_d_c = gdstk.boolean(GFil_d_nsd_block, GFil_d_salblock, operation='or',
                             **get_layer('keep_out'))
    GFil_d_0 = gdstk.boolean(GFil_d_a, GFil_d_b, operation='or', **get_layer('keep_out'))
    GFil_d = gdstk.boolean(GFil_d_0, GFil_d_c, operation='or', **get_layer('keep_out'))

    GFil_e1_out = gdstk.offset(keep_away3, 1.1, **get_layer('keep_out'))
    GFil_e1_in = gdstk.offset(keep_away3, -1.1, **get_layer('keep_out'))
    GFil_e1 = gdstk.boolean(GFil_e1_out, GFil_e1_in, operation='not', **get_layer('keep_out'))
    GFil_e2_out = gdstk.offset(keep_away4, 1.1, **get_layer('keep_out'))
    GFil_e2_in = gdstk.offset(keep_away4, -1.1, **get_layer('keep_out'))
    GFil_e2 = gdstk.boolean(GFil_e2_out, GFil_e2_in, operation='not', **get_layer('keep_out'))
    GFil_e = gdstk.boolean(GFil_e1, GFil_e2, operation='or', **get_layer('keep_out'))

    GFil_f = gdstk.offset(keep_away0, 1.1, **get_layer('keep_out'))

    blocking_a = gdstk.boolean(nofill, tile_border, operation='or', **get_layer('keep_out'))
    blocking_b = gdstk.boolean(GFil_d, GFil_e, operation='or', **get_layer('keep_out'))
    blocking0 = gdstk.boolean(blocking_a, blocking_b, operation='or', **get_layer('keep_out'))
    blocking = gdstk.boolean(GFil_f, blocking0, operation='or', **get_layer('keep_out'))
    top_cell.add(*(poly for poly in blocking))


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
        top_cell (gdstk.Cell): The top-level cell of the tile GDS.
    """
    nofill = top_cell.get_polygons(**get_layer('nofill_area'))
    tile_border = top_cell.get_polygons(**get_layer('tile_border'))
    filler = top_cell.get_polygons(**get_layer('filler'))
    drawing = top_cell.get_polygons(**get_layer('drawing'))
    keep_away0 = top_cell.get_polygons(**get_layer('keep_away_0'))

    MxFil_b = gdstk.offset(filler, 0.42, **get_layer('keep_out'))
    MxFil_c = gdstk.offset(drawing, 0.42, **get_layer('keep_out'))
    MxFil_d = gdstk.offset(keep_away0, 1.0, **get_layer('keep_out'))
    blocking_a = gdstk.boolean(nofill, tile_border, operation='or', **get_layer('keep_out'))
    blocking_b = gdstk.boolean(blocking_a, MxFil_b, operation='or', **get_layer('keep_out'))
    if MxFil_d:
        blocking_c = gdstk.boolean(MxFil_c, MxFil_d, operation='or', **get_layer('keep_out'))
    else:
        blocking_c = MxFil_c
    blocking = gdstk.boolean(blocking_b, blocking_c, operation='or', **get_layer('keep_out'))
    top_cell.add(*(poly for poly in blocking))


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
        top_cell (gdstk.Cell): The top-level cell of the tile GDS.
    """
    nofill = top_cell.get_polygons(**get_layer('nofill_area'))
    tile_border = top_cell.get_polygons(**get_layer('tile_border'))
    filler = top_cell.get_polygons(**get_layer('filler'))
    drawing = top_cell.get_polygons(**get_layer('drawing'))
    keep_away0 = top_cell.get_polygons(**get_layer('keep_away_0'))

    TMxFil_b = gdstk.offset(filler, 3.0, **get_layer('keep_out'))
    TMxFil_c = gdstk.offset(drawing, 3.0, **get_layer('keep_out'))
    TMxFil_d = gdstk.offset(keep_away0, 4.9, **get_layer('keep_out'))
    blocking_a = gdstk.boolean(nofill, tile_border, operation='or', **get_layer('keep_out'))
    blocking_b = gdstk.boolean(blocking_a, TMxFil_b, operation='or', **get_layer('keep_out'))
    if TMxFil_d:
        blocking_c = gdstk.boolean(TMxFil_c, TMxFil_d, operation='or', **get_layer('keep_out'))
    else:
        blocking_c = TMxFil_c
    blocking = gdstk.boolean(blocking_b, blocking_c, operation='or', **get_layer('keep_out'))
    top_cell.add(*(poly for poly in blocking))


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


# pylint: disable=unused-argument
def prepare_tile(pdk, raw_tile: Path, layer: str) -> bool:
    """
    Prepare a raw tile GDS file by applying blocking rules for a given layer.

    This is the main entry point for tile preparation. It selects the
    appropriate preparation function (`prepare_metal` or
    `prepare_topmetal`) based on the layer and writes a new GDS file
    with the modifications.

    Args:
        pdk (PdkInformation): Process design kit metadata (currently unused).
        raw_tile (Path): Path to the raw tile GDS file.
        layer (str): Layer name (must exist in FUNC_MAPPING).

    Returns:
        bool: True if the tile was successfully processed.
    """
    _, x, y = raw_tile.stem.split('_')
    print(f"Preparing Tile {x}x{y}")
    library = gdstk.read_gds(raw_tile, unit=1e-6)
    top_cell = library.top_level()[0]
    FUNC_MAPPING[layer](top_cell)
    out_file = str(raw_tile).replace('raw', 'modified')
    library.write_gds(out_file)
    return True
