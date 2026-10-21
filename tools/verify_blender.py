"""Blender-only synthetic integration test, NOT the Rust extractor implementation.

Run with blender --background --factory-startup --disable-autoexec
  --python-exit-code 1 --python tools/verify_blender.py -- /path/to/synthetic.glb
"""

import sys
import bpy

path = sys.argv[sys.argv.index("--") + 1]
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=path)

rigs = [obj for obj in bpy.context.scene.objects if obj.type == "ARMATURE"]
# Blender's importer creates an Icosphere custom bone-shape helper. That is not
# weapon geometry: identify exported meshes by their armature modifier instead.
meshes = [
    obj for obj in bpy.context.scene.objects
    if obj.type == "MESH" and any(mod.type == "ARMATURE" for mod in obj.modifiers)
]
assert len(rigs) == 1 and len(meshes) == 1, (len(rigs), len(meshes))
rig, mesh = rigs[0], meshes[0]
assert "j_gun" in rig.data.bones and "tag_origin" in rig.data.bones
assert len(mesh.data.vertices) == 3
assert "j_gun" in mesh.vertex_groups
assert bpy.data.actions
assert any(img.size[0] == 1 and img.size[1] == 1 for img in bpy.data.images)
assert any(
    material and material.use_nodes and any(
        node.type == "TEX_IMAGE" and node.image
        for node in material.node_tree.nodes
    )
    for material in mesh.data.materials
), "The exported image was not used by the mesh material"


def evaluated_positions():
    evaluated = mesh.evaluated_get(bpy.context.evaluated_depsgraph_get())
    geometry = evaluated.to_mesh()
    try:
        return [vertex.co.copy() for vertex in geometry.vertices]
    finally:
        evaluated.to_mesh_clear()

bpy.context.scene.frame_set(0)
first = rig.pose.bones["j_gun"].matrix.copy()
first_vertices = evaluated_positions()
bpy.context.scene.frame_set(100)
last = rig.pose.bones["j_gun"].matrix.copy()
last_vertices = evaluated_positions()
assert any(
    abs(first[row][column] - last[row][column]) > 0.01
    for row in range(4) for column in range(4)
), "Animation did not move the gun bone"
assert any(
    (first_vertex - last_vertex).length > 0.01
    for first_vertex, last_vertex in zip(first_vertices, last_vertices)
), "Bone motion did not deform the skinned mesh"
print("PASS: Blender imported synthetic skinned geometry, PNG material and moving bone animation:", path)
