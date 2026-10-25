// Port of hex-truchet-blender5.1.blend (celestialmaze / cmzw_). Geometry nodes: hex lattice (dual of a triangulated grid),
// per-cell random rotation (30 deg + 60 deg*k) of two quadratic Bezier tubes joining hex edge midpoints, plus a hex outline.
// Material: emission, two greens picked per cell, strength = max(smootherstep ramps of a fract() sawtooth) = a comet pulse.
uniform float U_SCALE;  // cells across the view height
uniform float U_SPEED;
const float S3 = 1.7320508;
vec2 hexRound(vec2 q){
  float x=q.x, z=q.y, y=-x-z; float rx=floor(x+.5), ry=floor(y+.5), rz=floor(z+.5);
  float dx=abs(rx-x), dy=abs(ry-y), dz=abs(rz-z);
  if (dx>dy && dx>dz) rx=-ry-rz; else if (dy>dz) ry=-rx-rz; else rz=-rx-ry;
  return vec2(rx,rz);
}
vec2 bez(vec2 p, vec2 a, vec2 b, vec2 c){ // (distance, parameter) to a quadratic Bezier, via a polyline
  float best=1e9, bt=0.; vec2 prev=a;
  for (int i=1;i<=14;i++){
    float t=float(i)/14.; vec2 q=(1.-t)*(1.-t)*a+2.*(1.-t)*t*b+t*t*c;
    vec2 pa=p-prev, ba=q-prev; float hh=clamp(dot(pa,ba)/dot(ba,ba),0.,1.); float d=length(pa-ba*hh);
    if (d<best){ best=d; bt=(float(i-1)+hh)/14.; } prev=q;
  }
  return vec2(best,bt);
}
float pulse(float u,float rnd,float speed){ // Math.003/Math/FRACT/Map Range nodes from the material
  float s = fract(u - speed + rnd) - 0.5;
  return max(smoother(0.35,0.64,s), mapRange(s,-0.36,0.5,0.05,1.0)*smoother(-0.36,0.5,s));
}
void main(){
  vec2 p = (gl_FragCoord.xy - 0.5*R)/R.y * U_SCALE;      // hex circumradius = 1
  vec2 qr = vec2(S3/3.*p.x - p.y/3., 2./3.*p.y);
  vec2 id = hexRound(qr);
  vec2 c = vec2(S3*(id.x + id.y*0.5), 1.5*id.y);
  vec2 l = p - c;
  float r1 = hash12(id+3.7), r2 = hash12(id+11.1);
  float k = floor(r1*6.0);
  float th = radians(30.0) + radians(60.0)*k;
  float cs=cos(-th), sn=sin(-th); vec2 pl = vec2(cs*l.x - sn*l.y, sn*l.x + cs*l.y); // into the canonical tile
  vec3 gA = vec3(0.3029,0.6655,0.1828), gB = vec3(0.1175,0.5793,0.267);
  vec3 tint = (r2 < 0.5) ? gA : gB;
  float sp = T*U_SPEED*0.22;
  float w = 0.022;
  vec3 col = vec3(0.004,0.010,0.012) + 0.015*(1.0-length(gl_FragCoord.xy/R-0.5));
  // curve A and mirrored curve B (Bezier.001 flipped in y)
  vec2 a = bez(pl, vec2(0.75,0.433), vec2(0.26,0.0), vec2(0.75,-0.433));
  vec2 b = bez(pl, vec2(-0.75,-0.433), vec2(0.0,0.0), vec2(0.0,0.866));
  float sA = pulse(a.y*0.9, r1, sp), sB = pulse(b.y*0.9+0.37, r2, sp*0.8);
  for (int i=0;i<2;i++){
    float d = (i==0)? a.x : b.x; float s = (i==0)? sA : sB;
    float core = smoothstep(w, w*0.35, d), halo = exp(-d*34.0)*0.6 * smoothstep(0.0,0.06,0.5-length(l)*0.5+0.0);
    col += tint*s*(core*2.2 + halo*1.2);
  }
  // hex outline (cell boundary at the apothem 0.866), dimmer, slower pulse around the ring
  float md = max(max(abs(l.x), abs(dot(l, vec2(0.5, 0.866)))), abs(dot(l, vec2(-0.5, 0.866))));
  float od = abs(md - 0.866);
  float ang = atan(l.y,l.x)/6.2831853 + 0.5;
  float so = 0.18 + 0.5*pulse(ang, r1, sp*0.5);
  col += tint*so*(smoothstep(w, w*0.35, od)*1.4 + exp(-od*30.0)*0.3);
  o = vec4(toSRGB(col*0.9), 1.0);
}
