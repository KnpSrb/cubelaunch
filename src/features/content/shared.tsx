import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { isTauri } from "@tauri-apps/api/core";
import clsx from "clsx";
import { Package, Upload } from "lucide-react";
import { useJobs } from "@/stores/jobs";
import { isActive } from "@/lib/types";

/**
 * Число завершённых задач указанных видов для инстанса: меняется, когда задача
 * доходит до конца, — удобно как зависимость для перечитывания списка.
 */
export function useFinishedJobs(instanceId: string, kinds: string[]): number {
  return useJobs(
    (s) =>
      Object.values(s.jobs).filter((j) => j.instanceId === instanceId && kinds.includes(j.kind) && !isActive(j))
        .length,
  );
}

/** Есть ли у инстанса выполняющаяся задача указанных видов. */
export function useRunningJob(instanceId: string, kinds: string[]): boolean {
  return useJobs((s) =>
    Object.values(s.jobs).some((j) => j.instanceId === instanceId && kinds.includes(j.kind) && isActive(j)),
  );
}

export function ContentIcon({ src, size = 36, className }: { src?: string | null; size?: number; className?: string }) {
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [src]);
  if (!src || failed) {
    return (
      <div
        className={clsx("flex shrink-0 items-center justify-center rounded-md bg-surface-2 text-muted", className)}
        style={{ width: size, height: size }}
      >
        <Package size={size * 0.5} />
      </div>
    );
  }
  return (
    <img
      src={src}
      alt=""
      loading="lazy"
      onError={() => setFailed(true)}
      className={clsx("shrink-0 rounded-md object-cover [image-rendering:pixelated]", className)}
      style={{ width: size, height: size }}
    />
  );
}

/**
 * Приём файлов, перетащенных из проводника (через событие Tauri — оно даёт
 * настоящие пути). Пока `enabled`, при перетаскивании показывается подложка.
 */
export function useFileDrop(enabled: boolean, onDrop: (paths: string[]) => void) {
  const [over, setOver] = useState(false);
  const handler = useRef(onDrop);
  handler.current = onDrop;
  useEffect(() => {
    if (!enabled || !isTauri()) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void import("@tauri-apps/api/webview").then(({ getCurrentWebview }) =>
      getCurrentWebview()
        .onDragDropEvent((e) => {
          const p = e.payload;
          if (p.type === "enter" || p.type === "over") setOver(true);
          else if (p.type === "leave") setOver(false);
          else if (p.type === "drop") {
            setOver(false);
            if (p.paths.length) handler.current(p.paths);
          }
        })
        .then((u) => {
          if (disposed) u();
          else unlisten = u;
        }),
    );
    return () => {
      disposed = true;
      unlisten?.();
      setOver(false);
    };
  }, [enabled]);
  return over;
}

export function DropOverlay({ show, label }: { show: boolean; label: string }) {
  const { t } = useTranslation();
  if (!show) return null;
  return (
    <div className="pointer-events-none absolute inset-2 z-20 flex flex-col items-center justify-center gap-2 rounded-lg border-2 border-dashed border-accent bg-bg/85 text-accent">
      <Upload size={32} />
      <p className="text-sm font-medium">{label || t("content.dropHere")}</p>
    </div>
  );
}
