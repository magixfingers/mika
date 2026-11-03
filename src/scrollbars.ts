// Thin scrollbars that only show while the user is active: any mouse move,
// wheel, scroll, touch or key press reveals them; they fade after a short idle.

const IDLE_MS = 1200;

export function autoHideScrollbars() {
  const root = document.documentElement;
  let timer = 0;
  const wake = () => {
    root.classList.add("scroll-active");
    window.clearTimeout(timer);
    timer = window.setTimeout(() => root.classList.remove("scroll-active"), IDLE_MS);
  };
  const events = ["mousemove", "mousedown", "wheel", "keydown", "touchstart"] as const;
  events.forEach((e) => window.addEventListener(e, wake, { passive: true }));
  // Scroll doesn't bubble, so listen in the capture phase to catch inner scrollers.
  window.addEventListener("scroll", wake, { passive: true, capture: true });
}
