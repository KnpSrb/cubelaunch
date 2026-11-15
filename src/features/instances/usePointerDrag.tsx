import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { createPortal } from "react-dom";

/** Что перетаскивается: группа целиком или инстанс. */
export type DragItem = { type: "group"; group: string } | { type: "instance"; id: string; label: string };
/** Куда отпустили: группа (`""` — без группы) и половина (для порядка групп). */
export type DropTarget = { group: string; after: boolean };

interface DragState {
  item: DragItem;
  x: number;
  y: number;
  target: DropTarget | null;
}

const THRESHOLD = 5;

function targetAt(x: number, y: number): DropTarget | null {
  const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-drop-group]");
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { group: el.dataset.dropGroup ?? "", after: y > r.top + r.height / 2 };
}

/**
 * Перетаскивание на pointer-событиях. HTML5 drag & drop в Tauri на Windows
 * недоступен, пока включён приём файлов из проводника (dragDrop окна).
 * Цели помечаются атрибутом `data-drop-group`.
 */
export function usePointerDrag(onDrop: (item: DragItem, target: DropTarget) => void) {
  const [drag, setDrag] = useState<DragState | null>(null);
  const cleanup = useRef<(() => void) | null>(null);
  const onDropRef = useRef(onDrop);
  onDropRef.current = onDrop;

  useEffect(() => () => cleanup.current?.(), []);

  const start = (e: ReactPointerEvent, item: DragItem) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("input, textarea, select")) return;
    const sx = e.clientX;
    const sy = e.clientY;
    let active = false;
    let last: DragState | null = null;

    const move = (ev: PointerEvent) => {
      if (!active && Math.hypot(ev.clientX - sx, ev.clientY - sy) < THRESHOLD) return;
      active = true;
      last = { item, x: ev.clientX, y: ev.clientY, target: targetAt(ev.clientX, ev.clientY) };
      setDrag(last);
    };
    const finish = (drop: boolean) => {
      cleanup.current?.();
      if (!active) return;
      // Клик после перетаскивания не должен выбирать/открывать элемент.
      const swallow = (ce: MouseEvent) => {
        ce.stopPropagation();
        ce.preventDefault();
      };
      window.addEventListener("click", swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
      setDrag(null);
      if (drop && last?.target) onDropRef.current(item, last.target);
    };
    const up = () => finish(true);
    const key = (ev: KeyboardEvent) => ev.key === "Escape" && finish(false);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    window.addEventListener("keydown", key, true);
    cleanup.current = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      window.removeEventListener("keydown", key, true);
      cleanup.current = null;
    };
  };

  return { drag, start };
}

/** Подпись у курсора во время перетаскивания. */
export function DragGhost({ drag, label }: { drag: DragState | null; label: string }) {
  if (!drag) return null;
  return createPortal(
    <div
      className="pointer-events-none fixed z-[100] rounded-md border border-accent bg-surface px-2.5 py-1 text-sm font-medium shadow-panel"
      style={{ left: drag.x + 12, top: drag.y + 8 }}
    >
      {label}
    </div>,
    document.body,
  );
}
