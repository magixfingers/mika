import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Mika } from "./Mika";
import { timeAgo } from "./format";
import type { AttentionItem } from "./types";
import "./App.css";
import "./AttentionPopup.css";

const AGENT_NAMES = { claude: "Claude Code", codex: "Codex" } as const;
const VERBS: Record<AttentionItem["state"], string> = {
  waiting: "needs you",
  turn: "your turn",
  done: "finished",
};

/** The list under the menu-bar ghost: every agent that finished, handed the
 *  turn back or is blocked. Clicking one goes to its terminal tab or app. */
export function AttentionPopup() {
  const [items, setItems] = useState<AttentionItem[]>([]);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    invoke<AttentionItem[]>("attention_list").then(setItems);
    const unlisten = listen<AttentionItem[]>("attention", (e) => setItems(e.payload));
    const tick = window.setInterval(() => setNow(Date.now()), 15_000);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") getCurrentWindow().hide();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      unlisten.then((off) => off());
      window.clearInterval(tick);
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  // Most urgent first (blocked, then your turn, then finished), newest first within each.
  const order = { waiting: 0, turn: 1, done: 2 };
  const sorted = [...items].sort((a, b) => order[a.state] - order[b.state] || b.since_ms - a.since_ms);

  return (
    <div className="attention">
      <p className="eyebrow attention-eyebrow">
        {items.length} agent{items.length === 1 ? "" : "s"} waiting on you
      </p>
      <ul className="attention-list">
        {sorted.map((item, i) => (
          <li key={item.session_id}>
            <button
              className="attention-row"
              data-state={item.state}
              autoFocus={i === 0}
              onClick={() => invoke("open_attention", { sessionId: item.session_id })}
              title={`Open ${AGENT_NAMES[item.agent]} in ${item.folder}`}
            >
              <Mika state={item.state} size={24} />
              <span className="attention-text">
                <span className="attention-title">
                  {AGENT_NAMES[item.agent]} <span className="attention-verb">{VERBS[item.state]}</span>
                </span>
                <span className="attention-meta">
                  {item.folder} · {timeAgo(item.since_ms, now)}
                </span>
              </span>
              <span className="attention-go" aria-hidden>
                ›
              </span>
            </button>
          </li>
        ))}
      </ul>
      <div className="attention-footer">
        <button className="attention-link" onClick={() => invoke("open_main_from_attention")}>
          Open Mika
        </button>
        <button className="attention-link" onClick={() => invoke("clear_attention")}>
          Clear all
        </button>
      </div>
    </div>
  );
}
