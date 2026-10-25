// Shared helpers. Blender's noise is its own Perlin variant, so these are close approximations, not bit-exact.
// ---- Blender's Noise Texture, re-implemented (Jenkins hash + Ken Perlin's improved gradient noise, as in gpu_shader_material_noise.glsl)
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
vec3 hash33(vec3 p){ return vec3(snoise(p), snoise(p+vec3(31.7,17.3,5.1)), snoise(p+vec3(7.7,43.1,23.9))); }   // cheap vec3 noise for the few places that need one
float hash13(vec3 p){ return fract(sin(dot(p,vec3(127.1,311.7,74.7)))*43758.5453123); }
float hash12(vec2 p){ return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453123); }
vec2 hash22(vec2 p){ p=vec2(dot(p,vec2(127.1,311.7)),dot(p,vec2(269.5,183.3))); return fract(sin(p)*43758.5453123); }
// Noise Texture node (3D, FBM): scale, detail, roughness, distortion, normalize. Lacunarity 2.
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
// the "Color" output: the Fac plus two more FBM samples at random offsets
vec3 bnoise3(vec3 p, float scale, float detail, float rough, float dist, bool norm){
  vec3 q = noise_distort(p*scale, dist);
  return vec3(fbm_core(q, detail, rough, norm), fbm_core(q + random_vec3_offset(3.0), detail, rough, norm), fbm_core(q + random_vec3_offset(4.0), detail, rough, norm));
}
float mapRange(float v,float a,float b,float c,float d){ return c+(d-c)*clamp((v-a)/(b-a),0.,1.); }
float smoother(float a,float b,float v){ float t=clamp((v-a)/(b-a),0.,1.); return t*t*t*(t*(t*6.-15.)+10.); }
float stepped(float v,float a,float b,float c,float d,float steps){ float t=clamp((v-a)/(b-a),0.,1.); return c+(d-c)*max(floor(t*(steps+1.0)-1e-4),0.0)/steps; }
vec3 toSRGB(vec3 c){ c = clamp(c,0.,1.); return mix(c*12.92, 1.055*pow(c,vec3(1./2.4))-.055, step(0.0031308,c)); }

vec3 hsvAdj(vec3 c, float h, float s, float v){  // Blender Hue/Saturation/Value: hue is added (0.5 = no change in Blender; here 0 = no change)
  vec4 K=vec4(0.,-1./3.,2./3.,-1.); vec4 p=mix(vec4(c.bg,K.wz),vec4(c.gb,K.xy),step(c.b,c.g)); vec4 q=mix(vec4(p.xyw,c.r),vec4(c.r,p.yzx),step(p.x,c.r));
  float d=q.x-min(q.w,q.y), e=1e-10; vec3 x=vec3(abs(q.z+(q.w-q.y)/(6.*d+e)), d/(q.x+e), q.x);
  x.x=fract(x.x+h); x.y*=s; x.z*=v;
  vec3 r=clamp(abs(fract(x.xxx+vec3(1.,2./3.,1./3.))*6.-3.)-1.,0.,1.); return x.z*mix(vec3(1.),r,x.y);
}

float hash11(float n){ return fract(sin(n*127.1)*43758.5453123); }
float smin(float a,float b,float c){ if (c<=0.) return min(a,b); float h=max(c-abs(a-b),0.)/c; return min(a,b)-h*h*h*c*(1./6.); }   // Blender Smooth Minimum
float smax(float a,float b,float c){ return -smin(-a,-b,c); }
uvec3 hash_pcg3d_u(uvec3 v){                               // Blender's hash_pcg3d_i: wrapping multiplies, arithmetic shift
  v = v*1664525u + 1013904223u; v.x += v.y*v.z; v.y += v.z*v.x; v.z += v.x*v.y;
  ivec3 s = ivec3(v) >> 16; v = v ^ uvec3(s);
  v.x += v.y*v.z; v.y += v.z*v.x; v.z += v.x*v.y; return v;
}
vec3 hash_int3_to_vec3(ivec3 k){ uvec3 h = hash_pcg3d_u(uvec3(k)) & 0x7fffffffu; return vec3(h) * (1.0/2147483647.0); }
float voro3(vec3 p){        // Voronoi Texture, F1 distance, randomness 1, scale 1 (3D), exactly as in Blender
  vec3 fl = floor(p); vec3 lp = p - fl; ivec3 cp = ivec3(fl); float d = 1e9;
  for (int k=-1;k<=1;k++) for (int j=-1;j<=1;j++) for (int i=-1;i<=1;i++){
    ivec3 co = ivec3(i,j,k); vec3 pt = vec3(co) + hash_int3_to_vec3(cp + co);
    d = min(d, distance(pt, lp));
  }
  return d;
}
vec3 rampStops(float t, vec3 c0, vec3 c1, vec3 c2, vec3 c3, vec3 c4, vec4 pos, bool bspline){ // 5-stop colour ramp (positions p0=0 .. p4=1 given by pos.xyz and the last implicit)
  return c0; }

// AgX (Troy Sobotka) as in three.js, plus an extra power for Blender's "High Contrast" look. Input scene-linear sRGB, output display sRGB.
vec3 agxContrast(vec3 x){ vec3 x2=x*x, x4=x2*x2; return +15.5*x4*x2 - 40.14*x4*x + 31.96*x4 - 6.868*x2*x + 0.4298*x2 + 0.1191*x - 0.00232; }
vec3 toAgX(vec3 color, float exposure, float punch){
  const mat3 toRec2020 = mat3(0.6274,0.0691,0.0164, 0.3293,0.9195,0.0880, 0.0433,0.0113,0.8956);
  const mat3 fromRec2020 = mat3(1.6605,-0.1246,-0.0182, -0.5876,1.1329,-0.1006, -0.0728,-0.0083,1.1187);
  const mat3 inset = mat3(0.856627153315983,0.137318972929847,0.11189821299995, 0.0951212405381588,0.761241990602591,0.0767994186031903, 0.0482516061458583,0.101439036467562,0.811302368396859);
  const mat3 outset = mat3(1.1271005818144368,-0.1413297634984383,-0.14132976349843826, -0.11060664309660323,1.157823702216272,-0.11060664309660294, -0.016493938717834573,-0.016493938717834257,1.2519364065950405);
  color *= exposure; color = toRec2020*color; color = inset*color;
  color = max(color, 1e-10); color = log2(color); color = (color + 12.47393)/(12.47393+4.026069); color = clamp(color,0.,1.);
  color = agxContrast(color);
  color = pow(max(color,0.), vec3(punch));                        // High Contrast look (approximation)
  color = outset*color; color = pow(max(color,0.), vec3(2.2)); color = fromRec2020*color;
  return toSRGB(clamp(color,0.,1.));
}
