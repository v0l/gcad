import sys
import bpy

src, dst = sys.argv[sys.argv.index("--") + 1:][:2]
bpy.ops.wm.open_mainfile(filepath=src)


def unhide(lc):
    lc.exclude = False
    lc.hide_viewport = False
    for c in lc.children:
        unhide(c)


unhide(bpy.context.view_layer.layer_collection)
for o in bpy.context.view_layer.objects:
    o.select_set(False)
n = 0
for o in bpy.context.view_layer.objects:
    if o.type == "MESH" and len(o.data.polygons) and not o.name.lower().startswith(("wgt", "cs_", "cs.")):
        o.hide_viewport = False
        o.hide_set(False)
        o.select_set(True)
        n += 1
if not n:
    raise SystemExit("no meshes")
bpy.ops.export_scene.gltf(filepath=dst, export_format="GLB", use_selection=True, export_apply=True,
                          export_animations=False, export_skins=False, export_materials="NONE",
                          export_cameras=False, export_lights=False)
