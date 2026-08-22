import { Component, type ErrorInfo, type ReactNode } from "react";
import { detectUiLanguage, format, messagesFor, type MessageKey } from "./i18n";

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

    // Read straight from the browser locale: this boundary sits *outside* the
    // i18n provider, and the settings store it would read is exactly the kind of
    // thing that may have failed.
    const messages = messagesFor(detectUiLanguage());
    const t = (key: MessageKey) => format(messages, key);

    return (
      <div className="crash-screen" role="alert">
        <h1>{t("crash.title")}</h1>
        <p>{t("crash.body")}</p>
        <pre className="crash-detail">{error.message}</pre>
        <button type="button" className="btn-primary" onClick={() => window.location.reload()}>
          {t("crash.reload")}
        </button>
      </div>
    );
  }
}
