// Port of rocks-blender5.2.blend (celestialmaze / cmzw_), seen from above, with its compositor (see rocks.post.frag).
// Geometry nodes: jittered grid -> merge by distance -> dual mesh = irregular cells, faces scaled 1.05, cells deleted where
// noise < 0.5, subdivision-rounded, extruded by noise (0.04..0.24) = round boulders. Material: Diffuse -> Shader to RGB -> stepped toon
// ramp * AO, times a pastel colour per rock (white noise of the island index), a stepped tone, and a colour noise pushed through
// Hue/Saturation/Value; shadows multiplied toward yellow-green. Ground: Principled, green noise colour.
uniform float U_SCALE;  // cells across the view height
uniform float U_SPEED;
float TT;
struct Cell { vec2 c; vec2 c2; float edge; float keep; float height; float rnd; };
Cell cellAt(vec2 p){
  vec2 ip = floor(p), fp = fract(p);
  float f1=9., f2=9.; vec2 cc=vec2(0.), c2=vec2(0.);
  for (int j=-2;j<=2;j++) for (int i=-2;i<=2;i++){
    vec2 g=vec2(float(i),float(j)); vec2 o2=hash22(ip+g)*0.92+0.04; vec2 q=g+o2-fp;
    float d=dot(q,q);
    if (d<f1){ f2=f1; c2=cc; f1=d; cc=ip+g+o2; } else if (d<f2){ f2=d; c2=ip+g+o2; }
  }
  Cell r; r.c=cc; r.c2=c2;
  vec2 mid=0.5*(cc+c2); r.edge = dot(mid-p, normalize(c2-cc));
  r.keep = bnoise(vec3(cc*0.45, TT), 1.0, 1.0, 0.5, 0.0, true);                       // Delete Geometry: noise < 0.5 is removed
  float hN = bnoise(vec3(cc*0.45+7.3, TT), 1.5, 1.0, 0.5, 0.0, true);
  r.height = mapRange(hN, 0.35, 0.8, 0.15, 1.0);                                     // Extrude Mesh offset scale (0.04..0.24)
  r.rnd = hash12(cc+1.3);
  return r;
}
float prof(float e){ return sqrt(clamp(e/0.42, 0.0, 1.0)); }                        // rounded top (subdivision surface)
float hgt(Cell C){ return step(0.5, C.keep) * C.height * 1.5 * prof(C.edge - 0.04); }
void main(){
  TT = T*U_SPEED*0.04;
  vec2 p = (gl_FragCoord.xy - 0.5*R)/R.y * U_SCALE;
  Cell C = cellAt(p);
  vec3 L = normalize(vec3(-0.55, 0.62, 0.60));
  vec2 toSun = normalize(L.xy); float slope = L.z/length(L.xy);
  vec3 col;
  float hp = hgt(C);
  // light shadow: marches towards the sun and checks whether a taller rock is in the way
  float sh = 0.0;
  for (int i=1;i<=3;i++){ float dd = 0.3*float(i); Cell S = cellAt(p + toSun*dd); sh = max(sh, step(hp + dd*slope, hgt(S))); }
  bool isRock = C.keep >= 0.5 && (C.edge - 0.04) > 0.0;
  if (!isRock) {
    vec3 gN = bnoise3(vec3(p*0.22, 2.0), 1.0, 2.0, 0.5, 1.4, true);
    vec3 grass = 0.27 * vec3(1.1*gN.x, 0.52*gN.y + 0.74, 0.5*gN.z);                // Mix (multiply-add) from the ground material
    float gap = (C.keep >= 0.5) ? 0.55 : 1.0;                     // a dark crack between neighbouring rocks
    col = grass * mix(1.0, 0.45, sh) * gap;
  } else {
    float e = C.edge - 0.04;                                   // faces scaled 1.05 -> small gaps
    vec2 gradOut = normalize(C.c2 - C.c);
    float bev = (1.0 - smoothstep(0.0, 0.4, e))*1.5;           // round boulder
    vec3 bump = bnoise3(vec3(p*3.0, C.rnd*10.0), 1.0, 2.0, 0.6, 0.0, true) - 0.5;
    vec3 n = normalize(vec3(-gradOut*bev + bump.xy*0.35, 1.0));
    float diff = max(dot(n,L),0.0)*0.8*(1.0 - 0.55*sh);
    float toon = stepped(diff, 0.04, 0.2, 0.32, 1.0, 2.0);       // Shader to RGB -> Map Range (Stepped, 2)
    float ao = mix(0.55, 1.0, smoothstep(0.0, 0.3, e)) * mix(0.8, 1.0, C.height);   // Ambient Occlusion
    // colour: pastel per rock * stepped tone * colour noise
    vec3 pastel = hsvAdj(vec3(hash12(C.c+2.1), hash12(C.c+5.7), hash12(C.c+8.3)), 0.16, 0.2, 1.1);
    float tone = stepped(bnoise(vec3(p*0.6, 1.0), 5.48, 2.0, 0.5, 2.1, true), 0.0, 0.76, 0.34, 1.0, 4.0);
    vec3 cn = hsvAdj(bnoise3(vec3(floor(p*8.0)/8.0, 3.0), 0.5, 2.0, 0.5, 2.1, true), 0.56, 0.48, 1.36);
    vec3 base = vec3(toon*ao) * pastel * tone * cn * 1.5;
    float shadow = step(diff, 0.08);                                             // Math (Less Than 0.08)
    col = mix(base, base*vec3(0.8426,1.0,0.35), shadow*0.36);                       // Mix (Multiply, 0.36); blue floor raised so shadows stay readable
  }
  o = vec4(toSRGB(col), 1.0);
}
