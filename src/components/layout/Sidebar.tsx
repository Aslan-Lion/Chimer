import { PanelLeftClose, PanelLeftOpen, Timer } from "lucide-react";
import { NAV_GROUPS } from "../../app/navigation";
import { navigateTo, toggleSidebar, uiStore } from "../../stores/uiStore";
import { cn } from "../../lib/cn";
import { IconButton } from "../ui/IconButton";

export function Sidebar() {
  const { activePage, sidebarCollapsed } = uiStore.use();

  return (
    <aside
      className={cn(
        "flex h-full shrink-0 flex-col border-r border-border bg-sidebar transition-[width] duration-200 ease-out",
        sidebarCollapsed ? "w-[76px]" : "w-[248px]",
      )}
    >
      <div
        className={cn(
          "flex h-[68px] shrink-0 items-center border-b border-border",
          sidebarCollapsed ? "justify-center px-0" : "gap-3 px-4",
        )}
      >
        <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-[11px] border border-[#3a4270] bg-[#1b1e33] text-accent">
          <Timer size={18} />
        </span>
        {!sidebarCollapsed ? (
          <div className="min-w-0">
            <p className="truncate text-[15px] font-semibold tracking-tight text-text">
              app.timer
            </p>
            <p className="truncate text-[11px] text-text-faint">Timer management</p>
          </div>
        ) : null}
      </div>

      <nav className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-3 py-5">
        {NAV_GROUPS.map((group) => (
          <div key={group.id} className="flex flex-col gap-1">
            {!sidebarCollapsed ? (
              <p className="px-2 pb-1 text-[10px] font-semibold tracking-[0.16em] text-text-faint uppercase">
                {group.label}
              </p>
            ) : (
              <div className="mx-2 mb-1 h-px bg-hairline" />
            )}

            {group.items.map((item) => {
              const Icon = item.icon;
              const active = activePage === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  title={sidebarCollapsed ? item.label : undefined}
                  onClick={() => navigateTo(item.id)}
                  className={cn(
                    "group relative flex items-center rounded-[10px] text-[13px] font-medium transition-colors duration-150",
                    sidebarCollapsed ? "h-10 justify-center" : "h-9 gap-3 px-2.5",
                    active
                      ? "bg-elevated text-text"
                      : "text-text-muted hover:bg-[#18181c] hover:text-text",
                  )}
                >
                  {active ? (
                    <span className="absolute top-1/2 left-0 h-5 w-[2px] -translate-y-1/2 rounded-full bg-accent" />
                  ) : null}
                  <Icon size={17} className={cn("shrink-0", active && "text-accent")} />
                  {!sidebarCollapsed ? <span className="truncate">{item.label}</span> : null}
                </button>
              );
            })}
          </div>
        ))}
      </nav>

      <div
        className={cn(
          "flex shrink-0 items-center border-t border-border py-3",
          sidebarCollapsed ? "justify-center px-0" : "justify-between px-3",
        )}
      >
        {!sidebarCollapsed ? (
          <span className="px-2 text-[11px] text-text-faint">v0.1.0</span>
        ) : null}
        <IconButton
          label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
          size="sm"
          onClick={toggleSidebar}
        >
          {sidebarCollapsed ? <PanelLeftOpen size={16} /> : <PanelLeftClose size={16} />}
        </IconButton>
      </div>
    </aside>
  );
}
