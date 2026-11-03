import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { disable as disableAutostart, enable as enableAutostart, isEnabled as autostartEnabled } from "@tauri-apps/plugin-autostart";
import { useSettings } from "./settings";
import type { Settings } from "./types";

const SOUNDS = ["Basso", "Blow", "Bottle", "Frog", "Funk", "Glass", "Hero", "Morse", "Ping", "Pop", "Purr", "Sosumi", "Submarine", "Tink"];

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="setting">
      <div className="setting-text">
        <span className="setting-label">{label}</span>
        {hint && <span className="muted small">{hint}</span>}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}

function Toggle({ on, onChange, label }: { on: boolean; onChange: (on: boolean) => void; label: string }) {
  return <button className="toggle" role="switch" aria-checked={on} aria-label={label} onClick={() => onChange(!on)} />;
}

function Choice<T extends string | number>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div className="choice" role="radiogroup" aria-label={label}>
      {options.map(([v, text]) => (
        <button key={String(v)} role="radio" aria-checked={v === value} onClick={() => onChange(v)}>
          {text}
        </button>
      ))}
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="panel settings-section">
      <h2>{title}</h2>
      {children}
    </section>
  );
}

/** Claude Code hooks: shows whether they're installed, with install and remove buttons. */
function HooksRow() {
  const [installed, setInstalled] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const refresh = () => invoke<boolean>("hooks_status").then(setInstalled).catch((e) => setError(String(e)));
  useEffect(() => {
    refresh();
  }, []);
  const run = async (command: "install_hooks" | "remove_hooks") => {
    setBusy(true);
    setError(null);
    try {
      await invoke(command);
    } catch (e) {
      setError(String(e));
    }
    await refresh();
    setBusy(false);
  };
  return (
    <Row
      label="Claude Code hooks"
      hint={
        error ??
        (installed
          ? "Installed. New Claude Code sessions report their status and alerts to Mika."
          : "Not installed. Install to get live status and “needs you” alerts. Your settings.json is backed up first.")
      }
    >
      {installed === null ? null : installed ? (
        <button className="button" disabled={busy} onClick={() => run("remove_hooks")}>
          Remove
        </button>
      ) : (
        <button className="button primary" disabled={busy} onClick={() => run("install_hooks")}>
          Install
        </button>
      )}
    </Row>
  );
}

function LaunchAtLoginRow() {
  const [on, setOn] = useState<boolean | null>(null);
  useEffect(() => {
    autostartEnabled().then(setOn).catch(() => setOn(false));
  }, []);
  const change = async (next: boolean) => {
    await (next ? enableAutostart() : disableAutostart());
    setOn(await autostartEnabled());
  };
  return (
    <Row label="Launch at login" hint="Start Mika automatically when you log in to your Mac.">
      {on !== null && <Toggle on={on} onChange={change} label="Launch at login" />}
    </Row>
  );
}

export function SettingsPage() {
  const { settings: s, update } = useSettings();
  if (!s) return <p className="muted loading">Loading settings…</p>;
  const set = <K extends keyof Settings>(key: K) => (value: Settings[K]) => update({ [key]: value } as Partial<Settings>);

  return (
    <div className="settings">
      <Section title="Alerts">
        <Row label="Alert sound" hint="Played when an agent needs you.">
          <Toggle on={s.sound_enabled} onChange={set("sound_enabled")} label="Alert sound" />
        </Row>
        {s.sound_enabled && (
          <Row label="Sound">
            <select className="select" value={s.sound} onChange={(e) => set("sound")(e.target.value)} aria-label="Sound">
              {SOUNDS.map((name) => (
                <option key={name}>{name}</option>
              ))}
            </select>
            <button className="button" onClick={() => invoke("preview_sound", { name: s.sound })}>
              Play
            </button>
          </Row>
        )}
        <Row label="Bounce Dock icon" hint="Until you switch to Mika.">
          <Toggle on={s.dock_bounce} onChange={set("dock_bounce")} label="Bounce Dock icon" />
        </Row>
        <Row label="Alert when an agent finishes" hint="Not just when it needs you.">
          <Toggle on={s.alert_on_done} onChange={set("alert_on_done")} label="Alert when an agent finishes" />
        </Row>
        <Row label="Quiet hours" hint="No alert sound during these hours; the screen still shows alerts.">
          <Toggle on={s.quiet_hours} onChange={set("quiet_hours")} label="Quiet hours" />
        </Row>
        {s.quiet_hours && (
          <Row label="From">
            <input className="time" type="time" value={s.quiet_start} onChange={(e) => set("quiet_start")(e.target.value)} aria-label="Quiet from" />
            <span className="muted">to</span>
            <input className="time" type="time" value={s.quiet_end} onChange={(e) => set("quiet_end")(e.target.value)} aria-label="Quiet until" />
          </Row>
        )}
      </Section>

      <Section title="Display">
        <Row label="Default range" hint="What Mika shows when it opens.">
          <Choice
            label="Default range"
            value={s.default_range}
            onChange={set("default_range")}
            options={[["day", "Day"], ["month", "Month"], ["total", "Total"]]}
          />
        </Row>
        <Row label="Show cost estimates" hint="Based on public API prices, not your bill.">
          <Toggle on={s.show_cost} onChange={set("show_cost")} label="Show cost estimates" />
        </Row>
        <Row label="Animations">
          <Choice
            label="Animations"
            value={s.motion}
            onChange={set("motion")}
            options={[["full", "Full"], ["subtle", "Subtle"], ["off", "Off"]]}
          />
        </Row>
        <Row label="Menu bar title">
          <Choice
            label="Menu bar title"
            value={s.tray_title}
            onChange={set("tray_title")}
            options={[["full", "Full"], ["compact", "Compact"], ["icon", "Icon"]]}
          />
        </Row>
      </Section>

      <Section title="Data & privacy">
        <Row label="Track Claude Code">
          <Toggle on={s.track_claude} onChange={set("track_claude")} label="Track Claude Code" />
        </Row>
        <Row label="Track Codex">
          <Toggle on={s.track_codex} onChange={set("track_codex")} label="Track Codex" />
        </Row>
        <Row
          label="Claude plan limits"
          hint="Reads Claude Code's sign-in from the Keychain and asks Anthropic. Off means no Keychain access and no network."
        >
          <Toggle on={s.claude_limits} onChange={set("claude_limits")} label="Claude plan limits" />
        </Row>
        <Row label="Update speed" hint="How often CPU, memory and agents refresh.">
          <Choice
            label="Update speed"
            value={s.sample_interval_ms}
            onChange={set("sample_interval_ms")}
            options={[[1000, "Real-time"], [2000, "Balanced"], [5000, "Battery"]]}
          />
        </Row>
      </Section>

      <Section title="Integrations">
        <HooksRow />
      </Section>

      <Section title="General">
        <LaunchAtLoginRow />
      </Section>
    </div>
  );
}
