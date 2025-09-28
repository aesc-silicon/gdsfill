"""
Command-line interface for GDSII dummy fill operations.

This module provides subcommands to:
- Insert dummy fill into a layout (`fill`)
- Erase dummy fill from a layout (`erase`)
- Calculate per-layer density (`density`)

Dummy fill is generated tile-by-tile using processes defined
in the PDK, and relies on Klayout for GDSII operations.
"""

import argparse
import math
import tempfile
from pathlib import Path
from multiprocessing import Process, Queue, cpu_count
from rich.live import Live
from rich.console import Console

from gdsfill.library.klayout import (
  get_version,
  export_layer,
  print_density,
  prepare_tile,
  erase_fill,
  merge_tile
)
from gdsfill.library.common import (
  PdkInformation,
  Tile,
  open_yaml
)
from gdsfill.library.fill import fill_layer


# pylint: disable=too-many-locals, too-many-arguments, too-many-positional-arguments
# pylint: disable=too-many-branches, too-many-statements
def _fill_layer(layer, pdk, args, tmpdirname):
    """
    Run the fill pipeline for a single layer.

    Steps:
      1. Export layer geometry into tiles.
      2. Modify tiles using the process-specific `prepare_tile`.
      3. Apply dummy fill to each tile with the selected algorithms.
      4. Optionally merge tiles back into a single GDS file.

    Args:
        layer (str): Target layer name.
        pdk (PdkInformation): PDK instance with layer rules.
        args (Argparser): Argparser object with all arguments.
        tmpdirname (Path | str): Temporary directory for intermediate data.

    Returns:
        None
    """
    print(f">>> Layer {layer}")

    output_path = Path(tmpdirname) / layer
    for stage in ('raw', 'modified', 'filled'):
        (output_path / stage).mkdir(parents=True, exist_ok=True)
    console = Console(color_system=None)

    with Live("Exporting tiles ...\n", console=console, refresh_per_second=4) as live:
        export_layer(pdk, args.input, output_path, layer, args.core_size)
        live.update("Exporting tiles ... done\n")

    tiles = open_yaml(output_path / "tiles.yaml")

    print("Preparing tiles:")
    lines = [f"  [ ] {tile.replace('_', 'x')}" for tile in tiles['tiles'].keys()]
    procs_modify = {}
    with Live("\n".join(lines), console=console, refresh_per_second=4) as live:
        tile_keys = list(tiles['tiles'].keys())
        for step in range(0, math.ceil(len(tile_keys) / args.max_processes)):
            start = step * args.max_processes
            for idx, tile in enumerate(tile_keys[slice(start, start + args.max_processes)]):
                raw_tile = output_path / "raw" / f"tile_{tile}.gds"
                proc = prepare_tile(pdk, raw_tile, layer)
                procs_modify[start + idx] = {'tile': tile.replace('_', 'x'), 'pid': proc}
            while procs_modify:
                for idx in list(procs_modify):
                    procs_modify[idx]['pid'].poll()
                    if procs_modify[idx]['pid'].returncode is not None:
                        lines[idx] = f"  [✔] {procs_modify[idx]['tile']}"
                        live.update("\n".join(lines))
                        del procs_modify[idx]

    print("\nFilling tiles:")
    lines = [f"  [ ] {tile.replace('_', 'x')}" for tile in tiles['tiles'].keys()]
    procs_fill = {}
    with Live("\n".join(lines), console=console, refresh_per_second=4) as live:
        tile_items = list(tiles['tiles'].items())
        for step in range(0, math.ceil(len(tile_items) / args.max_processes)):
            start = step * args.max_processes
            items = tile_items[slice(start, start + args.max_processes)]
            for idx, (tile, values) in enumerate(items):
                file = output_path / "modified" / f"tile_{tile}.gds"
                queue = Queue()
                proc = Process(target=fill_layer,
                               args=(pdk, file, layer, queue, tiles,
                                     Tile(values['x'], values['y'])))
                procs_fill[start + idx] = {
                    'tile': tile.replace('_', 'x'),
                    'queue': queue,
                    'pid': proc
                }
                proc.start()
            while procs_fill:
                for idx in list(procs_fill):
                    procs_fill[idx]['pid'].join(0.1)
                    if procs_fill[idx]['pid'].exitcode is not None:
                        result = procs_fill[idx]['queue'].get()
                        if result[0] == "success":
                            symbol = "✔"
                        elif result[0] == "skipped":
                            symbol = "-"
                        else:
                            symbol = "x"
                        lines[idx] = f"  [{symbol}] {procs_fill[idx]['tile']: <9} {result[1]}"
                        live.update("\n".join(lines))
                        del procs_fill[idx]

    print()
    if args.dry_run:
        print("--dry-run enabled: skipping merge step\n")
    else:
        with Live("Merging tiles ...\n", console=console, refresh_per_second=4) as live:
            merge_tile(pdk, args.input, output_path / "filled", output_path / "tiles.yaml")
            live.update("Merging tiles ... done\n")


