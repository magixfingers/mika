import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Settings } from "./types";

type SettingsApi = {
  settings: Settings | null;
  update: (patch: Partial<Settings>) => Promise<void>;
  /** Shows a change right away without saving it (e.g. while dragging a slider). */
  preview: (patch: Partial<Settings>) => void;
};

const SettingsContext = createContext<SettingsApi>({ settings: null, update: async () => {}, preview: () => {} });

/** Loads settings from Rust, keeps them in sync, and saves changes through `update`. */
export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings | null>(null);

  useEffect(() => {
    invoke<Settings>("get_settings").then(setSettings);
    const unlisten = listen<Settings>("settings", (e) => setSettings(e.payload));
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Motion is read by CSS and GSAP helpers anywhere in the tree.
  useEffect(() => {
    if (settings) document.documentElement.dataset.motion = settings.motion;
  }, [settings?.motion]);

  const update = useCallback(
    async (patch: Partial<Settings>) => {
      if (!settings) return;
      const next = { ...settings, ...patch };
      setSettings(next);
      setSettings(await invoke<Settings>("set_settings", { settings: next }));
    },
    [settings],
  );

  const preview = useCallback((patch: Partial<Settings>) => {
    setSettings((current) => (current ? { ...current, ...patch } : current));
  }, []);

  return <SettingsContext.Provider value={{ settings, update, preview }}>{children}</SettingsContext.Provider>;
}

export const useSettings = () => useContext(SettingsContext);
