import bpy, sys
out = sys.argv[sys.argv.index('--') + 1]
sc = bpy.context.scene
sc.view_settings.view_transform = 'Raw'; sc.view_settings.look = 'None'
sc.render.resolution_x = 540; sc.render.resolution_y = 540; sc.render.resolution_percentage = 100
sc.render.image_settings.file_format = 'PNG'; sc.render.image_settings.color_depth = '16'
try: sc.render.image_settings.media_type = 'IMAGE'
except Exception: pass
sc.frame_set(1)
mat = bpy.data.objects['Plane'].active_material; nt = mat.node_tree
src = nt.nodes['Math.004']; oo = [n for n in nt.nodes if n.bl_idname == 'ShaderNodeOutputMaterial'][0]
em = nt.nodes.new('ShaderNodeEmission'); nt.links.new(src.outputs[0], em.inputs['Color']); nt.links.new(em.outputs[0], oo.inputs['Surface'])
sc.render.filepath = out
if hasattr(sc.eevee, 'taa_render_samples'): sc.eevee.taa_render_samples = 1
bpy.ops.render.render(write_still=True)
print('FIELD done')
