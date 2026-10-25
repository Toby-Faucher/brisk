import sys, bpy, math, mathutils
args = sys.argv[sys.argv.index('--') + 1:]
out, res = args[0], int(args[1])
sc = bpy.context.scene
meshes = [o for o in bpy.data.objects if o.type == 'MESH']
info = [(o.name, tuple(round(x, 2) for x in o.dimensions), tuple(round(x, 2) for x in o.location)) for o in bpy.data.objects]
print('INFO', sc.render.engine, 'frames', sc.frame_start, sc.frame_end, 'res', sc.render.resolution_x, sc.render.resolution_y, info, {o.name: {k: o[k] for k in o.keys() if not k.startswith('_') and isinstance(o[k], (int, float))} for o in bpy.data.objects if o.keys()})
if not sc.camera:
    pts = []
    for o in meshes:
        for c in o.bound_box: pts.append(o.matrix_world @ mathutils.Vector(c))
    if pts:
        mn = mathutils.Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts))); mx = mathutils.Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    else: mn = mathutils.Vector((-1, -1, -1)); mx = mathutils.Vector((1, 1, 1))
    ctr = (mn + mx) / 2; size = max((mx - mn).length, 1.0)
    cam = bpy.data.objects.new('cam', bpy.data.cameras.new('cam')); sc.collection.objects.link(cam); sc.camera = cam
    flat = (mx - mn).z < 0.05 * max(mx.x - mn.x, mx.y - mn.y, 0.01)
    if flat:
        cam.data.type = 'ORTHO'; cam.data.ortho_scale = max(mx.x - mn.x, mx.y - mn.y); cam.location = (ctr.x, ctr.y, ctr.z + 10)
    else:
        cam.data.lens = 50; ang = math.radians(65); d = size * 1.9
        cam.location = (ctr.x, ctr.y - d * math.sin(ang), ctr.z + d * math.cos(ang) * 0.6 + size * 0.1)
        direction = ctr - cam.location; cam.rotation_euler = direction.to_track_quat('-Z', 'Y').to_euler()
sc.render.resolution_x = res; sc.render.resolution_y = res; sc.render.resolution_percentage = 100
if hasattr(sc, 'eevee'): sc.eevee.taa_render_samples = 8
try:
    sc.render.image_settings.media_type = 'IMAGE'
except Exception as e: print('mt', e)
sc.render.image_settings.file_format = 'PNG'
sc.render.filepath = out
bpy.ops.render.render(write_still=True)
import os
if not os.path.exists(out):
    try: bpy.data.images['Render Result'].save_render(out)
    except Exception as e: print('save_render failed', e)
print('DONE', out, os.path.exists(out))
