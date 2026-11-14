import { useRef, useState } from "react";
import type { ReactNode } from "react";
import {
  Copy,
  Eye,
  ImagePlus,
  LayoutTemplate,
  MonitorPlay,
  Palette,
  Pencil,
  Play,
  RotateCcw,
  Save,
  Trash2,
  Type,
  Volume2,
  X,
} from "lucide-react";
import { DisplayControls } from "../components/display/DisplayControls";
import { DisplayPreview } from "../components/display/DisplayPreview";
import { MonitorSelector } from "../components/display/MonitorSelector";
import { Button } from "../components/ui/Button";
import { Card, CardHeader } from "../components/ui/Card";
import { Modal } from "../components/ui/Modal";
import { NumberField } from "../components/ui/NumberField";
import { Toggle } from "../components/ui/Toggle";
import { cn } from "../lib/cn";
import {
  DISPLAY_ALIGNS,
  DISPLAY_BANDS,
  DISPLAY_CORNERS,
  DISPLAY_WEIGHTS,
  MESSAGE_COLOR_FROM_MESSAGE,
} from "../lib/display";
import { MESSAGE_FONTS } from "../lib/message";
import { SOUND_OPTIONS, playSound } from "../lib/sound";
import { libraryStore } from "../stores/libraryStore";
import {
  allLayouts,
  canDelete,
  deleteLayout,
  displayConfigStore,
  duplicateLayout,
  isModified as layoutIsModified,
  patchStyle,
  renameLayout,
  restoreBuiltInDefaults,
  saveLayout,
  selectLayout,
  selectedLayout,
} from "../stores/displayConfigStore";
import { settingsStore, updateSettings } from "../stores/settingsStore";
import { displayStore } from "../stores/displayStore";
import { setPreviewMessage, uiStore } from "../stores/uiStore";
import type { DisplayAlign, DisplayBand, DisplayCorner, DisplayTextStyle } from "../types";

const SELECT_CLASS =
  "h-10 w-full appearance-none rounded-xl border border-border bg-elevated px-3 text-[13px] text-text outline-none transition-colors duration-150 focus:border-accent/70";

const LABEL_CLASS = "text-[11px] font-medium tracking-[0.1em] text-text-faint uppercase";

/* ------------------------------------------- small form primitives */

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="flex min-w-0 flex-col gap-1.5">
      <span className={LABEL_CLASS}>{label}</span>
      {children}
      {hint ? <span className="text-[11px] text-text-faint">{hint}</span> : null}
    </label>
  );
}

function SelectField({
  label,
  hint,
  value,
  onChange,
  children,
}: {
  label: string;
  hint?: string;
  value: string;
  onChange: (value: string) => void;
  children: ReactNode;
}) {
  return (
    <Field label={label} hint={hint}>
      <select
        value={value}
        onChange={(event) => onChange(event.currentTarget.value)}
        className={SELECT_CLASS}
      >
        {children}
      </select>
    </Field>
  );
}

