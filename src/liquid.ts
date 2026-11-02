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

// Crisp-edged shapes give the glass detail to bend; a soft gradient shows no refraction.
const SHAPES = [
  { colors: ["#ff8a5c", "#d9480f"], x: 0.18, y: 0.14, r: 0.34, dx: 0.05, dy: 0.03, speed: 0.00012 },
  { colors: ["#3fe0b0", "#0b8f6b"], x: 0.92, y: 0.4, r: 0.3, dx: 0.04, dy: 0.05, speed: 0.0001 },
  { colors: ["#8f7bff", "#3b2bbf"], x: 0.12, y: 0.72, r: 0.38, dx: 0.06, dy: 0.04, speed: 0.00008 },
  { colors: ["#ffd65c", "#e09a00"], x: 0.78, y: 0.92, r: 0.22, dx: 0.05, dy: 0.03, speed: 0.00014 },
  { colors: ["#ff5fa2", "#b0105a"], x: 0.62, y: 0.12, r: 0.14, dx: 0.04, dy: 0.04, speed: 0.00016 },
];

function drawBackdrop(ctx: CanvasRenderingContext2D, w: number, h: number, t: number) {
  ctx.fillStyle = "#0b0b12";
  ctx.fillRect(0, 0, w, h);
  const size = Math.max(w, h);

  // Thin diagonal lines: fine detail that makes refraction visible everywhere.
  ctx.strokeStyle = "rgba(255, 255, 255, 0.06)";
  ctx.lineWidth = Math.max(1, size / 600);
  const step = size / 28;
  const drift = ((t * 0.004) % step) - step;
  ctx.beginPath();
  for (let x = -h + drift; x < w + h; x += step) {
    ctx.moveTo(x, 0);
    ctx.lineTo(x + h, h);
  }
  ctx.stroke();

  for (const [i, sh] of SHAPES.entries()) {
    const phase = t * sh.speed + i * 2.1;
    const cx = (sh.x + Math.sin(phase) * sh.dx) * w;
    const cy = (sh.y + Math.cos(phase * 1.3) * sh.dy) * h;
    const r = sh.r * size;
    const g = ctx.createLinearGradient(cx - r, cy - r, cx + r, cy + r);
    g.addColorStop(0, sh.colors[0]);
    g.addColorStop(1, sh.colors[1]);
    ctx.fillStyle = g;
    ctx.beginPath();
    ctx.arc(cx, cy, r, 0, Math.PI * 2);
    ctx.fill();
    // A darker inner ring adds a second edge for the glass to catch.
    ctx.strokeStyle = "rgba(0, 0, 0, 0.25)";
    ctx.lineWidth = r * 0.08;
    ctx.beginPath();
    ctx.arc(cx, cy, r * 0.7, 0, Math.PI * 2);
    ctx.stroke();
  }
}

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

  const scene = new Scene();
  // Optics follow liquid-dom's Control Center showcase demo.
  const container = new Container({
    blur: 10,
    spacing: 10,
    bezelWidth: 44,
    tint: { r: 1, g: 1, b: 1, a: 0.1 },
    shadowColor: { r: 0, g: 0, b: 0, a: 0.25 },
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
  // A DOM canvas, not OffscreenCanvas: WebKit copies it to WebGPU more reliably.
  let backdropCanvas: HTMLCanvasElement | null = null;
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
    backdropCanvas = document.createElement("canvas");
    backdropCanvas.width = width;
    backdropCanvas.height = height;
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
