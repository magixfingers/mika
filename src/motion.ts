// GSAP-driven motion. Every effect is skipped when macOS "Reduce motion" is on
// or motion is Off in settings; Subtle keeps only small, functional motion.

import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import gsap from "gsap";
import { animate, spring, stagger, svg } from "animejs";

/** "off" when macOS Reduce motion is on or the user picked Off in settings. */
export function motionLevel(): "full" | "subtle" | "off" {
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return "off";
  const chosen = document.documentElement.dataset.motion;
  return chosen === "off" || chosen === "subtle" ? chosen : "full";
}

const reduceMotion = () => motionLevel() === "off";

/** Counts a number up (or down) to `value` instead of jumping, writing the text directly. */
export function useRollingNumber<T extends HTMLElement>(value: number, format: (n: number) => string) {
  const ref = useRef<T>(null);
  const shown = useRef(value);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (reduceMotion() || shown.current === value) {
      shown.current = value;
      el.textContent = format(value);
      return;
    }
    const state = { n: shown.current };
    // Small changes (live ticks) roll quickly; big ones (range switches) take a beat longer.
    const jump = Math.abs(value - shown.current) / Math.max(1, Math.abs(value));
    const tween = gsap.to(state, {
      n: value,
      duration: jump > 0.2 ? 0.9 : 0.6,
      ease: "power3.out",
      onUpdate: () => {
        shown.current = state.n;
        el.textContent = format(Math.round(state.n));
      },
    });
    return () => {
      tween.kill();
    };
  }, [value, format]);
  return ref;
}

/**
 * Slides an ink pill under the active option of a segmented control.
 * `active` is a selector for the current option inside the container.
 */
export function useSlidingPill(container: RefObject<HTMLElement | null>, active: string, key: unknown) {
  const pill = useRef<HTMLSpanElement>(null);
  const placed = useRef(false);
  useLayoutEffect(() => {
    const box = container.current;
    const target = box?.querySelector<HTMLElement>(active);
    const el = pill.current;
    if (!box || !el) return;
    // No active option (e.g. the tab bar while Settings is open): fade the pill out.
    if (!target) {
      gsap.to(el, { opacity: 0, duration: reduceMotion() ? 0 : 0.2 });
      return;
    }
    const place = (animate: boolean) => {
      const to = { x: target.offsetLeft, width: target.offsetWidth, opacity: 1 };
      if (!animate || reduceMotion()) gsap.set(el, to);
      else gsap.to(el, { ...to, duration: 0.45, ease: "power3.out" });
    };
    place(placed.current);
    placed.current = true;
    // Keep it aligned when fonts load or the window resizes.
    const observer = new ResizeObserver(() => place(false));
    observer.observe(box);
    return () => observer.disconnect();
  }, [container, active, key]);
  return pill;
}

/** Rows inside cards, which cascade in just behind their card. */
const ROWS = ".load-row, .limit-group > *, .model, .agent, .session, .setting";
/** Bars that fill from empty to their value; both read their fill from `--fill`. */
const BARS = ".meter, .share";

/**
 * The Activity charts' part of the entrance (anime.js): the heatmap ripples out
 * from today's cell back through the year, and the trend line draws itself.
 * `delay` is in seconds. Returns the animations so the caller can revert them.
 */
function chartEntrance(root: HTMLElement, delay: number): { revert: () => unknown }[] {
  const running: { revert: () => unknown }[] = [];
  const cells = [...root.querySelectorAll<HTMLElement>(".heatmap .cell")];
  if (cells.length) {
    const today = Math.max(0, cells.findIndex((c) => "today" in c.dataset));
    running.push(
      animate(cells, {
        scale: [0.2, 1],
        opacity: [0, 1],
        // Cells run down each week, so the grid is 7 wide in DOM order.
        delay: stagger(13, { grid: [7, cells.length / 7], from: today, start: delay * 1000 }),
        duration: 420,
        ease: "outBack(2)",
      }),
    );
  }
  const line = root.querySelector<SVGPathElement>(".trend path");
  if (line?.getAttribute("d")) {
    running.push(
      animate(svg.createDrawable(line), { draw: ["0 0", "0 1"], delay: delay * 1000, duration: 1200, ease: "inOutQuad" }),
    );
  }
  return running;
}

