// The painter of one layer of the stage's light (light.ts): a canvas in the card layer, as
// large as it, under the cards or over them, which WebGL draws a scene on each time the stage
// writes one. A canvas lies at the one depth its style gives it, as any element does, so the
// light under the cards and the light over them are two. It only paints: what is lit, where
// and how brightly is the scene's, worked out from stage time.
//
// A ribbon is a strip of triangles along its curve, its look a fragment shader's: across it a
// hot core a little off its middle, as a ribbon lit from one side, a band of its color with
// soft edges and a halo; along it streaks that run toward its head. The other shapes are
// distance fields on a quad, and sparks are points.
//
// Every color written is premultiplied and no channel of it passes its alpha. A page's
// compositor may do anything with one that does, and WebKit's snapshots, which the stage's
// checks are taken from, wrap it into black and green. So light is not added to the page: it
// is soft-clipped, which burns what is past full toward white as a bloom does, and laid over
// the table as paint is, with a little shade round a ribbon that gives its color room on a
// mid-toned table.
import {
  along,
  TONES,
  widthOf,
  type Glint,
  type Ribbon,
  type Scene,
} from "./light.ts";

const NOISE = `
float hash(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float noise(vec2 p){ vec2 i = floor(p), f = fract(p); f = f*f*(3.-2.*f);
  return mix(mix(hash(i), hash(i+vec2(1.,0.)), f.x),
             mix(hash(i+vec2(0.,1.)), hash(i+vec2(1.,1.)), f.x), f.y); }`;

/** Light as the page takes it: soft-clipped, with an alpha no less than its brightest
 * channel, or than `shade` where it only darkens. */
const LIT = `
vec4 lit(vec3 rgb, float shade){ vec3 c = 1. - exp(-rgb*1.5);
  return vec4(c, max(max(c.r, max(c.g, c.b)), shade)); }`;

const PLACED = `uniform vec2 u_res;
vec4 placed(vec2 p){ return vec4(p.x/u_res.x*2.-1., 1.-p.y/u_res.y*2., 0., 1.); }`;

const RIBBON_VERTEX = `attribute vec2 a_pos; attribute vec4 a_dat;
varying float v_side, v_body, v_arc;${PLACED}
void main(){ v_side = a_dat.x; v_body = a_dat.y; v_arc = a_dat.z; gl_Position = placed(a_pos); }`;

const RIBBON_FRAGMENT = `precision highp float; varying float v_side, v_body, v_arc;
uniform vec3 u_col, u_hot; uniform float u_time, u_gain, u_len, u_px, u_lean, u_seed;${NOISE}${LIT}
void main(){
  float b = max(v_body, 0.7*u_px), d = abs(v_side), x = v_side/b;
  float run = v_arc/64. - u_time*2.6 + u_seed*40.;
  float n = noise(vec2(run, x*1.4 + u_seed*3.)), n2 = noise(vec2(run*2.9, x*2.6 + 7.));
  float body = smoothstep(1.08, 0.4, abs(x)) * (0.7 + 0.42*n + 0.2*n2);
  float core = exp(-pow((x - u_lean)*2.5, 2.)) * (0.72 + 0.4*n);
  float pulse = pow(0.5 + 0.5*sin(6.2832*(v_arc/280. - u_time*1.5)), 10.);
  float ends = smoothstep(0., 12., v_arc) * smoothstep(0., 3., u_len - v_arc);
  float glow = exp(-d/(b*0.9 + 2.5*u_px + 2.)) * ends;
  vec3 rgb = u_col*(body*0.95 + glow*0.4) + u_hot*core*(0.8 + 0.7*pulse);
  float shade = body*0.5 + exp(-d/(b*2.2 + 9.))*0.07*ends;
  float fade = smoothstep(0., 8., v_arc);
  gl_FragColor = lit(rgb*u_gain*fade, shade*min(u_gain, 1.)*fade);
}`;

const GLINT_VERTEX = `attribute vec2 a_c; uniform vec2 u_at, u_ax, u_ay, u_half;
varying vec2 v_p;${PLACED}
void main(){ v_p = a_c*u_half; gl_Position = placed(u_at + u_ax*v_p.x + u_ay*v_p.y); }`;

