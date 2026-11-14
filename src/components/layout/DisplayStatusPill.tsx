import { Badge } from "../ui/Badge";
import { StatusDot } from "../ui/StatusDot";
import { displayStore } from "../../stores/displayStore";

/** Reports the live state of the native presentation window. */
export function DisplayStatusPill() {
  const { windowState } = displayStore.use();

  if (!windowState.open) {
    return (
      <Badge tone="neutral">
        <StatusDot tone="idle" />
        Display closed
      </Badge>
    );
  }

  const mode = windowState.fullscreen ? "fullscreen" : windowState.maximized ? "maximised" : "windowed";

  return (
    <Badge tone="success">
      <StatusDot tone="running" pulse />
      Display open · {mode}
    </Badge>
  );
}
