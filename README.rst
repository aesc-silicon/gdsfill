gdsfill
=======

**gdsfill** is an open-source tool for inserting dummy metal fill into semiconductor layouts.
It helps designers meet density requirements and prepare GDSII layouts for manufacturing by analyzing, erasing, and generating dummy fill patterns across multiple layers.
The tool is designed to integrate easily into existing design flows and ensures reproducible, automated preparation of layouts before tape-out.

This project is still under development. Please report any issues you encounter and always verify your layout before tape-out deadlines to prevent submission failures.

Installation
############

**gdsfill** is not yet available as a Python package.
Until then, please install it directly from GitHub:

.. code-block:: text

   python3 -m venv .venv
   source .venv/bin/activate
   pip install git+https://github.com/aesc-silicon/gdsfill.git

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

The following command inserts dummy metal fill into each layer:

.. code-block:: text

   gdsfill fill <my-layout.gds>

By default, **gdsfill** creates a temporary directory for intermediate files.
Add ``--keep-data`` to store all generated files in a directory named ``gdsfill-tmp``:

.. code-block:: text

   gdsfill fill <my-layout.gds> --keep-data

To simulate the process without modifying the layout file, use ``--dry-run``:

.. code-block:: text

   gdsfill fill <my-layout.gds> --dry-run
