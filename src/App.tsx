import { useEffect } from "react";
import { AppRouter } from "./app/AppRouter";
import { ErrorBoundary } from "./components/ui/ErrorBoundary";
import { settingsStore } from "./stores/settingsStore";
import { startSessionBridge } from "./stores/sessionStore";
import { startTimerBridge } from "./stores/timerStore";
import { startNetworkBridge } from "./stores/networkStore";
import { startNotificationBridge } from "./services/notifications";

/**
 * Application root. Both native windows mount this component; the router
 * decides whether the control or the presentation interface is rendered.
 */
export default function App() {
  const { settings } = settingsStore.use();

  // The accent token is consumed by Tailwind utilities, so overriding the CSS
  // variable re-themes both windows instantly without a rebuild.
  useEffect(() => {
    document.documentElement.style.setProperty(
      "--color-accent",
      settings.appearance.accentColor,
    );
  }, [settings.appearance.accentColor]);

  // Both windows subscribe to the Rust timer engine and recover the current
  // snapshot on start-up, which is also how a countdown survives a restart: the
  // engine restores it from disk and hands back the same state here. The session
  // bridge is attached the same way, so a running session is recovered too.
  useEffect(() => {
    let cancelled = false;
    const disposers: Array<() => void> = [];

    void (async () => {
      const timer = await startTimerBridge();
      const session = await startSessionBridge();
      // Started after the timer bridge so the first snapshot is the baseline
      // rather than a completion: a countdown restored from disk does not
      // announce itself again on every launch. The bridge claims the window's
      // own notification cues, so nothing is played twice.
      const notifications = startNotificationBridge();
      // The network bridge mirrors the host engine into the control window and
      // keeps the library mirror in step. It is attached after the timer and
      // session bridges so a library publish reads settled state.
      const network = await startNetworkBridge();
      if (cancelled) {
        timer();
        session();
        notifications();
        network();
      } else {
        disposers.push(timer, session, notifications, network);
      }
    })();

    return () => {
      cancelled = true;
      disposers.forEach((dispose) => dispose());
    };
  }, []);

  // Last line of defence: a render failure anywhere in the routed tree is
  // reported in place instead of unmounting the root and leaving a blank window.
  return (
    <ErrorBoundary label="app.timer">
      <AppRouter />
    </ErrorBoundary>
  );
}
