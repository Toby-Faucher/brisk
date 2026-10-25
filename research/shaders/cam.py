import bpy
sc=bpy.context.scene; c=sc.camera
print('CAM', c.name, c.data.type, 'ortho', c.data.ortho_scale, 'lens', c.data.lens, 'loc', tuple(round(x,3) for x in c.location), 'rot', tuple(round(x,3) for x in c.rotation_euler), 'shift', c.data.shift_x, c.data.shift_y, 'sensor', c.data.sensor_width)
