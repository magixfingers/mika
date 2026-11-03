/// <reference types="@webgpu/types" />
// Renders liquid-dom's WebGPU glass behind the DOM.
//
// Mika's WebView can't copy live DOM into GPU textures (that needs Chrome's
// HTML-in-Canvas flag), so text stays in the DOM on top. This draws the
// backdrop with a WGSL shader and a glass shape under every `.glass` element
// (the floating controls).

import { Container, Glass, Scene, WebGpuGlassCore } from "@liquid-dom/core";

const MAX_DPR = 2;
// How quickly the shader's energy follows the target, per frame (0..1).
const ENERGY_EASE = 0.02;

/**
 * Agent activity, 0..1, set by the app from tokens per second. The backdrop
 * flows faster and blooms brighter as it rises, so busy agents are visible
 * at a glance.
 */
export const liquidActivity = { target: 0 };

// The Vercel mesh (develop, preview and ship stops) as a domain-warped noise
// field blooming around the totals, over the #fafafa canvas, with fine grain.
const BACKDROP_SHADER = /* wgsl */ `
struct Uniforms {
  resolution: vec2f,
  time: f32,
  energy: f32,
  hero: vec4f, // x, y, width, height in device pixels
};
@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
  var corners = array(vec2f(-1.0, -3.0), vec2f(-1.0, 1.0), vec2f(3.0, 1.0));
  return vec4f(corners[i], 0.0, 1.0);
}

fn hash(p: vec2f) -> f32 {
  return fract(sin(dot(p, vec2f(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2f) -> f32 {
  let i = floor(p);
  let f = fract(p);
  let s = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(hash(i), hash(i + vec2f(1.0, 0.0)), s.x),
    mix(hash(i + vec2f(0.0, 1.0)), hash(i + vec2f(1.0, 1.0)), s.x),
    s.y,
  );
}

fn fbm(p: vec2f) -> f32 {
  var v = 0.0;
  var a = 0.5;
  var q = p;
  for (var k = 0; k < 4; k++) {
    v += a * noise(q);
    q = q * 2.03 + vec2f(1.7, 9.2);
    a *= 0.5;
  }
  return v;
}

@fragment
fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let center = u.hero.xy + u.hero.zw * 0.5;
  let scale = max(u.hero.z, u.hero.w * 2.0);
  let p = (pos.xy - center) / scale;
  let t = u.time;

  // Two layers of warping make the colors flow rather than slide.
  let warp = vec2f(fbm(p * 2.2 + vec2f(t, -t * 0.7)), fbm(p * 2.2 + vec2f(-t * 0.6, t) + 4.1));
  let q = p * 1.6 + (warp - 0.5) * (0.9 + 0.5 * u.energy);

  let blue = vec3f(0.0, 0.486, 0.941);
  let cyan = vec3f(0.314, 0.89, 0.761);
  let violet = vec3f(0.475, 0.157, 0.792);
  let pink = vec3f(1.0, 0.0, 0.502);
  let amber = vec3f(0.976, 0.796, 0.157);

  var col = mix(blue, cyan, smoothstep(0.25, 0.75, fbm(q + vec2f(0.0, t * 0.5))));
  col = mix(col, violet, smoothstep(0.45, 0.85, fbm(q * 1.3 + 7.3)) * 0.8);
  col = mix(col, pink, smoothstep(0.55, 0.9, fbm(q * 1.1 - 3.1 + t * 0.3)) * 0.75);
  col = mix(col, amber, smoothstep(0.6, 0.95, fbm(q * 0.9 + 11.7)) * 0.6);

  // Bloom around the hero, stronger while agents are busy.
  let falloff = exp(-dot(p * vec2f(1.0, 1.6), p * vec2f(1.0, 1.6)) * 3.2);
  let strength = falloff * (0.34 + 0.22 * u.energy);
  var rgb = mix(vec3f(0.98), col, strength);

  // Fine film grain keeps the gradient from banding.
  rgb += (hash(pos.xy + fract(t) * 91.0) - 0.5) * 0.018;
  return vec4f(rgb, 1.0);
}
`;

/**
 * Starts the glass renderer on `canvas`. Resolves to a stop function, or null
 * when WebGPU is unavailable.
 */
