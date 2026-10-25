#!/usr/bin/env python3
"""Builds showcase.html: the three ported shaders in one page (WebGL2). Usage: build.py OUT.html"""
import sys, json, pathlib, base64
here = pathlib.Path(__file__).parent
common = (here / 'common.glsl').read_text()
HEAD = "#version 300 es\nprecision highp float;\nuniform vec2 R; uniform float T; out vec4 o;\n"
shaders = {
  'hex': {'title': 'Hex Truchet', 'file': 'hex-truchet', 'src': 'hex-truchet-blender5.1.blend',
          'blurb': 'A hex lattice with two glowing tubes per cell, rotated at random, and a pulse that travels along each tube.',
          'notes': 'Ported: lattice, per-cell random rotation (30° + 60°k), both Bézier tubes, the hex outline, per-cell green choice, the sawtooth comet pulse. Different: it is 2D (no tube volume), glow is a falloff instead of Blender bloom.',
          'ctl': [['U_SCALE', 'Cells (view height)', 4, 16, 8, 0.5], ['U_SPEED', 'Pulse speed', 0, 3, 1, 0.05]]},
  'ocean': {'title': 'Ocean Waves', 'file': 'ocean', 'src': 'oceanwaves_b42.blend',
          'blurb': 'A flat, stylised sea. A travelling sine, bent by noise, pushes the colour through deep water, teal and foam.',
          'notes': 'Ported: all five noises, the warp of the wave coordinate, every mix and ramp in the colour graph, the stepped foam, the trough shading. Different: the original also displaces a 3D plane; this does not. Blender noise is approximated.',
          'ctl': [['U_WAVES', 'Wave periods', 0.5, 4, 1, 0.1], ['U_SPEED', 'Wave speed', 0, 3, 1, 0.05]]},
  'rocks': {'title': 'Rocks', 'file': 'rocks', 'src': 'rocks-blender5.2.blend',
          'blurb': 'A field of irregular rocks seen from above. Some are missing, the rest stand at different heights, toon-lit with soft shadows.',
          'notes': 'Ported: irregular cells, cells deleted by noise, noise-driven height, rounded tops, sun light through a 2-step toon ramp, ambient occlusion, yellow shadow tint, stepped per-rock tone, green ground. Different: no real extrusion or cast shadows; heights only change shading.',
          'ctl': [['U_SCALE', 'Cells (view height)', 4, 24, 12, 0.5], ['U_SPEED', 'Drift speed', 0, 5, 1, 0.1]]},
  'nebula': {'title': 'Nebula', 'file': 'nebula', 'src': 'nebula-b50.blend', 'aspect': 1,
          'blurb': 'A purple gas cloud around a bright star, made by folding space five times at every step of a 70-step ray march.',
          'notes': 'Ported literally: both Repeat zones, the fold, the rotation, the distance shading and the tint. Different: Blender frames run at 24 fps here, and the final colour is a plain sRGB conversion (no film curve).',
          'ctl': [['U_SPEED', 'Speed', 0, 4, 1, 0.05], ['U_ITER', 'March steps', 10, 70, 70, 1]]},
  'overcast': {'title': 'Overcast', 'file': 'overcast', 'src': 'overcast-b50.blend', 'aspect': 1,
          'blurb': 'A sky of streaming clouds seen from below. Seventy noise samples along each ray pile up into sheets.',
          'notes': 'Ported literally: the ray set-up, the 4-octave noise, the smoothstep density band, the 70-sample sum and the B-spline colour ramp. Different: Blender noise is approximated.',
          'ctl': [['U_SPEED', 'Speed', 0, 6, 1, 0.05], ['U_ITER', 'Samples', 10, 70, 48, 1]]},
  'tunnel0': {'title': 'Tunnel 0', 'file': 'tunnel0', 'src': 'tunnel0-b50.blend', 'aspect': 1,
          'blurb': 'A rush through a tube lined with Voronoi cells. Colours come from the direction of a noise vector, so they stay saturated.',
          'notes': 'Ported literally: the sphere tracer, the five Voronoi layers with Blender smooth min and max, the colour rule. Different: it stops stepping once the ray has settled (the original runs all 100 steps).',
          'ctl': [['U_SPEED', 'Speed', 0, 4, 1, 0.05], ['U_ITER', 'Trace steps', 10, 100, 60, 1]]},
  'storm': {'title': 'Storm', 'file': 'storm', 'src': 'storm-b50.blend', 'aspect': 1,
          'blurb': 'A smoky spiral with a bright eye. The colour pulses on a 120-frame cycle with a new random tint each time.',
          'notes': 'Ported literally: the 90-step march, the 3-fold inner sum, the 9-stop ease ramp, the per-cycle white-noise tint, the radial falloff and the tanh. Different: Blender noise is approximated.',
          'ctl': [['U_SPEED', 'Speed', 0, 4, 1, 0.05], ['U_ITER', 'March steps', 10, 90, 90, 1]]},
  'storm2d': {'title': 'Storm Flow', 'file': 'storm2d', 'src': 'storm-b50.blend (flat re-interpretation)', 'aspect': 1,
          'blurb': 'The storm as flat, drifting smoke: no tunnel, no spiral. Fluid-like folds carry thin bright filaments, and the colour cycles every 120 frames. Move the mouse over it and the smoke flows with you.',
          'notes': 'Not a literal port: the original is a ray march down a tube, which has no 2D form. This keeps its palette, 120-frame colour cycle with a new tint per cycle, brown to blue-white mix, folded-sine filaments and tanh roll-off, and moves them with a wandering drift plus a two-pass noise warp.',
          'ctl': [['U_SPEED', 'Speed', 0, 4, 1, 0.05], ['U_SCALE', 'Feature size', 0.8, 5, 2.2, 0.05], ['U_WARP', 'Fluidity', 0, 2, 1, 0.05], ['U_DETAIL', 'Layers', 2, 8, 6, 1], ['U_MOUSE', 'Mouse push', 0, 3, 1, 0.05]]}
}
for k, v in shaders.items():
    v['frag'] = HEAD + ''.join(f'uniform float {c[0]};\n' for c in v['ctl']) + common + '\n' + (here / f"{v['file']}.frag").read_text().replace('uniform float U_SCALE;', '').replace('uniform float U_SPEED;', '').replace('uniform float U_WAVES;', '').replace('uniform float U_ITER;', '').replace('uniform float U_SWIRL;', '').replace('uniform float U_SCALE;', '').replace('uniform float U_WARP;', '').replace('uniform float U_DETAIL;', '').replace('uniform float U_MOUSE;', '')
for k, v in shaders.items():
    pf = here / f"{v['file']}.post.frag"
    v['post'] = (HEAD + 'uniform sampler2D S;\n' + common + '\n' + pf.read_text().replace('uniform sampler2D S;', '')) if pf.exists() else None
for k, v in shaders.items():
    rf = here / 'ref' / f'{k}.jpg'
    v['ref'] = 'data:image/jpeg;base64,' + base64.b64encode(rf.read_bytes()).decode() if rf.exists() else None
tpl = (here / 'showcase.tpl.html').read_text()
open(sys.argv[1], 'w').write(tpl.replace('/*SHADERS*/', json.dumps(shaders)))
print('wrote', sys.argv[1])