/** Each shape is drawn about its own middle, `+y` its upward way: 1 a wedge, its base at
 * the foot of its box; 2 a crescent, open toward `+y`; 3 a flash, with a ring that spreads;
 * 4 a burst; 5 a beam, a cone of light from the foot of its box, hot along its middle and at
 * its base, its edges fringed a little warmer (the bloom where it leaves is a flash). */
const GLINT_FRAGMENT = `precision highp float; varying vec2 v_p; uniform int u_kind;
uniform vec3 u_col, u_hot; uniform vec2 u_size; uniform float u_gain, u_px, u_ring, u_seed;${NOISE}${LIT}
float seg(vec2 p, vec2 a, vec2 b){ vec2 pa = p-a, ba = b-a;
  return length(pa - ba*clamp(dot(pa, ba)/dot(ba, ba), 0., 1.)); }
void main(){
  vec3 rgb = vec3(0.); float shade = 0.;
  if (u_kind == 1) {
    vec2 p = v_p + vec2(0., u_size.y*0.5); float hw = u_size.x, h = u_size.y;
    float yv = h*(1. - abs(p.x)/hw);
    float d = min(seg(p, vec2(-hw, 0.), vec2(0., h)), seg(p, vec2(hw, 0.), vec2(0., h)));
    float inside = step(0., p.y)*step(p.y, yv)*step(abs(p.x), hw);
    float fill = inside * pow(clamp(p.y/max(yv, 1.), 0., 1.), 1.25) * 0.95;
    // Arena's: a thick white-hot rim along the two lower edges, hottest at the point, and a
    // translucent fill that is brightest along those edges and fades toward the card.
    float w = max(2.2, 0.03*hw + 0.8*u_px), line = exp(-d*d/(2.*w*w));
    float glow = exp(-d/(0.09*hw + 4.))*0.55;
    float apex = exp(-length(p - vec2(0., h))/(h*0.85));
    float arm = mix(0.6, 1., smoothstep(hw, hw*0.25, abs(p.x)));
    rgb = (u_col*(fill*1.7 + glow + line) + u_hot*(line*(0.65 + 0.9*apex) + fill*apex*0.55))*arm;
    shade = (fill*0.4 + exp(-d/11.)*0.2)*arm;
  } else if (u_kind == 2) {
    // A dome, open toward the blocker: a bright rim along its arc, thickest at its crown,
    // and inside it light that is brightest by the rim, fades toward its chord and is a
    // little uneven, as Arena's is (IEXcjBn3BXk 22:23.0).
    vec2 q = v_p/u_size; float r = max(length(q), 1e-3), d = length(v_p)*(1. - 1./r);
    float m = smoothstep(-0.15, 0.95, q.y/r), w = max(1.5 + 3.5*m, 0.7*u_px);
    float line = exp(-d*d/(2.*w*w)), glow = exp(-abs(d)/7.)*0.4;
    float n = noise(vec2(q.x*5. + u_seed*3., r*3.));
    float fill = step(r, 1.)*smoothstep(0., 0.35, q.y)*pow(r, 1.6)*(0.6 + 0.6*n);
    rgb = u_col*(glow + line*0.9 + fill*2.6)*m + u_hot*(line*m*m*0.85 + fill*m*0.6);
    shade = (glow + fill*0.5)*m*0.25;
  } else if (u_kind == 5) {
    vec2 p = v_p + vec2(0., u_size.y*0.5); float t = clamp(p.y/u_size.y, 0., 1.);
    float spread = u_size.x*(1. - 0.85*t) + 1.;
    float x = p.x/spread, along = step(0., p.y)*smoothstep(1., 0.7, t);
    float n = noise(vec2(p.y/40. + u_seed*7., x*2.));
    float cone = exp(-x*x*2.2)*along*pow(1. - t, 1.4)*(0.8 + 0.4*n);
    float core = exp(-x*x*16.)*along*pow(1. - t, 0.8);
    rgb = u_col*cone*0.9 + u_hot*core*1.3
      + vec3(0.25, -0.05, 0.15)*cone*smoothstep(0.3, 1., abs(x));
    shade = 0.;
  } else if (u_kind == 4) {
    float r = length(v_p)/u_size.x; vec2 way = v_p/max(length(v_p), 1e-3);
    float fine = noise(way*14. + u_seed*31.), broad = noise(way*4. + u_seed*17. + 5.);
    float spikes = pow(fine, 6.)*4.2 + pow(broad, 3.)*0.4;
    float edge = smoothstep(1., 0.6, r);
    float core = exp(-r*r*13.), bloom = exp(-r*r*4.5)*0.55;
    float rays = spikes*exp(-r*3.4)*smoothstep(0.04, 0.2, r);
    float ring = exp(-pow((r - 0.25 - u_ring*0.6)*11., 2.))*(1. - u_ring)*step(0.001, u_ring);
    rgb = (u_hot*(core*1.5 + bloom + rays) + u_col*(bloom*0.6 + rays*0.35 + ring*0.6))*edge;
  } else {
    float r = length(v_p)/u_size.x, core = exp(-r*r*5.5);
    float ring = exp(-pow((r - u_ring)*8., 2.))*(1. - u_ring);
    rgb = u_hot*core + u_col*(core*0.7 + ring*0.75);
  }
  gl_FragColor = lit(rgb*u_gain, shade*min(u_gain, 1.));
}`;

