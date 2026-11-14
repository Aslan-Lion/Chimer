import { Component } from "react";
import type { ErrorInfo, ReactNode } from "react";
import { Button } from "./Button";

export interface ErrorBoundaryProps {
  children: ReactNode;
  /** Short name used in the fallback text, e.g. "This dialog". */
  label?: string;
}

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * Keeps a failing subtree from taking the whole window down.
 *
 * React unmounts the entire root when a render throws and nothing catches it,
 * which in a WebView leaves a blank black window that only a reload can undo.
 * Every surface that can fail in isolation — the modals in particular — is
 * therefore wrapped so the failure stays local and the shell keeps rendering.
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error("[app.timer] Unhandled render error:", error, info.componentStack);
  }

  private retry = (): void => {
    this.setState({ error: null });
  };

  render(): ReactNode {
    const { error } = this.state;
    if (error === null) return this.props.children;

    return (
      <div
        role="alert"
        className="flex flex-col items-start gap-3 rounded-xl border border-[#4a2020] bg-[#2a1414] px-4 py-3 text-left"
      >
        <div>
          <p className="text-[13px] font-semibold text-danger">
            {this.props.label ?? "This section"} could not be displayed.
          </p>
          <p className="mt-1 text-[12px] leading-relaxed text-text-muted">{error.message}</p>
        </div>
        <Button size="sm" onClick={this.retry}>
          Try again
        </Button>
      </div>
    );
  }
}
