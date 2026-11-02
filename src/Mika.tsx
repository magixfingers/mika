import type { AgentState } from "./types";
import "./Mika.css";

export const STATE_LABELS: Record<AgentState, string> = {
  idle: "Idle",
  done: "Done",
  thinking: "Thinking",
  working: "Working",
  waiting: "Needs you",
};

/** Mika, the status character. Each state has its own pose and motion. */
export function Mika({ state, size = 56 }: { state: AgentState; size?: number }) {
  const happy = state === "done";
  const sleepy = state === "idle";
  return (
    <svg
      className={`mika mika-${state}`}
      width={size}
      height={size}
      viewBox="0 0 64 64"
      role="img"
      aria-label={`Agent status: ${STATE_LABELS[state]}`}
    >
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
      {state === "thinking" && (
        <g className="mika-dots">
          <circle cx="46" cy="10" r="2" />
          <circle cx="53" cy="6" r="2.5" />
          <circle cx="60" cy="3" r="3" />
        </g>
      )}
      {state === "waiting" && <text className="mika-alert" x="52" y="16">!</text>}
    </svg>
  );
}
