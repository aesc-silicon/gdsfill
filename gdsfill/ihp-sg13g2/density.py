# pylint:disable=duplicate-code
"""
Density report generator for GDSII layouts in KLayout.

This script calculates the metal density per layer inside the
edge seal region of the active layout. The edge seal defines
the reference area for density measurements, including holes.

The layer mapping (indices, drawing, fill) is read from
`constants.yaml`, which must be located in the same directory
as this script.
"""
from pathlib import Path
import pya
import yaml

script = Path(__file__).parent.resolve()
content = (script / "constants.yaml").read_text(encoding='utf-8')
constants = yaml.safe_load(content)

layout = pya.CellView.active().layout()
design_cell = layout.top_cell()

edgeseal = pya.Region(design_cell.begin_shapes_rec(layout.layer(39, 0))).merged()
density_area = (edgeseal + edgeseal.holes()).area()
for layer, data in constants['layers'].items():
    metal = pya.Region(design_cell.begin_shapes_rec(layout.layer(data['index'], data['drawing'])))
    fill = pya.Region(design_cell.begin_shapes_rec(layout.layer(data['index'], data['fill'])))

    metal_per = (metal.area() / density_area) * 100
    fill_per = (fill.area() / density_area) * 100

    print(f"{layer}: {round(metal_per + fill_per, 2)} %")
