"""Dump every node tree in a .blend to JSON (nodes, settings, input values, links).
Run headless:  blender -b FILE.blend --python dump_nodes.py -- OUT.json
"""
import sys, json, bpy

def val(v):
    try:
        if hasattr(v, '__len__') and not isinstance(v, str): return [val(x) for x in v]
        if isinstance(v, (int, float, bool, str)) or v is None: return v
        if hasattr(v, 'name'): return v.name
    except Exception: pass
    return str(v)

def tree(nt):
    out = {'name': nt.name, 'type': nt.bl_idname, 'nodes': [], 'links': []}
    for n in nt.nodes:
        d = {'name': n.name, 'type': n.bl_idname, 'label': n.label, 'loc': [round(n.location.x), round(n.location.y)], 'props': {}, 'inputs': [], 'outputs': [n_.identifier for n_ in n.outputs]}
        for p in n.bl_rna.properties:
            if p.identifier in ('rna_type', 'name', 'label', 'location', 'width', 'height', 'dimensions', 'inputs', 'outputs', 'internal_links', 'parent', 'select', 'show_options', 'show_preview', 'hide', 'mute', 'show_texture', 'bl_idname', 'bl_label', 'bl_description', 'bl_icon', 'bl_static_type', 'bl_width_default', 'bl_width_min', 'bl_width_max', 'bl_height_default', 'bl_height_min', 'bl_height_max', 'type', 'color', 'use_custom_color', 'width_hidden'): continue
            try: d['props'][p.identifier] = val(getattr(n, p.identifier))
            except Exception: pass
        if n.bl_idname == 'ShaderNodeValToRGB':
            d['ramp'] = {'interp': n.color_ramp.interpolation, 'stops': [[round(e.position,5), [round(x,5) for x in e.color]] for e in n.color_ramp.elements]}
        if n.bl_idname == 'ShaderNodeRGBCurve' or n.bl_idname == 'ShaderNodeFloatCurve':
            try: d['curve'] = [[[round(pt.location[0],4), round(pt.location[1],4)] for pt in c.points] for c in n.mapping.curves]
            except Exception: pass
        if n.bl_idname == 'ShaderNodeGroup' and n.node_tree: d['props']['node_tree'] = n.node_tree.name
        for i in n.inputs:
            e = {'name': i.name, 'id': i.identifier, 'linked': i.is_linked}
            if hasattr(i, 'default_value') and not i.is_linked:
                e['value'] = val(i.default_value)
            d['inputs'].append(e)
        out['nodes'].append(d)
    for l in nt.links:
        out['links'].append([l.from_node.name, l.from_socket.identifier, l.to_node.name, l.to_socket.identifier])
    return out

res = {'blender': bpy.app.version_string, 'materials': [], 'node_groups': [], 'worlds': [], 'images': [i.name for i in bpy.data.images], 'objects': [{'name': o.name, 'type': o.type} for o in bpy.data.objects]}
for m in bpy.data.materials:
    if m.node_tree: res['materials'].append(tree(m.node_tree) | {'material': m.name})
for g in bpy.data.node_groups: res['node_groups'].append(tree(g))
for w in bpy.data.worlds:
    if w.node_tree: res['worlds'].append(tree(w.node_tree) | {'world': w.name})
out = sys.argv[sys.argv.index('--') + 1] if '--' in sys.argv else '/dev/stdout'
json.dump(res, open(out, 'w'), indent=1)
print('wrote', out, len(res['materials']), 'materials,', len(res['node_groups']), 'node groups')
