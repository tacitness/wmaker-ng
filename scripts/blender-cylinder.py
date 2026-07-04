import pathlib

import bpy

out_dir = pathlib.Path("/tmp/wmaker-ng/blender")
out_dir.mkdir(parents=True, exist_ok=True)

bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()

bpy.ops.mesh.primitive_cylinder_add(vertices=64, radius=1.0, depth=2.0, location=(0, 0, 1))
cylinder = bpy.context.object
cylinder.name = "wmaker-ng-cylinder"

material = bpy.data.materials.new("wmaker-ng-blue")
material.diffuse_color = (0.18, 0.42, 0.9, 1.0)
cylinder.data.materials.append(material)

bpy.ops.object.light_add(type="AREA", location=(3, -4, 5))
light = bpy.context.object
light.name = "wmaker-ng-key-light"
light.data.energy = 450
light.data.size = 4

bpy.ops.object.camera_add(location=(4, -6, 3), rotation=(1.1, 0, 0.58))
bpy.context.scene.camera = bpy.context.object

bpy.context.scene.render.engine = "BLENDER_EEVEE_NEXT"
bpy.context.scene.render.resolution_x = 1280
bpy.context.scene.render.resolution_y = 720
bpy.context.scene.render.filepath = str(out_dir / "cylinder.png")

bpy.ops.wm.save_as_mainfile(filepath=str(out_dir / "cylinder.blend"))
bpy.ops.render.render(write_still=True)

print(f"wmaker-ng blender cylinder rendered: {bpy.context.scene.render.filepath}")
