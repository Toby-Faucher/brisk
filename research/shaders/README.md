# Brisk shader research (cmzw_ / celestialmaze Blender files)

Blender node files by celestialmaze (cmzw_, github.com/williamchange), re-implemented by hand as WebGL2 shaders for Brisk.
Licence of the originals: not yet checked (the `.blend` files, renders and zips are git-ignored on purpose).

## Layout
- `dump_nodes.py`, `show.py`, `show2.py`: dump a `.blend`'s node trees to JSON (`blender -b FILE --python dump_nodes.py -- out.json`) and print them compactly.
- `render*.py`, `cam.py`, `cm.py`, `pl*.py`, `noisecal*.py`, `fieldrender.py`: headless Blender helpers (EEVEE renders on bawn work; they gave the ground truth used to match each port).
- `dumps/` (node-graph JSON) and `ports/ref/` (Blender reference renders) are generated from the creator's files and are git-ignored; regenerate them with the scripts. `ports/`: the GLSL (`common.glsl` = Blender's exact Noise/Voronoi hash + AgX; one `.frag` per shader, optional `.post.frag`), `build.py` -> a single showcase HTML (needs `ports/ref/*.jpg` for the reference thumbnails, otherwise they are skipped).
- `ports/wip/storm-flow-water.frag`: Storm Flow with a real wave-equation water layer (needs the simulation code in `showcase.tpl.html`, currently unused). Shelved by Toby on 2026-10-25 ("come back later").

## State of the ports (all compared with Blender's own render)
| port | match |
|---|---|
| storm | essentially identical |
| nebula | very close (Raw view transform, plane extent +-1.316) |
| overcast | very close (AgX exposure fitted; plane extent +-0.376) |
| tunnel0 | same structure; thin lines need full resolution; heavy (use fewer steps) |
| ocean | very close (one wave, 85-frame loop) |
| rocks | same idea, flat top-down, not the painted 3/4 look |
| hex truchet | geometry/colours ported; 2D only |
| storm 2d ("Storm Flow") | NOT a port: a flat re-interpretation; drifting smoke, mouse drags the smoke |

## Lessons (saved here so they are not re-learned)
- Blender noise must be exact (Jenkins hash + Perlin gradient, `0.982` scale, FBM = detail+1 octaves); the file's colour management (Raw / Standard / AgX) and the plane's local extent both change the look completely.
- Verify any node reading against a real Blender render before trusting it: dump the intermediate field with a Raw emission render and compare numbers.
- Shaders must be OFF on the Stream screen (they share the GPU with video decode).
