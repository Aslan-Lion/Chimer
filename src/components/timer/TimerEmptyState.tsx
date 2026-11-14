import { Clock3 } from "lucide-react";
import { setCreateTimerOpen } from "../../stores/uiStore";
import { Button } from "../ui/Button";
import { Card } from "../ui/Card";
import { EmptyState } from "../ui/EmptyState";

/** Shown on the dashboard whenever no timer is armed. */
export function TimerEmptyState() {
  return (
    <Card className="flex h-full items-center justify-center p-6">
      <EmptyState
        icon={<Clock3 size={20} />}
        title="No active timer"
        description="Create a timer to arm the workspace. Once started, both this window and the display window stay synchronised."
        action={
          <Button
            variant="primary"
            icon={<Clock3 size={16} />}
            onClick={() => setCreateTimerOpen(true)}
          >
            Create timer
          </Button>
        }
      />
    </Card>
  );
}
