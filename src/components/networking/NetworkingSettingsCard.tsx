import { useEffect, useState } from "react";
import {
  Check,
  Copy,
  Laptop,
  MonitorSmartphone,
  Power,
  QrCode,
  RefreshCw,
  ShieldAlert,
  Smartphone,
  Tablet,
  Wifi,
  WifiOff,
} from "lucide-react";
import type { DeviceKind, DeviceRole } from "../../types";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Card, CardHeader } from "../ui/Card";
import { NumberField } from "../ui/NumberField";
import { Toggle } from "../ui/Toggle";
import { cn } from "../../lib/cn";
import {
  approveRequest,
  forgetPairing,
  issuePairing,
  networkStore,
  refreshNetwork,
  rejectRequest,
  startServer,
  stopServer,
} from "../../stores/networkStore";

const SELECT_CLASS =
  "h-10 w-full appearance-none rounded-xl border border-border bg-elevated px-3 text-[13px] text-text outline-none transition-colors duration-150 focus:border-accent/70";

const DEFAULT_PORT = 8722;
const MIN_PORT = 1024;

/** Operator guidance keyed by the stable error code the engine returns. */
const ERROR_GUIDANCE: Record<string, string> = {
  "invalid-port": `Ports below ${MIN_PORT} need administrator rights. Use ${MIN_PORT} or above.`,
  "port-in-use": "Another program is using this port. Choose a different one.",
  "permission-denied":
    "Windows blocked the port. Try one above 1024, or allow app.timer through Windows Firewall.",
  "address-unavailable": "That interface is no longer available. Refresh and pick another.",
  "no-address": "No usable network interface was found. Join a Wi-Fi or Ethernet network first.",
};

/** A coarse icon for the device family the host inferred from the user agent. */
function KindIcon({ kind, size = 15 }: { kind: DeviceKind; size?: number }) {
  if (kind === "phone") return <Smartphone size={size} />;
  if (kind === "tablet") return <Tablet size={size} />;
  if (kind === "desktop") return <Laptop size={size} />;
  return <MonitorSmartphone size={size} />;
}

/** Renders `seconds` remaining as `m:ss`; `null` when the code has expired. */
function remainingSeconds(expiresInMs: number, issuedAt: number, now: number): number {
  const elapsed = now - issuedAt;
  return Math.max(0, Math.ceil((expiresInMs - elapsed) / 1000));
}

