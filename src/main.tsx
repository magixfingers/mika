import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import App from "./App";
import { AlertPopup } from "./AlertPopup";
import { AttentionPopup } from "./AttentionPopup";
import { SettingsProvider } from "./settings";
import { autoHideScrollbars } from "./scrollbars";

autoHideScrollbars();

// The alert and attention popups load the same bundle at #alert and #attention.
const popup = window.location.hash.slice(1);
if (popup === "alert" || popup === "attention") document.documentElement.dataset.window = popup;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <SettingsProvider>
      {popup === "alert" ? <AlertPopup /> : popup === "attention" ? <AttentionPopup /> : <App />}
    </SettingsProvider>
  </React.StrictMode>,
);