/**
 * A staggered entrance whenever `key` changes: the matching cards rise in one
 * after another, the rows inside them cascade in just behind, every bar fills
 * from empty to its value, and the Activity charts ripple and draw in. Full motion only.
 */
export function useEntrance(scope: RefObject<HTMLElement | null>, selector: string, key: unknown) {
  useLayoutEffect(() => {
    const root = scope.current;
    if (!root || motionLevel() !== "full") return;
    const visible = (sel: string) => [...root.querySelectorAll<HTMLElement>(sel)].filter((el) => el.offsetParent !== null);
    const cards = visible(selector);
    if (cards.length === 0) return;
    const rows = visible(ROWS);
    const bars = visible(BARS);
    // Long lists (Settings, many sessions) share a fixed budget so they never drag.
    const spread = (count: number, each: number) => ({ amount: Math.min(0.6, count * each) });
    // Bars normally ease to new values with a CSS transition; GSAP drives them here.
    root.dataset.entering = "";
    const ctx = gsap.context(() => {
      const tl = gsap
        .timeline({ defaults: { ease: "power3.out" }, onComplete: () => delete root.dataset.entering })
        .from(cards, { y: 14, opacity: 0, duration: 0.55, stagger: 0.06, clearProps: "transform,opacity" });
      // Some pages have no rows or bars (GSAP warns about empty targets).
      if (rows.length) {
        tl.from(rows, { y: 8, opacity: 0, duration: 0.45, stagger: spread(rows.length, 0.035), clearProps: "transform,opacity" }, 0.12);
      }
      if (bars.length) {
        tl.from(bars, { "--fill": 0, duration: 0.9, ease: "power2.out", stagger: spread(bars.length, 0.05) }, 0.2);
      }
    }, root);
    // The charts start as their card (the Activity panel) lands.
    const chartCard = cards.findIndex((card) => card.classList.contains("activity"));
    const charts = chartCard < 0 ? [] : chartEntrance(cards[chartCard], chartCard * 0.06 + 0.25);
    return () => {
      ctx.revert();
      charts.forEach((chart) => chart.revert());
      delete root.dataset.entering;
    };
  }, [scope, selector, key]);
}

/** A small squash-and-pop on the element whenever `key` changes (not on first render). */
export function usePop<T extends Element>(key: unknown) {
  const ref = useRef<T>(null);
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    if (!ref.current || reduceMotion()) return;
    gsap.fromTo(
      ref.current,
      { scaleX: 1.12, scaleY: 0.88, transformOrigin: "50% 100%" },
      { scaleX: 1, scaleY: 1, duration: 0.7, ease: "elastic.out(1, 0.45)" },
    );
  }, [key]);
  return ref;
}

/** Shakes the element once each time `active` turns true. */
export function useShake<T extends Element>(active: boolean) {
  const ref = useRef<T>(null);
  useEffect(() => {
    if (!active || !ref.current || reduceMotion()) return;
    gsap.fromTo(
      ref.current,
      { x: 0 },
      { keyframes: { x: [0, -7, 6, -4, 3, -1, 0] }, duration: 0.55, ease: "power2.out", clearProps: "transform" },
    );
  }, [active]);
  return ref;
}

/** Springs an element up to `scale`, or back to rest at 1 (anime.js). */
export function springScale(el: Element, scale: number) {
  if (reduceMotion()) return;
  animate(el, { scale, ease: spring({ stiffness: 380, damping: 20 }) });
}

/** Glides a chart marker to a point and shows it. It jumps there if it was hidden or motion is off. */
export function glideTo(el: HTMLElement | null, to: { x: number; y?: number }) {
  if (!el) return;
  const still = reduceMotion();
  if (still || gsap.getProperty(el, "visibility") === "hidden") gsap.set(el, to);
  else gsap.to(el, { ...to, duration: 0.25, ease: "power3.out", overwrite: "auto" });
  gsap.to(el, { autoAlpha: 1, duration: still ? 0 : 0.15, overwrite: "auto" });
}

/** Fades a chart marker out. */
export function fadeAway(el: HTMLElement | null) {
  if (el) gsap.to(el, { autoAlpha: 0, duration: reduceMotion() ? 0 : 0.2, overwrite: "auto" });
}
