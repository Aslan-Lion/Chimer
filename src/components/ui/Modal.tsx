import { useEffect } from "react";
import type { ReactNode } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { X } from "lucide-react";
import { ErrorBoundary } from "./ErrorBoundary";
import { IconButton } from "./IconButton";

export interface ModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
}

export function Modal({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  width = 520,
}: ModalProps) {
  useEffect(() => {
    if (!open) return undefined;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          className="fixed inset-0 z-50 flex items-center justify-center p-6"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.16 }}
        >
          <div
            className="absolute inset-0 bg-black/60"
            onClick={onClose}
            aria-hidden="true"
          />
          <motion.div
            role="dialog"
            aria-modal="true"
            aria-label={title}
            className="relative w-full overflow-hidden rounded-[20px] border border-border bg-card shadow-[0_24px_60px_rgba(0,0,0,0.55)]"
            style={{ maxWidth: width }}
            initial={{ opacity: 0, y: 12, scale: 0.985 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 8, scale: 0.99 }}
            transition={{ duration: 0.18, ease: [0.22, 1, 0.36, 1] }}
          >
            <header className="flex items-start justify-between gap-4 border-b border-border px-5 py-4">
              <div>
                <h2 className="text-[15px] font-semibold text-text">{title}</h2>
                {description ? (
                  <p className="mt-0.5 text-[13px] text-text-muted">{description}</p>
                ) : null}
              </div>
              <IconButton label="Close" size="sm" onClick={onClose}>
                <X size={16} />
              </IconButton>
            </header>

            <div className="px-5 py-4">
              {/* A failure inside a form stays inside the dialog: the shell and
                  the rest of the page keep rendering instead of going blank. */}
              <ErrorBoundary label="This dialog">{children}</ErrorBoundary>
            </div>

            {footer ? (
              <footer className="flex items-center justify-end gap-2 border-t border-border bg-[#141417] px-5 py-4">
                {footer}
              </footer>
            ) : null}
          </motion.div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}
