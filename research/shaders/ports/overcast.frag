// Port of overcast-b50.blend (celestialmaze / cmzw_). One Repeat zone: 70 samples of a 4-octave noise along a ray; each sample adds
// smoothstep(|noise - 0.56|) so thin bands of density pile up into cloud sheets; a B-spline colour ramp (night blue -> orange -> pale violet).
uniform float U_SPEED;
uniform float U_ITER;
vec3 ramp(float x){
  const vec3 c[5] = vec3[5](vec3(0.00266,0.02049,0.05088), vec3(0.00311,0.05199,0.17057), vec3(0.12662,0.15371,0.38116), vec3(0.74881,0.33297,0.06903), vec3(0.70614,0.65577,1.0));
  const float p[5] = float[5](0.0, 0.21148, 0.41314, 0.5997, 1.0);
  x = clamp(x, 0.0, 1.0);
  int i = 0; for (int k=0;k<4;k++) if (x >= p[k+1]) i = k+1; i = min(i, 3);
  float u = clamp((x - p[i])/(p[i+1]-p[i]), 0.0, 1.0);
  vec3 a=c[max(i-1,0)], b=c[i], cc=c[i+1], d=c[min(i+2,4)];                      // cubic B-spline through the neighbouring stops
  return (pow(1.0-u,3.0)*a + (3.0*u*u*u - 6.0*u*u + 4.0)*b + (-3.0*u*u*u + 3.0*u*u + 3.0*u + 1.0)*cc + u*u*u*d)/6.0;
}
void main(){
  vec2 uv = (gl_FragCoord.xy - 0.5*R) / (0.5*R.y) * 0.376;   // the plane's local extent is only +-0.376 (object scale 2.66)
  float frame = T*24.0*U_SPEED;
  vec3 V2 = vec3(uv, 0.0)*vec3(3.0,3.0,0.0) + vec3(0.0,-0.12,0.0);
  vec3 A = V2 - vec3(0.0,0.8,0.1);
  vec3 B = vec3(0.0, -0.05*frame - 32.0, 0.0);
  float v = 0.0;
  for (int i=0;i<140;i++){
    if (float(i) >= U_ITER) break;
    float s = (float(i)*0.1429)/(V2.y + 2.0);
    vec3 Q = A*s + B;
    float n = bnoise(Q, 0.2, 4.0, 0.5497, 0.0, true);
    float w = mapRange(abs(n - 0.56), 0.04, 0.2, 0.0, 1.18);
    float sm = clamp((abs(n-0.56)-0.04)/0.16, 0.0, 1.0); sm = sm*sm*(3.0-2.0*sm) * 1.18;   // Map Range (Smoothstep)
    v += sm;
  }
  vec3 col = ramp(v*0.015625);
  o = vec4(toAgX(col, 1.75, 1.05), 1.0);   // the file uses AgX, look: High Contrast
}
