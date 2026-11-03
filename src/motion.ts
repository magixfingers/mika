// GSAP-driven motion. Every effect is skipped when macOS "Reduce motion" is on
// or motion is Off in settings; Subtle keeps only small, functional motion.

import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import gsap from "gsap";

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

/** Rises and fades in the matching children of `scope`, staggered, whenever `key` changes. */
export function useEntrance(scope: RefObject<HTMLElement | null>, selector: string, key: unknown) {
  useLayoutEffect(() => {
    const root = scope.current;
    if (!root || motionLevel() !== "full") return;
    const targets = [...root.querySelectorAll<HTMLElement>(selector)].filter((el) => el.offsetParent !== null);
    if (targets.length === 0) return;
    const ctx = gsap.context(() => {
      gsap.from(targets, { y: 14, opacity: 0, duration: 0.55, ease: "power3.out", stagger: 0.06, clearProps: "transform,opacity" });
    }, root);
    return () => ctx.revert();
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
