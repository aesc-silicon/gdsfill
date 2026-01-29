# pylint:disable=duplicate-code
"""
Erase dummy fill layers from an active GDSII layout in KLayout.

This script clears all shapes on the fill layers, effectively
removing previously generated dummy metal fill from the design.
"""
from pathlib import Path
import pya
import yaml

script = Path(__file__).parent.resolve()
content = (script / "constants.yaml").read_text(encoding='utf-8')
constants = yaml.safe_load(content)

layout = pya.CellView.active().layout()

layout.start_changes()
for layer, data in constants['layers'].items():
    layout.clear_layer(layout.layer(data['index'], data['fill']))
layout.end_changes()

print(f"Write GDSII with cleared layers to {pya.CellView.active().filename()}")
layout.write(pya.CellView.active().filename())