export async function startLiquidGlass(
  canvas: HTMLCanvasElement,
  root: HTMLElement,
  onError: (message: string) => void,
): Promise<(() => void) | null> {
  const gpu = (navigator as Navigator & { gpu?: GPU }).gpu;
  const adapter = await gpu?.requestAdapter();
  if (!gpu || !adapter) return null;
  const device = await adapter.requestDevice();
  device.addEventListener("uncapturederror", (e) => onError((e as GPUUncapturedErrorEvent).error.message));
  const context = canvas.getContext("webgpu");
  if (!context) return null;
  const format = gpu.getPreferredCanvasFormat();
  const core = new WebGpuGlassCore({ device, format });

  const module = device.createShaderModule({ code: BACKDROP_SHADER });
  const pipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: { module, entryPoint: "vs" },
    fragment: { module, entryPoint: "fs", targets: [{ format }] },
  });
  const uniforms = device.createBuffer({ size: 32, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  const bindGroup = device.createBindGroup({
    layout: pipeline.getBindGroupLayout(0),
    entries: [{ binding: 0, resource: { buffer: uniforms } }],
  });

  const scene = new Scene();
  // Optics follow liquid-dom's Control Center showcase demo, tinted for a light canvas.
  const container = new Container({
    blur: 10,
    spacing: 10,
    bezelWidth: 20,
    tint: { r: 1, g: 1, b: 1, a: 0.55 },
    shadowColor: { r: 0, g: 0, b: 0, a: 0.12 },
    shadowOffsetY: 14,
    shadowBlur: 30,
    specularOpacity: 0.7,
    specularFalloff: 1,
    specularWidth: 1,
    specularStrength: 1,
    specularSharpness: 3,
    oppositeSpecularStrength: 0.5,
  });
  scene.add(container);
  const shapes = new Map<Element, Glass>();

  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  let width = 0;
  let height = 0;
  let dpr = 1;
  let output: GPUTexture | null = null;
  let backdrop: GPUTexture | null = null;
  let raf = 0;
  let last = performance.now();
  let flow = 0;
  let energy = 0;

  const resize = () => {
    dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
    width = Math.max(1, Math.round(canvas.clientWidth * dpr));
    height = Math.max(1, Math.round(canvas.clientHeight * dpr));
    canvas.width = width;
    canvas.height = height;
    context.configure({
      device,
      format,
      usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_DST,
      alphaMode: "opaque",
    });
    output?.destroy();
    backdrop?.destroy();
    const usage =
      GPUTextureUsage.RENDER_ATTACHMENT |
      GPUTextureUsage.TEXTURE_BINDING |
      GPUTextureUsage.COPY_SRC |
      GPUTextureUsage.COPY_DST;
    output = device.createTexture({ size: [width, height], format, usage });
    backdrop = device.createTexture({ size: [width, height], format, usage });
  };

  // Mirrors every .glass element as a glass shape at the same place and size.
  const syncShapes = () => {
    const seen = new Set<Element>();
    for (const el of root.querySelectorAll(".glass")) {
      seen.add(el);
      const rect = el.getBoundingClientRect();
      let glass = shapes.get(el);
      if (!glass) {
        glass = new Glass({ width: rect.width, height: rect.height, cornerSmoothing: 0.6 });
        container.add(glass);
        shapes.set(el, glass);
      }
      glass.x = rect.left;
      glass.y = rect.top;
      glass.width = rect.width;
      glass.height = rect.height;
      glass.cornerRadius = Math.min(parseFloat(getComputedStyle(el).borderTopLeftRadius) || 0, rect.height / 2);
    }
    for (const [el, glass] of shapes) {
      if (!seen.has(el)) {
        glass.remove();
        shapes.delete(el);
      }
    }
  };

  const drawBackdrop = (encoder: GPUCommandEncoder, target: GPUTexture) => {
    const r = root.querySelector(".totals")?.getBoundingClientRect();
    const hero = r
      ? [r.left * dpr, r.top * dpr, r.width * dpr, r.height * dpr]
      : [0, 0, width, height * 0.3];
    device.queue.writeBuffer(uniforms, 0, new Float32Array([width, height, flow, energy, ...hero]));
    const pass = encoder.beginRenderPass({
      colorAttachments: [{ view: target.createView(), loadOp: "clear", storeOp: "store", clearValue: [0.98, 0.98, 0.98, 1] }],
    });
    pass.setPipeline(pipeline);
    pass.setBindGroup(0, bindGroup);
    pass.draw(3);
    pass.end();
  };

  const frame = (now: number) => {
    const nextDpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
    if (Math.round(canvas.clientWidth * nextDpr) !== width || Math.round(canvas.clientHeight * nextDpr) !== height) {
      resize();
    }
    if (!output || !backdrop) return;

    const dt = Math.min(0.1, (now - last) / 1000);
    last = now;
    energy += (Math.min(1, Math.max(0, liquidActivity.target)) - energy) * ENERGY_EASE;
    // Integrating speed (rather than scaling time) keeps the flow smooth as energy changes.
    if (!reduceMotion.matches) flow += dt * (0.025 + 0.12 * energy);

    const backdropEncoder = device.createCommandEncoder();
    drawBackdrop(backdropEncoder, backdrop);
    device.queue.submit([backdropEncoder.finish()]);

    syncShapes();
    core.render({ scene, width, height, dpr, outputTexture: output, backdropTexture: backdrop });
    const encoder = device.createCommandEncoder();
    encoder.copyTextureToTexture({ texture: output }, { texture: context.getCurrentTexture() }, [width, height]);
    device.queue.submit([encoder.finish()]);
    raf = requestAnimationFrame(frame);
  };

  resize();
  raf = requestAnimationFrame(frame);
  return () => {
    cancelAnimationFrame(raf);
    core.destroy();
    output?.destroy();
    backdrop?.destroy();
    uniforms.destroy();
    device.destroy();
  };
}
