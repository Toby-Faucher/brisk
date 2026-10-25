import bpy
for o in bpy.data.objects:
    if o.type == 'MESH':
        xs=[v.co.x for v in o.data.vertices]; ys=[v.co.y for v in o.data.vertices]
        print('PL2', o.name, 'local x', round(min(xs),3), round(max(xs),3), 'y', round(min(ys),3), round(max(ys),3), 'scale', tuple(round(x,3) for x in o.scale), 'dims', tuple(round(x,3) for x in o.dimensions))
