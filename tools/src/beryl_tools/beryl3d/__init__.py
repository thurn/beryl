"""Beryl 3D's tools, behind `just b3d` (docs/beryl-3d.md, 'Checks and isolation').

The main tools import this package in four places only: the `b3d` command (cli.py), the
`beryl3d` lane of the fast check (checks.fast_lanes), the rule that keeps Beryl 3D's paths
out of every other lane's choice (paths.split) and slow CI's start of Beryl 3D's sweep
(sweep.start_background). A check or command Beryl 3D adds lives here.
It is a package rather than a module because slow CI fingerprints only the top-level modules
of beryl_tools, so an edit here reruns no slow-CI lane but the python lane.
"""
