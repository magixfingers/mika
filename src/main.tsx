import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import App from "./App";
import { AlertPopup } from "./AlertPopup";
import { SettingsProvider } from "./settings";
import { autoHideScrollbars } from "./scrollbars";

autoHideScrollbars();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <SettingsProvider>
      {/* The alert window loads the same bundle at #alert. */}
      {window.location.hash === "#alert" ? <AlertPopup /> : <App />}
    </SettingsProvider>
  </React.StrictMode>,
);
