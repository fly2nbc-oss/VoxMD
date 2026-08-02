import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/**
 * Last resort for a render-time crash. Without it the webview goes blank and the
 * user has no indication of what happened or how to recover.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Unhandled render error:", error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;

    return (
      <div className="crash-screen" role="alert">
        <h1>VoxMD hit an unexpected error</h1>
        <p>
          Your settings and any Markdown files already written are unaffected. Reloading usually
          clears this.
        </p>
        <pre className="crash-detail">{error.message}</pre>
        <button type="button" className="btn-primary" onClick={() => window.location.reload()}>
          Reload
        </button>
      </div>
    );
  }
}
