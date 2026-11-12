/// <reference types="@webgpu/types" />
// Renders liquid-dom's WebGPU glass behind the DOM.
//
// Mika's WebView can't copy live DOM into GPU textures (that needs Chrome's
// HTML-in-Canvas flag), so text stays in the DOM on top. This paints a soft
// wallpaper for the glass to refract, then a glass shape under every `.glass`
// element (the floating controls) and every card.

import { Container, Glass, Scene, WebGpuGlassCore } from "@liquid-dom/core";

const MAX_DPR = 2;
// Cards get their own container so neighbours don't melt into one shape.
const CARDS = ".panel";

// Pastel blobs on a light base, like a macOS wallpaper: colourful enough for
// the refraction to show, light enough for ink text on top.
const BLOBS: [x: number, y: number, r: number, color: string][] = [
  [0.1, 0.12, 0.55, "#9cc4ff"],
  [0.95, 0.3, 0.5, "#d3b5ff"],
  [0.2, 0.72, 0.55, "#ffc9a8"],
  [0.85, 0.92, 0.5, "#a8ecd6"],
];

function paintWallpaper(device: GPUDevice, target: GPUTexture, width: number, height: number) {
  const canvas = new OffscreenCanvas(width, height);
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.fillStyle = "#eef1f7";
  ctx.fillRect(0, 0, width, height);
  // Radial fades rather than ctx.filter blur, which WebKit's canvas lacks.
  const size = Math.max(width, height);
  for (const [x, y, r, color] of BLOBS) {
    const cx = x * width;
    const cy = y * height;
    const gradient = ctx.createRadialGradient(cx, cy, 0, cx, cy, r * size * 0.6);
    gradient.addColorStop(0, color);
    gradient.addColorStop(1, `${color}00`);
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, width, height);
  }
  device.queue.copyExternalImageToTexture({ source: canvas }, { texture: target }, [width, height]);
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
  // Cards: frostier than the controls so their text stays readable.
  const cards = new Container({
    blur: 18,
    spacing: 0,
    bezelWidth: 16,
    tint: { r: 1, g: 1, b: 1, a: 0.5 },
    shadowColor: { r: 0, g: 0, b: 0, a: 0.08 },
    shadowOffsetY: 8,
    shadowBlur: 24,
    specularOpacity: 0.6,
    specularFalloff: 1,
    specularWidth: 1,
    specularStrength: 1,
    specularSharpness: 3,
    oppositeSpecularStrength: 0.4,
  });
  scene.add(cards);
  scene.add(container);
  const shapes = new Map<Element, Glass>();

  let width = 0;
  let height = 0;
  let dpr = 1;
  let output: GPUTexture | null = null;
  let backdrop: GPUTexture | null = null;
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
    paintWallpaper(device, backdrop, width, height);
  };

  // Mirrors every .glass element and card as a glass shape at the same place
  // and size. Cards are clipped to their page, which can scroll (Settings).
  const syncShapes = () => {
    const seen = new Set<Element>();
    const mirror = (el: Element, parent: Container, clip: DOMRect | null) => {
      const rect = el.getBoundingClientRect();
      const top = Math.max(rect.top, clip?.top ?? -Infinity);
      const bottom = Math.min(rect.bottom, clip?.bottom ?? Infinity);
      // Hidden (inactive page) or scrolled out of view.
      if (rect.width === 0 || bottom - top <= 0) return;
      seen.add(el);
      let glass = shapes.get(el);
      if (!glass) {
        glass = new Glass({ width: rect.width, height: bottom - top, cornerSmoothing: 0.6 });
        parent.add(glass);
        shapes.set(el, glass);
      }
      glass.x = rect.left;
      glass.y = top;
      glass.width = rect.width;
      glass.height = bottom - top;
      glass.cornerRadius = Math.min(parseFloat(getComputedStyle(el).borderTopLeftRadius) || 0, (bottom - top) / 2);
    };
    for (const el of root.querySelectorAll(".glass")) mirror(el, container, null);
    for (const el of root.querySelectorAll(CARDS)) {
      mirror(el, cards, el.closest(".page")?.getBoundingClientRect() ?? null);
    }
    for (const [el, glass] of shapes) {
      if (!seen.has(el)) {
        glass.remove();
        shapes.delete(el);
      }
    }
  };

  const frame = () => {
    const nextDpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
    if (Math.round(canvas.clientWidth * nextDpr) !== width || Math.round(canvas.clientHeight * nextDpr) !== height) {
      resize();
    }
    if (!output || !backdrop) return;

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
