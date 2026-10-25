// Port of tunnel0-b50.blend (celestialmaze / cmzw_). Sphere tracing in 100 steps: the distance is a smooth blend of the tube
// (0.5*|xy|), a Voronoi cell wall and the smooth-max of 5 stacked Voronoi layers; the colour is 3.74 * normalize(noise^1.37) / t.
// This port stops stepping once the ray has settled (the original keeps going for all 100).
uniform float U_SPEED;
uniform float U_ITER;
void main(){
  vec2 uv = (gl_FragCoord.xy - 0.5*R) / (0.5*R.y);
  float f = T*24.0*U_SPEED;
  vec3 D = normalize(vec3(uv, 0.0) + vec3(0.0,0.0,1.5));
  vec3 OFS = vec3(0.0, 0.0, 0.03*f);
  float t = 0.0; vec3 P = OFS;
  for (int k=0;k<100;k++){
    if (float(k) >= U_ITER) break;
    P = D*t + OFS;
    float r = length(vec2(P.xy)*0.5);
    vec3 W = 2.0*bnoise3(P, 0.2, 1.0, 0.5, 0.0, true);
    float d1 = voro3(P + W);
    float v = 0.0;
    for (int i=0;i<5;i++){ float fi = float(i)/5.0; v = smax(v, 0.98 - voro3(P - vec3(fi, fi, 3.16)), 1.0); }
    float b = max(v - 0.94, 0.15 - d1);
    float sd = smin(r, b, 0.83);
    float tn = t - (sd - 0.11);
    bool done = abs(tn - t) < 1e-9;   // effectively no early exit: the thin lines come from the late steps
    t = tn; if (done) break;
  }
  vec3 n = pow(max(bnoise3(P, 0.35, 2.0, 0.5, 0.0, true), 1e-4), vec3(1.37));
  vec3 col = 3.74 * normalize(n) / t;
  o = vec4(toSRGB(max(col, 0.0)), 1.0);
}
