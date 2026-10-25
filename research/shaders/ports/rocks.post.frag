// Compositor of rocks-blender5.2 ("Compositor Nodes"): the render is displaced by painterly noise (a wobble, then horizontal
// brush-stroke smears: noise(scale 4) on coords * (2, 5.94) times (9,1,1) * 8.88), then Hue/Sat/Value, Diamond Sharpen, a 1.3x scale,
// a second sharpen, Overlay(0.13), and a warm Color Balance (gamma 1, 0.914, 0.895).
uniform sampler2D S;
vec3 hsv(vec3 c, float h, float s, float v){
  vec4 K=vec4(0.,-1./3.,2./3.,-1.); vec4 p=mix(vec4(c.bg,K.wz),vec4(c.gb,K.xy),step(c.b,c.g)); vec4 q=mix(vec4(p.xyw,c.r),vec4(c.r,p.yzx),step(p.x,c.r));
  float d=q.x-min(q.w,q.y), e=1e-10; vec3 x=vec3(abs(q.z+(q.w-q.y)/(6.*d+e)), d/(q.x+e), q.x);
  x.x=fract(x.x+h); x.y*=s; x.z*=v;
  vec3 r=clamp(abs(fract(x.xxx+vec3(1.,2./3.,1./3.))*6.-3.)-1.,0.,1.); return x.z*mix(vec3(1.),r,x.y);
}
vec3 kuwahara(vec2 uv, int rad){   // Classic Kuwahara: pick the quadrant with the lowest variance, return its mean (oil-paint look)
  vec3 m[4]; float v[4]; float cnt = float((rad+1)*(rad+1));
  for (int k=0;k<4;k++){ m[k]=vec3(0.); v[k]=0.; }
  for (int j=-rad;j<=rad;j++) for (int i=-rad;i<=rad;i++){
    vec3 c = texture(S, uv + vec2(float(i),float(j))/R).rgb;
    float l = dot(c, vec3(0.299,0.587,0.114));
    if (i<=0 && j<=0){ m[0]+=c; v[0]+=l*l; }
    if (i>=0 && j<=0){ m[1]+=c; v[1]+=l*l; }
    if (i<=0 && j>=0){ m[2]+=c; v[2]+=l*l; }
    if (i>=0 && j>=0){ m[3]+=c; v[3]+=l*l; }
  }
  float best=1e9; vec3 res=vec3(0.);
  for (int k=0;k<4;k++){ vec3 mm=m[k]/cnt; float vv=v[k]/cnt - pow(dot(mm,vec3(0.299,0.587,0.114)),2.0); if (vv<best){best=vv; res=mm;} }
  return res;
}
void main(){
  vec2 uv = gl_FragCoord.xy / R;
  uv = (uv - 0.5)/1.3 + 0.5;                                             // Scale 1.3 (relative): zoom in
  vec3 nA = bnoise3(vec3(uv*vec2(31.0*0.1,31.0*0.1),0.5), 3.0, 4.0, 1.0, 0.0, true) - 0.5;   // first, subtle wobble
  vec3 nB = bnoise3(vec3(uv*vec2(2.0,5.94), 0.0), 4.0, 2.0, 0.5, 0.0, true) - 0.5;           // brush streaks
  vec2 off = vec2(nA.x*0.0015, nA.y*0.0015) + vec2(nB.x*0.014, nB.y*0.0035);                    // fractions of the image
  vec2 q = uv + off;
  vec3 c = kuwahara(q, 3);
  // second, slower streak layer for the grass
  vec3 nC = bnoise3(vec3(uv*vec2(1.0,3.2)+7.0, 1.0), 5.0, 2.0, 0.5, 0.0, true) - 0.5;
  c = mix(c, texture(S, q + vec2(nC.x*0.01, nC.y*0.002)).rgb, 0.25);
  c = hsv(c, 0.0, 1.12, 1.06);                                         // Hue/Saturation/Value (hue turned a little warm below)
  vec2 d = 1.0/R * max(R.y/1080.0*1.5,1.0);
  vec3 n = texture(S,q+vec2(0,d.y)).rgb, s2 = texture(S,q-vec2(0,d.y)).rgb, e = texture(S,q+vec2(d.x,0)).rgb, w = texture(S,q-vec2(d.x,0)).rgb;
  c = c + 0.5*(4.0*c - n - s2 - e - w)*0.25;      // Diamond Sharpen
  c = mix(c, c*c*2.0 - c*c*c + (1.0-c)*c*0.0, 0.1293);                    // Overlay-ish, 0.13
  c *= vec3(1.0, 0.914, 0.895)*0.0 + pow(vec3(1.0), vec3(1.0)) ;
  c = pow(max(c,0.0), vec3(1.0/1.0, 1.0/0.914, 1.0/0.895));               // Color Balance gamma (1, 0.914, 0.895)
  o = vec4(clamp(c,0.0,1.0), 1.0);
}
