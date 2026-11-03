import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Mika } from "./Mika";
import type { AlertInfo } from "./types";
import "./App.css";
import "./AlertPopup.css";

function waitingFor(sinceMs: number, now: number): string {
  const s = Math.max(0, Math.floor((now - sinceMs) / 1000));
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
}

/** The "needs you" popup, rendered in its own small always-on-top window. */
export function AlertPopup() {
  const [info, setInfo] = useState<AlertInfo | null>(null);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    invoke<AlertInfo | null>("current_alert").then(setInfo);
    const unlisten = listen<AlertInfo>("alert", (e) => setInfo(e.payload));
    const tick = window.setInterval(() => setNow(Date.now()), 1000);
    // Enter opens the session, Escape dismisses, like a system alert.
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter") invoke("open_alert_target");
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
  const app = info.host_app?.split("/").pop()?.replace(/\.app$/, "");

  return (
    <div className="alert-card" role="alertdialog" aria-labelledby="alert-title" aria-describedby="alert-body">
      <div className="alert-head" data-tauri-drag-region>
        <Mika state="waiting" size={44} />
        <div className="alert-text" data-tauri-drag-region>
          <p className="alert-title" id="alert-title">
            {info.title}
          </p>
          <p className="alert-body" id="alert-body">
            {info.body}
          </p>
          <p className="alert-meta">
            {info.tool ? `${info.tool} · ` : ""}waiting {waitingFor(info.since_ms, now)}
          </p>
        </div>
      </div>
      <div className="alert-actions">
        <button className="alert-button" onClick={() => invoke("dismiss_alert")}>
          Dismiss
        </button>
        <button className="alert-button primary" onClick={() => invoke("open_alert_target")} autoFocus>
          {app ? `Open in ${app}` : "Open session"}
        </button>
      </div>
    </div>
  );
}
