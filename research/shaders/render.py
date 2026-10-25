import sys, bpy
out = sys.argv[sys.argv.index('--') + 1]
sc = bpy.context.scene
print('ENGINE', sc.render.engine, 'res', sc.render.resolution_x, sc.render.resolution_y, sc.render.resolution_percentage, 'frames', sc.frame_start, sc.frame_end, 'cam', sc.camera and sc.camera.name, 'compositor', getattr(sc, 'use_nodes', None), 'comp tree', sc.compositing_node_group.name if getattr(sc, 'compositing_node_group', None) else None)
for o in bpy.data.objects:
    print('OBJ', o.name, o.type, {k: o[k] for k in o.keys() if not k.startswith('_')})
sc.render.resolution_x = 960; sc.render.resolution_y = 540; sc.render.resolution_percentage = 100
if hasattr(sc, 'eevee'): sc.eevee.taa_render_samples = 16
sc.render.filepath = out
bpy.ops.render.render(write_still=True)
print('DONE', out)
