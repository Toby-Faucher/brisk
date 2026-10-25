// "Storm Flow": storm-b50.blend (celestialmaze / cmzw_) as flat, drifting smoke. The original is a ray march down a tube, so there is no
// literal 2D form. This keeps its look: the same colour ramp that cycles every 120 frames with a new tint each cycle, the brown <-> blue-white
// mix, the tanh roll-off, and its folded-sine filaments (1/|distance| glow along the zero crossings). There is no eye and no spiral: the
// field is carried along by a slow, wandering drift and a two-pass domain warp, like fluid.
uniform float U_SPEED;
uniform float U_SCALE;     // size of the features (bigger = smaller smoke)
uniform float U_WARP;      // how strongly the flow folds over itself
uniform float U_DETAIL;    // number of folded layers
uniform float U_MOUSE;     // how strongly the pointer pushes the smoke
uniform vec4 U_TRAIL[24];  // recent pointer samples: xy = position (view units), zw = velocity already faded by age
float ease(float a){ return a*a*(3.0-2.0*a); }
float rampStorm(float x){   // Color Ramp (Ease) from the original: grey values at stops
  const float p[9] = float[9](0.03182,0.03561,0.06667,0.16552,0.18806,0.23988,0.49242,0.95833,1.0);
  const float v[9] = float[9](0.0,0.5,1.0,0.0,0.0,0.5,0.0,0.0,0.25);
  if (x <= p[0]) return v[0];
  for (int i=0;i<8;i++) if (x < p[i+1]) return mix(v[i], v[i+1], ease((x-p[i])/(p[i+1]-p[i])));
  return v[8];
}
void main(){
  vec2 uv = (gl_FragCoord.xy - 0.5*R) / (0.5*R.y);
  float f = T*24.0*U_SPEED;
  float t = f*0.012;
  // a slow drift whose direction wanders a little
  float dir = 0.6 + 0.5*sin(t*0.23);
  vec2 drift = vec2(cos(dir), sin(dir)) * t * 0.55;
  vec2 p = uv*U_SCALE;
  // the pointer pushes the flow: each recent sample drags nearby smoke along its velocity (soft falloff)
  vec2 push = vec2(0.0);
  for (int i=0;i<24;i++){ vec2 d = uv - U_TRAIL[i].xy; push += U_TRAIL[i].zw * exp(-dot(d,d)*9.0); }
  p -= push * 0.11 * U_SCALE * U_MOUSE;
  // the original's folded sine layers
  vec3 Q = vec3(p + drift*0.5, t*0.6);
  Q = cos(vec3(Q.y,Q.z,Q.x)*0.56 + Q.z)*0.6 + Q;
  float s = 1.15, n = 0.15;
  for (int j=0;j<8;j++){
    if (float(j) >= U_DETAIL) break;
    s -= abs(dot(sin(Q*n*10.0 + vec3(0.0,0.0,f/32.0)), vec3(0.05)))/n;
    n *= 2.0;
  }
  float fil = 1.0/(0.2 + 1.7*abs(s));                       // glow ~ 1/|distance|: thin filaments where the sum crosses zero
  float body = smoother(0.25, 0.8, bnoise(vec3(p*0.35 + drift*0.4, t*0.4), 1.0, 3.0, 0.55, 0.0, true));   // soft clouds so the dark areas flow too
  float I = tanh(fil*0.10*(0.35 + 1.5*body));
  // colour cycle: the original's ramp (120 frames) and a random tint per cycle
  float cyc = f/120.0;
  float R0 = rampStorm(fract(cyc));
  vec3 wn = vec3(hash11(floor(cyc)+1.0), hash11(floor(cyc)+2.0), hash11(floor(cyc)+3.0));
  float nv = bnoise(vec3(p*0.5, t*0.5) + wn*100.0, 1.0, 2.0, 0.5, 0.0, true);
  float m = R0 * smoother(0.25, 0.85, nv);
  vec3 mixc = mix(I*vec3(0.08,0.05,0.05), I*vec3(3.76,3.62,12.6)*0.3, m);
  vec3 col = pow(clamp(mixc*4.4, 0.0, 16.0), vec3(0.9));
  col *= 1.0 - 0.28*dot(uv, uv)*0.5;                          // faint vignette
  o = vec4(toSRGB(col), 1.0);
}
