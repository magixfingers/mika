// Thin scrollbars that only show while the mouse is active over Mika: a mouse
// move, click, wheel or touch reveals them; they fade after a short idle, and
// hide at once when the pointer leaves the window or Mika loses focus. Scrolling
// and key presses only count while the pointer is over the window, so content
// that scrolls by itself never brings them back.

const IDLE_MS = 1200;
/** The elements that scroll (App.css, "overflow-y: auto"). */
const SCROLLERS = ".page-settings, .page-overview, .page-agents";

// WebKit (the macOS WebView) doesn't repaint a scrollbar when only its
// ::-webkit-scrollbar-thumb style changes, so a thumb shown once stayed on
// screen until the next scroll. Briefly dropping the scrollbar and restoring it
// within one synchronous layout makes WebKit rebuild it with the new style;
// nothing is painted in between and the scroll position is kept.
function repaintScrollbars() {
  document.querySelectorAll<HTMLElement>(SCROLLERS).forEach((el) => {
    const previous = el.style.overflowY;
    el.style.overflowY = "hidden";
    void el.offsetWidth;
    el.style.overflowY = previous;
  });
}

export function autoHideScrollbars() {
  const root = document.documentElement;
  let timer = 0;

  const setActive = (active: boolean) => {
    if (root.classList.contains("scroll-active") === active) return;
    root.classList.toggle("scroll-active", active);
    repaintScrollbars();
  };
  const sleep = () => {
    window.clearTimeout(timer);
    setActive(false);
  };
  const wake = () => {
    setActive(true);
    window.clearTimeout(timer);
    timer = window.setTimeout(sleep, IDLE_MS);
  };

  let hovering = false;
  const pointer = () => {
    hovering = true;
    wake();
  };
  const whileHovering = () => {
    if (hovering) wake();
  };
  const leave = () => {
    hovering = false;
    sleep();
  };

  const events = ["mousemove", "mousedown", "wheel", "touchstart"] as const;
  events.forEach((e) => window.addEventListener(e, pointer, { passive: true }));
  window.addEventListener("keydown", whileHovering, { passive: true });
  // Scroll doesn't bubble, so listen in the capture phase to catch inner scrollers.
  window.addEventListener("scroll", whileHovering, { passive: true, capture: true });

  // Pointer left the window (no element it moved to) or Mika went to the background.
  document.addEventListener("mouseout", (e) => {
    if (!e.relatedTarget) leave();
  });
  document.documentElement.addEventListener("mouseleave", leave);
  window.addEventListener("blur", leave);
}
