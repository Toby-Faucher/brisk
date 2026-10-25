import bpy, sys, math
out = sys.argv[sys.argv.index('--') + 1]
cfgs = [(4.0, 0.0, 0.5), (4.0, 4.0, 0.5497), (4.0, 2.0, 0.5), (20.0, 2.0, 0.5)]
bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene
sc.render.engine = 'BLENDER_EEVEE'
sc.view_settings.view_transform = 'Raw'; sc.view_settings.look = 'None'
sc.render.resolution_x = 256; sc.render.resolution_y = 256
cam = bpy.data.objects.new('cam', bpy.data.cameras.new('cam')); sc.collection.objects.link(cam); sc.camera = cam
cam.data.type = 'ORTHO'; cam.data.ortho_scale = 2.0; cam.location = (0, 0, 2)
bpy.ops.mesh.primitive_plane_add(size=2.0); pl = bpy.context.active_object
mat = bpy.data.materials.new('m'); mat.use_nodes = True; nt = mat.node_tree; nt.nodes.clear()
tc = nt.nodes.new('ShaderNodeTexCoord'); nz = nt.nodes.new('ShaderNodeTexNoise'); em = nt.nodes.new('ShaderNodeEmission'); oo = nt.nodes.new('ShaderNodeOutputMaterial')
nz.noise_dimensions = '3D'; nz.noise_type = 'FBM'; nz.normalize = True
nt.links.new(tc.outputs['Object'], nz.inputs['Vector']); nt.links.new(nz.outputs['Fac'], em.inputs['Color']); nt.links.new(em.outputs['Emission'], oo.inputs['Surface'])
pl.data.materials.append(mat)
if hasattr(sc.eevee, 'taa_render_samples'): sc.eevee.taa_render_samples = 1
for i, (scale, detail, rough) in enumerate(cfgs):
    nz.inputs['Scale'].default_value = scale; nz.inputs['Detail'].default_value = detail; nz.inputs['Roughness'].default_value = rough
    sc.render.filepath = f'{out}-{i}.png'
    sc.render.image_settings.file_format = 'PNG'; sc.render.image_settings.color_depth = '16'
    bpy.ops.render.render(write_still=True)
print('CAL done', cfgs)
