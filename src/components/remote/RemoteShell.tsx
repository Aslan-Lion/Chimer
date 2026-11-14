import { type ReactNode, useEffect, useState } from "react";
import {
  AlertTriangle,
  Gamepad2,
  MonitorPlay,
  RefreshCw,
  ShieldAlert,
  Smartphone,
  Wifi,
  WifiOff,
} from "lucide-react";
import type { RemotePhase } from "../../stores/remoteStore";
import { remoteStore } from "../../stores/remoteStore";
import { Badge } from "../ui/Badge";
import type { BadgeTone } from "../ui/Badge";
import { Button } from "../ui/Button";
import { cn } from "../../lib/cn";
import { connectRemote, forgetRemoteDevice, pairRemote, retryRemote } from "../../services/remote";

/**
 * The frame every remote page is drawn in.
 *
 * A phone or tablet sees a single narrow column: a status strip, then the
 * surface. Nothing here is desktop chrome, so a LAN browser can never mistake
 * the remote page for the operator's control window.
 */

const PHASE_LABEL: Record<RemotePhase, string> = {
  idle: "Starting",
  connecting: "Connecting",
  pairing: "Not paired",
  "awaiting-approval": "Waiting for host approval",
  authenticating: "Approved — signing in",
  online: "Connected",
  failed: "Pairing failed",
  expired: "Code expired",
  revoked: "Access revoked",
};

const PHASE_TONE: Record<RemotePhase, BadgeTone> = {
  idle: "neutral",
  connecting: "neutral",
  pairing: "warning",
  "awaiting-approval": "warning",
  authenticating: "accent",
  online: "success",
  failed: "danger",
  expired: "warning",
  revoked: "danger",
};

/** A short status strip: connection state, host name and device role. */
function StatusBar() {
  const { phase, connected, server, role, device, reconnectAttempt } = remoteStore.use();

  return (
    <header className="flex shrink-0 flex-wrap items-center gap-x-2 gap-y-1.5 border-b border-border bg-[#0b0b0e] px-3 py-2.5 sm:px-4 sm:py-3">
      <span className="flex shrink-0 items-center gap-2 text-[13px] font-semibold text-text">
        {connected ? (
          <Wifi size={15} className="text-accent" />
        ) : (
          <WifiOff size={15} className="text-text-faint" />
        )}
        app.timer
      </span>

      <Badge tone={PHASE_TONE[phase]}>
        {phase === "connecting" && reconnectAttempt > 1
          ? `Reconnecting (${reconnectAttempt})`
          : PHASE_LABEL[phase]}
      </Badge>

      {role ? (
        <Badge tone={role === "controller" ? "accent" : "neutral"}>
          {role === "controller" ? <Gamepad2 size={11} /> : <MonitorPlay size={11} />}
          {role === "controller" ? "Controller" : "Display"}
        </Badge>
      ) : null}

      <span className="ml-auto min-w-0 max-w-full truncate text-[11px] text-text-faint sm:text-[12px]">
        {device?.name ?? server?.name ?? ""}
      </span>
    </header>
  );
}

/** A single, dismissable explanation of the most recent protocol problem. */
function Notice() {
  const { notice } = remoteStore.use();
  if (!notice) return null;
  return (
    <div className="flex items-start gap-2.5 rounded-xl border border-[#4a2020] bg-[#2a1414] px-3.5 py-3 text-[12px] leading-relaxed">
      <ShieldAlert size={15} className="mt-0.5 shrink-0 text-danger" />
      <p className="text-danger">{notice.message}</p>
    </div>
  );
}

/** The container passed to both remote surfaces. */
export function RemoteShell({ children }: { children: ReactNode }) {
  return (
    /*
     * The whole page is one viewport-tall column: the status strip is pinned
     * above a scrolling body. This matters on a phone, where the global
     * stylesheet disables document scrolling (`body { overflow: hidden }` is
     * there for the two native windows) — without a scroll container here every
     * control below the fold would be unreachable.
     */
    <div className="flex h-full flex-col overflow-hidden bg-[#050505] text-text">
      <StatusBar />
      {/*
       * One column on a phone, a wider single column on a tablet, and a broader
       * shell on a laptop or desktop. The surfaces inside decide their own
       * arrangement (see `RemoteController`), so widening the shell never just
       * stretches a phone layout. `overflow-x-hidden` guarantees no surface can
       * introduce a sideways scrollbar.
       */}
      <main className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto">
        <div className="mx-auto flex w-full max-w-[520px] flex-col gap-4 px-3.5 py-4 sm:max-w-[640px] sm:px-4 md:max-w-[760px] md:px-5 lg:max-w-[1000px] lg:gap-5 lg:px-6 lg:py-6">
          {children}
        </div>
      </main>
    </div>
  );
}

/**
 * The pairing screen: it is what a browser shows until the host has authorised
 * the device. Until then there is no timer, no library and no control — the
 * page has nothing to render because it has been given nothing.
 */
