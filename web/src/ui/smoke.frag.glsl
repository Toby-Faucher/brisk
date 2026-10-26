#version 300 es
// "Storm Flow" smoke for Brisk: research/shaders/ports/storm2d.frag plus the helpers it needs from common.glsl,
// recoloured to the accent (A, linear RGB). Feature size, speed and detail are fixed at the showcase defaults.
precision highp float;
uniform vec2 R; uniform float T; uniform vec3 A;
out vec4 o;
const float U_SPEED = 1.0, U_SCALE = 2.2, U_DETAIL = 6.0;
uint rotl(uint x, uint k){ return (x << k) | (x >> (32u - k)); }
uint hash_uint3(uint kx, uint ky, uint kz){
  uint a = 0xdeadbeefu + (3u << 2u) + 13u, b = a, c = a;
  c += kz; b += ky; a += kx;
  c ^= b; c -= rotl(b,14u); a ^= c; a -= rotl(c,11u); b ^= a; b -= rotl(a,25u); c ^= b; c -= rotl(b,16u);
  a ^= c; a -= rotl(c,4u);  b ^= a; b -= rotl(a,14u);  c ^= b; c -= rotl(b,24u);
  return c;
}
uint hash_int3(int x, int y, int z){ return hash_uint3(uint(x), uint(y), uint(z)); }
float negif(float v, uint cond){ return (cond != 0u) ? -v : v; }
float noise_grad(uint hash, float x, float y, float z){
  uint h = hash & 15u; float u = (h < 8u) ? x : y; float vt = ((h == 12u) || (h == 14u)) ? x : z; float v = (h < 4u) ? y : vt;
  return negif(u, h & 1u) + negif(v, h & 2u);
}
float fade(float t){ return t*t*t*(t*(t*6.0 - 15.0) + 10.0); }
float perlin_signed(vec3 p){
  vec3 fl = floor(p); ivec3 I = ivec3(fl); vec3 f = p - fl;
  float u = fade(f.x), v = fade(f.y), w = fade(f.z);
  return mix(mix(mix(noise_grad(hash_int3(I.x,I.y,I.z),       f.x,     f.y,     f.z),     noise_grad(hash_int3(I.x+1,I.y,I.z),     f.x-1.0, f.y,     f.z),     u),
                 mix(noise_grad(hash_int3(I.x,I.y+1,I.z),     f.x,     f.y-1.0, f.z),     noise_grad(hash_int3(I.x+1,I.y+1,I.z),   f.x-1.0, f.y-1.0, f.z),     u), v),
             mix(mix(noise_grad(hash_int3(I.x,I.y,I.z+1),     f.x,     f.y,     f.z-1.0), noise_grad(hash_int3(I.x+1,I.y,I.z+1),   f.x-1.0, f.y,     f.z-1.0), u),
                 mix(noise_grad(hash_int3(I.x,I.y+1,I.z+1),   f.x,     f.y-1.0, f.z-1.0), noise_grad(hash_int3(I.x+1,I.y+1,I.z+1), f.x-1.0, f.y-1.0, f.z-1.0), u), v), w);
}
float snoise(vec3 p){ return 0.9820 * perlin_signed(p); }                 // noise_scale3
float fbm_core(vec3 p, float detail, float rough, bool norm){
  float sum = 0.0, amp = 1.0, mx = 0.0, fs = 1.0;
  for (int i=0;i<=8;i++){
    if (float(i) > detail) break;
    sum += snoise(fs*p)*amp; mx += amp; amp *= rough; fs *= 2.0;
  }
  float rmd = detail - floor(detail);
  if (rmd != 0.0){
    float t = snoise(fs*p); float sum2 = sum + t*amp;
    return norm ? mix(0.5*sum/mx + 0.5, 0.5*sum2/(mx+amp) + 0.5, rmd) : mix(sum, sum2, rmd);
  }
  return norm ? 0.5*sum/mx + 0.5 : sum;
}
uint hash_uint2(uint kx, uint ky){
  uint a = 0xdeadbeefu + (2u << 2u) + 13u, b = a, c = a; b += ky; a += kx;
  c ^= b; c -= rotl(b,14u); a ^= c; a -= rotl(c,11u); b ^= a; b -= rotl(a,25u); c ^= b; c -= rotl(b,16u);
  a ^= c; a -= rotl(c,4u);  b ^= a; b -= rotl(a,14u);  c ^= b; c -= rotl(b,24u);
  return c;
}
float hash_vec2_to_float(vec2 k){ return float(hash_uint2(floatBitsToUint(k.x), floatBitsToUint(k.y))) / 4294967295.0; }
vec3 random_vec3_offset(float seed){ return vec3(100.0 + hash_vec2_to_float(vec2(seed,0.0))*100.0, 100.0 + hash_vec2_to_float(vec2(seed,1.0))*100.0, 100.0 + hash_vec2_to_float(vec2(seed,2.0))*100.0); }
vec3 noise_distort(vec3 p, float dist){
  if (dist == 0.0) return p;
  return p + dist * vec3(snoise(p + random_vec3_offset(0.0)), snoise(p + random_vec3_offset(1.0)), snoise(p + random_vec3_offset(2.0)));
}
float bnoise(vec3 p, float scale, float detail, float rough, float dist, bool norm){
  return fbm_core(noise_distort(p*scale, dist), detail, rough, norm);
}
float smoother(float a,float b,float v){ float t=clamp((v-a)/(b-a),0.,1.); return t*t*t*(t*(t*6.-15.)+10.); }
vec3 toSRGB(vec3 c){ c = clamp(c,0.,1.); return mix(c*12.92, 1.055*pow(c,vec3(1./2.4))-.055, step(0.0031308,c)); }
float hash11(float n){ return fract(sin(n*127.1)*43758.5453123); }
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
  vec3 mixc = mix(I*A*0.35, I*mix(A, vec3(1.0), 0.2)*0.9, m);   // Brisk: the original brown <-> blue-white, as dim <-> bright accent
  vec3 col = pow(clamp(mixc*4.4, 0.0, 16.0), vec3(0.9));
  col *= 1.0 - 0.28*dot(uv, uv)*0.5;                          // faint vignette
  o = vec4(toSRGB(col), 1.0);
}