/** Compact button group used for alignment, position and colour-source choices. */
function Segmented<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: Array<{ value: T; label: string }>;
  onChange: (value: T) => void;
}) {
  return (
    <div className="flex w-full rounded-xl border border-border bg-elevated p-0.5">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
          className={cn(
            "flex-1 rounded-[9px] px-2 py-1.5 text-[12px] font-medium capitalize transition-colors duration-150",
            value === option.value
              ? "bg-accent/90 text-[#0B0B10]"
              : "text-text-muted hover:bg-[#28282d] hover:text-text",
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

/** Slider with a live numeric readout, used for the responsive size scales. */
function RangeField({
  label,
  value,
  min,
  max,
  step,
  format,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  format: (value: number) => string;
  onChange: (value: number) => void;
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className={cn(LABEL_CLASS, "flex items-center justify-between")}>
        <span>{label}</span>
        <span className="font-mono text-[11px] text-text-muted normal-case">{format(value)}</span>
      </span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(event) => onChange(Number(event.currentTarget.value))}
        className="h-1.5 w-full cursor-pointer appearance-none rounded-full bg-[#26262c]"
        style={{ accentColor: "var(--color-accent)" }}
      />
    </label>
  );
}

function ColorInput({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  return (
    <span className="flex items-center gap-3">
      <input
        type="color"
        value={value}
        onChange={(event) => onChange(event.currentTarget.value)}
        className="h-10 w-14 shrink-0 cursor-pointer rounded-xl border border-border bg-elevated p-1"
      />
      <span className="font-mono text-[13px] text-text-muted">{value.toUpperCase()}</span>
    </span>
  );
}

function ColorField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <Field label={label}>
      <ColorInput value={value} onChange={onChange} />
    </Field>
  );
}

/** One typography block: family, size, weight and alignment for an element. */
function TypographyPanel({
  title,
  caption,
  value,
  onChange,
}: {
  title: string;
  caption: string;
  value: DisplayTextStyle;
  onChange: (patch: Partial<DisplayTextStyle>) => void;
}) {
  return (
    <div className="flex flex-col gap-4">
      <div>
        <p className="text-[13px] font-semibold text-text">{title}</p>
        <p className="mt-0.5 text-[12px] text-text-muted">{caption}</p>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <SelectField
          label="Font family"
          value={value.font}
          onChange={(font) => onChange({ font })}
        >
          {MESSAGE_FONTS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </SelectField>

        <SelectField
          label="Font weight"
          value={String(value.weight)}
          onChange={(weight) => onChange({ weight: Number(weight) })}
        >
          {DISPLAY_WEIGHTS.map((weight) => (
            <option key={weight} value={weight}>
              {weight}
            </option>
          ))}
        </SelectField>
      </div>

      <RangeField
        label="Size"
        value={value.scale}
        min={0.5}
        max={2}
        step={0.05}
        format={(scale) => `${Math.round(scale * 100)}%`}
        onChange={(scale) => onChange({ scale })}
      />

      <Field label="Text alignment">
        <Segmented value={value.align} options={DISPLAY_ALIGNS} onChange={(align) => onChange({ align })} />
      </Field>
    </div>
  );
}

type DialogMode = { mode: "save" | "rename" } | null;

/* ------------------------------------------------------------------ page */

export default function DisplaySettingsPage() {
  const { settings } = settingsStore.use();
  const { windowState, monitors } = displayStore.use();
  const { style, selectedLayoutId } = displayConfigStore.use();
  const { messages } = libraryStore.use();
  const { previewMessageId } = uiStore.use();

  const [nonce, setNonce] = useState(0);
  const [dialog, setDialog] = useState<DialogMode>(null);
  const [draftName, setDraftName] = useState("");
  const [imageError, setImageError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  const previewMessage = messages.find((message) => message.id === previewMessageId) ?? null;
  const current = selectedLayout();
  const modified = layoutIsModified();
  const deletable = canDelete(selectedLayoutId);
  const { layout, timer, messageText, theme } = style;

  const builtIns = allLayouts().filter((entry) => entry.builtIn);
  const customs = allLayouts().filter((entry) => !entry.builtIn);

  const openDialog = (mode: "save" | "rename") => {
    setDraftName(mode === "save" ? `${current?.name ?? "Layout"} copy` : (current?.name ?? ""));
    setDialog({ mode });
  };

  const commitDialog = () => {
    if (dialog?.mode === "save") saveLayout(draftName);
    if (dialog?.mode === "rename") renameLayout(selectedLayoutId, draftName);
    setDialog(null);
  };

  const pickBackgroundImage = (file: File | undefined) => {
    if (!file) return;
    // The image travels inside the saved layout, which is stored locally, so an
    // oversized file is refused rather than silently truncated by the browser.
    if (file.size > 3_000_000) {
      setImageError("Choose an image under 3 MB so it can be stored with the layout.");
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const url = typeof reader.result === "string" ? reader.result : null;
      if (!url) {
        setImageError("That image could not be read.");
        return;
      }
      setImageError(null);
      patchStyle({ theme: { backgroundImage: url } });
    };
    reader.onerror = () => setImageError("That image could not be read.");
    reader.readAsDataURL(file);
  };

  return (
    <div className="mx-auto flex w-full max-w-[1440px] flex-col gap-5">
      {/* ------------------------------------------------------- presets */}
      <Card>
        <CardHeader
          icon={<LayoutTemplate size={16} />}
          eyebrow="Presentation engine"
          title="Display layout presets"
          subtitle="Reusable layouts. Built-ins ship with the app and can never be deleted."
          action={
            <span
              className={cn(
                "rounded-full border px-3 py-1 text-[11px] font-medium tracking-[0.08em] uppercase",
                modified
                  ? "border-accent/40 bg-accent/10 text-accent"
                  : "border-border bg-elevated text-text-faint",
              )}
            >
              {modified ? "Modified" : "Saved"}
            </span>
          }
        />
        <div className="flex flex-col gap-4 px-5 pb-5">
          <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)]">
            <SelectField label="Layout" value={selectedLayoutId} onChange={selectLayout}>
              <optgroup label="Built-in">
                {builtIns.map((entry) => (
                  <option key={entry.id} value={entry.id}>
                    {entry.name}
                  </option>
                ))}
              </optgroup>
              {customs.length > 0 ? (
                <optgroup label="Custom">
                  {customs.map((entry) => (
                    <option key={entry.id} value={entry.id}>
                      {entry.name}
                    </option>
                  ))}
                </optgroup>
              ) : null}
            </SelectField>

            <div className="flex flex-wrap items-end gap-2">
              <Button icon={<Save size={16} />} onClick={() => openDialog("save")}>
                Save as New
              </Button>
              <Button
                icon={<Copy size={16} />}
                onClick={() => duplicateLayout(selectedLayoutId)}
              >
                Duplicate
              </Button>
              <Button
                icon={<Pencil size={16} />}
                disabled={!deletable}
                title={deletable ? "Rename this layout" : "Built-in layouts cannot be renamed"}
                onClick={() => openDialog("rename")}
              >
                Rename
              </Button>
              <Button
                variant="danger"
                icon={<Trash2 size={16} />}
                disabled={!deletable}
                title={deletable ? "Delete this layout" : "Built-in layouts cannot be deleted"}
                onClick={() => deleteLayout(selectedLayoutId)}
              >
                Delete
              </Button>
              <Button
                icon={<RotateCcw size={16} />}
                title="Select the default layout and discard unsaved changes"
                onClick={restoreBuiltInDefaults}
              >
                Restore Defaults
              </Button>
            </div>
          </div>

          <p className="text-[12px] text-text-faint">
            {current ? (
              <>
                Based on <span className="text-text-muted">{current.name}</span>
                {current.builtIn ? " (built-in)" : " (custom)"}
                {customs.length > 0
                  ? ` · ${customs.length} custom layout${customs.length === 1 ? "" : "s"} saved`
                  : ""}
                .{" "}
              </>
            ) : null}
            Editing a layout changes the applied presentation only — a built-in definition is never
            overwritten, so “Restore Defaults” always brings the original back.
          </p>
        </div>
      </Card>

      {/* ------------------------------------------------------- preview */}
      <Card>
        <CardHeader
          icon={<Eye size={16} />}
          eyebrow="Live preview"
          title="What the display window shows"
          subtitle="Rendered by the same surface the display window uses — every change appears here and on the open window immediately."
          action={
            <Button
              size="sm"
              icon={<Play size={16} />}
              title="Replay the current message's entrance and exit animation"
              onClick={() => setNonce((value) => value + 1)}
            >
              Replay Animation
            </Button>
          }
        />
        <div className="flex flex-col gap-4 px-5 pb-5">
          <DisplayPreview style={style} message={previewMessage} nonce={nonce} />

          <div className="grid gap-3 md:grid-cols-2">
            <SelectField
              label="Preview message"
              hint="The same message is pushed to the display window, using the existing presentation channel."
              value={previewMessageId ?? ""}
              onChange={(value) => setPreviewMessage(value === "" ? null : value)}
            >
              <option value="">No message</option>
              {messages.map((message) => (
                <option key={message.id} value={message.id}>
                  {message.name}
                </option>
              ))}
            </SelectField>

            <div className="flex flex-col gap-1.5">
              <span className={LABEL_CLASS}>Motion</span>
              <div className="flex h-10 items-center rounded-xl border border-border bg-elevated px-3 text-[13px] text-text-muted">
                {previewMessage
                  ? `“${previewMessage.animation}” entrance, then its exit${
                      previewMessage.durationMs > 0
                        ? ` after ${Math.round(previewMessage.durationMs / 1000)}s`
                        : " (sticky — no timeout)"
                    }`
                  : "Selected a message to preview its animation."}
              </div>
              <span className="text-[11px] text-text-faint">
                Animations are presentation only; the countdown itself is unchanged.
              </span>
            </div>
          </div>
        </div>
      </Card>

      {/* -------------------------------------------------- layout & type */}
      <div className="grid gap-5 xl:grid-cols-3">
        <Card>
          <CardHeader
            icon={<LayoutTemplate size={16} />}
            eyebrow="Layout"
            title="Placement"
            subtitle="Where each element sits and what is shown at all."
          />
          <div className="flex flex-col gap-4 px-5 pb-5">
            <Field label="Timer position">
              <Segmented
                value={layout.timerPosition}
                options={DISPLAY_BANDS}
                onChange={(timerPosition: DisplayBand) => patchStyle({ layout: { timerPosition } })}
              />
            </Field>

            <Field label="Message position">
              <Segmented
                value={layout.messagePosition}
                options={DISPLAY_BANDS}
                onChange={(messagePosition: DisplayBand) =>
                  patchStyle({ layout: { messagePosition } })
                }
              />
            </Field>

            <Field label="Progress indicator position">
              <Segmented
                value={layout.progressPosition}
                options={DISPLAY_BANDS}
                onChange={(progressPosition: DisplayBand) =>
                  patchStyle({ layout: { progressPosition } })
                }
              />
            </Field>

            <Field label="Overtime counter position">
              <select
                value={layout.overtimeCorner}
                onChange={(event) =>
                  patchStyle({
                    layout: { overtimeCorner: event.currentTarget.value as DisplayCorner },
                  })
                }
                className={SELECT_CLASS}
              >
                {DISPLAY_CORNERS.map((corner) => (
                  <option key={corner.value} value={corner.value}>
                    {corner.label}
                  </option>
                ))}
              </select>
            </Field>

            <Field label="Overall alignment">
              <Segmented
                value={layout.align}
                options={DISPLAY_ALIGNS}
                onChange={(align: DisplayAlign) => patchStyle({ layout: { align } })}
              />
            </Field>

            <RangeField
              label="Spacing"
              value={layout.spacing}
              min={0.5}
              max={2}
              step={0.05}
              format={(spacing) => `${Math.round(spacing * 100)}%`}
              onChange={(spacing) => patchStyle({ layout: { spacing } })}
            />

            <div className="flex flex-col divide-y divide-[#2b2b31] border-t border-hairline pt-1">
              <Toggle
                checked={layout.showSessionInfo}
                onChange={(showSessionInfo) => patchStyle({ layout: { showSessionInfo } })}
                label="Session information"
                description="Show the running session's name."
              />
              <Toggle
                checked={layout.showStageInfo}
                onChange={(showStageInfo) => patchStyle({ layout: { showStageInfo } })}
                label="Stage information"
                description="Show “Stage n / m · name” during a session."
              />
              <Toggle
                checked={layout.showTimerName}
                onChange={(showTimerName) => patchStyle({ layout: { showTimerName } })}
                label="Timer name"
                description="Print the timer title above the digits when no session is loaded."
              />
              <Toggle
                checked={layout.showProgressBar}
                onChange={(showProgressBar) => patchStyle({ layout: { showProgressBar } })}
                label="Progress indicator"
                description="Fill as the countdown elapses, straight from the engine's progress."
              />
              <Toggle
                checked={layout.showMessages}
                onChange={(showMessages) => patchStyle({ layout: { showMessages } })}
                label="Messages"
                description="Allow presets and stage messages to be presented."
              />
            </div>
          </div>
        </Card>

        <Card>
          <CardHeader
            icon={<Type size={16} />}
            eyebrow="Typography"
            title="Countdown digits"
            subtitle="Sizes multiply a responsive base, so one setting fits a laptop and a 4K panel."
          />
          <div className="px-5 pb-5">
            <TypographyPanel
              title="Timer"
              caption="Font, weight, size and alignment of the countdown itself."
              value={timer}
              onChange={(patch) => patchStyle({ timer: patch })}
            />
          </div>
        </Card>

        <Card>
          <CardHeader
            icon={<Type size={16} />}
            eyebrow="Typography"
            title="Messages"
            subtitle="Configured separately from the timer, so both can be tuned independently."
          />
          <div className="px-5 pb-5">
            <TypographyPanel
              title="Messages"
              caption="Scales each message's own library size rather than replacing it."
              value={messageText}
              onChange={(patch) => patchStyle({ messageText: patch })}
            />
          </div>
        </Card>
      </div>

      {/* -------------------------------------------- colours & background */}
      <div className="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)]">
        <Card>
          <CardHeader
            icon={<Palette size={16} />}
            eyebrow="Colours"
            title="Colours & background"
            subtitle="A dark surface with a bright timer reads best at a distance; the live preview shows the contrast you get."
          />
          <div className="grid gap-4 px-5 pb-5 md:grid-cols-2">
            <ColorField
              label="Background colour"
              value={theme.background}
              onChange={(background) => patchStyle({ theme: { background } })}
            />

            <ColorField
              label="Timer text colour"
              value={theme.timerColor}
              onChange={(timerColor) => patchStyle({ theme: { timerColor } })}
            />

            <ColorField
              label="Progress indicator colour"
              value={theme.progressColor}
              onChange={(progressColor) => patchStyle({ theme: { progressColor } })}
            />

            <ColorField
              label="Overtime text colour"
              value={theme.overtimeColor}
              onChange={(overtimeColor) => patchStyle({ theme: { overtimeColor } })}
            />

            <div className="flex flex-col gap-1.5">
              <span className={LABEL_CLASS}>Message text colour</span>
              <Segmented
                value={theme.messageColor === MESSAGE_COLOR_FROM_MESSAGE ? "own" : "override"}
                options={[
                  { value: "own", label: "From message" },
                  { value: "override", label: "Override" },
                ]}
                onChange={(mode) =>
                  patchStyle({
                    theme: {
                      messageColor:
                        mode === "own" ? MESSAGE_COLOR_FROM_MESSAGE : "#F4F4F5",
                    },
                  })
                }
              />
              {theme.messageColor === MESSAGE_COLOR_FROM_MESSAGE ? (
                <span className="text-[11px] text-text-faint">
                  Each message keeps the colour set in the message library.
                </span>
              ) : (
                <ColorInput
                  value={theme.messageColor}
                  onChange={(messageColor) => patchStyle({ theme: { messageColor } })}
                />
              )}
            </div>

            <div className="flex flex-col gap-1.5">
              <span className={LABEL_CLASS}>Background image</span>
              <div className="flex items-center gap-2">
                <Button
                  icon={<ImagePlus size={16} />}
                  onClick={() => fileInput.current?.click()}
                >
                  Choose Image
                </Button>
                {theme.backgroundImage ? (
                  <Button
                    icon={<X size={16} />}
                    onClick={() => patchStyle({ theme: { backgroundImage: null } })}
                  >
                    Remove
                  </Button>
                ) : null}
              </div>
              <input
                ref={fileInput}
                type="file"
                accept="image/*"
                className="hidden"
                onChange={(event) => {
                  pickBackgroundImage(event.currentTarget.files?.[0]);
                  event.currentTarget.value = "";
                }}
              />
              <span className="text-[11px] text-text-faint">
                {theme.backgroundImage
                  ? "Stored with the layout and dimmed behind the timer for readability."
                  : "Optional. Dimmed behind the timer so the digits stay readable."}
              </span>
              {imageError ? <span className="text-[11px] text-danger">{imageError}</span> : null}
            </div>
          </div>
        </Card>

        <Card>
          <CardHeader
            icon={<Volume2 size={16} />}
            eyebrow="Sound"
            title="Notification sounds"
            subtitle="Synthesised in the control window — no assets, no extra dependency."
          />
          <div className="flex flex-col gap-3 px-5 pb-5">
            <div className="flex flex-wrap gap-2">
              {SOUND_OPTIONS.map((option) => (
                <Button
                  key={option.value}
                  size="sm"
                  disabled={option.value === "none"}
                  icon={<Play size={14} />}
                  title={`Play the ${option.label.toLowerCase()} notification`}
                  onClick={() => playSound(option.value)}
                >
                  {option.label}
                </Button>
              ))}
            </div>

            <p className="text-[12px] text-text-faint">
              Completion, warning, stage and session cues are routed by the Rust engine and played
              once, by the control window. Which cue fires, and at which events, is configured under
              Settings → Notifications.
            </p>
          </div>
        </Card>
      </div>

      {/* ------------------------------------------------------ native window */}
      <Card>
        <CardHeader
          icon={<MonitorPlay size={16} />}
          eyebrow="Native window"
          title="Display window"
          subtitle="A real, independent Windows window — never a modal or a second presentation window."
        />
        <div className="flex flex-col gap-4 px-5 pb-5">
          <DisplayControls />

          <div className="flex max-w-md items-end gap-4">
            <NumberField
              label="Default width"
              value={settings.display.defaultWidth}
              max={7680}
              suffix="px"
              onChange={(defaultWidth) => updateSettings("display", { defaultWidth })}
            />
            <NumberField
              label="Default height"
              value={settings.display.defaultHeight}
              max={4320}
              suffix="px"
              onChange={(defaultHeight) => updateSettings("display", { defaultHeight })}
            />
          </div>

          <p className="text-[12px] text-text-faint">
            Size and position are reused from the last time the window was open; these values apply
            the first time it opens.
          </p>

          <div className="flex flex-col divide-y divide-[#2b2b31] border-t border-hairline pt-1 md:max-w-xl">
            <Toggle
              checked={settings.display.startFullscreen}
              onChange={(startFullscreen) => updateSettings("display", { startFullscreen })}
              label="Start fullscreen"
              description="Applied the next time the display window opens. It never forces fullscreen on its own."
            />
            <Toggle
              checked={settings.display.alwaysOnTop}
              onChange={(alwaysOnTop) => updateSettings("display", { alwaysOnTop })}
              label="Always on top"
              description="Applied the next time the display window opens."
            />
          </div>
        </div>
      </Card>

      <Card>
        <CardHeader
          icon={<MonitorPlay size={16} />}
          eyebrow="Hardware"
          title="Connected monitors"
          subtitle="Enumerated by the Rust backend straight from Windows; selecting one retargets the open window."
          action={
            <span className="font-mono text-[12px] text-text-faint">
              {windowState.open ? "window open" : "window closed"}
            </span>
          }
        />
        <div className="px-5 pb-5">
          <MonitorSelector />
          <p className="mt-3 text-[12px] text-text-faint">
            {monitors.length} monitor(s) available{windowState.monitorName ? ` · presenting on ${windowState.monitorName}` : ""}.
          </p>
        </div>
      </Card>

      <Modal
        open={dialog !== null}
        onClose={() => setDialog(null)}
        title={dialog?.mode === "rename" ? "Rename layout" : "Save layout"}
        description={
          dialog?.mode === "rename"
            ? "Give this custom layout a new name."
            : "Store the applied settings as a new custom layout you can reuse."
        }
        width={460}
        footer={
          <>
            <Button onClick={() => setDialog(null)}>Cancel</Button>
            <Button variant="primary" onClick={commitDialog}>
              {dialog?.mode === "rename" ? "Rename" : "Save"}
            </Button>
          </>
        }
      >
        <Field label="Layout name">
          <input
            autoFocus
            type="text"
            value={draftName}
            onChange={(event) => setDraftName(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") commitDialog();
            }}
            className="h-11 w-full rounded-xl border border-border bg-elevated px-3 text-[14px] text-text outline-none transition-colors duration-150 focus:border-accent/70"
          />
        </Field>
      </Modal>
    </div>
  );
}