export function PairingScreen({ initialCode }: { initialCode: string }) {
  const { phase, notice, connected, hasCredentials, role } = remoteStore.use();
  const [code, setCode] = useState(initialCode);
  const [name, setName] = useState("");

  // A code arriving in the URL is redeemed automatically; the operator only has
  // to type one when the fragment is missing.
  useEffect(() => {
    if (initialCode) setCode(initialCode);
  }, [initialCode]);

  const valid = /^\d{4,8}$/.test(code);
  /** The host holds the request and a human has to approve it. */
  const waiting = phase === "awaiting-approval";
  /** The token is stored; the host's first snapshot has not arrived yet. */
  const signingIn = phase === "authenticating";
  /** The page is mid-handshake; the button reports what it is waiting for. */
  const pending = phase === "connecting" || signingIn;
  /** The attempt ended without a pairing, and the operator needs a way back. */
  const ended = phase === "failed" || phase === "expired";

  function submit() {
    if (!valid) return;
    // One entry point for every case: submitting while the socket is closed
    // opens it and sends the code the moment the host greets us.
    pairRemote(code, name);
  }

  function forget() {
    forgetRemoteDevice();
    connectRemote(name);
  }

  const submitLabel = waiting
    ? "Waiting for host approval"
    : signingIn
      ? "Approved — signing in…"
      : pending
        ? "Connecting…"
        : ended
          ? "Try again"
          : "Pair";

  return (
    <RemoteShell>
      <div className="mx-auto w-full max-w-[440px] rounded-2xl border border-border bg-[#101013] p-4 sm:p-5">
        <div className="flex items-center gap-2.5">
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border border-border bg-[#18181b] text-accent">
            <Smartphone size={17} />
          </span>
          <div className="min-w-0">
            <h1 className="text-[15px] font-semibold text-text">Pair this device</h1>
            <p className="text-[12px] text-text-muted">
              {role === "display"
                ? "This will become a read-only display."
                : "Ask the operator for the code shown on the host."}
            </p>
          </div>
        </div>

        {waiting || signingIn ? (
          <div
            className={cn(
              "mt-5 flex items-center gap-3 rounded-xl border px-4 py-3.5",
              waiting ? "border-[#4a4120] bg-[#241f10]" : "border-[#3a4270] bg-[#1b1e33]",
            )}
          >
            <RefreshCw
              size={16}
              className={cn("shrink-0 animate-spin", waiting ? "text-warning" : "text-accent")}
            />
            <div className="min-w-0">
              <p
                className={cn(
                  "text-[13px] font-medium",
                  waiting ? "text-warning" : "text-accent",
                )}
              >
                {waiting ? "Waiting for host approval" : "Approved — signing in"}
              </p>
              <p className="mt-0.5 text-[12px] text-text-muted">
                {waiting
                  ? "The request reached the host. app.timer is waiting for the operator to approve this device — keep this page open."
                  : "This device is paired. The host is sending its current state…"}
              </p>
            </div>
          </div>
        ) : (
          <div className="mt-5 flex flex-col gap-3">
            <label className="flex flex-col gap-1.5">
              <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
                Pairing code
              </span>
              <input
                value={code}
                onChange={(event) => setCode(event.currentTarget.value.replace(/\D/g, "").slice(0, 8))}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void submit();
                }}
                inputMode="numeric"
                autoComplete="one-time-code"
                placeholder="000000"
                className="h-14 w-full min-w-0 rounded-xl border border-border bg-[#18181b] text-center font-mono text-[22px] tracking-[0.3em] text-text outline-none focus:border-accent/70 sm:text-2xl sm:tracking-[0.4em]"
              />
            </label>

            <label className="flex flex-col gap-1.5">
              <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
                Device name (optional)
              </span>
              <input
                value={name}
                onChange={(event) => setName(event.currentTarget.value.slice(0, 48))}
                placeholder="e.g. Stage left iPad"
                className="h-11 rounded-xl border border-border bg-[#18181b] px-3 text-[16px] text-text outline-none focus:border-accent/70"
              />
            </label>

            <Button
              size="lg"
              block
              variant="primary"
              disabled={!valid || pending}
              onClick={() => void submit()}
            >
              {submitLabel}
            </Button>

            {ended ? (
              <button
                type="button"
                onClick={() => retryRemote()}
                className="inline-flex items-center justify-center gap-1.5 text-[12px] text-text-muted hover:text-text"
              >
                <RefreshCw size={13} />
                Try connecting again
              </button>
            ) : null}

            {!ended && !connected ? (
              <p className="text-center text-[11px] text-text-faint">
                Trying to reach the host over the local network…
              </p>
            ) : null}
          </div>
        )}

        <div className="mt-4">
          <Notice />
        </div>

        {hasCredentials && !waiting && !signingIn ? (
          <button
            type="button"
            onClick={() => void forget()}
            className="mt-4 inline-flex items-center gap-1.5 text-[12px] text-text-faint hover:text-text-muted"
          >
            <AlertTriangle size={13} />
            This browser holds an old device token — pair again instead
          </button>
        ) : null}
      </div>

      <p className={cn("mx-auto max-w-[440px] px-1 text-center text-[11px] leading-relaxed text-text-faint")}>
        {notice?.code === "revoked"
          ? "The host removed this device. Pair again to regain access."
          : "Pairing needs the operator to approve this device on the host. Nothing on the host changes until they do."}
      </p>
    </RemoteShell>
  );
}
