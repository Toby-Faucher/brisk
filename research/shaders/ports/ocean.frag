// Port of oceanwaves_b42.blend (celestialmaze / cmzw_). A flat toon ocean: UV-space noise drives a travelling sine
// (sin(x' * phase + frame*0.073)); its value blends deep -> teal -> foam colour bands; a phase-shifted sine darkens wave troughs.
// The original also displaces the mesh with a rotating warp; this 2D port skips the displacement.
uniform float U_WAVES;  // wave periods across the plane (the original reads an object property called "wave", set to 1)
uniform float U_SPEED;
void main(){
  vec2 f = gl_FragCoord.xy / R;
  vec2 uv = f;
  float frame = mod(T*24.0*U_SPEED, 85.0);             // the file loops every 85 frames (0.073 rad * 85 = 2 pi)
  vec3 UV = vec3(uv, 0.0), UVm = vec3(uv.x, 2.0*uv.y, 0.0);
  // x' = X(UV + 0.146*(2*noise-1)) - 0.59*noise1(uv.y)
  float nA = bnoise(UV, 5.9, 2.17, 0.7346, 0.0, true);
  float n1 = bnoise(vec3(uv.y), 1.91, 2.0, 0.5, 0.0, true);
  float xp = uv.x + 0.146*(2.0*nA - 1.0) - 0.59*n1;
  float ph = -6.283*U_WAVES;
  float S1 = sin(xp*ph + 0.073*frame);            // Math / Math.001
  float S2 = sin(xp*ph - 2.8 + 0.073*frame*0.0);  // Math.006 (trough shading phase)
  float N4 = bnoise(UVm, 8.82, 1.8, 0.7593, 0.78, false);
  float N2 = bnoise(UVm, 27.55, 1.8, 0.7593, 1.64, false);
  float N3 = bnoise(UVm, 10.64, 1.8, 0.3519, 1.41, true);
  vec3 c9 = mix(vec3(0.09809,0.1917,0.214), vec3(0.3457,0.7017,0.7874), mapRange(N4,-0.09,0.6,0.,1.));
  vec3 c3 = mix(vec3(0.002835,0.1606,0.1706), c9, clamp(N2,0.,1.));
  vec3 c1 = mix(vec3(0.002852,0.006552,0.07324), c3, mapRange(S1,-1.18,-0.1,0.,1.));
  vec3 c2 = mix(c1, vec3(0.01056,0.3185,0.2114), smoothstep(-0.25,0.72,S1));
  vec3 c6 = mix(vec3(0.01056,0.3185,0.2114), vec3(0.3355,0.7874,0.745), N3);
  vec3 c5 = mix(c2, c6, smoother(-0.41,0.8,S1));
  vec3 c10 = mix(vec3(0.1349,0.6835,0.6921), vec3(1.0), stepped(N3,-0.12,1.0,0.11,1.0,3.0)); // foam, 3 steps
  vec3 c7 = mix(c5, c10, smoother(0.28,1.31,S1));
  vec3 c8 = mix(c7, vec3(0.0), smoother(-0.23,0.96,S2)*0.4);
  o = vec4(toAgX(c8, 1.1, 1.25), 1.0);   // the file uses AgX High Contrast, exposure 1.1
}
