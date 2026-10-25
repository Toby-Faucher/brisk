import bpy
sc=bpy.context.scene; v=sc.view_settings
print('CM', v.view_transform, v.look, 'exposure', v.exposure, 'gamma', v.gamma, 'display', sc.display_settings.display_device, 'seq', sc.sequencer_colorspace_settings.name, 'samples', getattr(sc.eevee,'taa_render_samples',None), 'film_filter', getattr(sc.render,'filter_size',None), 'bloom', getattr(sc.eevee,'use_bloom',None))
