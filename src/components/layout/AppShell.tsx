import { AnimatePresence, motion } from "framer-motion";
import { PAGE_INDEX } from "../../app/navigation";
import { settingsStore } from "../../stores/settingsStore";
import { uiStore } from "../../stores/uiStore";
import { cn } from "../../lib/cn";
import { Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";
import { CreateTimerModal } from "../timer/CreateTimerModal";
import { DisplayWindowManager } from "../display/DisplayWindowManager";

export function AppShell() {
  const { activePage } = uiStore.use();
  const { settings } = settingsStore.use();

  const item = PAGE_INDEX[activePage];
  const Page = item.page;
  const compact = settings.appearance.density === "compact";

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-bg text-text">
      <Sidebar />

      <div className="flex min-w-0 flex-1 flex-col">
        <TopBar title={item.label} description={item.description} />

        <main
          className={cn(
            "min-h-0 flex-1 overflow-y-auto",
            compact ? "px-6 pt-5 pb-8" : "px-8 pt-6 pb-10",
          )}
        >
          <AnimatePresence mode="wait">
            <motion.div
              key={activePage}
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -6 }}
              transition={{ duration: 0.18, ease: [0.22, 1, 0.36, 1] }}
            >
              <Page />
            </motion.div>
          </AnimatePresence>
        </main>
      </div>

      <CreateTimerModal />
      <DisplayWindowManager />
    </div>
  );
}
