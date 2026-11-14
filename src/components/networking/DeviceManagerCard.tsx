import { useState } from "react";
import {
  ChevronDown,
  Gamepad2,
  Laptop,
  Link2Off,
  MonitorPlay,
  MonitorSmartphone,
  ShieldCheck,
  Smartphone,
  Tablet,
  Trash2,
  Users,
} from "lucide-react";
import type { ConnectedDevice, DeviceKind, DevicePermission, DeviceRole, DeviceView } from "../../types";
import { DEVICE_PERMISSIONS, PERMISSION_LABELS } from "../../types";
import { formatDateTime } from "../../lib/time";
import { cn } from "../../lib/cn";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Card, CardHeader } from "../ui/Card";
import { EmptyState } from "../ui/EmptyState";
import {
  changeDevicePermissions,
  changeDeviceRole,
  disconnect,
  networkStore,
  revoke,
} from "../../stores/networkStore";

function KindIcon({ kind, size = 15 }: { kind: DeviceKind | null; size?: number }) {
  if (kind === "phone") return <Smartphone size={size} />;
  if (kind === "tablet") return <Tablet size={size} />;
  if (kind === "desktop") return <Laptop size={size} />;
  return <MonitorSmartphone size={size} />;
}

function RoleBadge({ role }: { role: DeviceRole }) {
  return role === "controller" ? (
    <Badge tone="accent">
      <Gamepad2 size={11} /> Controller
    </Badge>
  ) : (
    <Badge tone="neutral">
      <MonitorPlay size={11} /> Display
    </Badge>
  );
}

/** One paired device, with its role, permissions and revocation control. */
function PairedDeviceRow({ device, busy }: { device: DeviceView; busy: boolean }) {
  const [expanded, setExpanded] = useState(false);
  const isDisplay = device.role === "display";

  function togglePermission(permission: DevicePermission) {
    const next = device.permissions.includes(permission)
      ? device.permissions.filter((entry) => entry !== permission)
      : [...device.permissions, permission];
    void changeDevicePermissions(device.id, next);
  }

  return (
    <li className="rounded-xl border border-border bg-elevated">
      <div className="flex flex-wrap items-center gap-3 px-3.5 py-3">
        <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-border bg-card text-text-muted">
          <KindIcon kind={device.kind} />
        </span>

        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <p className="truncate text-[13px] font-medium text-text">{device.name}</p>
            <span
              className={cn(
                "h-1.5 w-1.5 shrink-0 rounded-full",
                device.connected ? "bg-success" : "bg-text-faint",
              )}
              title={device.connected ? "Connected" : "Offline"}
            />
          </div>
          <p className="truncate font-mono text-[11px] text-text-faint">
            {device.id}
            {device.addresses.length > 0 ? ` · ${device.addresses.join(", ")}` : ""}
          </p>
        </div>

        <RoleBadge role={device.role} />

        <div className="flex items-center gap-2">
          <select
            value={device.role}
            disabled={busy}
            onChange={(event) =>
              void changeDeviceRole(device.id, event.currentTarget.value as DeviceRole)
            }
            className="h-8 appearance-none rounded-lg border border-border bg-card px-2 text-[12px] text-text outline-none focus:border-accent/70"
            aria-label="Device role"
          >
            <option value="controller">Controller</option>
            <option value="display">Display</option>
          </select>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            icon={<ChevronDown size={14} className={cn("transition-transform", expanded && "rotate-180")} />}
            onClick={() => setExpanded((value) => !value)}
          >
            Permissions
          </Button>
          <Button
            size="sm"
            variant="danger"
            disabled={busy}
            icon={<Trash2 size={14} />}
            onClick={() => void revoke(device.id)}
            title="Revoke access and close every connection for this device"
          >
            Revoke
          </Button>
        </div>
      </div>

      {expanded ? (
        <div className="border-t border-border px-3.5 py-3">
          {isDisplay ? (
            <p className="flex items-center gap-2 text-[12px] text-text-muted">
              <ShieldCheck size={14} className="text-text-faint" />
              A display is read-only and never issues commands, so it holds no permissions.
            </p>
          ) : (
            <>
              <p className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
                Permissions
              </p>
              <div className="mt-2 grid grid-cols-1 gap-1.5 sm:grid-cols-2">
                {DEVICE_PERMISSIONS.map((permission) => {
                  const checked = device.permissions.includes(permission);
                  return (
                    <label
                      key={permission}
                      className="flex cursor-pointer items-center gap-2.5 rounded-lg px-2 py-1.5 hover:bg-card"
                    >
                      <input
                        type="checkbox"
                        checked={checked}
                        disabled={busy}
                        onChange={() => togglePermission(permission)}
                        className="h-4 w-4 accent-[var(--color-accent)]"
                      />
                      <span className="text-[12px] text-text">{PERMISSION_LABELS[permission]}</span>
                    </label>
                  );
                })}
              </div>
            </>
          )}
          <p className="mt-3 text-[11px] text-text-faint">
            Paired {formatDateTime(device.pairedAt)} · Last seen {formatDateTime(device.lastSeenAt)}
          </p>
        </div>
      ) : null}
    </li>
  );
}

