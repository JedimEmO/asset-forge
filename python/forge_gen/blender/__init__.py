"""forge_gen.blender — the four commands that run inside headless Blender.

``prop`` (lift → normalized prop), ``rig`` (lift → rigged ``.blend`` on the
profile's skeleton), ``export`` (rigged ``.blend`` → self-contained body
``.glb``) and ``rig_build`` (the profile's ``rig.blend``/``rig.glb`` from
its fixture clip). Each module is two programs in one file: the outer half
the CLI imports (stdlib only; it finds Blender through ``launcher`` and
hands the file itself to ``blender --background --factory-startup --python
<file> -- <argv>``), and the inner half that runs when ``import bpy``
succeeds. ``_common`` is what the inner halves share. Nothing here imports
``bpy``, ``bmesh``, ``mathutils`` or ``numpy`` at module level.
"""
