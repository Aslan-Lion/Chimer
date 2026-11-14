import { Settings2 } from "lucide-react";
import { navigateTo } from "../../stores/uiStore";
import { IconButton } from "../ui/IconButton";
import { DisplayStatusPill } from "./DisplayStatusPill";

export interface TopBarProps {
  title: string;
  description: string;
}

export function TopBar({ title, description }: TopBarProps) {
  return (
    <header className="flex h-[68px] shrink-0 items-center justify-between gap-6 border-b border-border bg-bg px-8">
      <div className="min-w-0">
        <p className="text-[11px] font-medium tracking-[0.14em] text-text-faint uppercase">
          app.timer
        </p>
        <h1 className="truncate text-[17px] font-semibold tracking-tight text-text">{title}</h1>
      </div>

      <div className="flex shrink-0 items-center gap-3">
        <DisplayStatusPill />
        <IconButton label="Open settings" onClick={() => navigateTo("settings")}>
          <Settings2 size={17} />
        </IconButton>
      </div>
      <span className="sr-only">{description}</span>
    </header>
  );
}
