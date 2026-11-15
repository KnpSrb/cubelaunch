import clsx from "clsx";

/** Полоса прогресса; `value` 0..1, `undefined` — неопределённый прогресс. */
export function ProgressBar({ value, className }: { value?: number; className?: string }) {
  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value === undefined ? undefined : Math.round(value * 100)}
      className={clsx("h-1.5 overflow-hidden rounded-full bg-surface-3", className)}
    >
      <div
        className={clsx("h-full rounded-full bg-accent transition-[width] duration-200", value === undefined && "w-1/3 animate-pulse")}
        style={value === undefined ? undefined : { width: `${Math.max(2, value * 100)}%` }}
      />
    </div>
  );
}
