import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import App from "./App";
import { AlertPopup } from "./AlertPopup";
import { SettingsProvider } from "./settings";
import { autoHideScrollbars } from "./scrollbars";

autoHideScrollbars();

const isAlert = window.location.hash === "#alert";
if (isAlert) document.documentElement.dataset.window = "alert";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <SettingsProvider>
      {/* The alert window loads the same bundle at #alert. */}
      {isAlert ? <AlertPopup /> : <App />}
    </SettingsProvider>
  </React.StrictMode>,
);
