import { useMemo, useState } from "react";

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

  const disabled = useMemo(() => {
    if (!requiresText || !confirmText) {
      return false;
    }
    return typed.trim() !== confirmText.trim();
  }, [typed, confirmText, requiresText]);

  if (!open) {
    return null;
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
      <div className="w-full max-w-md rounded-lg border border-slate-200 bg-white p-4 shadow-xl">
        <h3 className="text-lg font-semibold text-slate-900">{title}</h3>
        <p className="mt-2 text-sm text-slate-700">{description}</p>
        {requiresText && confirmText && (
          <div className="mt-3">
            <p className="mb-1 text-xs text-slate-500">Type `{confirmText}` to continue</p>
            <input
              className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900"
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
            />
          </div>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm text-slate-700 hover:bg-slate-100"
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={disabled}
            className="rounded-md bg-blue-600 px-3 py-1.5 text-sm text-white hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
