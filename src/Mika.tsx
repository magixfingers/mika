import { useEffect, useRef } from "react";
import gsap from "gsap";
import { motionLevel } from "./motion";
import { useSettings } from "./settings";
import type { AgentState } from "./types";
import "./Mika.css";

export const STATE_LABELS: Record<AgentState, string> = {
  idle: "Idle",
  done: "Done",
  thinking: "Thinking",
  working: "Working",
  turn: "Your turn",
  waiting: "Needs you",
};

/** The character's animatable parts, found inside its <svg>. */
type Parts = {
  body: SVGGElement;
  eyes: SVGGElement;
  dots: SVGCircleElement[];
  question: SVGTextElement;
  exclaim: SVGTextElement;
  ring: SVGCircleElement;
  sparks: SVGPathElement[];
  zzz: SVGTextElement;
};

type Clip = (p: Parts) => gsap.core.Timeline;

const tl = () => gsap.timeline();
const BODY = { transformOrigin: "50% 100%" };
const CENTER = { transformOrigin: "50% 50%" };

function hop(p: Parts, height = 6) {
  return tl()
    .to(p.body, { ...BODY, y: -height, scaleX: 0.95, scaleY: 1.06, duration: 0.2, ease: "power2.out" })
    .to(p.body, { y: 0, scaleX: 1.07, scaleY: 0.9, duration: 0.16, ease: "power2.in" })
    .to(p.body, { scaleX: 1, scaleY: 1, duration: 0.22, ease: "back.out(3)" });
}

function burst(p: Parts) {
  const t = tl();
  p.sparks.forEach((s, i) => {
    const angle = (i / p.sparks.length) * Math.PI * 2 - Math.PI / 2;
    t.fromTo(
      s,
      { ...CENTER, x: 0, y: 0, scale: 0, opacity: 1 },
      { x: Math.cos(angle) * 22, y: Math.sin(angle) * 22, scale: 1, opacity: 0, duration: 0.7, ease: "power2.out" },
      i * 0.03,
    );
  });
  return t;
}

const blink = (p: Parts) =>
  tl().to(p.eyes, { ...CENTER, scaleY: 0.1, duration: 0.07, yoyo: true, repeat: 1, ease: "power1.inOut" });

