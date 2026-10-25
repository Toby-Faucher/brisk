// Port of nebula-b50.blend (celestialmaze / cmzw_). Shader nodes with two Repeat zones: a 70-step ray march whose every step
// folds the sample point 5 times (a Kali-set style fractal) and sums 1/step-size; the result is shaded by distance and tinted
// (1.04, 0.86, 1.64). Object coordinates of a 2x2 plane are the ray directions.
uniform float U_SPEED;
uniform float U_ITER;     // outer march steps (the original uses 70)
void main(){
  vec2 uv = (gl_FragCoord.xy - 0.5*R) / (0.5*R.y) * 1.316;   // the plane's local extent is +-1.316
  float t = T*24.0*U_SPEED*0.001562;               // frame_current * 0.001562
  vec3 u = vec3(uv, 0.0);
  float d = 0.0, acc = 0.0;
  for (int i=0;i<200;i++){
    if (float(i) >= U_ITER) break;
    vec3 P = u*d + vec3(0.0, 0.0, t*4.0 + d);
    vec3 Q0 = cos(vec3(P.y,P.z,P.x)*0.5 + (5.0*t + d))*0.5 + P;
    float s = 5.0 - length(Q0.xy);
    vec3 p = Q0; float n = 0.1;
    float ca = cos(t*0.125), sa = sin(t*0.125);
    for (int j=0;j<5;j++){
      p = vec3(ca*p.x - sa*p.y, sa*p.x + ca*p.y, p.z);
      vec3 sv = sin(p*n*20.0 + (p.z + t));
      s -= abs(dot(sv, vec3(0.05)))/n;
      n *= 2.0;
    }
    float dd = abs(s)*0.1 + 0.02;
    acc += 1.0/dd; d += dd;
  }
  float L = length(pow(abs(u), vec3(0.19)));       // the colour uses the screen coordinate u (not the ray point)
  float rr = max(length(u), 1e-5);
  float a = (acc/d/485.0) / sqrt(rr);
  float tmax = pow(0.64/L, 5.0);
  float tt = clamp(1.0 - rr, 0.0, 1.0); float sm = tt*tt*(3.0-2.0*tt);
  float v = a + tmax*sm;
  vec3 col = v * vec3(1.04, 0.86, 1.64);
  o = vec4(clamp(col,0.0,1.0), 1.0);   // the file's view transform is Raw: no colour curve
}
