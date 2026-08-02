import { X } from "lucide-react";
import { useEffect, useId, useRef, type ReactNode } from "react";

interface Props {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** `drawer` slides in from the right (Settings); `dialog` is centred. */
  variant?: "drawer" | "dialog";
  /** Extra class on the panel, e.g. to narrow the About box. */
  panelClassName?: string;
}

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * Shared shell for the Settings drawer, the Podcast dialog and the About box.
 *
 * Adds the modal semantics all three were missing: a labelled `dialog` role,
 * Escape to close, an initial focus target, focus restored to the trigger on
 * close, and a focus trap — previously Tab walked straight out of the panel into
 * the toolbar underneath, which stayed operable behind the overlay.
 */
export function Modal({ title, onClose, children, variant = "dialog", panelClassName }: Props) {
  const panelRef = useRef<HTMLDivElement | null>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const titleId = useId();

  useEffect(() => {
    returnFocusRef.current = document.activeElement as HTMLElement | null;
    const panel = panelRef.current;
    // Focus the first control so keyboard users start inside the panel.
    const first = panel?.querySelector<HTMLElement>(FOCUSABLE);
    (first ?? panel)?.focus();

    return () => returnFocusRef.current?.focus?.();
  }, []);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
        return;
      }
      if (e.key !== "Tab") return;

      const panel = panelRef.current;
      if (!panel) return;
      const items = Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
        (el) => el.offsetParent !== null || el === document.activeElement,
      );
      if (items.length === 0) {
        e.preventDefault();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [onClose]);

  const Panel = variant === "drawer" ? "aside" : "div";
  const panelClass =
    variant === "drawer" ? "drawer" : `modal-dialog${panelClassName ? ` ${panelClassName}` : ""}`;

  return (
    <div
      className={`drawer-overlay${variant === "dialog" ? " about-overlay" : ""}`}
      // Closing on click rather than mousedown: releasing a text selection that
      // started inside the panel used to dismiss it.
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <Panel
        ref={panelRef as never}
        className={panelClass}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <div className="drawer-header">
          <strong id={titleId}>{title}</strong>
          <button
            type="button"
            className="icon-btn"
            title="Close"
            aria-label="Close"
            onClick={onClose}
          >
            <X size={18} aria-hidden />
          </button>
        </div>
        <div className="drawer-body">{children}</div>
      </Panel>
    </div>
  );
}
