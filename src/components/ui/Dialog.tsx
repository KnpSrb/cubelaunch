import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import clsx from "clsx";
import { Button, Input } from "./index";

/** Модальное окно: затемнение, закрытие по Esc и клику по фону, фокус внутрь. */
export function Dialog({
  open,
  onClose,
  title,
  children,
  footer,
  width = "md",
}: {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  width?: "sm" | "md" | "lg" | "xl";
}) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const prev = document.activeElement as HTMLElement | null;
    // Фокус на первое поле ввода, иначе на само окно.
    const first = ref.current?.querySelector<HTMLElement>("input, select, textarea, [data-autofocus]");
    (first ?? ref.current)?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      prev?.focus?.();
    };
  }, [open, onClose]);

  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-[90] flex items-center justify-center bg-black/50 p-6"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        tabIndex={-1}
        className={clsx(
          "flex max-h-full w-full flex-col rounded-lg border border-border bg-surface shadow-panel outline-none",
          width === "sm" && "max-w-md",
          width === "md" && "max-w-xl",
          width === "lg" && "max-w-3xl",
          width === "xl" && "max-w-5xl",
        )}
      >
        <div className="flex items-center gap-2 border-b border-border px-5 py-3">
          <h2 className="flex-1 text-base font-semibold">{title}</h2>
          <button type="button" aria-label={t("common.close")} onClick={onClose} className="text-muted hover:text-text">
            <X size={18} />
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>
        {footer && <div className="flex justify-end gap-2 border-t border-border px-5 py-3">{footer}</div>}
      </div>
    </div>
  );
}

export function ConfirmDialog({
  open,
  title,
  body,
  confirmLabel,
  danger,
  onConfirm,
  onClose,
}: {
  open: boolean;
  title: ReactNode;
  body: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={title}
      width="sm"
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button
            data-autofocus
            variant={danger ? "danger" : "primary"}
            onClick={() => {
              onConfirm();
              onClose();
            }}
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      <p className="text-sm">{body}</p>
    </Dialog>
  );
}

/** Диалог с одним текстовым полем (переименование, группа). */
export function PromptDialog({
  open,
  title,
  initial,
  hint,
  placeholder,
  suggestions,
  allowEmpty,
  onSubmit,
  onClose,
}: {
  open: boolean;
  title: ReactNode;
  initial: string;
  hint?: ReactNode;
  placeholder?: string;
  suggestions?: string[];
  allowEmpty?: boolean;
  onSubmit: (value: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [value, setValue] = useState(initial);
  useEffect(() => {
    if (open) setValue(initial);
  }, [open, initial]);
  const valid = allowEmpty || value.trim().length > 0;
  const submit = () => {
    if (!valid) return;
    onSubmit(value.trim());
    onClose();
  };
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={title}
      width="sm"
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" disabled={!valid} onClick={submit}>
            {t("common.save")}
          </Button>
        </>
      }
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
        className="flex flex-col gap-2"
      >
        <Input
          value={value}
          placeholder={placeholder}
          maxLength={64}
          list={suggestions ? "prompt-suggestions" : undefined}
          onChange={(e) => setValue(e.target.value)}
          onFocus={(e) => e.target.select()}
        />
        {suggestions && (
          <datalist id="prompt-suggestions">
            {suggestions.map((s) => (
              <option key={s} value={s} />
            ))}
          </datalist>
        )}
        {hint && <p className="text-xs text-muted">{hint}</p>}
      </form>
    </Dialog>
  );
}
