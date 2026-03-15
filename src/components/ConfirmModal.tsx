import { useEffect, useMemo, useRef, useState } from "react";

interface ConfirmModalProps {
  open: boolean;
  level: "standard" | "dangerous";
  title: string;
  description: string;
  confirmText?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
}

const TITLE_ID = "confirm-modal-title";
const DESC_ID = "confirm-modal-desc";
const CONFIRM_INPUT_ID = "confirm-modal-input";

export default function ConfirmModal({
  open,
  level,
  title,
  description,
  confirmText,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  onConfirm,
  onCancel,
}: ConfirmModalProps) {
  const [typed, setTyped] = useState("");
  const requiresText = level === "dangerous" && Boolean(confirmText);
  const dialogRef = useRef<HTMLDivElement>(null);

  const disabled = useMemo(() => {
    if (!requiresText || !confirmText) {
      return false;
    }
    return typed.trim() !== confirmText.trim();
  }, [typed, confirmText, requiresText]);

  useEffect(() => {
    if (!open) {
      return;
    }
    const previouslyFocused = document.activeElement as HTMLElement | null;

    const dialog = dialogRef.current;
    if (!dialog) {
      return;
    }
    const focusables = dialog.querySelectorAll<HTMLElement>(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
    );
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (first) {
      first.focus();
    }

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onCancel();
        return;
      }
      if (e.key !== "Tab" || focusables.length === 0) {
        return;
      }
      const target = e.target as HTMLElement;
      if (e.shiftKey) {
        if (target === first) {
          e.preventDefault();
          last?.focus();
        }
      } else {
        if (target === last) {
          e.preventDefault();
          first?.focus();
        }
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      previouslyFocused?.focus();
    };
  }, [open, onCancel]);

  if (!open) {
    return null;
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={TITLE_ID}
        aria-describedby={DESC_ID}
        className="w-full max-w-md rounded-lg border border-slate-200 bg-white p-4 shadow-xl"
      >
        <h3 id={TITLE_ID} className="text-lg font-semibold text-slate-900">
          {title}
        </h3>
        <p id={DESC_ID} className="mt-2 text-sm text-slate-700">
          {description}
        </p>
        {requiresText && confirmText && (
          <div className="mt-3">
            <label htmlFor={CONFIRM_INPUT_ID} className="mb-1 block text-xs text-slate-500">
              Type `{confirmText}` to continue
            </label>
            <input
              id={CONFIRM_INPUT_ID}
              className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900"
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              aria-required="true"
            />
          </div>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm text-slate-700 hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300 focus-visible:ring-offset-1"
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={disabled}
            className="rounded-md bg-blue-600 px-3 py-1.5 text-sm text-white hover:bg-blue-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-300 focus-visible:ring-offset-1 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
