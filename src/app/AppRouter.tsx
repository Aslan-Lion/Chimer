import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauriRuntime } from "../services/backend";
import { DISPLAY_WINDOW_LABEL } from "../services/display";
import { ControlApp } from "./ControlApp";
import { DisplayApp } from "./DisplayApp";
import { RemoteApp } from "./RemoteApp";

/**
 * Single entry point shared by every webview.
 *
 * The control window and the display window run the exact same bundle; the
 * native window label decides which interface is mounted. This is what lets
 * Milestone 2 add the second window without a second build target.
 *
 * A *browser* is the third case: when this page is served over the LAN by the
 * Milestone 6 networking server there is no Tauri shell, so the label check
 * would wrongly fall through to `ControlApp` and hand a LAN visitor the
 * operator's dashboard. A non-Tauri runtime is therefore routed to `RemoteApp`
 * first, and can only ever reach the remote controller or the remote display.
 */
export function AppRouter() {
  if (!isTauriRuntime()) {
    return <RemoteApp />;
  }

  const label = getCurrentWindow().label;

  if (label === DISPLAY_WINDOW_LABEL) {
    return <DisplayApp />;
  }

  return <ControlApp />;
}
