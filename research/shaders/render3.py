import sys, bpy, math, mathutils
args = sys.argv[sys.argv.index('--') + 1:]
out, res = args[0], int(args[1])
mode = args[2] if len(args) > 2 else 'top'
sc = bpy.context.scene
for o in bpy.data.objects:
    print('OBJ', o.name, o.type, 'dims', tuple(round(x,2) for x in o.dimensions), 'loc', tuple(round(x,2) for x in o.location), 'rot', tuple(round(math.degrees(x)) for x in o.rotation_euler), 'mods', [(m.type, m.name) for m in o.modifiers], 'mat', [s.material.name for s in o.material_slots if s.material])
cam = bpy.data.objects.new('cam', bpy.data.cameras.new('cam')); sc.collection.objects.link(cam); sc.camera = cam
cam.data.type = 'ORTHO'; cam.data.ortho_scale = 3.4
if mode == 'top':
    cam.location = (0, 0, 8); cam.rotation_euler = (0, 0, 0)
else:  # 3/4 view
    d = 8.0; ang = math.radians(50)
    cam.location = (0, -d*math.sin(ang), d*math.cos(ang)); cam.rotation_euler = (ang, 0, 0)
sc.render.resolution_x = res; sc.render.resolution_y = res; sc.render.resolution_percentage = 100
if hasattr(sc, 'eevee'): sc.eevee.taa_render_samples = 8
sc.render.filepath = out
bpy.ops.render.render(write_still=True)
print('DONE', out)