// A library of short clips per state; Mika plays a random one, rests, then another.
const CLIPS: Record<AgentState, Clip[]> = {
  working: [
    (p) => tl().add(hop(p)).add(hop(p, 4)).add(hop(p)),
    // Typing: eyes down at the keyboard, quick little shuffles.
    (p) =>
      tl()
        .to(p.eyes, { y: 3, duration: 0.2 })
        .to(p.body, { x: 1.5, duration: 0.06, yoyo: true, repeat: 9, ease: "none" })
        .to(p.eyes, { y: 0, duration: 0.2 }),
    // Spin with a squash on landing.
    (p) =>
      tl()
        .to(p.body, { ...CENTER, rotation: 360, duration: 0.8, ease: "power2.inOut" })
        .set(p.body, { rotation: 0 })
        .add(hop(p, 3)),
    // Dash: leaning into quick steps one way, then the other.
    (p) =>
      tl()
        .to(p.body, { ...BODY, x: -4, rotation: -8, duration: 0.25, ease: "power2.out" })
        .to(p.body, { x: 4, rotation: 8, duration: 0.35, ease: "power2.inOut", yoyo: true, repeat: 2 })
        .to(p.body, { x: 0, rotation: 0, duration: 0.25, ease: "power2.out" }),
    (p) => tl().add(hop(p, 8)).add(burst(p), "-=0.3"),
  ],
  thinking: [
    // Thought dots rise one by one while the eyes look up and away.
    (p) =>
      tl()
        .to(p.eyes, { x: 2, y: -2, duration: 0.3 })
        .fromTo(p.dots, { opacity: 0, y: 4 }, { opacity: 1, y: 0, duration: 0.25, stagger: 0.25 }, "<")
        .to(p.dots, { opacity: 0, duration: 0.3, stagger: 0.1 }, "+=0.8")
        .to(p.eyes, { x: 0, y: 0, duration: 0.3 }, "<"),
    // Scanning left and right.
    (p) =>
      tl()
        .to(p.eyes, { x: -3, duration: 0.35, ease: "power2.inOut" })
        .to(p.eyes, { x: 3, duration: 0.5, ease: "power2.inOut" })
        .to(p.eyes, { x: 0, duration: 0.35, ease: "power2.inOut" })
        .add(blink(p)),
    // Head tilt with a question mark.
    (p) =>
      tl()
        .to(p.body, { ...BODY, rotation: -10, duration: 0.35, ease: "power2.out" })
        .fromTo(p.question, { ...CENTER, opacity: 0, scale: 0.4, y: 4 }, { opacity: 1, scale: 1, y: 0, duration: 0.35, ease: "back.out(3)" }, "<")
        .to(p.question, { opacity: 0, y: -4, duration: 0.3 }, "+=0.9")
        .to(p.body, { rotation: 0, duration: 0.4, ease: "power2.inOut" }, "<"),
    // Slow float, deep in thought.
    (p) => tl().to(p.body, { y: -4, duration: 0.9, ease: "sine.inOut", yoyo: true, repeat: 1 }),
  ],
  waiting: [
    (p) =>
      tl().to(p.body, {
        ...BODY,
        keyframes: { rotation: [0, -9, 8, -5, 3, 0] },
        duration: 0.8,
        ease: "power1.inOut",
      }),
    // Jump up with a bouncing "!".
    (p) =>
      tl()
        .add(hop(p, 9))
        .fromTo(p.exclaim, { ...BODY, opacity: 0, scale: 0.3 }, { opacity: 1, scale: 1, duration: 0.4, ease: "back.out(4)" }, 0.1)
        .to(p.exclaim, { y: -3, duration: 0.18, yoyo: true, repeat: 3 })
        .to(p.exclaim, { opacity: 0, duration: 0.25 }),
    // Radar ring pulses out, like a ping.
    (p) =>
      tl()
        .fromTo(p.ring, { ...CENTER, scale: 0.7, opacity: 0.8 }, { scale: 1.7, opacity: 0, duration: 0.9, ease: "power2.out", repeat: 1 })
        .to(p.body, { ...CENTER, scale: 1.06, duration: 0.15, yoyo: true, repeat: 3 }, 0),
    // Knock knock.
    (p) =>
      tl()
        .to(p.body, { x: 3, duration: 0.07, yoyo: true, repeat: 3, ease: "power1.inOut" })
        .to(p.body, { x: 3, duration: 0.07, yoyo: true, repeat: 3, ease: "power1.inOut" }, "+=0.25")
        .fromTo(p.exclaim, { ...BODY, opacity: 0, scale: 0.5 }, { opacity: 1, scale: 1, duration: 0.3, ease: "back.out(3)" }, 0)
        .to(p.exclaim, { opacity: 0, duration: 0.25 }, "+=0.3"),
  ],
  // Your turn: calm, with a curious look now and then. No alarm.
  turn: [
    (p) => tl().to(p.body, { ...BODY, scaleY: 1.03, scaleX: 0.98, duration: 1.4, ease: "sine.inOut", yoyo: true, repeat: 1 }),
    (p) =>
      tl()
        .to(p.body, { ...BODY, rotation: -8, duration: 0.35, ease: "power2.out" })
        .fromTo(p.question, { ...CENTER, opacity: 0, scale: 0.4, y: 4 }, { opacity: 1, scale: 1, y: 0, duration: 0.35, ease: "back.out(3)" }, "<")
        .to(p.question, { opacity: 0, y: -4, duration: 0.3 }, "+=1.2")
        .to(p.body, { rotation: 0, duration: 0.4, ease: "power2.inOut" }, "<"),
    blink,
  ],
  done: [
    (p) =>
      tl()
        .fromTo(p.body, { ...BODY, scaleX: 1.15, scaleY: 0.85 }, { scaleX: 1, scaleY: 1, duration: 0.7, ease: "elastic.out(1, 0.4)" })
        .add(burst(p), 0.1),
    // Happy nod.
    (p) => tl().to(p.body, { ...BODY, rotation: 6, y: 1, duration: 0.18, yoyo: true, repeat: 3, ease: "sine.inOut" }),
  ],
  idle: [
    (p) => tl().to(p.body, { ...BODY, scaleY: 1.03, scaleX: 0.98, duration: 1.8, ease: "sine.inOut", yoyo: true, repeat: 1 }),
    blink,
    // Dozing: a little "z" drifts up.
    (p) =>
      tl()
        .fromTo(p.zzz, { opacity: 0, x: 0, y: 0, scale: 0.6 }, { opacity: 1, x: 4, y: -8, scale: 1, duration: 1, ease: "sine.out" })
        .to(p.zzz, { opacity: 0, x: 8, y: -14, duration: 0.8, ease: "sine.in" }),
  ],
};

