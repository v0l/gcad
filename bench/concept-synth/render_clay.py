import json
import math
import os
import sys

import bpy
from mathutils import Vector

SIZE = 512
AZIMUTH = math.radians(35)
ELEVATION = math.radians(25)


def clear():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def import_glb(path):
    bpy.ops.import_scene.gltf(filepath=path)
    return [o for o in bpy.context.scene.objects if o.type == "MESH"]


def bounds(objs):
    lo = Vector((1e18,) * 3)
    hi = Vector((-1e18,) * 3)
    for o in objs:
        for corner in o.bound_box:
            w = o.matrix_world @ Vector(corner)
            lo = Vector(min(a, b) for a, b in zip(lo, w))
            hi = Vector(max(a, b) for a, b in zip(hi, w))
    return lo, hi


def has_texture(objs):
    for o in objs:
        for slot in o.material_slots:
            m = slot.material
            if m and m.use_nodes:
                for n in m.node_tree.nodes:
                    if n.type == "TEX_IMAGE" and n.image is not None and n.image.has_data:
                        return True
    return False


def setup_render(textured):
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.render.resolution_x = SIZE
    scene.render.resolution_y = SIZE
    scene.render.film_transparent = True
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    scene.view_settings.view_transform = "Standard"
    shading = scene.display.shading
    shading.light = "STUDIO"
    shading.show_cavity = True
    if textured:
        shading.color_type = "TEXTURE"
    else:
        shading.color_type = "SINGLE"
        shading.single_color = (0.72, 0.70, 0.66)


def camera_for(lo, hi):
    centre = (lo + hi) / 2
    radius = (hi - lo).length / 2
    cam = bpy.data.cameras.new("cam")
    cam.type = "ORTHO"
    cam.ortho_scale = radius * 2 * 1.08
    obj = bpy.data.objects.new("cam", cam)
    bpy.context.scene.collection.objects.link(obj)
    direction = Vector((math.cos(ELEVATION) * math.sin(AZIMUTH), -math.cos(ELEVATION) * math.cos(AZIMUTH), math.sin(ELEVATION)))
    obj.location = centre + direction * radius * 4
    look = (centre - obj.location).normalized()
    obj.rotation_euler = look.to_track_quat("-Z", "Y").to_euler()
    bpy.context.scene.camera = obj


def render_one(glb, out):
    clear()
    objs = import_glb(glb)
    if not objs:
        raise RuntimeError("no mesh")
    lo, hi = bounds(objs)
    textured = has_texture(objs)
    setup_render(textured)
    camera_for(lo, hi)
    bpy.context.scene.render.filepath = out
    bpy.ops.render.render(write_still=True)
    return textured


def main():
    argv = sys.argv[sys.argv.index("--") + 1:]
    jobs = json.load(open(argv[0]))
    results = []
    for job in jobs:
        try:
            textured = render_one(job["glb"], job["out"])
            results.append({"id": job["id"], "out": job["out"], "textured": textured})
        except Exception as e:
            results.append({"id": job["id"], "error": str(e)})
        print("RENDERED", job["id"], flush=True)
    json.dump(results, open(argv[1], "w"))


main()