const DOT_VERTEX = `attribute vec2 a_pos; attribute vec4 a_dat; uniform float u_scale;
varying vec3 v_col;${PLACED}
void main(){ v_col = a_dat.yzw; gl_PointSize = a_dat.x*u_scale; gl_Position = placed(a_pos); }`;

const DOT_FRAGMENT = `precision highp float; varying vec3 v_col;${LIT}
void main(){ float r = length(gl_PointCoord - 0.5)*2.;
  gl_FragColor = lit(v_col*max(exp(-r*r*3.2) - 0.04, 0.), 0.); }`;

interface Program {
  program: WebGLProgram;
  uniform(name: string): WebGLUniformLocation | null;
  attribute(name: string): number;
}

function compile(gl: WebGLRenderingContext, vertex: string, fragment: string) {
  const program = gl.createProgram();
  const sources: [number, string][] = [
    [gl.VERTEX_SHADER, vertex],
    [gl.FRAGMENT_SHADER, fragment],
  ];
  for (const [type, source] of sources) {
    const shader = gl.createShader(type);
    if (!shader) throw new Error("no shader");
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      throw new Error(
        gl.getShaderInfoLog(shader) ?? "a shader did not compile",
      );
    }
    gl.attachShader(program, shader);
  }
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error(gl.getProgramInfoLog(program) ?? "a program did not link");
  }
  const uniforms = new Map<string, WebGLUniformLocation | null>();
  const made: Program = {
    program,
    uniform(name) {
      if (!uniforms.has(name)) {
        uniforms.set(name, gl.getUniformLocation(program, name));
      }
      return uniforms.get(name) ?? null;
    },
    attribute: (name) => gl.getAttribLocation(program, name),
  };
  return made;
}

/** How many points a ribbon's curve is drawn through. */
const POINTS = 60;
/** The most device pixels a stage pixel is drawn with. */
const DENSEST = 2;

export class Glow {
  /** Its context is gone: the browser took it back, and nothing more can be painted. */
  lost = false;
  readonly #layer: HTMLElement;
  readonly #canvas: HTMLCanvasElement;
  readonly #gl: WebGLRenderingContext;
  readonly #ribbon: Program;
  readonly #glint: Program;
  readonly #dot: Program;
  readonly #strip: WebGLBuffer;
  readonly #corners: WebGLBuffer;
  readonly #watch: ResizeObserver | undefined;
  /** The layer's size on the stage, and how many device pixels one of its pixels is. */
  #size = { wide: 0, high: 0, dense: 1 };
  #stale = true;
  #blank = true;

  /** A painter on `layer`, `under` its cards or `over` them, if the browser has WebGL to
   * give it. */
  static on(layer: HTMLElement, where: "under" | "over"): Glow | undefined {
    const canvas = layer.ownerDocument.createElement("canvas");
    const gl = canvas.getContext("webgl", {
      alpha: true,
      premultipliedAlpha: true,
      antialias: true,
    });
    if (!gl) return undefined;
    try {
      return new Glow(layer, canvas, gl, where);
    } catch {
      return undefined;
    }
  }

