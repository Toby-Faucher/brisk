// Compositor of oceanwaves_b42: Diamond Sharpen(1.0) -> Soften(0.42) -> Sobel(0.087), plus 22% of a 13 px Gaussian blur added back, then a tiny Kuwahara (size 1, skipped).
uniform sampler2D S;
vec3 tx(vec2 px){ return texture(S, px / R).rgb; }
void main(){
  vec2 p = gl_FragCoord.xy; float k = max(R.y/1080.0, 0.4);   // the original is 1080 px; scale the kernels
  vec2 d = vec2(k);
  vec3 c = tx(p), n = tx(p+vec2(0,d.y)), s = tx(p-vec2(0,d.y)), e = tx(p+vec2(d.x,0)), w = tx(p-vec2(d.x,0));
  vec3 sharp = max(5.0*c - n - s - e - w, 0.0);                           // Filter: Diamond Sharpen, fac 1
  vec3 nw = tx(p+vec2(-d.x,d.y)), ne = tx(p+d), sw = tx(p-d), se = tx(p+vec2(d.x,-d.y));
  vec3 soft = (4.0*c + 2.0*(n+s+e+w) + nw+ne+sw+se)/16.0;                 // Filter: Soften
  vec3 base = mix(sharp, soft, 0.4198);
  vec3 gx = (ne + 2.0*e + se) - (nw + 2.0*w + sw), gy = (nw + 2.0*n + ne) - (sw + 2.0*s + se);
  vec3 sobel = sqrt(gx*gx + gy*gy);                                      // Filter: Sobel, fac 0.087
  base = mix(base, sobel, 0.087);
  vec3 blur = vec3(0.0); float ws = 0.0;                                  // Blur: Fast Gaussian 13 px
  for (int j=-3;j<=3;j++) for (int i=-3;i<=3;i++){ float g=exp(-float(i*i+j*j)/6.0); blur += g*tx(p+vec2(float(i),float(j))*2.1*k); ws+=g; }
  blur /= ws;
  vec3 col = base + 0.2235*blur;                                          // Mix: Add, 0.2235
  o = vec4(clamp(col,0.0,1.0), 1.0);
}
