import { useEffect, useRef } from "react";

/**
 * Глобальные горячие клавиши. Ключ — комбинация в нижнем регистре:
 * "mod+n" (mod = Ctrl, на macOS — Cmd), "mod+,", "escape", "f5".
 * Без модификатора сочетания не срабатывают, пока фокус в поле ввода.
 */
export function useHotkeys(map: Record<string, (e: KeyboardEvent) => void>) {
  const ref = useRef(map);
  ref.current = map;

  useEffect(() => {
    const isMac = navigator.platform.toLowerCase().includes("mac");
    const onKey = (e: KeyboardEvent) => {
      const mod = isMac ? e.metaKey : e.ctrlKey;
      const parts = [mod && "mod", e.shiftKey && "shift", e.altKey && "alt", e.key.toLowerCase()];
      const combo = parts.filter(Boolean).join("+");
      const handler = ref.current[combo];
      if (!handler) return;
      const target = e.target as HTMLElement | null;
      const typing = target && /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName);
      if (typing && !mod && e.key !== "Escape") return;
      e.preventDefault();
      handler(e);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
