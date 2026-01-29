# pylint:disable=duplicate-code
"""
Merge filled tiles into the active GDSII layout in KLayout.

This script combines previously filled tile GDS files into the
active layout, effectively reconstructing the design with dummy
fill applied.
"""
import sys
from pathlib import Path
import pya
import yaml

# pylint: disable=duplicate-code
try:
    output_path
except NameError:
    print("Missing output_path argument. Please define '-rd output_path=<path-to-export>'")
    sys.exit(1)

try:
    tiles_file
except NameError:
    print("Missing filled_tile argument. Please define '-rd tiles_file=<files-yaml-file>'")
    sys.exit(1)


script = Path(__file__).parent.resolve()
content = (script / "constants.yaml").read_text(encoding='utf-8')
constants = yaml.safe_load(content)

# pylint: disable=undefined-variable
tiles = yaml.safe_load(Path(tiles_file).read_text(encoding='utf-8'))  # noqa: F821

layout = pya.CellView.active().layout()

layout.start_changes()
for tile, data in tiles['tiles'].items():
    filled_layout = pya.Layout()
    filled_layout.read(Path(output_path) / f"tile_{tile}.gds")  # noqa: F821
    print(f"Merging all tiles into tile_{tile}.gds")
    for topcell in filled_layout.top_cells():
        layout.top_cell().copy_tree(topcell)
layout.end_changes()

print(f"Write GDS with cleared layers to {pya.CellView.active().filename()}")
layout.write(pya.CellView.active().filename())
