// Базовые UI-примитивы. Цвета только через токены темы (bg-surface, text-muted, accent…).

import clsx from "clsx";
import type {
  ButtonHTMLAttributes,
  ComponentProps,
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
} from "react";

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

export function Button({
  variant = "secondary",
  size = "md",
  icon,
  className,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: "sm" | "md" | "lg";
  icon?: ReactNode;
}) {
  return (
    <button
      type="button"
      className={clsx(
        "inline-flex items-center justify-center gap-2 rounded-md font-medium whitespace-nowrap transition-colors",
        "disabled:cursor-not-allowed disabled:opacity-50",
        size === "sm" && "h-7 px-2.5 text-xs",
        size === "md" && "h-8 px-3 text-sm",
        size === "lg" && "h-11 px-5 text-base",
        variant === "primary" && "bg-accent text-accent-fg hover:brightness-110",
        variant === "secondary" && "border border-border bg-surface-2 text-text hover:bg-surface-3",
        variant === "ghost" && "text-text hover:bg-surface-2",
        variant === "danger" && "bg-danger text-white hover:brightness-110",
        className,
      )}
      {...rest}
    >
      {icon}
      {children}
    </button>
  );
}

const fieldBase =
  "h-8 rounded-md border border-border bg-surface px-2.5 text-sm text-text outline-none transition-colors " +
  "placeholder:text-muted focus:border-accent disabled:opacity-50";

export function Input({ className, ...rest }: ComponentProps<"input">) {
  return <input className={clsx(fieldBase, className)} {...rest} />;
}

/** Числовое поле, отдающее number (пустое значение → 0). */
export function NumberInput({
  value,
  onValueChange,
  className,
  suffix,
  ...rest
}: Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange" | "type"> & {
  value: number;
  onValueChange: (v: number) => void;
  suffix?: string;
}) {
  return (
    <div className="inline-flex items-center gap-2">
      <input
        type="number"
        className={clsx(fieldBase, "w-28", className)}
        value={Number.isFinite(value) ? value : 0}
        onChange={(e) => onValueChange(e.target.value === "" ? 0 : Number(e.target.value))}
        {...rest}
      />
      {suffix && <span className="text-sm text-muted">{suffix}</span>}
    </div>
  );
}

export function Select<T extends string>({
  value,
  onValueChange,
  options,
  className,
  ...rest
}: Omit<SelectHTMLAttributes<HTMLSelectElement>, "value" | "onChange"> & {
  value: T;
  onValueChange: (v: T) => void;
  options: { value: T; label: string }[];
}) {
  return (
    <select
      className={clsx(fieldBase, "pr-7", className)}
      value={value}
      onChange={(e) => onValueChange(e.target.value as T)}
      {...rest}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export function Switch({
  checked,
  onCheckedChange,
  label,
  ariaLabel,
  disabled,
}: {
  checked: boolean;
  onCheckedChange: (v: boolean) => void;
  /** Без подписи — только переключатель (подпись для скринридера — ariaLabel). */
  label?: ReactNode;
  ariaLabel?: string;
  disabled?: boolean;
}) {
  return (
    <label className={clsx("flex items-center gap-3", label ? "py-1" : "", disabled ? "opacity-50" : "cursor-pointer")}>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={ariaLabel}
        disabled={disabled}
        onClick={() => onCheckedChange(!checked)}
        className={clsx(
          "relative h-5 w-9 shrink-0 rounded-full transition-colors",
          checked ? "bg-accent" : "bg-surface-3",
        )}
      >
        <span
          className={clsx(
            "absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform",
            checked && "translate-x-4",
          )}
        />
      </button>
      {label && <span className="text-sm">{label}</span>}
    </label>
  );
}

/** Сегментированный переключатель (radio-группа кнопками). */
export function Segmented<T extends string>({
  value,
  onValueChange,
  options,
}: {
  value: T;
  onValueChange: (v: T) => void;
  options: { value: T; label: ReactNode }[];
}) {
  return (
    <div role="radiogroup" className="inline-flex rounded-md border border-border bg-surface-2 p-0.5">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onValueChange(o.value)}
          className={clsx(
            "inline-flex h-7 items-center gap-1.5 rounded px-3 text-sm transition-colors",
            value === o.value ? "bg-surface text-text shadow-panel" : "text-muted hover:text-text",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Field({
  label,
  hint,
  children,
}: {
  label: ReactNode;
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="grid grid-cols-[220px_1fr] items-start gap-4 py-2.5">
      <div className="pt-1.5 text-sm font-medium">{label}</div>
      <div className="flex min-w-0 flex-col gap-1.5">
        {children}
        {hint && <p className="text-xs text-muted">{hint}</p>}
      </div>
    </div>
  );
}

export function Section({ title, children, id }: { title: ReactNode; children: ReactNode; id?: string }) {
  return (
    <section id={id} className="rounded-lg border border-border bg-surface p-5 shadow-panel">
      <h2 className="mb-2 text-base font-semibold">{title}</h2>
      <div className="divide-y divide-border">{children}</div>
    </section>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded border border-border bg-surface-2 px-1.5 py-0.5 font-mono text-[11px] text-muted">
      {children}
    </kbd>
  );
}
