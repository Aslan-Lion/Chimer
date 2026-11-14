import { useEffect, useMemo } from "react";
import { remoteStore } from "../stores/remoteStore";
import { connectRemote, disconnectRemote, readPairCodeFromUrl } from "../services/remote";
import { PairingScreen } from "../components/remote/RemoteShell";
import { RemoteController } from "../components/remote/RemoteController";
import { RemoteDisplay } from "../components/remote/RemoteDisplay";

/**
 * Entry point for a browser that reached the host over the LAN.
 *
 * This is deliberately *not* `ControlApp`. A remote page can never render the
 * operator's dashboard, whatever it asks for: the only two surfaces reachable
 * from here are the read-only display and the permission-gated controller. The
 * host, not this page, decides what the device may do — so routing the wrong
 * way here could at worst show a page that no button would work on.
 *
 * The path selects the *intended* surface (`/remote/display` versus anything
 * else); the role the host assigned at pairing is authoritative and wins.
 */
export function RemoteApp() {
  const { phase, role } = remoteStore.use();

  const wantsDisplay = useMemo(
    () => window.location.pathname.replace(/\/+$/, "").endsWith("/remote/display"),
    [],
  );
  const initialCode = useMemo(() => readPairCodeFromUrl() ?? "", []);

  useEffect(() => {
    connectRemote(defaultDeviceName());
    return () => disconnectRemote();
  }, []);

  if (phase === "online") {
    return wantsDisplay || role === "display" ? <RemoteDisplay /> : <RemoteController />;
  }

  return <PairingScreen initialCode={initialCode} />;
}

/** A short, human label the host can show while asking the operator to approve. */
function defaultDeviceName(): string {
  const agent = navigator.userAgent;
  if (/iPad/i.test(agent)) return "iPad";
  if (/iPhone/i.test(agent)) return "iPhone";
  if (/Android/i.test(agent)) return /Mobile/i.test(agent) ? "Android phone" : "Android tablet";
  if (/Macintosh/i.test(agent)) return "Mac";
  if (/Windows/i.test(agent)) return "Windows browser";
  return "Browser";
}
