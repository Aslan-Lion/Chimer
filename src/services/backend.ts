import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

export interface AppInfo {
  name: string;
  version: string;
  runtime: string;
}

/**
 * True when the frontend is running inside the Tauri webview. During plain
 * `npm run dev` the UI can still be inspected in a browser, so every backend
 * call needs a deterministic fallback.
 */
export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * True inside the control window.
 *
 * Both native windows run the same bundle on the same machine, so any cue that
 * reaches both — a completion, a stage change — has to be claimed by exactly one
 * of them. The control window is that one; the presentation window only renders.
 * Outside the desktop shell there is a single window, which behaves as control.
 */
export function isControlWindow(): boolean {
  if (!isTauriRuntime()) return true;
  try {
    return getCurrentWindow().label === "main";
  } catch {
    return true;
  }
}

/** Reads application metadata straight from the Rust backend. */
export async function getAppInfo(): Promise<AppInfo> {
  if (!isTauriRuntime()) {
    return { name: "app.timer", version: "0.1.0", runtime: "browser-preview" };
  }
  return invoke<AppInfo>("app_info");
}
