import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Mika } from "./Mika";
import type { AlertInfo, PermissionChoice } from "./types";
import "./App.css";
import "./AlertPopup.css";

function waitingFor(sinceMs: number, now: number): string {
  const s = Math.max(0, Math.floor((now - sinceMs) / 1000));
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
}

/** Shell commands that can destroy work or reach outside the project. Flagged
 *  in red, and never offered as "Always allow". A hint, not a guarantee. */
const RISKY = [
  /\brm\s+(-\w*[rf]\w*\s+)+/,
  /\bsudo\b/,
  /\bgit\s+push\b.*(--force\b|-f\b)/,
  /\bgit\s+(reset\s+--hard|clean\s+-\w*f)/,
  /\b(curl|wget)\b[^|]*\|\s*(ba|z)?sh\b/,
  /\bchmod\s+-R\b|\bchown\s+-R\b/,
  /\b(mkfs|dd\s+if=)/,
  />\s*\/dev\/(disk|sd)/,
];
export const isRisky = (tool: string | null, detail: string | null) =>
  tool === "Bash" && !!detail && RISKY.some((re) => re.test(detail));

/** Ignore clicks this soon after a new request appears, so a click meant for
 *  another window can't approve something the user hasn't read. */
const ARM_MS = 600;

/** The alert popup ("needs you", "finished", or a permission request to answer),
 *  in its own small always-on-top window. Clicking the message goes to the session. */
export function AlertPopup() {
  const [info, setInfo] = useState<AlertInfo | null>(null);
  const [now, setNow] = useState(Date.now());
  const [armed, setArmed] = useState(true);
  const kindRef = useRef<AlertInfo["kind"] | null>(null);
  kindRef.current = info?.kind ?? null;

  // Re-arm the buttons for each new permission request.
  const requestId = info?.request_id ?? null;
  useEffect(() => {
    if (requestId === null) return;
    setArmed(false);
    const timer = window.setTimeout(() => setArmed(true), ARM_MS);
    return () => window.clearTimeout(timer);
  }, [requestId]);

  useEffect(() => {
    invoke<AlertInfo | null>("current_alert").then(setInfo);
    const unlisten = listen<AlertInfo>("alert", (e) => setInfo(e.payload));
    const tick = window.setInterval(() => setNow(Date.now()), 1000);
    // Enter opens the session, Escape dismisses, like a system alert. The popup
    // takes focus when it appears, so Enter never approves a permission request:
    // a keystroke meant for the editor could land here.
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter" && kindRef.current !== "permission") invoke("open_alert_target");
      if (e.key === "Escape") invoke("dismiss_alert");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      unlisten.then((fn) => fn());
      window.clearInterval(tick);
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  if (!info) return null;
  // Herdr runs agents in its "Herdr Core" helper; the app the user sees is Herdr.
  const app = info.host_app?.split("/").pop()?.replace(/\.app$/, "").replace(/ Core$/, "");
  const done = info.kind === "done";
  const asking = info.kind === "permission" && info.request_id !== null;
  const risky = asking && isRisky(info.tool, info.detail);
  const open = () => invoke("open_alert_target");
  const answer = (choice: PermissionChoice) => {
    if (armed) invoke("answer_permission", { id: info.request_id, choice });
  };

  return (
    <div className="alert-shell" data-tauri-drag-region>
      <div className="alert-card" data-kind={info.kind} role="alertdialog" aria-labelledby="alert-title" aria-describedby="alert-body">
        <div className="alert-eyebrow" data-tauri-drag-region>
          <span className="alert-label">
            <span className="alert-dot" aria-hidden />
            {done ? "Finished" : asking ? "Wants permission" : "Needs you"}
          </span>
          <span className="alert-time">{waitingFor(info.since_ms, now)}</span>
        </div>
        {/* The whole message is a click target; the eyebrow above stays the drag handle. */}
        <div className="alert-head" onClick={open} title={app ? `Go to the session in ${app}` : "Go to the session"}>
          <Mika state={done ? "done" : "waiting"} size={40} />
          <div className="alert-text">
            <p className="alert-title" id="alert-title">
              {info.title}
            </p>
            <p className="alert-body" id="alert-body">
              {info.body}
              {info.tool && !asking && <span className="alert-tool">{info.tool}</span>}
            </p>
          </div>
        </div>
        {asking && info.detail && (
          <div className="alert-detail" data-risky={risky || undefined}>
            {risky && <span className="alert-risk">Risky</span>}
            <code>{info.detail}</code>
          </div>
        )}
        {asking ? (
          <div className="alert-actions">
            <button className="alert-button ghost" onClick={open} title={app ? `Answer in ${app}` : "Answer in the terminal"}>
              {app ? `In ${app}` : "In terminal"}
            </button>
            <span className="alert-spacer" />
            <button className="alert-button" disabled={!armed} onClick={() => answer("deny")}>
              Deny
            </button>
            {info.can_always && !risky && (
              <button className="alert-button" disabled={!armed} onClick={() => answer("always")} title="Allow, and don't ask again for this">
                Always allow
              </button>
            )}
            <button className="alert-button primary" disabled={!armed} onClick={() => answer("allow")}>
              Allow
            </button>
          </div>
        ) : (
          <div className="alert-actions">
            <button className="alert-button" onClick={() => invoke("dismiss_alert")}>
              Dismiss
            </button>
            <button className="alert-button primary" onClick={open}>
              {app ? `Open in ${app}` : "Open session"}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
