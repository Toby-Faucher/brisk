import FRAG from './smoke.frag.glsl?raw';

type GLish = { getExtension(name: string): unknown };

/** The canvas's WebGL2 context if it can run the smoke (float render targets included), else null = CSS fallback. */
export function smokeGL<T extends GLish>(canvas: { getContext(id: string, opts?: object): unknown }): T | null {
  const gl = canvas.getContext('webgl2', { antialias: false, alpha: false, powerPreference: 'low-power' }) as T | null;
  return gl && gl.getExtension('EXT_color_buffer_float') ? gl : null;
}

/** Shaders are off on the Stream route (no WebGL context next to video decode) or when the setting says so. */
// `idle`: the Stream page's "Nothing is streaming" state; the caller unmounts the smoke before a session starts.
export const smokeAllowed = (path: string, shaders: boolean, idle = false) => shaders && (idle || !/^\/stream(\/|$)/.test(path));

/** '#5eead4' -> linear RGB for the shader. */
export function accentLinear(hex: string): [number, number, number] {
  const v = parseInt(hex.slice(1), 16);
  return [16, 8, 0].map((s) => (((v >> s) & 255) / 255) ** 2.2) as [number, number, number];
}

const SCALE = 0.5; // half resolution: the smoke is soft anyway
const VS = '#version 300 es\nin vec2 p;void main(){gl_Position=vec4(p,0.,1.);}';

export type SmokeRun = { setActive(on: boolean): void; destroy(): void };

/** Draw the smoke into `cv` at 30 fps while active and the tab is visible. `accent()` is read every frame. */
export function runSmoke(cv: HTMLCanvasElement, gl: WebGL2RenderingContext, accent: () => string): SmokeRun | null {
  const sh = (t: number, s: string) => {
    const x = gl.createShader(t)!;
    gl.shaderSource(x, s);
    gl.compileShader(x);
    return x;
  };
  const pr = gl.createProgram()!;
  gl.attachShader(pr, sh(gl.VERTEX_SHADER, VS));
  gl.attachShader(pr, sh(gl.FRAGMENT_SHADER, FRAG));
  gl.linkProgram(pr);
  if (!gl.getProgramParameter(pr, gl.LINK_STATUS)) return null;
  gl.useProgram(pr);
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(pr, 'p');
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
  const uR = gl.getUniformLocation(pr, 'R'), uT = gl.getUniformLocation(pr, 'T'), uA = gl.getUniformLocation(pr, 'A');

  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const seed = Math.random() * 50;
  let active = false, raf = 0, last = 0, drawn = false;

  function draw(ts: number) {
    const w = Math.max(2, Math.round(cv.clientWidth * SCALE)), h = Math.max(2, Math.round(cv.clientHeight * SCALE));
    if (cv.width !== w || cv.height !== h) {
      cv.width = w;
      cv.height = h;
    }
    gl.viewport(0, 0, w, h);
    gl.uniform2f(uR, w, h);
    gl.uniform1f(uT, ts / 1000 + seed);
    gl.uniform3f(uA, ...accentLinear(accent()));
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    drawn = true;
  }
  function loop(ts: number) {
    raf = 0;
    if (!active || document.hidden || (still && drawn)) return; // stopped; visibilitychange restarts it
    raf = requestAnimationFrame(loop);
    if (ts - last < 33) return; // 30 fps cap
    last = ts;
    draw(ts);
  }
  const kick = () => {
    if (active && !raf && !document.hidden) raf = requestAnimationFrame(loop);
  };
  document.addEventListener('visibilitychange', kick);

  return {
    setActive(on) {
      active = on;
      kick();
    },
    destroy() {
      active = false;
      cancelAnimationFrame(raf);
      document.removeEventListener('visibilitychange', kick);
      (gl.getExtension('WEBGL_lose_context') as WEBGL_lose_context | null)?.loseContext();
    },
  };
}
