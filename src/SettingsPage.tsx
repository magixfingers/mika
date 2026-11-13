import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { disable as disableAutostart, enable as enableAutostart, isEnabled as autostartEnabled } from "@tauri-apps/plugin-autostart";
import { useSettings } from "./settings";
import { DEFAULT_GLASS, type GlassLook, type Settings } from "./types";

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

/** A range slider that previews while dragging and saves on release. */
function Slider({
  value,
  min = 0,
  max = 100,
  unit = "%",
  label,
  onPreview,
  onCommit,
}: {
  value: number;
  min?: number;
  max?: number;
  unit?: string;
  label: string;
  onPreview: (v: number) => void;
  onCommit: () => void;
}) {
  return (
    <>
      <input
        className="range"
        type="range"
        min={min}
        max={max}
        value={value}
        aria-label={label}
        onChange={(e) => onPreview(Number(e.target.value))}
        onPointerUp={onCommit}
        onKeyUp={onCommit}
        onBlur={onCommit}
      />
      <span className="range-value">
        {value}
        {unit}
      </span>
    </>
  );
}

const GLASS_SLIDERS: { key: Exclude<keyof GlassLook, "backdrop">; label: string; hint: string; max?: number; unit?: string }[] = [
  { key: "transparency", label: "Transparency", hint: "How much of the backdrop shows through." },
  { key: "frost", label: "Frost", hint: "Blur behind the glass.", max: 40, unit: " px" },
  { key: "refraction", label: "Refraction", hint: "How far the edges bend what's behind them." },
  { key: "shine", label: "Shine", hint: "The light catching the edges." },
  { key: "shadow", label: "Shadow", hint: "Depth under cards and controls." },
];

function GlassSettings() {
  const { settings, update, preview } = useSettings();
  if (!settings) return null;
  const look = settings.glass;
  const save = () => update({ glass: look });
  const isDefault = (Object.keys(DEFAULT_GLASS) as (keyof GlassLook)[]).every((k) => look[k] === DEFAULT_GLASS[k]);
  return (
    <>
      {GLASS_SLIDERS.map(({ key, label, hint, max, unit }) => (
        <Row key={key} label={label} hint={hint}>
          <Slider
            label={label}
            value={look[key]}
            max={max}
            unit={unit}
            onPreview={(v) => preview({ glass: { ...look, [key]: v } })}
            onCommit={save}
          />
        </Row>
      ))}
      <Row label="Backdrop" hint="The colours behind the glass.">
        <Choice
          label="Backdrop"
          value={look.backdrop}
          onChange={(backdrop) => update({ glass: { ...look, backdrop } })}
          options={[["pastel", "Pastel"], ["mono", "Mono"], ["ocean", "Ocean"], ["sunset", "Sunset"]]}
        />
      </Row>
      <Row label="Reset glass" hint="Back to the original look.">
        <button className="button" disabled={isDefault} onClick={() => update({ glass: DEFAULT_GLASS })}>
          Reset
        </button>
      </Row>
    </>
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

const UPDATE_HINT =
  "This version of Mika can answer permission requests from its popup. Update the hooks to turn that on; your file is backed up first.";

const HOOK_COPY = {
  claude: {
    label: "Claude Code hooks",
    on: "Installed. New Claude Code sessions tell Mika when they're thinking, working or waiting for you.",
    off: "Lets Mika see when Claude Code is thinking, working or waiting for you, and alert you. Your settings.json is backed up first.",
  },
  codex: {
    label: "Codex hooks",
    on: "Installed. Codex asks you once to trust them; after that, Codex tells Mika when it's working or needs your approval.",
    off: "Lets Mika see when Codex is working or waiting for your approval, and alert you. Your hooks.json is backed up first.",
  },
};

/** An agent's hooks: whether they're installed, with install and remove buttons. */
function HooksRow({ agent }: { agent: "claude" | "codex" }) {
  const copy = HOOK_COPY[agent];
  const [status, setStatus] = useState<"installed" | "outdated" | "missing" | null>(null);
  const installed = status === null ? null : status === "installed";
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const refresh = () => invoke<"installed" | "outdated" | "missing">("hooks_status", { agent }).then(setStatus).catch((e) => setError(String(e)));
  useEffect(() => {
    refresh();
  }, []);
  const run = async (command: "install_hooks" | "remove_hooks") => {
    setBusy(true);
    setError(null);
    try {
      await invoke(command, { agent });
    } catch (e) {
      setError(String(e));
    }
    await refresh();
    setBusy(false);
  };
  return (
    <Row
      label={copy.label}
      hint={error ?? (installed ? copy.on : status === "outdated" ? UPDATE_HINT : copy.off)}
    >
      {installed === null ? null : installed ? (
        <button className="button" disabled={busy} onClick={() => run("remove_hooks")}>
          Remove
        </button>
      ) : (
        <button className="button primary" disabled={busy} onClick={() => run("install_hooks")}>
          {status === "outdated" ? "Update" : "Install"}
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
        <Row label="Popup window" hint="A card on your desktop with a ringtone until you respond, and a button to open the session.">
          <Toggle on={s.alert_popup} onChange={set("alert_popup")} label="Popup window" />
        </Row>
        {s.alert_popup && (
          <Row
            label="Answer permission requests"
            hint="Allow or deny right in the popup, without switching to the terminal. Needs the latest hooks below."
          >
            <Toggle on={s.answer_from_mika} onChange={set("answer_from_mika")} label="Answer permission requests" />
          </Row>
        )}
        <Row label="Your turn" hint="When an agent has finished and is waiting for your next message. Nothing is blocked, so it never rings or pops up.">
          <Choice
            label="Your turn"
            value={s.turn_alert}
            onChange={set("turn_alert")}
            options={[["off", "Off"], ["status", "Show"], ["chime", "Chime"]]}
          />
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
        <Row label="Skin" hint="Liquid Glass needs WebGPU; without it Mika stays Classic.">
          <Choice
            label="Skin"
            value={s.skin}
            onChange={set("skin")}
            options={[["classic", "Classic"], ["liquid", "Liquid Glass"]]}
          />
        </Row>
        {s.skin === "liquid" && <GlassSettings />}
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
        <HooksRow agent="claude" />
        <HooksRow agent="codex" />
      </Section>

      <Section title="General">
        <LaunchAtLoginRow />
      </Section>
    </div>
  );
}
