<script module lang="ts">
  // The page swap remounts the Layer, so the preset counter lives here.
  let mounts = -1;
</script>

<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  const NAMES = ['Lava plasma', 'Metaball pond', 'Warp tunnel', 'Moire grid'];
  // Cosine palettes: a + b * cos(6.28 * (c * t + d)), one vec3 each for a, b, c, d per preset.
  const PAL = [
    [0.5, 0.35, 0.45, 0.5, 0.5, 0.5, 1, 1, 1, 0.0, 0.1, 0.2],
    [0.4, 0.5, 0.6, 0.5, 0.4, 0.4, 1, 1, 0.5, 0.8, 0.9, 0.3],
    [0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 1, 0.7, 0.4, 0.0, 0.15, 0.2],
    [0.4, 0.5, 0.45, 0.5, 0.45, 0.5, 2, 1, 1, 0.5, 0.2, 0.25],
  ];
  mounts++;
  let idx = $state(mounts % NAMES.length);
  let seen = 0;
  let kick = mounts > 0 ? 1 : 0; // brightness flash after a page change, decays in the loop
  let redraw: (() => void) | null = null;
  $effect(() => {
    void path;
    if (seen++ === 0) return;
    idx = (idx + 1) % NAMES.length;
    kick = 1;
    redraw?.();
  });

  const VS = 'attribute vec2 p;void main(){gl_Position=vec4(p,0.,1.);}';
  const FS = `precision mediump float;
uniform vec2 R;uniform float T,K,M;uniform vec3 P,Rp[3];uniform vec3 A,B,C,D;
float h(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5);}
float n(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.-2.*f);
return mix(mix(h(i),h(i+vec2(1,0)),f.x),mix(h(i+vec2(0,1)),h(i+1.),f.x),f.y);}
float fbm(vec2 p){float s=0.,a=.5;for(int i=0;i<4;i++){s+=a*n(p);p=p*2.03+7.1;a*=.5;}return s;}
vec3 pal(float t){return A+B*cos(6.2832*(C*t+D));}
void main(){
  vec2 uv=(gl_FragCoord.xy-.5*R)/R.y;
  vec2 m=(P.xy-.5*R)/R.y;
  float v=0.;
  if(M<.5){
    vec2 q=uv*2.;
    vec2 w=vec2(fbm(q+T*.15),fbm(q+5.2-T*.12));
    vec2 w2=vec2(fbm(q+3.*w+T*.1),fbm(q+3.*w+8.3));
    v=fbm(q+3.5*w2)+.6*w2.x+.4/(1.+8.*length(uv-m));
  }else if(M<1.5){
    float s=0.;
    for(int i=0;i<6;i++){float f=float(i);
      vec2 c=.55*vec2(sin(T*.31+f*1.7),cos(T*.27+f*2.3))*vec2(1.5,.9);
      s+=.035/(.002+dot(uv-c,uv-c));}
    s+=.05/(.002+dot(uv-m,uv-m));
    v=.25*log(1.+s)+.1*sin(s*.6-T);
  }else if(M<2.5){
    vec2 d=uv-.4*m;float r=length(d),a=atan(d.y,d.x);
    v=.5*sin(8./(r+.15)-T*2.+3.*sin(a*3.+T*.3))+.5*sin(a*5.+T)+.8/(1.+3.*r);
    v=v*.35+.4;
  }else{
    vec2 q=uv*6.;
    float a=sin(q.x+T)+sin(q.y*1.3-T*.8)+sin((q.x+q.y)*.7+T*.5)+sin(length(q-6.*m)*1.5-T*1.5);
    v=a*.12+.45;
  }
  for(int i=0;i<3;i++){
    float age=T-Rp[i].z;
    if(age>0.&&age<3.){vec2 c=(Rp[i].xy-.5*R)/R.y;float d=abs(length(uv-c)-age*.45);
      v+=(1.-age/3.)*.5*smoothstep(.06,0.,d);}
  }
  vec3 col=pal(v);
  col=mix(vec3(dot(col,vec3(.33))),col,1.1);
  col*=.2+.2*smoothstep(1.3,0.,length(uv))+K*.25;
  gl_FragColor=vec4(col,1.);
}`;

  let cv: HTMLCanvasElement | undefined = $state();
  onMount(() => {
    const gl = cv?.getContext('webgl', { antialias: false, powerPreference: 'low-power' });
    if (!cv || !gl) return; // the CSS gradient stays as the background
    const mk = (type: number, src: string) => {
      const s = gl.createShader(type) as WebGLShader;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      return s;
    };
    const prog = gl.createProgram() as WebGLProgram;
    gl.attachShader(prog, mk(gl.VERTEX_SHADER, VS));
    gl.attachShader(prog, mk(gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return;
    gl.useProgram(prog);
    gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(prog, 'p');
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    const U = (n: string) => gl.getUniformLocation(prog, n);
    const [uR, uT, uK, uM, uP, uRp] = [U('R'), U('T'), U('K'), U('M'), U('P'), U('Rp')];
    const [uA, uB, uC, uD] = [U('A'), U('B'), U('C'), U('D')];

    const reduced = matchMedia('(prefers-reduced-motion: reduce)');
    const SCALE = 0.5;
    let w = 1, hgt = 1;
    let px = 0.5, py = 0.5, tx = 0.5, ty = 0.5; // pointer, 0..1 from the left/bottom
    const rip = new Float32Array(9).fill(-99); // three ripples: x, y, start time
    let ri = 0;
    let raf = 0, last = 0, t0 = performance.now();
    const size = () => {
      w = Math.max(2, Math.round(innerWidth * SCALE));
      hgt = Math.max(2, Math.round(innerHeight * SCALE));
      cv.width = w;
      cv.height = hgt;
      gl.viewport(0, 0, w, hgt);
    };
    const draw = () => {
      const t = (performance.now() - t0) / 1000;
      const p = PAL[idx];
      gl.uniform2f(uR, w, hgt);
      gl.uniform1f(uT, t + idx * 40);
      gl.uniform1f(uK, kick);
      gl.uniform1f(uM, idx);
      gl.uniform3f(uP, px * w, py * hgt, 0);
      gl.uniform3fv(uRp, rip);
      gl.uniform3f(uA, p[0], p[1], p[2]);
      gl.uniform3f(uB, p[3], p[4], p[5]);
      gl.uniform3f(uC, p[6], p[7], p[8]);
      gl.uniform3f(uD, p[9], p[10], p[11]);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    };
    const loop = (now: number) => {
      raf = requestAnimationFrame(loop);
      if (now - last < 33) return; // 30 fps cap
      last = now;
      px += (tx - px) * 0.12;
      py += (ty - py) * 0.12;
      kick *= 0.93;
      draw();
    };
    const start = () => {
      if (!raf && !reduced.matches && !document.hidden) raf = requestAnimationFrame(loop);
    };
    const stop = () => {
      cancelAnimationFrame(raf);
      raf = 0;
    };
    const frame = () => {
      if (reduced.matches) kick = 0;
      if (reduced.matches || !raf) draw(); // one still frame when not animating
    };
    redraw = frame;
    const move = (e: PointerEvent) => {
      tx = e.clientX / innerWidth;
      ty = 1 - e.clientY / innerHeight;
    };
    const down = (e: PointerEvent) => {
      const t = (performance.now() - t0) / 1000 + idx * 40;
      rip.set([(e.clientX / innerWidth) * w, (1 - e.clientY / innerHeight) * hgt, t], ri * 3);
      ri = (ri + 1) % 3;
    };
    const vis = () => (document.hidden ? stop() : start());
    const resize = () => {
      size();
      frame();
    };
    const motion = () => (reduced.matches ? (stop(), draw()) : start());
    size();
    draw();
    start();
    addEventListener('pointermove', move, { passive: true });
    addEventListener('pointerdown', down, { passive: true });
    addEventListener('resize', resize);
    document.addEventListener('visibilitychange', vis);
    reduced.addEventListener('change', motion);
    return () => {
      stop();
      redraw = null;
      removeEventListener('pointermove', move);
      removeEventListener('pointerdown', down);
      removeEventListener('resize', resize);
      document.removeEventListener('visibilitychange', vis);
      reduced.removeEventListener('change', motion);
    };
  });
</script>

{#if !stream}
  <div class="sh-bg" aria-hidden="true">
    <canvas bind:this={cv}></canvas>
  </div>
  <div class="sh-tag" aria-hidden="true">shader {idx + 1}/{NAMES.length}: {NAMES[idx]}</div>
{/if}

<style>
  /* The gradient shows when WebGL is missing; the canvas covers it otherwise. */
  .sh-bg { position: fixed; inset: 0; z-index: -1; pointer-events: none; background: radial-gradient(90% 80% at 20% 10%, #5b21b6, transparent 60%), radial-gradient(80% 70% at 85% 90%, #be185d, transparent 60%), #120a26; }
  canvas { display: block; width: 100%; height: 100%; }
  .sh-tag { position: fixed; z-index: 60; right: 10px; bottom: 8px; pointer-events: none; font: 500 11px/1 'Geist Mono Variable', ui-monospace, monospace; letter-spacing: .06em; text-transform: lowercase; color: #fff; background: rgb(8 6 18 / .8); padding: 4px 8px; border-radius: 6px; }
</style>
