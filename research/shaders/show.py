import json,sys
d=json.load(open(sys.argv[1])); kind,name=sys.argv[2],sys.argv[3]
pool={'m':d['materials'],'g':d['node_groups'],'w':d['worlds']}[kind]
t=[x for x in pool if name in (x.get('material'),x.get('world'),x['name'])][0]
inl={}
for a,as_,b,bs in t['links']: inl.setdefault(b,{})[bs]=(a,as_)
keep={'operation','data_type','blend_type','use_clamp','noise_dimensions','noise_type','normalize','interpolation_type','clamp','rotation_type','attribute_name','attribute_type','vector_type','input_type','integer','mode','distribution','transform_space','domain','feature','distance','voronoi_dimensions','color_mode','fill_mode','spline_type'}
def fmt(v):
    if isinstance(v,float): return f"{v:.4g}"
    if isinstance(v,list): return "("+",".join(fmt(x) for x in v)+")"
    return str(v)
for n in sorted(t['nodes'],key=lambda n:(n['loc'][0],n['loc'][1])):
    if n['type']=='NodeFrame': continue
    props={k:v for k,v in n['props'].items() if k in keep and v not in (None,'')}
    ins=[]
    for i in n['inputs']:
        if i['linked']:
            a=inl.get(n['name'],{}).get(i['id']); ins.append(f"{i['id']}<-{a[0]}.{a[1]}" if a else f"{i['id']}<-?")
        elif 'value' in i and i['value'] not in (None,[],''):
            ins.append(f"{i['id']}={fmt(i['value'])}")
    if 'ramp' in n: print(f"   RAMP {n['ramp']}")
    print(f"{n['name']} [{n['type'].replace('ShaderNode','').replace('GeometryNode','G:')}] {fmt(props) if props else ''}\n    "+"; ".join(ins))