def func_fill(args, pdk):
    """
    Subcommand: Insert dummy fill into each layer of a GDS file.

    Args:
        args (Namespace): Parsed CLI arguments.
        pdk (PdkInformation): PDK instance with layer rules.

    Returns:
        None
    """
    if args.keep_data:
        tmpdirname = Path.cwd() / "gdsfill-tmp" / args.input.stem.split('.')[0]
        print(f"Data are stored in {tmpdirname}")
        for layer, _ in pdk.get_layers():
            _fill_layer(layer, pdk, args, tmpdirname)
    else:
        with tempfile.TemporaryDirectory(prefix='gdsfill-') as tmpdirname:
            for layer, _ in pdk.get_layers():
                _fill_layer(layer, pdk, args, tmpdirname)


def func_density(args, pdk):
    """
    Subcommand: Calculate density for each layer of a GDS file.

    Args:
        args (Namespace): Parsed CLI arguments.
        pdk (PdkInformation): PDK instance with layer rules.

    Returns:
        None
    """
    print_density(pdk, args.input)


def func_erase(args, pdk):
    """
    Subcommand: Erase all dummy fill from a GDS file.

    Args:
        args (Namespace): Parsed CLI arguments.
        pdk (PdkInformation): PDK instance with layer rules.

    Returns:
        None
    """
    erase_fill(pdk, args.input)


def is_valid_file(value: str):
    """
    Argparse validator: ensure argument points to an existing file.

    Args:
        value (str): Path to the file.

    Returns:
        Path: Validated Path object.

    Raises:
        argparse.ArgumentTypeError: If the path does not exist or is not a file.
    """
    file_ = Path(value)
    if file_.is_file():
        return file_
    raise argparse.ArgumentTypeError(f"File {value} doesn't exist!")


def arguments():
    """
    Define CLI arguments and subcommands.

    Returns:
        argparse.Namespace: Parsed command-line arguments.
    """
    parser = argparse.ArgumentParser()
    parser.add_argument("--process", default="ihp-sg13g2")
    subparsers = parser.add_subparsers(help='subcommand help')
    fill = subparsers.add_parser('fill', help='Fill chip with dummy metal')
    fill.add_argument("input", type=is_valid_file)
    fill.add_argument('--keep-data', action=argparse.BooleanOptionalAction)
    fill.add_argument('--dry-run', action=argparse.BooleanOptionalAction)
    fill.add_argument('--config-file', type=is_valid_file)
    fill.add_argument('--core-size', type=float, nargs=4, metavar=('llx', 'lly', 'urx', 'ury'),
                      help="lower left (x, y) and upper right (x, y) points of the chip core.")
    fill.add_argument('--max-processes', type=int, default=cpu_count(),
                      help="Limits the number of processes for preparing and filling tiles. "
                      "Defaults to the available CPU cores.")
    fill.set_defaults(func=func_fill)

    erase = subparsers.add_parser('erase', help='Erase dummy metal from chip')
    erase.add_argument("input", type=is_valid_file)
    erase.set_defaults(func=func_erase)

    density = subparsers.add_parser('density', help='Calculate density for each layer')
    density.add_argument("input", type=is_valid_file)
    density.set_defaults(func=func_density)

    return parser.parse_args()


def main():
    """
    Entry point for the gdsfill CLI.

    Parses arguments, checks Klayout version compatibility,
    and dispatches to the selected subcommand.

    Returns:
        int: Exit code (0 on success, nonzero on error).
    """
    args = arguments()
    pdk = PdkInformation(args.process, args.config_file if 'config_file' in args else None)
    klayout_version = get_version()
    if klayout_version < pdk.get_minimum_klayout_version():
        print(f"Please install Klayout {pdk.get_minimum_klayout_version()}")
        return 1

    args.func(args, pdk)
    return 0


if __name__ == '__main__':
    main()
