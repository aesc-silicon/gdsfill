"""
Interface to run KLayout scripts and commands for GDSII processing.

All commands rely on subprocess execution of KLayout (`klayout`)
with the appropriate runtime arguments (`-r` for scripts, `-rd`
for runtime definitions).
"""
import subprocess
import re
from pathlib import Path
from typing import List
from packaging.version import Version


def _run(commands: List[str]):
    """
    Execute a KLayout command as a subprocess.

    Args:
        commands (List[str]): A list of command-line arguments to pass to
            the `klayout` executable. Each element should be a separate
            argument (do not provide a single concatenated string).

    Returns:
        CompletedProcess: Result object from subprocess.run,
        including stdout, stderr, and return code.

    Notes:
        - Always prefixes the command list with `klayout`.
        - Prints the stderr output if the command fails.
    """
    cmd = ["klayout"] + commands
    result = subprocess.run(cmd, check=False, capture_output=True)
    if result.returncode != 0:
        print(f"Command: {' '.join(cmd)}")
        print(result.stderr.decode('utf-8').rstrip('\n'))
    return result


def _popen(commands: List[str]):
    """
    Launch a KLayout subprocess with the given command-line arguments.

    Args:
        commands (List[str]): A list of command-line arguments to pass to
            the `klayout` executable. Each element should be a separate
            argument (do not provide a single concatenated string).

    Returns:
        subprocess.Popen: A Popen object representing the launched process,
        which can be used to interact with or wait for the process.
    """
    cmd = ["klayout"] + commands
    return subprocess.Popen(cmd)


def _get_script_path(pdk, script_name: str) -> str:
    """
    Resolve the path to a KLayout script for a given PDK.

    Args:
        pdk: PdkInformation object with a `get_name()` method.
        script_name: Filename of the script to locate.

    Returns:
        str: Absolute path to the script.
    """
    root = Path(__file__).parent.parent.resolve()
    return str(root / pdk.get_name() / script_name)


def get_version():
    """
    Query the installed KLayout version.

    Returns:
        Version: Parsed semantic version of KLayout,
        or `0.0.0` if the version cannot be detected.
    """
    result = _run(["-v"])
    match = re.search(r"KLayout (\d+\.\d+\.\d+)", result.stdout.decode('utf-8'))
    if match:
        return Version(match.group(1))
    return Version("0.0.0")


def export_layer(pdk, inputfile, output_path, layer, core_size=None):
    """
    Export a layout layer into tiled GDS files.

    Args:
        pdk: PdkInformation object with tile configuration.
        inputfile: Path to the input GDS file.
        output_path: Directory where exported tiles are stored.
        layer: Name of the layer to export.

    Notes:
        Uses the PDK’s configured tile width for the layer.
    """
    filename = Path(inputfile)
    tile_width = pdk.get_layer_tile_width(layer)
    cmd = ["-zz", "-r", _get_script_path(pdk, "export.py"), "-rd", f"output_path={output_path}",
           "-rd", f"layer_name={layer}", "-rd", f"tile_width={tile_width}", str(filename)]
    if core_size:
        cmd += ["-rd", f"core_size={','.join(map(str, core_size))}"]
    _run(cmd)


def prepare_tile(pdk, inputfile, layer):
    """
    Run the tile preparation step for a specific layer.

    Args:
        pdk: PdkInformation object.
        inputfile: Input tile to prepare.
        layer: Name of the layer being prepared.

    Notes:
        Prepares tiles for further fill.
    """
    filename = Path(inputfile)
    cmd = ["-zz", "-r", _get_script_path(pdk, "prepare.py"), "-rd", f"layer_name={layer}",
           str(filename)]
    return _popen(cmd)


def print_density(pdk, inputfile):
    """
    Print the density of each configured layer.

    Args:
        pdk: PdkInformation object with layer definitions.
        inputfile: Path to the GDS file to analyze.

    Output:
        Prints per-layer density percentages to stdout.
    """
    file_ = Path(inputfile)
    result = _run(["-zz", "-r", _get_script_path(pdk, "density.py"), str(file_)])
    print(result.stdout.decode('utf-8').rstrip('\n'))


def erase_fill(pdk, inputfile):
    """
    Erase dummy fill shapes from a layout.

    Args:
        pdk: PdkInformation object with fill layer definitions.
        inputfile: Path to the GDS file to process.

    Notes:
        Clears all shapes from fill layers as defined by the PDK.
    """
    file_ = Path(inputfile)
    _run(["-zz", "-r", _get_script_path(pdk, "erase.py"), str(file_)])


def merge_tile(pdk, inputfile, output_path, tiles_file):
    """
    Merge filled tiles back into the main layout.

    Args:
        pdk: PdkInformation object with process configuration.
        inputfile: Path to the input GDS file.
        output_path: Directory containing filled tile GDS files.
        tiles_file: YAML file with tile definitions.

    Notes:
        Merges each filled tile GDS into the active layout
        and writes back the combined result.
    """
    filename = Path(inputfile)
    cmd = ["-zz", "-r", _get_script_path(pdk, "merge.py"), "-rd", f"output_path={output_path}",
           "-rd", f"tiles_file={tiles_file}", str(filename)]
    _run(cmd)
