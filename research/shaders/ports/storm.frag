// Port of storm-b50.blend (celestialmaze / cmzw_). 90-step march with a 3-fold inner loop (sum of |sin| layers), a colour ramp that
// cycles every 120 frames, a random hue per cycle (1D white noise), a radial hot centre, and tanh to tame the brightness.
uniform float U_SPEED;
uniform float U_ITER;
float ease(float a){ return a*a*(3.0-2.0*a); }
float rampStorm(float x){   // Color Ramp (Ease): grey values at stops
  const float p[9] = float[9](0.03182,0.03561,0.06667,0.16552,0.18806,0.23988,0.49242,0.95833,1.0);
  const float v[9] = float[9](0.0,0.5,1.0,0.0,0.0,0.5,0.0,0.0,0.25);
  if (x <= p[0]) return v[0];
  for (int i=0;i<8;i++) if (x < p[i+1]) return mix(v[i], v[i+1], ease((x-p[i])/(p[i+1]-p[i])));
  return v[8];
}
void main(){
  vec2 uv = (gl_FragCoord.xy - 0.5*R) / (0.5*R.y);
  float f = T*24.0*U_SPEED;
  vec3 O4 = vec3(uv, 0.0)*vec3(4.0,4.0,0.0);
  float ang = f*0.001, ca = cos(ang), sa = sin(ang);
  vec3 Rv = vec3(ca*O4.x - sa*O4.y, sa*O4.x + ca*O4.y, 0.0);
  vec3 D = normalize(Rv + vec3(0.0,0.0,1.2));
  vec3 OFS = vec3(0.0,0.0,0.02*f);
  float d = 0.0, dd = 0.0; vec3 Q = vec3(0.0);
  for (int i=0;i<180;i++){
    if (float(i) >= U_ITER) break;
    vec3 P = D*d + OFS;
    Q = cos(vec3(P.y,P.z,P.x)*0.56 + P.z)*0.6 + P;
    float s = 10.0 - length(Q.xy), n = 0.15;
    for (int j=0;j<3;j++){ s -= abs(dot(sin(Q*n*10.0 + vec3(0.0,0.0,f/32.0)), vec3(0.05)))/n; n *= 2.0; }
    dd = abs(s)*0.048 + 0.07; d += dd;
  }
  float rho = length(O4.xy);
  float cyc = f/120.0;
  float R0 = rampStorm(fract(cyc));
  vec3 wn = vec3(hash11(floor(cyc)+1.0), hash11(floor(cyc)+2.0), hash11(floor(cyc)+3.0));
  float nv = bnoise(wn*100.0 + Q, 0.1, 2.0, 0.5, 0.0, true);
  float rad = smoother(-0.56, 1.28, rho);
  float m18 = nv*rad;
  float m = R0 * smoother(0.2, 1.76, m18);
  float I = tanh(((2.0/dd)/d/2.7)/pow(max(rho,1e-4), 0.74));
  vec3 mixc = mix(I*vec3(0.08,0.05,0.05), I*vec3(3.76,3.62,12.6), m);
  vec3 col = pow(max(mixc/pow(max(rho,1e-4),0.74)*9.2, 0.0), vec3(0.9));
  o = vec4(toSRGB(col), 1.0);
}
