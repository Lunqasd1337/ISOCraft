import { Component, type ErrorInfo, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, Download, RefreshCw } from "lucide-react";
import i18n from "../i18n";
import { localizeBackendError } from "../utils/backendErrors";

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
  exporting: boolean;
  diagStatus:
    | { ok: true; path: string }
    | { ok: false; error: string }
    | null;
}

export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = {
    error: null,
    exporting: false,
    diagStatus: null,
  };

  static getDerivedStateFromError(error: Error): Partial<ErrorBoundaryState> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("ISOCraft UI error boundary caught:", error, info);
  }

  private handleReload = () => {
    window.location.reload();
  };

  private handleExportDiagnostics = async () => {
    if (this.state.exporting) return;
    this.setState({ exporting: true });
    try {
      const dest = await save({
        defaultPath: "isocraft-diagnostics.log",
        filters: [{ name: i18n.t("dialogs.logFiles"), extensions: ["log", "txt"] }],
      });
      if (!dest) return;
      const savedPath = await invoke<string>("export_diagnostics", {
        destPath: dest,
      });
      this.setState({ diagStatus: { ok: true, path: savedPath } });
    } catch (err) {
      this.setState({ diagStatus: { ok: false, error: String(err) } });
    } finally {
      this.setState({ exporting: false });
    }
  };

  render() {
    const { error, exporting, diagStatus } = this.state;
    if (!error) {
      return this.props.children;
    }

    return (
      <div className="h-screen w-screen bg-canvas text-text-primary flex items-center justify-center p-6">
        <div className="w-full max-w-[520px] bg-surface border border-border-subtle rounded-lg shadow-xl p-6 space-y-4">
          <div className="flex items-start gap-3.5">
            <div className="w-10 h-10 rounded-full bg-status-danger-bg text-status-danger flex items-center justify-center shrink-0">
              <AlertTriangle className="w-5 h-5" strokeWidth={1.75} />
            </div>
            <div className="min-w-0 space-y-1">
              <h1 className="text-[16px] font-semibold text-text-primary">
                {i18n.t("errorBoundary.title")}
              </h1>
              <p className="text-[13px] text-text-secondary leading-relaxed">
                {i18n.t("errorBoundary.description")}
              </p>
            </div>
          </div>

          <pre className="p-3 rounded-md bg-canvas border border-border-subtle text-[12px] font-mono text-status-danger whitespace-pre-wrap break-words max-h-36 overflow-y-auto select-text">
            {error.message || String(error)}
          </pre>

          {diagStatus && (
            <div
              className={`px-3 py-2 rounded-md text-[12px] break-all ${
                diagStatus.ok
                  ? "bg-status-success-bg text-status-success"
                  : "bg-status-danger-bg text-status-danger"
              }`}
            >
              {diagStatus.ok
                ? i18n.t("errorBoundary.exportSuccess", { path: diagStatus.path })
                : i18n.t("errorBoundary.exportError", {
                    error: localizeBackendError(diagStatus.error),
                  })}
            </div>
          )}

          <div className="flex items-center justify-end gap-2 pt-1">
            <button
              type="button"
              onClick={this.handleExportDiagnostics}
              disabled={exporting}
              className="inline-flex items-center gap-1.5 px-3.5 py-1.5 rounded-md text-[13px] font-medium bg-surface border border-border-subtle hover:bg-subtle text-text-primary transition-all duration-150 active:scale-[0.98] disabled:opacity-50 disabled:active:scale-100"
            >
              <Download className="w-3.5 h-3.5" strokeWidth={1.6} />
              <span>{i18n.t("errorBoundary.exportDiagBtn")}</span>
            </button>
            <button
              type="button"
              onClick={this.handleReload}
              className="inline-flex items-center gap-1.5 px-3.5 py-1.5 rounded-md text-[13px] font-semibold bg-accent hover:bg-accent-hover text-accent-fg transition-all duration-150 active:scale-[0.98]"
            >
              <RefreshCw className="w-3.5 h-3.5" strokeWidth={1.7} />
              <span>{i18n.t("errorBoundary.reloadBtn")}</span>
            </button>
          </div>
        </div>
      </div>
    );
  }
}
