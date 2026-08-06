"""Headless FBX -> GLB conversion for Bevy.

Usage:
    nix run nixpkgs#blender -- --background --factory-startup \
        --python scripts/fbx2glb.py -- in.fbx out.glb
"""

import sys

argv = sys.argv[sys.argv.index("--") + 1 :]
src, dst = argv[0], argv[1]

import bpy

# --factory-startup already gives a clean session; empty the default cube scene.
bpy.ops.wm.read_factory_settings(use_empty=True)

bpy.ops.import_scene.fbx(
    filepath=src,
    use_anim=True,
    # Rebuilds bone orientation from the hierarchy instead of trusting FBX's
    # often-garbage bone rolls. Almost always what you want for game rigs.
    automatic_bone_orientation=True,
)

bpy.ops.export_scene.gltf(
    filepath=dst,
    export_format="GLB",
    export_yup=True,        # glTF convention; Bevy's loader expects it
    export_skins=True,
    export_animations=True,
    export_morph=True,      # blend shapes
    export_apply=False,     # True applies modifiers but DESTROYS shape keys
)
