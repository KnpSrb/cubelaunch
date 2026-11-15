import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import clsx from "clsx";
import { ClipboardCopy, Eraser, Save, Search, Square } from "lucide-react";
import { Button, Input, Switch } from "@/components/ui";
import { api, inTauri } from "@/lib/ipc";
import type { LogLevel, LogLine } from "@/lib/types";
import { useGames } from "@/stores/games";
import { notify } from "@/stores/toasts";

/** Сколько строк рендерим (после фильтра). Полный лог доступен через «Скопировать»/«Сохранить». */
const RENDER_LIMIT = 5000;

type Filter = "error" | "warn" | "info" | "debug";
const FILTER_OF: Record<LogLevel, Filter> = {
  fatal: "error",
  error: "error",
  warn: "warn",
  info: "info",
  debug: "debug",
  trace: "debug",
};

function formatLine(l: LogLine) {
  const head = [l.time && `[${l.time}]`, l.thread && `[${l.thread}/${l.level.toUpperCase()}]`].filter(Boolean).join(" ");
  return head ? `${head} ${l.text}` : l.text;
}

export function LogView({ instanceId, fileName }: { instanceId: string; fileName: string }) {
  const { t } = useTranslation();
  const lines = useGames((s) => s.logs[instanceId]) ?? [];
  const status = useGames((s) => s.status[instanceId]);
  const loadLog = useGames((s) => s.loadLog);
  const clearView = useGames((s) => s.clearView);
  const [filters, setFilters] = useState<Set<Filter>>(new Set(["error", "warn", "info", "debug"]));
  const [query, setQuery] = useState("");
  const [autoscroll, setAutoscroll] = useState(true);
  const scroller = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void loadLog(instanceId).catch(notify.error);
  }, [instanceId, loadLog]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return lines.filter((l) => filters.has(FILTER_OF[l.level]) && (!q || l.text.toLowerCase().includes(q)));
  }, [lines, filters, query]);
  const shown = filtered.length > RENDER_LIMIT ? filtered.slice(-RENDER_LIMIT) : filtered;

  useEffect(() => {
    if (autoscroll && scroller.current) scroller.current.scrollTop = scroller.current.scrollHeight;
  }, [shown.length, autoscroll]);

  // Ручная прокрутка вверх выключает автопрокрутку, возврат вниз — включает.
  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
    if (atBottom !== autoscroll) setAutoscroll(atBottom);
  };

  const toggle = (f: Filter) => {
    const next = new Set(filters);
    if (next.has(f)) next.delete(f);
    else next.add(f);
    setFilters(next);
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(filtered.map(formatLine).join("\n"));
      notify.success(t("log.copied"));
    } catch (e) {
      notify.error(e);
    }
  };

  const save = async () => {
    try {
      const path = await saveDialog({ defaultPath: fileName, filters: [{ name: "Log", extensions: ["log", "txt"] }] });
      if (!path) return;
      await api.saveGameLog(instanceId, path);
      notify.success(t("log.saved"));
    } catch (e) {
      notify.error(e);
    }
  };

  const running = status?.state === "running";

  return (
    <div className="flex h-full min-h-0 flex-col gap-2">
      {status && (
        <div
          className={clsx(
            "flex items-center gap-3 rounded-md border px-3 py-2 text-sm",
            status.state === "crashed" ? "border-danger text-danger" : "border-border",
          )}
        >
          {running && <span className="h-2 w-2 animate-pulse rounded-full bg-accent" />}
          <span className="flex-1">{t(`game.state.${status.state}`, { code: status.exitCode ?? "?" })}</span>
          {running && (
            <Button size="sm" variant="danger" icon={<Square size={13} />} onClick={() => api.killGame(instanceId).catch(notify.error)}>
              {t("game.stop")}
            </Button>
          )}
        </div>
      )}
      {status?.crashReport && (
        <details className="rounded-md border border-danger/50 bg-surface px-3 py-2 text-sm">
          <summary className="cursor-pointer font-medium text-danger">
            {t("game.crashReport")}: <span className="font-normal text-muted">{status.crashReport.path}</span>
          </summary>
          <pre className="mt-2 max-h-72 overflow-auto font-mono text-xs whitespace-pre-wrap select-text">
            {status.crashReport.text}
          </pre>
        </details>
      )}

      <div className="flex flex-wrap items-center gap-2">
        {(["error", "warn", "info", "debug"] as Filter[]).map((f) => (
          <label key={f} className="flex items-center gap-1.5 text-sm">
            <input type="checkbox" checked={filters.has(f)} onChange={() => toggle(f)} className="accent-[var(--accent)]" />
            {t(`log.levels.${f}`)}
          </label>
        ))}
        <div className="relative ml-auto">
          <Search size={14} className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-muted" />
          <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("log.search")} className="w-56 pl-8" />
        </div>
        <Switch checked={autoscroll} onCheckedChange={setAutoscroll} label={t("log.autoscroll")} />
      </div>

      <div
        ref={scroller}
        onScroll={onScroll}
        className="min-h-0 flex-1 overflow-auto rounded-md border border-border bg-surface p-2 font-mono text-xs leading-5 select-text"
        role="log"
        aria-live="off"
      >
        {!status && lines.length === 0 && <p className="p-2 font-sans text-sm text-muted">{t("game.notStarted")}</p>}
        {status && shown.length === 0 && <p className="p-2 font-sans text-sm text-muted">{t("log.empty")}</p>}
        {filtered.length > RENDER_LIMIT && (
          <p className="pb-1 font-sans text-muted">{t("log.truncated", { count: RENDER_LIMIT, total: filtered.length })}</p>
        )}
        {shown.map((l, i) => (
          <div
            key={i}
            className={clsx(
              "break-words whitespace-pre-wrap",
              (l.level === "error" || l.level === "fatal") && "text-danger",
              l.level === "warn" && "text-warning",
              (l.level === "debug" || l.level === "trace") && "text-muted",
              l.thread === "Cubelaunch" && "text-accent",
            )}
          >
            {formatLine(l)}
          </div>
        ))}
      </div>

      <div className="flex gap-2">
        <Button size="sm" icon={<ClipboardCopy size={14} />} onClick={copy} disabled={filtered.length === 0}>
          {t("log.copy")}
        </Button>
        {inTauri && (
          <Button size="sm" icon={<Save size={14} />} onClick={save} disabled={lines.length === 0}>
            {t("log.save")}
          </Button>
        )}
        <Button size="sm" variant="ghost" icon={<Eraser size={14} />} onClick={() => clearView(instanceId)}>
          {t("log.clear")}
        </Button>
      </div>
    </div>
  );
}
