import bpy, math
for o in bpy.data.objects:
    if o.type == 'MESH': print('PL', o.name, 'rot(deg)', tuple(round(math.degrees(x)) for x in o.rotation_euler), 'scale', tuple(round(x,3) for x in o.scale), 'loc', tuple(round(x,3) for x in o.location), 'parent', o.parent)
