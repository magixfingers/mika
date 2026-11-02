/// <reference types="@webgpu/types" />
// Renders liquid-dom's WebGPU glass behind the DOM.
//
// Mika's WebView can't copy live DOM into GPU textures (that needs Chrome's
// HTML-in-Canvas flag), so text stays in the DOM on top. This draws only the
// color field and a glass shape under every `.glass` element (the floating controls).

import { Container, Glass, Scene, WebGpuGlassCore } from "@liquid-dom/core";

const MAX_DPR = 2;
// Backdrop redraws are the costly part; the drift is slow, so 30 fps is plenty.
const BACKDROP_INTERVAL_MS = 33;

// The Vercel mesh: develop, preview and ship gradient stops blooming behind the
// totals, the only color on the page. Positions are relative to the hero box.
const MESH = [
  { color: "0, 124, 240", x: 0.1, y: 0.35, r: 0.55, a: 0.32 },
  { color: "80, 227, 194", x: 0.42, y: 0.1, r: 0.5, a: 0.34 },
  { color: "121, 40, 202", x: 0.68, y: 0.55, r: 0.45, a: 0.22 },
  { color: "255, 0, 128", x: 0.9, y: 0.25, r: 0.45, a: 0.26 },
  { color: "249, 203, 40", x: 0.55, y: 0.95, r: 0.4, a: 0.24 },
];

type Box = { x: number; y: number; w: number; h: number };

function drawBackdrop(ctx: CanvasRenderingContext2D, w: number, h: number, hero: Box, t: number) {
  ctx.fillStyle = "#fafafa";
  ctx.fillRect(0, 0, w, h);
  const size = Math.max(hero.w, hero.h * 2);
  for (const [i, m] of MESH.entries()) {
    const phase = t * 0.00012 + i * 1.7;
    const cx = hero.x + (m.x + Math.sin(phase) * 0.05) * hero.w;
    const cy = hero.y + (m.y + Math.cos(phase * 1.3) * 0.08) * hero.h;
    const g = ctx.createRadialGradient(cx, cy, 0, cx, cy, m.r * size);
    g.addColorStop(0, `rgba(${m.color}, ${m.a})`);
    g.addColorStop(1, `rgba(${m.color}, 0)`);
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, w, h);
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
        const r = root.querySelector(".totals")?.getBoundingClientRect();
        const hero = r
          ? { x: r.left * dpr, y: r.top * dpr, w: r.width * dpr, h: r.height * dpr }
          : { x: 0, y: 0, w: width, h: height * 0.3 };
        drawBackdrop(ctx, width, height, hero, reduceMotion.matches ? 0 : now);
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
