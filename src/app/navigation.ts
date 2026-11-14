import type { ComponentType } from "react";
import type { LucideIcon } from "lucide-react";
import {
  Clock3,
  History as HistoryIcon,
  LayoutDashboard,
  LibraryBig,
  ListOrdered,
  MessageSquareText,
  MonitorPlay,
  Settings2,
} from "lucide-react";
import DashboardPage from "../pages/Dashboard";
import TimersPage from "../pages/Timers";
import PresetsPage from "../pages/Presets";
import MessagesPage from "../pages/Messages";
import SessionsPage from "../pages/Sessions";
import DisplaySettingsPage from "../pages/DisplaySettings";
import HistoryPage from "../pages/History";
import SettingsPage from "../pages/Settings";

export type PageId =
  | "dashboard"
  | "timers"
  | "presets"
  | "messages"
  | "sessions"
  | "display"
  | "history"
  | "settings";

export interface NavItem {
  id: PageId;
  label: string;
  description: string;
  icon: LucideIcon;
  page: ComponentType;
}

export interface NavGroup {
  id: string;
  label: string;
  items: NavItem[];
}

export const NAV_GROUPS: NavGroup[] = [
  {
    id: "workspace",
    label: "Workspace",
    items: [
      {
        id: "dashboard",
        label: "Dashboard",
        description: "Live workspace overview",
        icon: LayoutDashboard,
        page: DashboardPage,
      },
      {
        id: "timers",
        label: "Timers",
        description: "Every timer you have configured",
        icon: Clock3,
        page: TimersPage,
      },
    ],
  },
  {
    id: "library",
    label: "Library",
    items: [
      {
        id: "presets",
        label: "Timer Presets",
        description: "Reusable countdown configurations",
        icon: LibraryBig,
        page: PresetsPage,
      },
      {
        id: "messages",
        label: "Messages",
        description: "Presentation messages for the display window",
        icon: MessageSquareText,
        page: MessagesPage,
      },
      {
        id: "sessions",
        label: "Sessions",
        description: "Sequential multi-stage countdown programmes",
        icon: ListOrdered,
        page: SessionsPage,
      },
    ],
  },
  {
    id: "display",
    label: "Display",
    items: [
      {
        id: "display",
        label: "Display Window",
        description: "Second-screen presentation window",
        icon: MonitorPlay,
        page: DisplaySettingsPage,
      },
    ],
  },
  {
    id: "system",
    label: "System",
    items: [
      {
        id: "history",
        label: "History",
        description: "Completed and stopped sessions",
        icon: HistoryIcon,
        page: HistoryPage,
      },
      {
        id: "settings",
        label: "Settings",
        description: "Application preferences",
        icon: Settings2,
        page: SettingsPage,
      },
    ],
  },
];

const ALL_ITEMS = NAV_GROUPS.flatMap((group) => group.items);

export const PAGE_INDEX: Record<PageId, NavItem> = ALL_ITEMS.reduce(
  (accumulator, item) => {
    accumulator[item.id] = item;
    return accumulator;
  },
  {} as Record<PageId, NavItem>,
);
