import type { DisplayLayoutPreset, DisplayStyle } from "../types";
import { createStore } from "./createStore";
import { readStorage, writeStorage } from "../lib/storage";
import { createId } from "../lib/id";
import {
  BUILT_IN_LAYOUTS,
  DEFAULT_DISPLAY_STYLE,
  DEFAULT_LAYOUT_ID,
  findLayout,
  mergeStyle,
  normaliseCustomLayouts,
  normaliseStyle,
  type DisplayStylePatch,
} from "../lib/display";

const STORAGE_KEY = "app-timer.display-config";

/**
 * The presentation configuration document.
 *
 * It owns exactly one copy of the display style — layout, typography and
 * colours — and the custom layouts the operator saved. `settings.display` holds
 * only how the native window itself opens and behaves, so no flag is ever
 * stored in two places. Rust receives the applied style through
 * `PresentationOptions` and re-broadcasts it untouched.
 */
interface DisplayConfigState {
  /** Layout the applied style was based on, built-in or custom. */
  selectedLayoutId: string;
  /** Layouts saved by the operator. Built-ins are never stored here. */
  customLayouts: DisplayLayoutPreset[];
  /** The style currently applied to the presentation surface. */
  style: DisplayStyle;
}

interface StoredConfig extends Partial<DisplayConfigState> {}

function load(): DisplayConfigState {
  const stored = readStorage<StoredConfig>(STORAGE_KEY, {});
  const customLayouts = normaliseCustomLayouts(stored.customLayouts);
  const style = normaliseStyle(stored.style);

  // A selection that points at a layout which no longer exists falls back to
  // the default, so the interface never shows a preset that cannot be found.
  const known = [...BUILT_IN_LAYOUTS, ...customLayouts].some(
    (layout) => layout.id === stored.selectedLayoutId,
  );

  return {
    selectedLayoutId: known ? (stored.selectedLayoutId as string) : DEFAULT_LAYOUT_ID,
    customLayouts,
    style,
  };
}

export const displayConfigStore = createStore<DisplayConfigState>(load());

displayConfigStore.subscribe(() => {
  writeStorage(STORAGE_KEY, displayConfigStore.get());
});

/** Every layout the operator can choose from, built-ins first. */
export function allLayouts(): DisplayLayoutPreset[] {
  return [...BUILT_IN_LAYOUTS, ...displayConfigStore.get().customLayouts];
}

/** The layout the applied style is based on, if it still exists. */
export function selectedLayout(): DisplayLayoutPreset | null {
  const { selectedLayoutId, customLayouts } = displayConfigStore.get();
  return findLayout([...BUILT_IN_LAYOUTS, ...customLayouts], selectedLayoutId);
}

/** True when the applied style differs from the layout it came from. */
export function isModified(): boolean {
  const layout = selectedLayout();
  if (!layout) return true;
  return JSON.stringify(layout.style) !== JSON.stringify(displayConfigStore.get().style);
}

/** Applies a layout, replacing the whole style with its definition. */
export function selectLayout(id: string): void {
  const layout = findLayout(allLayouts(), id);
  if (!layout) return;
  displayConfigStore.set({ selectedLayoutId: id, style: normaliseStyle(layout.style) });
}

/** Layers a change over the applied style. The layout definitions stay intact. */
export function patchStyle(patch: DisplayStylePatch): void {
  displayConfigStore.set((previous) => ({ style: mergeStyle(previous.style, patch) }));
}

/** Replaces the applied style outright, used by the preset import/restore paths. */
export function replaceStyle(style: DisplayStyle): void {
  displayConfigStore.set({ style: normaliseStyle(style) });
}

/** Saves the applied style as a new custom layout and selects it. */
export function saveLayout(name: string): DisplayLayoutPreset {
  const layout: DisplayLayoutPreset = {
    id: createId(),
    name: name.trim() || "Custom layout",
    builtIn: false,
    style: displayConfigStore.get().style,
  };
  displayConfigStore.set((previous) => ({
    customLayouts: [layout, ...previous.customLayouts],
    selectedLayoutId: layout.id,
  }));
  return layout;
}

/** Copies a layout — built-in or custom — into a new custom layout. */
export function duplicateLayout(id: string): DisplayLayoutPreset | null {
  const source = findLayout(allLayouts(), id);
  if (!source) return null;
  const copy: DisplayLayoutPreset = {
    id: createId(),
    name: `${source.name} copy`,
    builtIn: false,
    style: normaliseStyle(source.style),
  };
  displayConfigStore.set((previous) => ({
    customLayouts: [copy, ...previous.customLayouts],
    selectedLayoutId: copy.id,
  }));
  return copy;
}

/** Renames a custom layout. Built-in names are fixed. */
export function renameLayout(id: string, name: string): void {
  const trimmed = name.trim();
  if (!trimmed) return;
  displayConfigStore.set((previous) => ({
    customLayouts: previous.customLayouts.map((layout) =>
      layout.id === id && !layout.builtIn ? { ...layout, name: trimmed } : layout,
    ),
  }));
}

/**
 * Deletes a custom layout.
 *
 * A built-in is refused outright — it ships with the application and there is
 * no stored copy to remove — and deleting the selected layout falls back to the
 * default one so the applied style always has a definition behind it.
 */
export function deleteLayout(id: string): void {
  const target = findLayout(allLayouts(), id);
  if (!target || target.builtIn) return;

  displayConfigStore.set((previous) => {
    const customLayouts = previous.customLayouts.filter((layout) => layout.id !== id);
    if (previous.selectedLayoutId !== id) return { customLayouts };
    return {
      customLayouts,
      selectedLayoutId: DEFAULT_LAYOUT_ID,
      style: DEFAULT_DISPLAY_STYLE,
    };
  });
}

/** True when a layout may be deleted: custom layouts only. */
export function canDelete(id: string | null): boolean {
  const layout = findLayout(allLayouts(), id);
  return layout !== null && !layout.builtIn;
}

/**
 * Restores the built-in defaults: the default layout is selected and the
 * applied style returns to its definition, discarding unsaved changes.
 */
export function restoreBuiltInDefaults(): void {
  displayConfigStore.set({
    selectedLayoutId: DEFAULT_LAYOUT_ID,
    style: DEFAULT_DISPLAY_STYLE,
  });
}
