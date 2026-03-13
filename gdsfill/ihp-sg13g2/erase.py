# pylint:disable=duplicate-code
"""
Erase dummy fill layers from an active GDSII layout in KLayout.

This script clears all shapes on the fill layers, effectively
removing previously generated dummy metal fill from the design.
"""
from pathlib import Path
import sys
import pya
import yaml

try:
    layers  # pylint: disable=used-before-assignment
except NameError:
    print("Missing layers argument. Please define '-rd layers=<layer1,layer2>'")
    sys.exit(1)

# pylint: disable=undefined-variable
selected_layers = layers.split(',')  # noqa: F821

script = Path(__file__).parent.resolve()
content = (script / "constants.yaml").read_text(encoding='utf-8')
constants = yaml.safe_load(content)

layout = pya.CellView.active().layout()


layout.start_changes()
for layer, data in constants['layers'].items():
    if layer not in selected_layers:
        continue
    layout.clear_layer(layout.layer(data['index'], data['fill']))
layout.end_changes()

print(f"Write GDSII with cleared layers to {pya.CellView.active().filename()}")
layout.write(pya.CellView.active().filename())
