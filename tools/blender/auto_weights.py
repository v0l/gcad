import bpy
import json
import sys

argv = sys.argv[sys.argv.index("--") + 1:]
src, out = argv[0], argv[1]

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=src)

arm = next(o for o in bpy.context.scene.objects if o.type == "ARMATURE")
mesh = next(o for o in bpy.context.scene.objects if o.type == "MESH")

for mod in list(mesh.modifiers):
    mesh.modifiers.remove(mod)
mesh.vertex_groups.clear()
mesh.parent = None

bpy.ops.object.select_all(action="DESELECT")
mesh.select_set(True)
bpy.context.view_layer.objects.active = mesh
bpy.ops.object.mode_set(mode="EDIT")
bpy.ops.mesh.select_all(action="SELECT")
bpy.ops.mesh.remove_doubles(threshold=1.0e-5)
bpy.ops.object.mode_set(mode="OBJECT")

bpy.ops.object.select_all(action="DESELECT")
arm.data.pose_position = "REST"
mesh.select_set(True)
arm.select_set(True)
bpy.context.view_layer.objects.active = arm
bpy.ops.object.parent_set(type="ARMATURE_AUTO")

world = mesh.matrix_world
names = [g.name for g in mesh.vertex_groups]
verts = []
for v in mesh.data.vertices:
    p = world @ v.co
    w = [(names[g.group], g.weight) for g in v.groups if g.weight > 0.0]
    verts.append({"p": [p.x, p.y, p.z], "w": w})

with open(out, "w") as f:
    json.dump({"vertices": verts}, f)
print("weights for", len(verts), "vertices,", len(names), "groups")
