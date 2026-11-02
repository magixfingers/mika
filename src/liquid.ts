/// <reference types="@webgpu/types" />
// Renders liquid-dom's WebGPU glass behind the DOM.
//
// Mika's WebView can't copy live DOM into GPU textures (that needs Chrome's
// HTML-in-Canvas flag), so text stays in the DOM on top. This draws only the
// color field and a glass shape under every `.glass` element.

import { Container, Glass, Scene, WebGpuGlassCore } from "@liquid-dom/core";

const MAX_DPR = 2;
// Backdrop redraws are the costly part; the drift is slow, so 30 fps is plenty.
const BACKDROP_INTERVAL_MS = 33;

const BLOBS = [
  { color: "232, 135, 95", x: 0.2, y: 0.18, r: 0.55, dx: 0.06, dy: 0.04, speed: 0.00011 },
  { color: "63, 207, 158", x: 0.85, y: 0.42, r: 0.5, dx: 0.05, dy: 0.06, speed: 0.00009 },
  { color: "120, 100, 255", x: 0.3, y: 0.85, r: 0.6, dx: 0.07, dy: 0.04, speed: 0.00007 },
];

function drawBackdrop(ctx: OffscreenCanvasRenderingContext2D, w: number, h: number, t: number) {
  ctx.fillStyle = "#07070b";
  ctx.fillRect(0, 0, w, h);
  const size = Math.max(w, h);
  for (const [i, b] of BLOBS.entries()) {
    const phase = t * b.speed + i * 2.1;
    const cx = (b.x + Math.sin(phase) * b.dx) * w;
    const cy = (b.y + Math.cos(phase * 1.3) * b.dy) * h;
    const g = ctx.createRadialGradient(cx, cy, 0, cx, cy, b.r * size);
    g.addColorStop(0, `rgba(${b.color}, 0.55)`);
    g.addColorStop(1, `rgba(${b.color}, 0)`);
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, w, h);
  }
}

/**
 * Starts the glass renderer on `canvas`. Resolves to a stop function, or null
 * when WebGPU is unavailable.
 */
export async function startLiquidGlass(canvas: HTMLCanvasElement, root: HTMLElement): Promise<(() => void) | null> {
  const gpu = (navigator as Navigator & { gpu?: GPU }).gpu;
  const adapter = await gpu?.requestAdapter();
  if (!gpu || !adapter) return null;
  const device = await adapter.requestDevice();
  const context = canvas.getContext("webgpu");
  if (!context) return null;
  const format = gpu.getPreferredCanvasFormat();
  const core = new WebGpuGlassCore({ device, format });

  const scene = new Scene();
  const container = new Container({
    blur: 14,
    spacing: 10,
    thickness: 40,
    bezelWidth: 18,
    dispersion: 0.4,
    tint: { r: 0.08, g: 0.08, b: 0.12, a: 0.35 },
    specularStrength: 0.7,
    shadowColor: { r: 0, g: 0, b: 0, a: 0.35 },
    shadowBlur: 24,
    shadowOffsetY: 8,
  });
  scene.add(container);
  const shapes = new Map<Element, Glass>();

  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  let width = 0;
  let height = 0;
  let dpr = 1;
  let output: GPUTexture | null = null;
  let backdrop: GPUTexture | null = null;
  let backdropCanvas: OffscreenCanvas | null = null;
  let lastBackdrop = -Infinity;
  let raf = 0;

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
    backdropCanvas = new OffscreenCanvas(width, height);
    lastBackdrop = -Infinity;
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

  const frame = (now: number) => {
    const nextDpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
    if (Math.round(canvas.clientWidth * nextDpr) !== width || Math.round(canvas.clientHeight * nextDpr) !== height) {
      resize();
    }
    if (!output || !backdrop || !backdropCanvas) return;
    if (now - lastBackdrop >= BACKDROP_INTERVAL_MS && (!reduceMotion.matches || lastBackdrop < 0)) {
      const ctx = backdropCanvas.getContext("2d");
      if (ctx) {
        drawBackdrop(ctx, width, height, reduceMotion.matches ? 0 : now);
        device.queue.copyExternalImageToTexture({ source: backdropCanvas }, { texture: backdrop }, [width, height]);
      }
      lastBackdrop = now;
    }
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
    device.destroy();
  };
}