/** The Local Networking section: server control, pairing code and approvals. */
export function NetworkingSettingsCard() {
  const { status, pending, pairing, pairingIssuedAt, busy, error } = networkStore.use();

  const [port, setPort] = useState(DEFAULT_PORT);
  const [addressChoice, setAddressChoice] = useState("auto");
  const [bindAll, setBindAll] = useState(false);
  const [seeded, setSeeded] = useState(false);
  const [copied, setCopied] = useState(false);
  const [now, setNow] = useState(() => Date.now());

  // Seed the draft once from the first status so a later stop/start does not
  // overwrite what the operator is typing.
  useEffect(() => {
    if (seeded || !status) return;
    setPort(status.port || DEFAULT_PORT);
    setAddressChoice(status.preferredAddress ?? "auto");
    setBindAll(status.bindAll);
    setSeeded(true);
  }, [status, seeded]);

  // Ticks only to redraw the pairing-code expiry; it is presentation, never a
  // countdown the host relies on.
  useEffect(() => {
    if (!pairing) return;
    const handle = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(handle);
  }, [pairing]);

  const running = status?.running ?? false;
  const portValid = port >= MIN_PORT && port <= 65535;
  const secondsLeft =
    pairing && pairingIssuedAt ? remainingSeconds(pairing.expiresInMs, pairingIssuedAt, now) : null;

  const addresses = status?.addresses ?? [];
  const guidance = error?.code ? ERROR_GUIDANCE[error.code] : undefined;

  async function toggleEnabled(next: boolean) {
    if (next) {
      await startServer(port, addressChoice === "auto" ? null : addressChoice, bindAll);
    } else {
      await stopServer();
    }
  }

  async function copyUrl() {
    if (!pairing) return;
    try {
      await navigator.clipboard.writeText(pairing.url);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  }

  return (
    <Card>
      <CardHeader
        icon={running ? <Wifi size={16} /> : <WifiOff size={16} />}
        eyebrow="Networking"
        title="Local network server"
        subtitle="Let phones, tablets and other computers on this Wi-Fi control or view app.timer."
        action={
          <Badge tone={running ? "success" : "neutral"}>
            {running ? "Running" : "Stopped"}
          </Badge>
        }
      />

      <div className="flex flex-col gap-5 px-5 pb-5">
        <div className="rounded-xl border border-border bg-[#141417] px-4">
          <Toggle
            checked={running}
            disabled={busy || !portValid}
            onChange={toggleEnabled}
            label="Enable local networking"
            description="Runs a WebSocket server on this machine only. Nothing leaves your local network."
          />
        </div>

        {error ? (
          <div className="flex items-start gap-2.5 rounded-xl border border-[#4a2020] bg-[#2a1414] px-3.5 py-3">
            <ShieldAlert size={15} className="mt-0.5 shrink-0 text-danger" />
            <div className="min-w-0 text-[12px] leading-relaxed">
              <p className="font-medium text-danger">{error.message}</p>
              {guidance ? <p className="mt-1 text-text-muted">{guidance}</p> : null}
            </div>
          </div>
        ) : null}

        <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
          <NumberField
            label="Port"
            value={port}
            onChange={setPort}
            max={65535}
            suffix={portValid ? undefined : `< ${MIN_PORT}`}
          />

          <label className="flex flex-col gap-1.5">
            <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
              Network interface
            </span>
            <select
              value={addressChoice}
              disabled={running}
              onChange={(event) => setAddressChoice(event.currentTarget.value)}
              className={cn(SELECT_CLASS, running && "opacity-60")}
            >
              <option value="auto">Automatic (best local address)</option>
              {addresses.map((option) => (
                <option key={option.address} value={option.address}>
                  {option.address}
                  {option.label ? ` · ${option.label}` : ""}
                  {option.isVirtual ? " (virtual)" : ""}
                </option>
              ))}
            </select>
          </label>

          <div className="flex items-end">
            <div className="w-full rounded-xl border border-border bg-[#141417] px-4">
              <Toggle
                checked={bindAll}
                disabled={running}
                onChange={setBindAll}
                label="All interfaces"
                description="Bind every adapter, including virtual ones."
              />
            </div>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button
            icon={<Power size={15} />}
            variant={running ? "danger" : "primary"}
            disabled={busy || (!running && !portValid)}
            onClick={() => toggleEnabled(!running)}
          >
            {running ? "Stop server" : "Start server"}
          </Button>
          <Button
            icon={<RefreshCw size={15} />}
            disabled={busy}
            onClick={() => void refreshNetwork()}
            title="Re-read this machine's network interfaces"
          >
            Refresh interfaces
          </Button>
          {status?.url ? (
            <span className="ml-auto font-mono text-[12px] text-text-muted">{status.url}</span>
          ) : null}
        </div>

        {running ? (
          <dl className="grid grid-cols-2 gap-4 border-t border-border pt-4 text-[13px] sm:grid-cols-4">
            <div>
              <dt className="text-text-faint">Bound address</dt>
              <dd className="mt-0.5 font-mono text-text">
                {status?.boundAddress ?? (status?.bindAll ? "0.0.0.0" : "—")}
              </dd>
            </div>
            <div>
              <dt className="text-text-faint">Devices paired</dt>
              <dd className="mt-0.5 font-mono text-text">{status?.pairedCount ?? 0}</dd>
            </div>
            <div>
              <dt className="text-text-faint">Connected now</dt>
              <dd className="mt-0.5 font-mono text-text">{status?.connectedCount ?? 0}</dd>
            </div>
            <div>
              <dt className="text-text-faint">Pending</dt>
              <dd className="mt-0.5 font-mono text-text">{pending.length}</dd>
            </div>
          </dl>
        ) : null}

        {running ? (
          <div className="border-t border-border pt-4">
            <div className="flex items-center justify-between gap-3">
              <div>
                <p className="text-[13px] font-medium text-text">Pair a device</p>
                <p className="mt-0.5 text-[12px] text-text-muted">
                  Scan the code with a phone camera to open the remote interface.
                </p>
              </div>
              <Button
                icon={<QrCode size={15} />}
                disabled={busy}
                onClick={() => void issuePairing()}
              >
                {pairing ? "New code" : "Show pairing code"}
              </Button>
            </div>

            {pairing ? (
              /**
               * A vertical stack on purpose. The QR code used to sit beside the
               * code and the copy controls, and the SVG the engine renders
               * carries its own 256×256 size, so it overflowed its column and
               * covered them. Here every element has its own row, and the QR
               * lives in a box whose size it is told to fill, so the two can
               * never overlap however narrow the panel gets.
               */
              <div className="mt-4 flex flex-col gap-4 rounded-xl border border-border bg-[#141417] p-4">
                <div className="text-center">
                  <p className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
                    Pairing code
                  </p>
                  <p className="mt-1 font-mono text-2xl tracking-[0.35em] text-text">
                    {pairing.code}
                  </p>
                  <p className="mt-2 text-[12px] text-text-muted">
                    {secondsLeft && secondsLeft > 0
                      ? `Expires in ${Math.floor(secondsLeft / 60)}:${String(secondsLeft % 60).padStart(2, "0")}`
                      : "This code has expired. Issue a new one."}
                  </p>
                </div>

                <div className="flex flex-wrap items-center justify-center gap-2">
                  <Button
                    size="sm"
                    icon={copied ? <Check size={14} /> : <Copy size={14} />}
                    onClick={() => void copyUrl()}
                  >
                    {copied ? "Copied" : "Copy link"}
                  </Button>
                  <Button size="sm" variant="ghost" onClick={forgetPairing}>
                    Hide
                  </Button>
                </div>

                <div className="mx-auto w-full max-w-[260px]">
                  <div className="rounded-xl border border-border bg-white p-3">
                    {/*
                      The SVG is generated by Rust; it is a QR code, not markup
                      input. The size classes override the intrinsic width and
                      height it carries, so it is drawn to the box instead of
                      spilling out of it. The white padding keeps the quiet zone
                      a scanner needs to lock on.
                    */}
                    <div
                      className="mx-auto aspect-square w-full [&>svg]:block [&>svg]:h-full [&>svg]:w-full"
                      dangerouslySetInnerHTML={{ __html: pairing.svg }}
                    />
                  </div>
                </div>

                <p className="text-center text-[12px] leading-relaxed text-text-faint">
                  Point a phone camera at the code to open the remote interface. The code is
                  single-use and expires in two minutes; a device only gains access after you
                  approve it below.
                </p>
              </div>
            ) : null}

            <div className="mt-5">
              <p className="text-[13px] font-medium text-text">
                Waiting for approval {pending.length > 0 ? `(${pending.length})` : ""}
              </p>
              {pending.length === 0 ? (
                <p className="mt-1 text-[12px] text-text-muted">
                  No device has entered a pairing code yet.
                </p>
              ) : (
                <ul className="mt-3 flex flex-col gap-2">
                  {pending.map((request) => (
                    <li
                      key={request.requestId}
                      className="flex flex-wrap items-center gap-3 rounded-xl border border-border bg-elevated px-3.5 py-3"
                    >
                      <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-border bg-card text-text-muted">
                        <KindIcon kind={request.kind} />
                      </span>
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-[13px] font-medium text-text">
                          {request.name || "Unnamed device"}
                        </p>
                        <p className="truncate font-mono text-[11px] text-text-faint">
                          {request.address}
                        </p>
                      </div>
                      <div className="flex items-center gap-2">
                        <ApproveButtons requestId={request.requestId} disabled={busy} />
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={busy}
                          onClick={() => void rejectRequest(request.requestId)}
                        >
                          Reject
                        </Button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        ) : (
          <p className="border-t border-border pt-4 text-[12px] text-text-faint">
            Start the server to generate a pairing code. Everything else in app.timer keeps working
            while networking is off.
          </p>
        )}
      </div>
    </Card>
  );
}

/** Approve as a controller or as a read-only display. */
function ApproveButtons({ requestId, disabled }: { requestId: string; disabled: boolean }) {
  const approve = (role: DeviceRole) => void approveRequest(requestId, role);
  return (
    <>
      <Button size="sm" variant="primary" disabled={disabled} onClick={() => approve("controller")}>
        Approve as controller
      </Button>
      <Button size="sm" disabled={disabled} onClick={() => approve("display")}>
        As display
      </Button>
    </>
  );
}
