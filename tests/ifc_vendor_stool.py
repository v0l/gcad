import sys
import ifcopenshell
import ifcopenshell.api

f = ifcopenshell.api.run("project.create_file", version="IFC4")
ifcopenshell.api.run("root.create_entity", f, ifc_class="IfcProject", name="vendor")
ifcopenshell.api.run("unit.assign_unit", f)
ctx = ifcopenshell.api.run("context.add_context", f, context_type="Model")
body = ifcopenshell.api.run(
    "context.add_context",
    f,
    context_type="Model",
    context_identifier="Body",
    target_view="MODEL_VIEW",
    parent=ctx,
)
stool = ifcopenshell.api.run("root.create_entity", f, ifc_class="IfcFurniture", name="stool")
x, y, z = 0.4, 0.3, 0.5
vertices = [[(0, 0, 0), (x, 0, 0), (x, y, 0), (0, y, 0), (0, 0, z), (x, 0, z), (x, y, z), (0, y, z)]]
faces = [[(0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7)]]
rep = ifcopenshell.api.run(
    "geometry.add_mesh_representation", f, context=body, vertices=vertices, faces=faces
)
ifcopenshell.api.run("geometry.assign_representation", f, product=stool, representation=rep)
f.write(sys.argv[1])