  private constructor(
    layer: HTMLElement,
    canvas: HTMLCanvasElement,
    gl: WebGLRenderingContext,
    where: "under" | "over",
  ) {
    this.#layer = layer;
    this.#canvas = canvas;
    this.#gl = gl;
    this.#ribbon = compile(gl, RIBBON_VERTEX, RIBBON_FRAGMENT);
    this.#glint = compile(gl, GLINT_VERTEX, GLINT_FRAGMENT);
    this.#dot = compile(gl, DOT_VERTEX, DOT_FRAGMENT);
    this.#strip = gl.createBuffer();
    this.#corners = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.#corners);
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]),
      gl.STATIC_DRAW,
    );
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    canvas.className = `stage-lights stage-lights-${where}`;
    canvas.setAttribute("aria-hidden", "true");
    canvas.addEventListener("webglcontextlost", () => {
      this.lost = true;
    });
    layer.append(canvas);
    // The layer is scaled with the stage (game/Stage.tsx): what it is scaled by changes with
    // the room its parent has.
    if (typeof ResizeObserver === "function") {
      this.#watch = new ResizeObserver(() => {
        this.#stale = true;
      });
      this.#watch.observe(layer);
      if (layer.parentElement) this.#watch.observe(layer.parentElement);
    }
  }

  /** Takes the canvas off the page and gives its context back. */
  gone() {
    this.#watch?.disconnect();
    this.#canvas.remove();
    this.#gl.getExtension("WEBGL_lose_context")?.loseContext();
  }

  #fit() {
    if (!this.#stale && this.#watch) return;
    this.#stale = false;
    const [wide, high] = [this.#layer.offsetWidth, this.#layer.offsetHeight];
    const shown = this.#layer.getBoundingClientRect().width / (wide || 1) || 1;
    const dense = Math.min(shown * (window.devicePixelRatio || 1), DENSEST);
    this.#size = { wide, high, dense };
    const [across, down] = [Math.round(wide * dense), Math.round(high * dense)];
    if (this.#canvas.width !== across) this.#canvas.width = across;
    if (this.#canvas.height !== down) this.#canvas.height = down;
  }

  /** Paints `scene`, and nothing of the scene before it. */
  paint(scene: Scene) {
    const lit =
      scene.ribbons.length > 0 ||
      scene.glints.length > 0 ||
      scene.dots.length > 0;
    if (!lit && this.#blank) return;
    this.#fit();
    const gl = this.#gl;
    gl.viewport(0, 0, this.#canvas.width, this.#canvas.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    this.#blank = !lit;
    if (!lit || this.#size.wide === 0) return;
    const time = scene.now / 1000;
    for (const glint of scene.glints) this.#shape(glint);
    for (const ribbon of scene.ribbons) this.#along(ribbon, time);
    this.#sparks(scene.dots);
  }

  #sized(program: Program) {
    const gl = this.#gl;
    gl.useProgram(program.program);
    gl.uniform2f(program.uniform("u_res"), this.#size.wide, this.#size.high);
    gl.uniform1f(program.uniform("u_px"), 1 / this.#size.dense);
  }

  #toned(program: Program, tone: Ribbon["tone"], warm: number, gain: number) {
    const gl = this.#gl;
    const [{ glow, core }, gold] = [TONES[tone], TONES.gold];
    const turned = glow.map((v, i) => v + (gold.glow[i]! - v) * warm);
    gl.uniform3fv(program.uniform("u_col"), turned);
    gl.uniform3fv(program.uniform("u_hot"), core);
    gl.uniform1f(program.uniform("u_gain"), gain);
  }

  #along(ribbon: Ribbon, time: number) {
    const { curve, from, to, cut, wide, tone, gain, seed } = ribbon;
    if (to - from < 0.004 || gain <= 0) return;
    const gl = this.#gl;
    const points: { x: number; y: number; arc: number }[] = [];
    let arc = 0;
    for (let i = 0; i < POINTS; i++) {
      const p = along(curve, from + ((to - from) * i) / (POINTS - 1));
      const last = points[i - 1];
      if (last) arc += Math.hypot(p.x - last.x, p.y - last.y);
      points.push({ ...p, arc });
    }
    // The side its core leans to is the one up the stage.
    const [mid, next] = [points[POINTS >> 1]!, points[(POINTS >> 1) + 1]!];
    const flip = next.x - mid.x < 0 ? -1 : 1;
    const data = new Float32Array(POINTS * 2 * 6);
    points.forEach((p, i) => {
      const [q, o] = [
        points[Math.min(i + 1, POINTS - 1)]!,
        points[Math.max(i - 1, 0)]!,
      ];
      const [x, y] = [q.x - o.x, q.y - o.y];
      const long = Math.hypot(x, y) || 1;
      const normal = { x: (y / long) * flip, y: (-x / long) * flip };
      const body = widthOf(cut, wide, p.arc, arc);
      const half = body * 2.6 + 16;
      for (const side of [half, -half]) {
        const at = (i * 2 + (side < 0 ? 1 : 0)) * 6;
        data.set(
          [p.x + normal.x * side, p.y + normal.y * side, side, body, p.arc, 0],
          at,
        );
      }
    });
    const program = this.#ribbon;
    this.#sized(program);
    this.#toned(program, tone, 0, gain);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.#strip);
    gl.bufferData(gl.ARRAY_BUFFER, data, gl.DYNAMIC_DRAW);
    this.#pointed(program, "a_pos", 2, 24, 0);
    this.#pointed(program, "a_dat", 4, 24, 8);
    gl.uniform1f(program.uniform("u_time"), time);
    gl.uniform1f(program.uniform("u_len"), arc);
    gl.uniform1f(program.uniform("u_lean"), cut === "arrow" ? 0.22 : 0);
    gl.uniform1f(program.uniform("u_seed"), seed);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, POINTS * 2);
  }

  #pointed(
    program: Program,
    name: string,
    size: number,
    stride: number,
    offset: number,
  ) {
    const gl = this.#gl;
    const at = program.attribute(name);
    if (at < 0) return;
    gl.enableVertexAttribArray(at);
    gl.vertexAttribPointer(at, size, gl.FLOAT, false, stride, offset);
  }

  #shape({ kind, at, up, size, tone, warm, gain, ring, seed = 0 }: Glint) {
    if (!(gain > 0.003) || size.x <= 0 || size.y <= 0) return;
    const gl = this.#gl;
    const program = this.#glint;
    this.#sized(program);
    this.#toned(program, tone, warm, gain);
    // A wedge's box stands on its base; the others lie about their middle. Each has room
    // round it for its halo.
    const foot = kind === "wedge" || kind === "beam";
    const lift = foot ? size.y / 2 : 0;
    const room = kind === "flash" || kind === "burst" ? 0 : 26;
    const high = foot ? size.y / 2 : size.y;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.#corners);
    this.#pointed(program, "a_c", 2, 0, 0);
    gl.uniform2f(
      program.uniform("u_at"),
      at.x + up.x * lift,
      at.y + up.y * lift,
    );
    gl.uniform2f(program.uniform("u_ax"), up.y, -up.x);
    gl.uniform2f(program.uniform("u_ay"), up.x, up.y);
    gl.uniform2f(program.uniform("u_half"), size.x + room, high + room);
    gl.uniform2f(program.uniform("u_size"), size.x, size.y);
    gl.uniform1i(
      program.uniform("u_kind"),
      ({ wedge: 1, crescent: 2, flash: 3, burst: 4, beam: 5 } as const)[kind],
    );
    gl.uniform1f(program.uniform("u_seed"), seed);
    gl.uniform1f(program.uniform("u_ring"), Math.min(Math.max(ring, 0), 1));
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  #sparks(dots: readonly number[]) {
    if (dots.length === 0) return;
    const gl = this.#gl;
    const program = this.#dot;
    this.#sized(program);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.#strip);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(dots), gl.DYNAMIC_DRAW);
    this.#pointed(program, "a_pos", 2, 24, 0);
    this.#pointed(program, "a_dat", 4, 24, 8);
    gl.uniform1f(program.uniform("u_scale"), this.#size.dense);
    gl.drawArrays(gl.POINTS, 0, dots.length / 6);
  }
}
