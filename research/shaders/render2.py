import sys, bpy, math
args = sys.argv[sys.argv.index('--') + 1:]
out, frame, plane_name = args[0], int(args[1]), args[2]
sc = bpy.context.scene
pl = bpy.data.objects[plane_name]
print('PLANE dims', tuple(round(x,3) for x in pl.dimensions), 'loc', tuple(round(x,3) for x in pl.location), 'rot', tuple(round(x,3) for x in pl.rotation_euler), 'scale', tuple(pl.scale), 'modifiers', [m.type for m in pl.modifiers])
cam = bpy.data.objects.new('cam', bpy.data.cameras.new('cam')); sc.collection.objects.link(cam); sc.camera = cam
cam.data.type = 'ORTHO'; cam.data.ortho_scale = max(pl.dimensions.x, pl.dimensions.y) * 1.0
cam.location = (pl.location.x, pl.location.y, pl.location.z + 8); cam.rotation_euler = (0, 0, 0)
sc.render.resolution_x = 540; sc.render.resolution_y = 540; sc.render.resolution_percentage = 100
sc.frame_set(frame)
if hasattr(sc, 'eevee'): sc.eevee.taa_render_samples = 8
sc.render.filepath = out
bpy.ops.render.render(write_still=True)
print('DONE', out)
