/// <reference types="@webgpu/types" />
// Renders liquid-dom's WebGPU glass behind the DOM.
//
// Mika's WebView can't copy live DOM into GPU textures (that needs Chrome's
// HTML-in-Canvas flag), so text stays in the DOM on top. This draws a plain
// #fafafa backdrop and a glass shape under every `.glass` element (the
// floating controls).

import { Container, Glass, Scene, WebGpuGlassCore } from "@liquid-dom/core";

const MAX_DPR = 2;
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

  // The page canvas color, cleared into the texture the glass refracts.
  const drawBackdrop = (encoder: GPUCommandEncoder, target: GPUTexture) => {
    encoder
      .beginRenderPass({
        colorAttachments: [{ view: target.createView(), loadOp: "clear", storeOp: "store", clearValue: [0.98, 0.98, 0.98, 1] }],
      })
      .end();
  };

  const frame = () => {
    const nextDpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
    if (Math.round(canvas.clientWidth * nextDpr) !== width || Math.round(canvas.clientHeight * nextDpr) !== height) {
      resize();
    }
    if (!output || !backdrop) return;

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
    device.destroy();
  };
}