// Rest between clips; idle rests longer so it reads as calm.
const REST: Record<AgentState, [number, number]> = {
  working: [0.15, 0.6],
  thinking: [0.3, 0.9],
  waiting: [0.2, 0.6],
  turn: [1.2, 3],
  done: [0.8, 1.6],
  idle: [1.5, 4],
};

/**
 * Mika, the status character. Each state has a set of short animations that
 * play in random order. Small instances (in lists) stick to the first, gentlest
 * clip so a column of them stays calm.
 */
export function Mika({ state, size = 56 }: { state: AgentState; size?: number }) {
  const svg = useRef<SVGSVGElement>(null);
  const motion = useSettings().settings?.motion;
  const happy = state === "done";
  const sleepy = state === "idle";

  useEffect(() => {
    const root = svg.current;
    const level = motionLevel();
    if (!root || level === "off") return;
    const q = <T extends Element>(sel: string) => root.querySelector<T>(sel)!;
    const parts: Parts = {
      body: q(".mika-body"),
      eyes: q(".mika-eyes"),
      dots: [...root.querySelectorAll<SVGCircleElement>(".mika-dots circle")],
      question: q(".mika-question"),
      exclaim: q(".mika-alert"),
      ring: q(".mika-ring"),
      sparks: [...root.querySelectorAll<SVGPathElement>(".mika-spark")],
      zzz: q(".mika-zzz"),
    };
    // Small copies and Subtle motion stick to the first, gentlest clip.
    const clips = size < 30 || level === "subtle" ? CLIPS[state].slice(0, 1) : CLIPS[state];
    const [restMin, restMax] = REST[state];
    let current: gsap.core.Timeline | null = null;
    let timer: gsap.core.Tween | null = null;
    let lastIndex = -1;

    const playNext = () => {
      let i = Math.floor(Math.random() * clips.length);
      if (clips.length > 1 && i === lastIndex) i = (i + 1) % clips.length;
      lastIndex = i;
      current = clips[i](parts);
      current.eventCallback("onComplete", () => {
        timer = gsap.delayedCall(gsap.utils.random(restMin, restMax), playNext);
      });
    };
    playNext();

    return () => {
      current?.kill();
      timer?.kill();
      // Reset every part so the next state starts from the neutral pose.
      gsap.set([parts.body, parts.eyes], { clearProps: "transform" });
      gsap.set([...parts.dots, parts.question, parts.exclaim, parts.ring, ...parts.sparks, parts.zzz], {
        clearProps: "transform",
        opacity: 0,
      });
    };
  }, [state, size, motion]);

  return (
    <svg
      ref={svg}
      className={`mika mika-${state}`}
      width={size}
      height={size}
      viewBox="0 0 64 64"
      role="img"
      aria-label={`Agent status: ${STATE_LABELS[state]}`}
    >
      <circle className="mika-ring mika-prop" cx="32" cy="34" r="26" />
      <g className="mika-body">
        <path className="mika-shape" d="M32 6c14 0 24 11 24 26v20c0 4-3 6-6 4l-4-3-5 4-5-4-4 4-4-4-5 4-5-4-4 3c-3 2-6 0-6-4V32C8 17 18 6 32 6z" />
        <g className="mika-eyes">
          {happy ? (
            <>
              <path d="M21 31q3-4 6 0" />
              <path d="M37 31q3-4 6 0" />
            </>
          ) : sleepy ? (
            <>
              <path d="M21 31h6" />
              <path d="M37 31h6" />
            </>
          ) : (
            <>
              <ellipse cx="24" cy="30" rx="3" ry="4" />
              <ellipse cx="40" cy="30" rx="3" ry="4" />
            </>
          )}
        </g>
      </g>
      <g className="mika-dots">
        <circle className="mika-prop" cx="46" cy="10" r="2" />
        <circle className="mika-prop" cx="53" cy="6" r="2.5" />
        <circle className="mika-prop" cx="60" cy="3" r="3" />
      </g>
      <text className="mika-question mika-prop" x="50" y="14">?</text>
      <text className="mika-alert mika-prop" x="52" y="16">!</text>
      <text className="mika-zzz mika-prop" x="48" y="14">z</text>
      {[0, 1, 2, 3, 4, 5].map((i) => (
        <path key={i} className="mika-spark mika-prop" d="M32 16l1.2 2.8 2.8 1.2-2.8 1.2L32 24l-1.2-2.8L28 20l2.8-1.2z" />
      ))}
    </svg>
  );
}
