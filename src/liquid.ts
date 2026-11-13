/// <reference types="@webgpu/types" />
// Renders liquid-dom's WebGPU glass behind the DOM.
//
// Mika's WebView can't copy live DOM into GPU textures (that needs Chrome's
// HTML-in-Canvas flag), so text stays in the DOM on top. This paints a soft
// wallpaper for the glass to refract, then a glass shape under every `.glass`
// element (the floating controls) and every card.

import { Container, Glass, Scene, WebGpuGlassCore } from "@liquid-dom/core";
import { DEFAULT_GLASS, type Backdrop, type GlassLook } from "./types";

const MAX_DPR = 2;
// Cards get their own container so neighbours don't melt into one shape.
const CARDS = ".panel";

// Soft blobs on a light base, like a macOS wallpaper: colourful enough for the
// refraction to show, light enough for ink text on top.
type Blob = [x: number, y: number, r: number, color: string];
const BACKDROPS: Record<Backdrop, { base: string; blobs: Blob[] }> = {
  pastel: {
    base: "#eef1f7",
    blobs: [[0.1, 0.12, 0.55, "#9cc4ff"], [0.95, 0.3, 0.5, "#d3b5ff"], [0.2, 0.72, 0.55, "#ffc9a8"], [0.85, 0.92, 0.5, "#a8ecd6"]],
  },
  mono: {
    base: "#f2f2f2",
    blobs: [[0.1, 0.12, 0.55, "#cfcfcf"], [0.95, 0.3, 0.5, "#e2e2e2"], [0.2, 0.72, 0.55, "#d6d6d6"], [0.85, 0.92, 0.5, "#c4c4c4"]],
  },
  ocean: {
    base: "#eaf3f7",
    blobs: [[0.1, 0.12, 0.55, "#8ec5ff"], [0.95, 0.3, 0.5, "#7fe0d6"], [0.2, 0.72, 0.55, "#a0d8ff"], [0.85, 0.92, 0.5, "#9fe7c9"]],
  },
  sunset: {
    base: "#f7efe9",
    blobs: [[0.1, 0.12, 0.55, "#ffb3a1"], [0.95, 0.3, 0.5, "#f5a8c8"], [0.2, 0.72, 0.55, "#ffd59e"], [0.85, 0.92, 0.5, "#ffc7b0"]],
  },
};

function paintWallpaper(device: GPUDevice, target: GPUTexture, width: number, height: number, backdrop: Backdrop) {
  const { base, blobs } = BACKDROPS[backdrop] ?? BACKDROPS.pastel;
  const canvas = new OffscreenCanvas(width, height);
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.fillStyle = base;
  ctx.fillRect(0, 0, width, height);
  // Radial fades rather than ctx.filter blur, which WebKit's canvas lacks.
  const size = Math.max(width, height);
  for (const [x, y, r, color] of blobs) {
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
 * Maps the user's sliders onto both glass layers. Controls are clearer than
 * cards, which stay frostier so their text reads. At the defaults this gives
 * the original hand-tuned values (controls: blur 10, tint 0.55, bezel 20...).
 */
function applyLook(controls: Container, cards: Container, look: GlassLook) {
  const clear = look.transparency / 100;
  controls.tint = { r: 1, g: 1, b: 1, a: Math.max(0, 1 - clear) };
  cards.tint = { r: 1, g: 1, b: 1, a: Math.max(0, 0.95 - clear) };
  controls.blur = (look.frost * 10) / 18;
  cards.blur = look.frost;
  controls.bezelWidth = (look.refraction / 100) * 40;
  cards.bezelWidth = (look.refraction / 100) * 32;
  controls.specularOpacity = look.shine / 100;
  cards.specularOpacity = (look.shine / 100) * (0.6 / 0.7);
  controls.shadowColor = { r: 0, g: 0, b: 0, a: (look.shadow / 100) * 0.24 };
  cards.shadowColor = { r: 0, g: 0, b: 0, a: (look.shadow / 100) * 0.16 };
}

export type LiquidGlass = {
  stop: () => void;
  /** Applies a new look on the next frame, repainting the backdrop if it changed. */
  setLook: (look: GlassLook) => void;
};

/**
 * Starts the glass renderer on `canvas`. Resolves to its controls, or null
 * when WebGPU is unavailable.
 */
export async function startLiquidGlass(
  canvas: HTMLCanvasElement,
  root: HTMLElement,
  onError: (message: string) => void,
  initialLook: GlassLook = DEFAULT_GLASS,
): Promise<LiquidGlass | null> {
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
  // Optics follow liquid-dom's Control Center showcase demo, tinted for a light
  // canvas; applyLook sets the parts the user can tune.
  const container = new Container({
    spacing: 10,
    shadowOffsetY: 14,
    shadowBlur: 30,
    specularFalloff: 1,
    specularWidth: 1,
    specularStrength: 1,
    specularSharpness: 3,
    oppositeSpecularStrength: 0.5,
  });
  // Cards: frostier than the controls so their text stays readable.
  const cards = new Container({
    spacing: 0,
    shadowOffsetY: 8,
    shadowBlur: 24,
    specularFalloff: 1,
    specularWidth: 1,
    specularStrength: 1,
    specularSharpness: 3,
    oppositeSpecularStrength: 0.4,
  });
  scene.add(cards);
  scene.add(container);
  let look = initialLook;
  applyLook(container, cards, look);
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
    paintWallpaper(device, backdrop, width, height, look.backdrop);
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
  return {
    stop: () => {
      cancelAnimationFrame(raf);
      core.destroy();
      output?.destroy();
      backdrop?.destroy();
      device.destroy();
    },
    setLook: (next) => {
      const repaint = next.backdrop !== look.backdrop;
      look = next;
      applyLook(container, cards, look);
      if (repaint && backdrop) paintWallpaper(device, backdrop, width, height, look.backdrop);
    },
  };
}
