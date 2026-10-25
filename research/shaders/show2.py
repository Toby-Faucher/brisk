import json,sys
# compact printer that keeps every constant (no filtering of default values), with Repeat zone info
d=json.load(open(sys.argv[1])); m=d['materials'][0]
inl={}
for a,as_,b,bs in m['links']: inl.setdefault(b,{})[bs]=(a,as_)
def f(v):
    if isinstance(v,float): return f"{v:.5g}"
    if isinstance(v,list): return "("+",".join(f(x) for x in v)+")"
    return str(v)
skip_types={'NodeReroute','NodeFrame'}
# resolve reroutes
byname={n['name']:n for n in m['nodes']}
def src(node,sock):
    a=inl.get(node,{}).get(sock)
    while a and byname[a[0]]['type']=='NodeReroute':
        a=inl.get(a[0],{}).get('Input')
    return a
for n in sorted(m['nodes'],key=lambda n:(n['loc'][0],n['loc'][1])):
    if n['type'] in skip_types: continue
    t=n['type'].replace('ShaderNode','').replace('GeometryNode','G:')
    op=n['props'].get('operation') or n['props'].get('blend_type') or n['props'].get('interpolation_type') or ''
    items=[]
    for i in n['inputs']:
        if i['name']=='' : continue
        a=src(n['name'],i['id'])
        if a: items.append(f"{i['name']}<-{a[0]}.{a[1]}")
        elif i.get('value') is not None: items.append(f"{i['name']}={f(i['value'])}")
    extra=''
    if 'Repeat' in n['type']: extra=f" items={n['props'].get('repeat_items')}"
    if 'ramp' in n: extra=f" RAMP={n['ramp']}"
    print(f"{n['name']} [{t}] {op}{extra}\n    {'; '.join(items)}")
