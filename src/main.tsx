import { TokenUnitProvider } from "./lib/tokenUnits";
import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import { QuickPanel } from "./pages/QuickPanel";
import { NotchBar } from "./pages/NotchBar";
import { I18nProvider } from "./lib/i18n";
import { ThemeProvider } from "./lib/theme";
import { SubscriptionsProvider } from "./lib/subscriptions";
import { IN_TAURI } from "./lib/api";
import "./index.css";

// Outside Tauri (browser preview / design QA) there is no webview label
// and no vibrancy behind the page; `?window=quick` / `?window=notch`
// selects the corresponding surface.
const isMac = navigator.userAgent.includes("Mac");
const windowLabel = IN_TAURI
  ? getCurrentWebviewWindow().label
  : new URLSearchParams(location.search).get("window") ?? "main";
const isQuickPanel = windowLabel === "quick";
const isNotchBar = windowLabel === "notch";

// macOS gets the vibrancy material treatment (transparent windows +
// translucent surface washes, see index.css); other platforms keep the
// solid palette.
if (isMac && IN_TAURI) {
  document.documentElement.classList.add("mac");
}

if (isQuickPanel) {
  document.documentElement.classList.add("quick-panel");
  // The quick window is only transparent on macOS (see setup_tray); on
  // Windows/Linux it is opaque, so the page must paint its own background.
  if (!isMac || !IN_TAURI) {
    document.documentElement.classList.add("quick-panel-opaque");
  }
}

// The notch window is a fully transparent stage; the bar paints itself.
if (isNotchBar) {
  document.documentElement.classList.add("notch-window");
}

async function start() {
  if (IN_TAURI && windowLabel === "main") {
    try {
      const preferences = await invoke<Record<string, string>>("get_legacy_preferences");
      for (const [key, value] of Object.entries(preferences)) {
        if (localStorage.getItem(key) === null) localStorage.setItem(key, value);
      }
      await invoke("acknowledge_legacy_preferences");
    } catch (error) {
      document.body.textContent = `无法导入旧版设置，请重新启动 Willow 重试。 / Could not import previous settings. Restart Willow to retry. ${String(error)}`;
      return;
    }
  }
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <I18nProvider>
        <TokenUnitProvider>
        <SubscriptionsProvider>
          {isNotchBar ? <NotchBar /> : isQuickPanel ? <QuickPanel /> : <App />}
        </SubscriptionsProvider>
        </TokenUnitProvider>
      </I18nProvider>
    </ThemeProvider>
  </React.StrictMode>,
  );
}

void start();
