import type { ReactNode } from "react";
import { Palette, Bell, Info, SlidersHorizontal, MonitorCog } from "lucide-react";
import { useAppInfo } from "../hooks/useAppInfo";
import { useMonitors } from "../hooks/useMonitors";
import { Card, CardHeader } from "../components/ui/Card";
import { Button } from "../components/ui/Button";
import { Toggle } from "../components/ui/Toggle";
import { NetworkingSettingsCard } from "../components/networking/NetworkingSettingsCard";
import { DeviceManagerCard } from "../components/networking/DeviceManagerCard";
import { SOUND_OPTIONS, playSound } from "../lib/sound";
import { resetSettings, settingsStore, updateSettings } from "../stores/settingsStore";
import type { InterfaceDensity, MessageSound, StartupBehaviour } from "../types";
import type { TimerFormat } from "../lib/time";

const SELECT_CLASS =
  "h-10 w-full appearance-none rounded-xl border border-border bg-elevated px-3 text-[13px] text-text outline-none transition-colors duration-150 focus:border-accent/70";

const DENSITIES: InterfaceDensity[] = ["comfortable", "compact"];
const STARTUP: StartupBehaviour[] = ["dashboard", "restore"];
const FORMATS: TimerFormat[] = ["hh:mm:ss", "mm:ss"];

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase">
        {label}
      </span>
      {children}
    </label>
  );
}

