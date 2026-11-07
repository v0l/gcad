import bpy
import math
import sys

argv = sys.argv[sys.argv.index("--") + 1:]
src, out, clip, seconds = argv[0], argv[1], argv[2], float(argv[3])
view = argv[4] if len(argv) > 4 else "side"

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=src)
scene = bpy.context.scene
arm = next(o for o in scene.objects if o.type == "ARMATURE")
ours = next(o for o in scene.objects if o.type == "MESH")

theirs = ours.copy()
theirs.data = ours.data.copy()
scene.collection.objects.link(theirs)
for mod in list(theirs.modifiers):
    theirs.modifiers.remove(mod)
theirs.vertex_groups.clear()
theirs.parent = None
theirs.matrix_world = ours.matrix_world.copy()
bpy.ops.object.select_all(action="DESELECT")
theirs.select_set(True)
bpy.context.view_layer.objects.active = theirs
bpy.ops.object.mode_set(mode="EDIT")
bpy.ops.mesh.select_all(action="SELECT")
bpy.ops.mesh.remove_doubles(threshold=1.0e-5)
bpy.ops.object.mode_set(mode="OBJECT")
bpy.ops.object.select_all(action="DESELECT")
arm.data.pose_position = "REST"
theirs.select_set(True)
arm.select_set(True)
bpy.context.view_layer.objects.active = arm
bpy.ops.object.parent_set(type="ARMATURE_AUTO")
arm.data.pose_position = "POSE"

action = next(a for a in bpy.data.actions if a.name.startswith(clip))
arm.animation_data_create()
arm.animation_data.action = action
fps = 24
scene.render.fps = fps
scene.frame_set(int(round(seconds * fps)) + int(action.frame_range[0]))

second = arm.copy()
second.data = arm.data
scene.collection.objects.link(second)
second.animation_data_create()
second.animation_data.action = action
theirs.parent = second
for mod in theirs.modifiers:
    if mod.type == "ARMATURE":
        mod.object = second

lo = [min((ours.matrix_world @ v.co)[k] for v in ours.data.vertices) for k in range(3)]
hi = [max((ours.matrix_world @ v.co)[k] for v in ours.data.vertices) for k in range(3)]
size = max(hi[k] - lo[k] for k in range(3))
gap = size * 1.1
if view == "side":
    second.location.y += gap
else:
    second.location.x += gap
scene.frame_set(scene.frame_current)
bpy.context.view_layer.update()
deps = bpy.context.evaluated_depsgraph_get()
points = []
for obj in (ours, theirs):
    ev = obj.evaluated_get(deps)
    m = ev.to_mesh()
    points += [ev.matrix_world @ v.co for v in m.vertices]
    ev.to_mesh_clear()
lo = [min(p[k] for p in points) for k in range(3)]
hi = [max(p[k] for p in points) for k in range(3)]
span = max(hi[k] - lo[k] for k in range(3))
centre = [(lo[k] + hi[k]) / 2 for k in range(3)]
cam_data = bpy.data.cameras.new("cam")
cam_data.type = "ORTHO"
cam_data.ortho_scale = span * 1.1
cam = bpy.data.objects.new("cam", cam_data)
scene.collection.objects.link(cam)
scene.camera = cam
if view == "side":
    cam.location = (centre[0] + span * 3, centre[1], centre[2])
    cam.rotation_euler = (math.radians(90), 0, math.radians(90))
else:
    cam.location = (centre[0], centre[1] - span * 3, centre[2])
    cam.rotation_euler = (math.radians(90), 0, 0)

scene.render.engine = "BLENDER_WORKBENCH"
scene.display.shading.light = "MATCAP"
scene.display.shading.color_type = "SINGLE"
scene.render.resolution_x = 1400
scene.render.resolution_y = 800
scene.render.filepath = out
bpy.ops.render.render(write_still=True)
print("rendered", out)
