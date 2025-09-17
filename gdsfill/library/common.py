"""
Common utilities for handling PDK configuration and tile metadata.

This module provides:
- `PdkInformation`: an interface for accessing process design kit (PDK)
  configuration, constants, and layer-specific parameters.
- `Tile`: a dataclass representing the position of a tile in the layout.
- Utility functions for validating input files and loading YAML
  configuration files.

The functions and classes in this module are used across the GDS fill
workflow for tasks such as density calculation, layer export, and tile
processing.
"""

from dataclasses import dataclass
from pathlib import Path
import yaml
from packaging.version import Version


class PdkInformation:
    """
    Accessor for PDK configuration and constants.

    Loads a process-specific configuration (`configs/`) and
    constants (`constants.yaml`) from the PDK directory and provides
    convenient methods to query layer settings, algorithms, density
    requirements, and rules for dummy fill.

    Attributes:
        data (dict): Contents of the process config file.
        constants (dict): Contents of the process `constants.yaml`.
    """

    def __init__(self, process, config_file):
        """
        Initialize a PDK information object.

        Args:
            process (str): Name of the process directory (e.g., "ihp-sg13g2").
            config_file (Path or str): Optional path to a custom config file.
                                       If None, defaults to `configs/<process>.yaml`.
        """
        script = Path(__file__).parent.parent.resolve()
        self.data = open_yaml(config_file if config_file else script / f"configs/{process}.yaml")
        self.constants = open_yaml(script / process / "constants.yaml")

    def get_minimum_klayout_version(self):
        """
        Get the minimum required KLayout version for this PDK.

        Returns:
            Version: Required version parsed as a `packaging.version.Version`.
        """
        return Version(self.constants['minimum_klayout'])

    def get_name(self):
        """
        Get the name of the PDK.

        Returns:
            str: Name of the process design kit.
        """
        return self.data['PDK']

    def get_layers(self):
        """
        Get all configured layers for this PDK.

        Returns:
            Iterable[Tuple[str, dict]]: List of (layer_name, properties).
        """
        return self.data['layers'].items()

    def get_layer(self, layer: str) -> dict:
        """
        Get configuration details for a specific layer.

        Args:
            layer (str): Layer name.

        Returns:
            dict: Layer configuration dictionary.
        """
        return self.data['layers'][layer]

    def get_layer_tile_width(self, layer: str) -> int:
        """
        Get the tile width for a given layer.

        Args:
            layer (str): Layer name.

        Returns:
            int: Tile width in database units.
        """
        if 'tile_width' in self.get_layer(layer):
            return int(self.get_layer(layer)['tile_width'])
        return int(self.constants['tile_width'])

    def get_layer_algorithm(self, layer: str) -> list:
        """
        Get the fill algorithm(s) for a given layer.

        Args:
            layer (str): Layer name.

        Returns:
            List[str]: List of algorithm names.
        """
        algo = self.get_layer(layer)['algorithm']
        if isinstance(algo, str):
            return [algo]
        return algo

    def get_layer_density(self, layer: str) -> float:
        """
        Get the target metal density for a layer.

        Args:
            layer (str): Layer name.

        Returns:
            float: Target density percentage.
        """
        return self.get_layer(layer)['density']

    def get_layer_deviation(self, layer: str) -> float:
        """
        Get the allowed density deviation for a layer.

        Args:
            layer (str): Layer name.

        Returns:
            float: Allowed deviation percentage.
        """
        return self.get_layer(layer)['deviation']

    def get_layer_index(self, layer: str) -> int:
        """
        Get the numeric index for a given layer.

        Args:
            layer (str): Layer name.

        Returns:
            int: Layer index.
        """
        return self.constants['layers'][layer]['index']

    def get_layer_fill_datatype(self, layer: str) -> int:
        """
        Get the GDS datatype used for fill shapes of a layer.

        Args:
            layer (str): Layer name.

        Returns:
            int: Fill datatype.
        """
        return self.constants['layers'][layer]['fill']

    def get_layer_max_depth(self, layer: str) -> int:
        """
        Get the maximum recursion depth for fill placement.

        Args:
            layer (str): Layer name.

        Returns:
            int: Maximum recursion depth.
        """
        return self.constants['layers'][layer]['max_depth']

    def has_fill_algorithm(self, layer: str, algorithm: str) -> bool:
        """
        Check if a layer supports a given fill algorithm.

        Args:
            layer (str): Layer name.
            algorithm (str): Algorithm identifier.

        Returns:
            bool: True if supported, False otherwise.
        """
        return self.constants['layers'][layer].get(algorithm) is not None

    def get_fill_rules(self, layer: str, algorithm: str) -> dict:
        """
        Get the rule set for a given layer and fill algorithm.

        Args:
            layer (str): Layer name.
            algorithm (str): Algorithm identifier.

        Returns:
            dict: Dictionary of fill rules (e.g., min/max size, spacing).
        """
        return self.constants['layers'][layer][algorithm]


@dataclass
class Tile:
    """
    Representation of a tile’s position in the layout.

    Attributes:
        x (int): X-coordinate of the tile.
        y (int): Y-coordinate of the tile.
    """

    x: int
    y: int


def inputfile_exists(inputfile: str):
    """
    Check if a given file path exists.

    Args:
        inputfile (str): Path to the file.

    Returns:
        bool: True if the file exists, False otherwise.
    """
    file_ = Path(inputfile)
    return file_.exists()


def open_yaml(yamlfile: Path):
    """
    Load the contents of a YAML file.

    Args:
        yamlfile (Path): Path to the YAML file.

    Returns:
        dict or bool: Parsed YAML content, or False if the file does not exist.
    """
    if not yamlfile.exists():
        return False
    content = Path(yamlfile).read_text(encoding='utf-8')
    return yaml.safe_load(content)