export default function SettingsPage() {
  const { settings } = settingsStore.use();
  const { monitors } = useMonitors();
  const { info, error } = useAppInfo();

  return (
    <div className="mx-auto flex w-full max-w-[1100px] flex-col gap-5">
      <Card>
        <CardHeader
          icon={<SlidersHorizontal size={16} />}
          eyebrow="General"
          title="Startup & behaviour"
          subtitle="How app.timer behaves when it launches and while running."
        />
        <div className="grid grid-cols-1 gap-4 px-5 pb-5 md:grid-cols-3">
          <Field label="Startup behaviour">
            <select
              value={settings.general.startupBehaviour}
              onChange={(event) =>
                updateSettings("general", {
                  startupBehaviour: event.currentTarget.value as StartupBehaviour,
                })
              }
              className={SELECT_CLASS}
            >
              {STARTUP.map((value) => (
                <option key={value} value={value}>
                  {value === "dashboard" ? "Open dashboard" : "Restore last session"}
                </option>
              ))}
            </select>
          </Field>

          <Field label="Default timer format">
            <select
              value={settings.general.defaultTimerFormat}
              onChange={(event) =>
                updateSettings("general", {
                  defaultTimerFormat: event.currentTarget.value as TimerFormat,
                })
              }
              className={SELECT_CLASS}
            >
              {FORMATS.map((value) => (
                <option key={value} value={value}>
                  {value}
                </option>
              ))}
            </select>
          </Field>

          <div className="rounded-xl border border-border bg-[#141417] px-4">
            <Toggle
              checked={settings.general.confirmBeforeStopping}
              onChange={(confirmBeforeStopping) =>
                updateSettings("general", { confirmBeforeStopping })
              }
              label="Confirm before stopping"
              description="Ask for confirmation when a running timer is stopped."
            />
          </div>
        </div>
      </Card>

      <Card>
        <CardHeader
          icon={<Palette size={16} />}
          eyebrow="Appearance"
          title="Theme & density"
          subtitle="Changes apply instantly across both windows."
        />
        <div className="grid grid-cols-1 gap-4 px-5 pb-5 md:grid-cols-3">
          <Field label="Accent colour">
            <div className="flex items-center gap-3">
              <input
                type="color"
                value={settings.appearance.accentColor}
                onChange={(event) =>
                  updateSettings("appearance", { accentColor: event.currentTarget.value })
                }
                className="h-10 w-14 cursor-pointer rounded-xl border border-border bg-elevated p-1"
              />
              <span className="font-mono text-[13px] text-text-muted">
                {settings.appearance.accentColor.toUpperCase()}
              </span>
            </div>
          </Field>

          <Field label="Interface density">
            <select
              value={settings.appearance.density}
              onChange={(event) =>
                updateSettings("appearance", {
                  density: event.currentTarget.value as InterfaceDensity,
                })
              }
              className={SELECT_CLASS}
            >
              {DENSITIES.map((value) => (
                <option key={value} value={value}>
                  {value}
                </option>
              ))}
            </select>
          </Field>

          <Field label="Theme">
            <div className="flex h-10 items-center rounded-xl border border-border bg-elevated px-3 text-[13px] text-text-muted">
              Dark (only theme)
            </div>
          </Field>
        </div>
      </Card>

      <Card>
        <CardHeader
          icon={<MonitorCog size={16} />}
          eyebrow="Display"
          title="Presentation defaults"
          subtitle="Used when the display window is opened."
        />
        <div className="grid grid-cols-1 gap-4 px-5 pb-5 md:grid-cols-3">
          <Field label="Default monitor">
            <select
              value={settings.display.defaultMonitorKey ?? ""}
              onChange={(event) =>
                updateSettings("display", {
                  defaultMonitorKey: event.currentTarget.value || null,
                })
              }
              className={SELECT_CLASS}
            >
              <option value="">Follow the primary monitor</option>
              {monitors.map((monitor, index) => (
                <option key={monitor.key} value={monitor.key}>
                  {monitor.name ?? `Display ${index + 1}`}
                  {monitor.isPrimary ? " (primary)" : ""}
                </option>
              ))}
            </select>
          </Field>

          <div className="rounded-xl border border-border bg-[#141417] px-4">
            <Toggle
              checked={settings.display.startFullscreen}
              onChange={(startFullscreen) => updateSettings("display", { startFullscreen })}
              label="Start fullscreen"
              description="Applied the next time the display window opens."
            />
          </div>

          <div className="rounded-xl border border-border bg-[#141417] px-4">
            <Toggle
              checked={settings.display.alwaysOnTop}
              onChange={(alwaysOnTop) => updateSettings("display", { alwaysOnTop })}
              label="Always on top"
              description="Applied the next time the display window opens."
            />
          </div>
        </div>
        <p className="px-5 pb-5 text-[12px] text-text-faint">
          Layout, typography, colours and message presentation live on the Display Window page, where
          they are previewed live.
        </p>
      </Card>

      <Card>
        <CardHeader
          icon={<Bell size={16} />}
          eyebrow="Notifications"
          title="Sounds"
          subtitle="Audio cues for warnings, stage changes and completion."
        />
        <div className="grid grid-cols-1 gap-4 px-5 pb-5 md:grid-cols-2 xl:grid-cols-4">
          <div className="rounded-xl border border-border bg-[#141417] px-4">
            <Toggle
              checked={settings.notifications.soundsEnabled}
              onChange={(soundsEnabled) => updateSettings("notifications", { soundsEnabled })}
              label="Enable sounds"
              description="Master switch for all audio feedback."
            />
          </div>

          <Field label="Completion sound">
            <div className="flex items-center gap-2">
              <select
                value={settings.notifications.completionSound}
                disabled={!settings.notifications.soundsEnabled}
                onChange={(event) =>
                  updateSettings("notifications", {
                    completionSound: event.currentTarget.value as MessageSound,
                  })
                }
                className={`${SELECT_CLASS} disabled:pointer-events-none disabled:opacity-40`}
              >
                {SOUND_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
              <Button
                size="sm"
                disabled={!settings.notifications.soundsEnabled}
                onClick={() => playSound(settings.notifications.completionSound)}
                title="Play the selected sound through the machine's speakers"
              >
                Preview
              </Button>
            </div>
          </Field>

          <div className="rounded-xl border border-border bg-[#141417] px-4">
            <Toggle
              checked={settings.notifications.warningSounds}
              disabled={!settings.notifications.soundsEnabled}
              onChange={(warningSounds) => updateSettings("notifications", { warningSounds })}
              label="Warning sounds"
              description="Play the stage's message sound at its warning points."
            />
          </div>

          <div className="rounded-xl border border-border bg-[#141417] px-4">
            <Toggle
              checked={settings.notifications.stageSounds}
              disabled={!settings.notifications.soundsEnabled}
              onChange={(stageSounds) => updateSettings("notifications", { stageSounds })}
              label="Stage sounds"
              description="Play a cue when each stage begins."
            />
            <Toggle
              checked={settings.notifications.sessionSounds}
              disabled={!settings.notifications.soundsEnabled}
              onChange={(sessionSounds) => updateSettings("notifications", { sessionSounds })}
              label="Session sounds"
              description="Play a cue when the whole session finishes."
            />
          </div>
        </div>
      </Card>

      <NetworkingSettingsCard />
      <DeviceManagerCard />

      <Card>
        <CardHeader
          icon={<Info size={16} />}
          eyebrow="About"
          title="Runtime information"
          subtitle="Read from the Rust backend over the Tauri IPC bridge."
        />
        <div className="flex items-center justify-between gap-4 px-5 pb-5">
          <dl className="grid grid-cols-3 gap-6 text-[13px]">
            <div>
              <dt className="text-text-faint">Application</dt>
              <dd className="mt-0.5 text-text">{info?.name ?? "—"}</dd>
            </div>
            <div>
              <dt className="text-text-faint">Version</dt>
              <dd className="mt-0.5 font-mono text-text">{info?.version ?? "—"}</dd>
            </div>
            <div>
              <dt className="text-text-faint">Runtime</dt>
              <dd className="mt-0.5 font-mono text-text">{info?.runtime ?? "—"}</dd>
            </div>
          </dl>
          <Button onClick={resetSettings}>Reset to defaults</Button>
        </div>
        {error ? (
          <p className="px-5 pb-5 text-[12px] text-danger">Backend unavailable: {error}</p>
        ) : null}
      </Card>
    </div>
  );
}
