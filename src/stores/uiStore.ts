import type { PageId } from "../app/navigation";
import { createStore } from "./createStore";

interface UiState {
  activePage: PageId;
  sidebarCollapsed: boolean;
  createTimerOpen: boolean;
  /** Message currently being previewed on the Messages page. */
  previewMessageId: string | null;
}

export const uiStore = createStore<UiState>({
  activePage: "dashboard",
  sidebarCollapsed: false,
  createTimerOpen: false,
  previewMessageId: null,
});

export function navigateTo(page: PageId): void {
  uiStore.set({ activePage: page });
}

export function toggleSidebar(): void {
  uiStore.set((previous) => ({ sidebarCollapsed: !previous.sidebarCollapsed }));
}

export function setCreateTimerOpen(open: boolean): void {
  uiStore.set({ createTimerOpen: open });
}

export function setPreviewMessage(id: string | null): void {
  uiStore.set({ previewMessageId: id });
}
