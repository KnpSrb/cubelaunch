import { useEffect, useRef, useState, type ReactNode } from "react";
import clsx from "clsx";

export interface MenuItem {
  label: ReactNode;
  icon?: ReactNode;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
}

function MenuItems({ items, onDone }: { items: (MenuItem | "separator")[]; onDone: () => void }) {
  return (
    <>
      {items.map((item, i) =>
        item === "separator" ? (
          <div key={i} className="my-1 h-px bg-border" role="separator" />
        ) : (
          <button
            key={i}
            type="button"
            role="menuitem"
            disabled={item.disabled}
            onClick={() => {
              onDone();
              item.onSelect();
            }}
            className={clsx(
              "flex w-full items-center gap-2.5 rounded px-2.5 py-1.5 text-left text-sm",
              "hover:bg-surface-2 disabled:pointer-events-none disabled:opacity-50",
              item.danger && "text-danger",
            )}
          >
            <span className={clsx(!item.danger && "text-muted")}>{item.icon}</span>
            {item.label}
          </button>
        ),
      )}
    </>
  );
}

/** Контекстное меню по ПКМ в координатах курсора. */
export function ContextMenu({
  at,
  items,
  onClose,
}: {
  at: { x: number; y: number } | null;
  items: (MenuItem | "separator")[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    if (!at) return setPos(null);
    // Не даём меню вылезти за край окна.
    const el = ref.current;
    const w = el?.offsetWidth ?? 220;
    const h = el?.offsetHeight ?? 260;
    setPos({ x: Math.min(at.x, window.innerWidth - w - 8), y: Math.min(at.y, window.innerHeight - h - 8) });
    const close = (e: Event) => {
      if (e instanceof MouseEvent && ref.current?.contains(e.target as Node)) return;
      onClose();
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", onKey);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    document.addEventListener("scroll", close, true);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      document.removeEventListener("scroll", close, true);
    };
  }, [at, onClose]);

  if (!at) return null;
  const p = pos ?? at;
  return (
    <div
      ref={ref}
      role="menu"
      className="fixed z-[80] min-w-52 rounded-md border border-border bg-surface p-1 shadow-panel"
      style={{ left: p.x, top: p.y, visibility: pos ? "visible" : "hidden" }}
    >
      <MenuItems items={items} onDone={onClose} />
    </div>
  );
}

/** Выпадающее меню: закрывается по клику снаружи и по Escape. */
export function Menu({ trigger, items, align = "left" }: {
  trigger: (props: { open: boolean; toggle: () => void }) => ReactNode;
  items: (MenuItem | "separator")[];
  align?: "left" | "right";
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div ref={ref} className="relative">
      {trigger({ open, toggle: () => setOpen((o) => !o) })}
      {open && (
        <div
          role="menu"
          className={clsx(
            "absolute top-full z-50 mt-1 min-w-52 rounded-md border border-border bg-surface p-1 shadow-panel",
            align === "right" ? "right-0" : "left-0",
          )}
        >
          <MenuItems items={items} onDone={() => setOpen(false)} />
        </div>
      )}
    </div>
  );
}