/** One live socket, including connections that have not paired yet. */
function ConnectionRow({
  connection,
  busy,
}: {
  connection: ConnectedDevice;
  busy: boolean;
}) {
  return (
    <li className="flex flex-wrap items-center gap-3 rounded-xl border border-border bg-elevated px-3.5 py-2.5">
      <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-border bg-card text-text-muted">
        <KindIcon kind={connection.kind} size={14} />
      </span>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[13px] text-text">
          {connection.authenticated ? connection.name : "Not yet paired"}
        </p>
        <p className="truncate font-mono text-[11px] text-text-faint">{connection.address}</p>
      </div>
      {connection.authenticated && connection.role ? (
        <RoleBadge role={connection.role} />
      ) : (
        <Badge tone="warning">Unpaired</Badge>
      )}
      <Button
        size="sm"
        variant="ghost"
        disabled={busy}
        icon={<Link2Off size={14} />}
        onClick={() => void disconnect(connection.connectionId)}
      >
        Disconnect
      </Button>
    </li>
  );
}

/** Paired and connected devices, plus the controls that change their access. */
export function DeviceManagerCard() {
  const { devices, connections, status, busy } = networkStore.use();
  const running = status?.running ?? false;

  return (
    <Card>
      <CardHeader
        icon={<Users size={16} />}
        eyebrow="Networking"
        title="Devices"
        subtitle="Who may connect, what they may do, and how to cut them off."
        action={
          <Badge tone={devices.length > 0 ? "accent" : "neutral"}>
            {devices.length} paired
          </Badge>
        }
      />

      <div className="flex flex-col gap-5 px-5 pb-5">
        {!running ? (
          <p className="rounded-xl border border-border bg-[#141417] px-4 py-3 text-[12px] text-text-muted">
            The local server is stopped. Start it above to pair devices.
          </p>
        ) : null}

        <div>
          <p className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Connected now {connections.length > 0 ? `(${connections.length})` : ""}
          </p>
          {connections.length === 0 ? (
            <p className="mt-1 text-[12px] text-text-muted">No device is connected.</p>
          ) : (
            <ul className="mt-2 flex flex-col gap-2">
              {connections.map((connection) => (
                <ConnectionRow
                  key={connection.connectionId}
                  connection={connection}
                  busy={busy}
                />
              ))}
            </ul>
          )}
        </div>

        <div>
          <p className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
            Paired devices
          </p>
          {devices.length === 0 ? (
            <EmptyState
              className="mt-2"
              icon={<Users size={18} />}
              title="No paired devices"
              description="Approve a pairing request above and the device appears here with its role and permissions."
            />
          ) : (
            <ul className="mt-2 flex flex-col gap-2">
              {devices.map((device) => (
                <PairedDeviceRow key={device.id} device={device} busy={busy} />
              ))}
            </ul>
          )}
        </div>
      </div>
    </Card>
  );
}
