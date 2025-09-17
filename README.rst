gdsfill
=======

**gdsfill** is an open-source tool for inserting dummy metal fill into semiconductor layouts.
It helps designers meet density requirements and prepare GDSII layouts for manufacturing by analyzing, erasing, and generating dummy fill patterns across multiple layers.
The tool is designed to integrate easily into existing design flows and ensures reproducible, automated preparation of layouts before tape-out.

This project is still under development. Please report any issues you encounter and always verify your layout before tape-out deadlines to prevent submission failures.

Installation
############

**gdsfill** can be installed as a Python package. We recommend using a virtual environment to keep dependencies isolated.

.. code-block:: text

   $ python3 -m venv venv
   $ source venv/bin/activate
   (venv) $ pip install --updage pip
   (venv) $ pip install gdsfill

Density
#######

This command calculates the utilization per layer and prints the values.
It is useful to check layer density before and after running the fill process:

.. code-block:: text

   gdsfill density <my-layout.gds>

Erase
#####

If a layout already contains dummy fill, or if previous fills should be removed, this command erases all dummy metal fill from a layout:

.. code-block:: text

   gdsfill erase <my-layout.gds>

Fill
####

To insert dummy metal fill into all supported layers of a layout, run:

.. code-block:: text

   gdsfill fill <my-layout.gds>

Some algorithms, such as the track filler, require information about the chip core region. You can provide the lower-left and upper-right coordinates as floating-point values:

.. code-block:: text

   gdsfill fill <my-layout.gds> --core-size llx lly urx ury

By default, **gdsfill** creates a temporary directory for intermediate data.
Use ``--keep-data`` to retain all generated files in a directory called ``gdsfill-tmp``:

.. code-block:: text

   gdsfill fill <my-layout.gds> --keep-data

If you only want to simulate the process without modifying the layout file, use ``--dry-run``:

.. code-block:: text

   gdsfill fill <my-layout.gds> --dry-run


Custom Configuration
####################

By default, **gdsfill** inserts dummy metal fill into each layer using predefined parameters.
To apply different parameters or restrict fill to specific layers, you can create a custom configuration file.

The following example config inserts fill only into **TopMetal1** and **TopMetal2**:

.. code-block:: yaml

   PDK: ihp-sg13g2
   layers:
     TopMetal1:
       algorithm: Square
       density: 60
       deviation: 1
     TopMetal2:
       algorithm: Square
       density: 60
       deviation: 1

.. note::
   Example config files are available in ``gdsfill/configs``.

To use a custom config file, pass it with ``--config-file``:

.. code-block:: text

   gdsfill fill <my-layout.gds> --config-file <my-config-file.yaml>
