import { useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import clsx from "clsx";
import {
  Boxes,
  ChevronDown,
  ChevronRight,
  Copy,
  Download,
  FolderInput,
  FolderOpen,
  FolderPlus,
  LayoutGrid,
  List,
  PackageOpen,
  Pencil,
  Play,
  Plus,
  ScrollText,
  Search,
  Settings2,
  ShieldCheck,
  Square,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { Button, Input, Segmented } from "@/components/ui";
import { ContextMenu, type MenuItem } from "@/components/ui/Menu";
import { ConfirmDialog, PromptDialog } from "@/components/ui/Dialog";
import { ProgressBar } from "@/components/ui/ProgressBar";
import { useSettings } from "@/stores/settings";
import { notify } from "@/stores/toasts";
import { jobForInstance, useJobs } from "@/stores/jobs";
import { useUi } from "@/stores/ui";
import { useHotkeys } from "@/app/hotkeys";
import { api } from "@/lib/ipc";
import { formatDate, progressRatio } from "@/lib/format";
import { isActive, LOADER_NAMES, loaderLabel, type Instance, type InstanceView, type JobInfo } from "@/lib/types";
import { InstanceIcon } from "./InstanceIcon";
import { DropOverlay, useFileDrop } from "@/features/content/shared";
import { usePlay } from "./usePlay";
import { useGames } from "@/stores/games";
import { useNavigate } from "react-router";
import { CloneDialog, InstanceExportDialog } from "./TransferDialogs";
import { DragGhost, usePointerDrag, type DragItem, type DropTarget } from "./usePointerDrag";

const COLLAPSED_KEY = "cubelaunch.collapsedGroups";

function loadCollapsed(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? "[]"));
  } catch {
    return new Set();
  }
}

/** Статус инстанса для отображения: идёт установка / ошибка / не установлен / готов. */
function getStatus(inst: Instance, jobs: Record<number, JobInfo>) {
  const job = jobForInstance(jobs, inst.id);
  if (isActive(job)) return { kind: "installing" as const, job: job! };
  if (!inst.installed && job?.state === "failed") return { kind: "failed" as const, job };
  if (!inst.installed) return { kind: "notInstalled" as const, job };
  return { kind: "ready" as const, job };
}

function StatusLine({ inst }: { inst: Instance }) {
  const { t } = useTranslation();
  const jobs = useJobs((s) => s.jobs);
  const running = useGames((s) => s.status[inst.id]?.state === "running");
  const st = getStatus(inst, jobs);
  if (running)
    return (
      <span className="flex items-center gap-1 text-xs text-accent">
        <span className="h-1.5 w-1.5 rounded-full bg-accent" />
        {t("game.running")}
      </span>
    );
  if (st.kind === "installing" && st.job?.kind === "launch")
    return <span className="text-xs text-muted">{st.job.stage ? t(`jobs.stage.${st.job.stage}`) : ""}</span>;
  if (st.kind === "installing") return <ProgressBar value={progressRatio(st.job!.progress)} className="w-full" />;
  if (st.kind === "failed")
    return (
      <span className="flex items-center gap-1 text-xs text-danger">
        <TriangleAlert size={12} />
        {t("instances.installFailed")}
      </span>
    );
  if (st.kind === "notInstalled") return <span className="text-xs text-warning">{t("instances.notInstalled")}</span>;
  return null;
}

export function InstancesPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const settings = useSettings((s) => s.settings);
  const save = useSettings((s) => s.save);
  const jobs = useJobs((s) => s.jobs);
  const { selectedInstance: selectedId, selectInstance, openNewInstance, openImport } = useUi();
  // Перетащенный .mrpack / .zip CurseForge — импорт модпака.
  const dropping = useFileDrop(true, (paths) => {
    const pack = paths.find((p) => /.(mrpack|zip)$/i.test(p));
    if (pack) openImport(pack);
    else notify.info(t("modpacks.dropOnlyPacks"));
  });
  const games = useGames((s) => s.status);
  const { play, stop } = usePlay();
  const navigate = useNavigate();

  const instances = useQuery({ queryKey: ["instances"], queryFn: api.listInstances });
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState(loadCollapsed);
  const [menu, setMenu] = useState<{ at: { x: number; y: number }; id: string } | null>(null);
  const [dialog, setDialog] = useState<null | "rename" | "group" | "delete" | "clone" | "export" | "newGroup">(null);
  const [groupMenu, setGroupMenu] = useState<{ at: { x: number; y: number }; group: string } | null>(null);
  const [groupDialog, setGroupDialog] = useState<null | { kind: "rename" | "delete"; group: string }>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const list = instances.data ?? [];
  const selected = list.find((i) => i.id === selectedId) ?? null;
  const groupsQuery = useQuery({ queryKey: ["groups"], queryFn: api.listGroups });
  // Порядок групп — из groups.json; пока не загрузился — по алфавиту.
  const groups = useMemo(() => {
    const fromList = [...new Set(list.map((i) => i.group).filter((g): g is string => !!g))].sort((a, b) => a.localeCompare(b));
    const order = groupsQuery.data ?? [];
    return [...order, ...fromList.filter((g) => !order.includes(g))];
  }, [list, groupsQuery.data]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return list;
    return list.filter(
      (i) => i.name.toLowerCase().includes(q) || i.minecraft.toLowerCase().includes(q) || i.group?.toLowerCase().includes(q),
    );
  }, [list, query]);

  const { drag, start: startDrag } = usePointerDrag((item: DragItem, target: DropTarget) => {
    if (item.type === "instance") {
      const inst = list.find((i) => i.id === item.id);
      if (inst && (inst.group ?? "") !== target.group) run(api.setInstanceGroup(item.id, target.group || null));
      return;
    }
    if (target.group === item.group) return;
    const rest = groups.filter((g) => g !== item.group);
    const at = target.group === "" ? rest.length : rest.indexOf(target.group) + (target.after ? 1 : 0);
    rest.splice(Math.max(0, at), 0, item.group);
    queryClient.setQueryData(["groups"], rest);
    api.setGroupOrder(rest).catch(notify.error);
  });
  const draggingInstance = drag?.item.type === "instance";

  // Группы в пользовательском порядке (пустые тоже), «Без группы» — последней.
  const sections = useMemo(() => {
    const map = new Map<string, Instance[]>();
    for (const i of filtered) {
      const key = i.group ?? "";
      map.set(key, [...(map.get(key) ?? []), i]);
    }
    const out: [string, Instance[]][] = groups.filter((g) => map.has(g) || !query.trim()).map((g) => [g, map.get(g) ?? []]);
    if (map.has("") || (draggingInstance && groups.length > 0)) out.push(["", map.get("") ?? []]);
    return out;
  }, [filtered, groups, query, draggingInstance]);

  const view = settings?.appearance.instanceView ?? "grid";
  const iconSize = settings?.appearance.iconSize ?? 64;
  const setView = (v: InstanceView) => {
    if (!settings) return;
    save({ ...settings, appearance: { ...settings.appearance, instanceView: v } }).catch(notify.error);
  };

  const toggleGroup = (g: string) => {
    const next = new Set(collapsed);
    if (next.has(g)) next.delete(g);
    else next.add(g);
    setCollapsed(next);
    try {
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...next]));
    } catch {
      /* необязательно */
    }
  };

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["instances"] });
    void queryClient.invalidateQueries({ queryKey: ["groups"] });
  };
  const run = (p: Promise<unknown>) => p.then(refresh).catch(notify.error);

  const isRunning = (i: Instance) => games[i.id]?.state === "running";
  const actions = {
    play: (i: Instance) => (isRunning(i) ? stop(i) : play(i)),
    edit: (i: Instance) => navigate(`/instance/${encodeURIComponent(i.id)}/version`),
    install: (i: Instance) => run(api.installInstance(i.id)),
    openFolder: (i: Instance) => api.openInstanceFolder(i.id).catch(notify.error),
    rename: (name: string) => selected && run(api.renameInstance(selected.id, name)),
    setGroup: (group: string) => selected && run(api.setInstanceGroup(selected.id, group || null)),
    delete: () => {
      if (!selected) return;
      const name = selected.name;
      run(
        api.deleteInstance(selected.id).then(() => {
          selectInstance(null);
          notify.success(t("selection.deleted", { name }));
        }),
      );
    },
  };

  const menuItems = (i: Instance): (MenuItem | "separator")[] => {
    const installing = isActive(jobForInstance(jobs, i.id));
    const running = isRunning(i);
    return [
      running
        ? { label: t("game.stop"), icon: <Square size={15} />, onSelect: () => actions.play(i) }
        : { label: t("selection.play"), icon: <Play size={15} />, onSelect: () => actions.play(i), disabled: installing },
      { label: t("selection.edit"), icon: <Settings2 size={15} />, onSelect: () => actions.edit(i) },
      { label: t("game.openLog"), icon: <ScrollText size={15} />, onSelect: () => navigate(`/instance/${encodeURIComponent(i.id)}/log`) },
      "separator",
      {
        label: i.installed ? t("selection.verify") : t("selection.install"),
        icon: i.installed ? <ShieldCheck size={15} /> : <Download size={15} />,
        onSelect: () => actions.install(i),
        disabled: installing || running,
      },
      { label: t("selection.rename"), icon: <Pencil size={15} />, onSelect: () => setDialog("rename") },
      { label: t("selection.setGroup"), icon: <FolderInput size={15} />, onSelect: () => setDialog("group") },
      { label: t("selection.openFolder"), icon: <FolderOpen size={15} />, onSelect: () => actions.openFolder(i) },
      { label: t("selection.copy"), icon: <Copy size={15} />, onSelect: () => setDialog("clone") },
      { label: t("instanceExport.open"), icon: <PackageOpen size={15} />, onSelect: () => setDialog("export"), disabled: installing },
      "separator",
      {
        label: t("selection.delete"),
        icon: <Trash2 size={15} />,
        onSelect: () => setDialog("delete"),
        danger: true,
        disabled: installing || running,
      },
    ];
  };

  const onContext = (e: ReactMouseEvent, i: Instance) => {
    e.preventDefault();
    selectInstance(i.id);
    setMenu({ at: { x: e.clientX, y: e.clientY }, id: i.id });
  };

  useHotkeys({
    "mod+f": () => searchRef.current?.focus(),
    f2: () => selected && setDialog("rename"),
    delete: () => selected && setDialog("delete"),
    enter: () => selected && !isRunning(selected) && play(selected),
  });

  const itemProps = (i: Instance) => ({
    onPointerDown: (e: React.PointerEvent) => startDrag(e, { type: "instance", id: i.id, label: i.name }),
    onClick: () => selectInstance(i.id),
    onDoubleClick: () => !isRunning(i) && !isActive(jobForInstance(jobs, i.id)) && play(i),
    onContextMenu: (e: ReactMouseEvent) => onContext(e, i),
    "aria-selected": selectedId === i.id,
  });

  const menuInstance = menu ? list.find((i) => i.id === menu.id) : undefined;

  return (
    <div className="relative flex h-full">
      <DropOverlay show={dropping} label={t("modpacks.dropHere")} />
      <div className="flex min-w-0 flex-1 flex-col" onClick={(e) => e.target === e.currentTarget && selectInstance(null)}>
        <div className="flex items-center gap-2 px-5 pt-4 pb-3">
          <h1 className="mr-auto text-lg font-semibold">{t("instances.title")}</h1>
          <Button variant="ghost" icon={<FolderPlus size={15} />} onClick={() => setDialog("newGroup")} title={t("groups.newHint")}>
            {t("groups.new")}
          </Button>
          <div className="relative">
            <Search size={14} className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-muted" />
            <Input
              ref={searchRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("instances.search")}
              className="w-64 pl-8"
              title="Ctrl+F"
            />
          </div>
          <Segmented<InstanceView>
            value={view}
            onValueChange={setView}
            options={[
              { value: "grid", label: <LayoutGrid size={15} aria-label={t("instances.viewGrid")} /> },
              { value: "list", label: <List size={15} aria-label={t("instances.viewList")} /> },
            ]}
          />
        </div>

        {instances.isSuccess && list.length === 0 ? (
          <div className="flex flex-1 items-center justify-center p-8">
            <div className="flex max-w-sm flex-col items-center text-center">
              <div className="mb-4 flex h-16 w-16 items-center justify-center rounded-2xl bg-surface-2 text-accent">
                <Boxes size={32} />
              </div>
              <h2 className="text-base font-semibold">{t("instances.empty.title")}</h2>
              <p className="mt-1.5 mb-5 text-sm text-muted">{t("instances.empty.hint")}</p>
              <Button variant="primary" icon={<Plus size={16} />} onClick={openNewInstance}>
                {t("instances.empty.action")}
              </Button>
            </div>
          </div>
        ) : (
          <div
            className="min-h-0 flex-1 overflow-y-auto px-5 pb-5"
            onClick={(e) => e.target === e.currentTarget && selectInstance(null)}
          >
            {list.length > 0 && filtered.length === 0 && (
              <p className="py-8 text-center text-sm text-muted">{t("instances.nothingFound", { query })}</p>
            )}
            {sections.map(([group, items]) => {
              const isCollapsed = collapsed.has(group);
              const showHeader = groups.length > 0 || group !== "";
              const target = drag?.target?.group === group ? drag.target : null;
              const dropHere = !!target && (drag?.item.type === "instance" || (drag?.item.type === "group" && drag.item.group !== group));
              return (
                <section
                  key={group || "__none"}
                  data-drop-group={group}
                  className={clsx(
                    "relative mb-4 rounded-md transition-colors",
                    dropHere && drag?.item.type === "instance" && "bg-accent/10 outline-2 outline-accent/60 outline-dashed",
                    drag?.item.type === "group" && drag.item.group === group && "opacity-50",
                  )}
                >
                  {dropHere && drag?.item.type === "group" && (
                    <div className={clsx("absolute right-0 left-0 h-0.5 rounded bg-accent", target.after ? "-bottom-2" : "-top-2")} />
                  )}
                  {showHeader && (
                    <button
                      type="button"
                      onClick={() => toggleGroup(group)}
                      onPointerDown={(e) => group && startDrag(e, { type: "group", group })}
                      onContextMenu={(e) => {
                        if (!group) return;
                        e.preventDefault();
                        setGroupMenu({ at: { x: e.clientX, y: e.clientY }, group });
                      }}
                      aria-expanded={!isCollapsed}
                      title={group ? t("groups.dragHint") : undefined}
                      className="mb-2 flex w-full items-center gap-1.5 border-b border-border pb-1 text-left text-sm font-semibold text-muted hover:text-text"
                    >
                      {isCollapsed ? <ChevronRight size={15} /> : <ChevronDown size={15} />}
                      {group || t("instances.ungrouped")}
                      <span className="font-normal">({items.length})</span>
                    </button>
                  )}
                  {!isCollapsed && items.length === 0 && (
                    <p className="rounded-md border border-dashed border-border px-3 py-4 text-center text-xs text-muted">{t("groups.empty")}</p>
                  )}
                  {!isCollapsed && items.length > 0 &&
                    (view === "grid" ? (
                      <div
                        role="listbox"
                        className="grid gap-2"
                        style={{ gridTemplateColumns: `repeat(auto-fill, minmax(${Math.max(112, iconSize + 48)}px, 1fr))` }}
                      >
                        {items.map((i) => (
                          <div
                            key={i.id}
                            role="option"
                            tabIndex={0}
                            {...itemProps(i)}
                            onKeyDown={(e) => e.key === " " && selectInstance(i.id)}
                            className={clsx(
                              "flex cursor-default flex-col items-center gap-1.5 rounded-lg border p-3 text-center transition-colors",
                              selectedId === i.id
                                ? "border-accent bg-surface"
                                : "border-transparent hover:bg-surface",
                            )}
                          >
                            <InstanceIcon name={i.name} src={i.modpack?.iconUrl} size={iconSize} />
                            <span className="line-clamp-2 text-sm font-medium break-words">{i.name}</span>
                            <span className="text-xs text-muted">
                              {i.minecraft}
                              {i.loader && ` · ${LOADER_NAMES[i.loader.kind]}`}
                            </span>
                            <StatusLine inst={i} />
                          </div>
                        ))}
                      </div>
                    ) : (
                      <table className="w-full text-sm" role="listbox">
                        <thead className="text-left text-xs text-muted">
                          <tr>
                            <th className="px-2 py-1 font-medium">{t("instances.colName")}</th>
                            <th className="px-2 py-1 font-medium">{t("instances.colVersion")}</th>
                            <th className="px-2 py-1 font-medium">{t("instances.colPlayed")}</th>
                            <th className="w-40 px-2 py-1" />
                          </tr>
                        </thead>
                        <tbody>
                          {items.map((i) => (
                            <tr
                              key={i.id}
                              role="option"
                              {...itemProps(i)}
                              className={clsx(
                                "cursor-default",
                                selectedId === i.id ? "bg-accent/15" : "hover:bg-surface",
                              )}
                            >
                              <td className="px-2 py-1.5">
                                <span className="flex items-center gap-2 font-medium">
                                  <InstanceIcon name={i.name} src={i.modpack?.iconUrl} size={24} />
                                  {i.name}
                                </span>
                              </td>
                              <td className="px-2 py-1.5 text-muted">
                                {i.minecraft}
                                {i.loader && ` · ${loaderLabel(i.minecraft, i.loader)}`}
                              </td>
                              <td className="px-2 py-1.5 text-muted">
                                {i.lastPlayed ? formatDate(i.lastPlayed) : t("instances.never")}
                              </td>
                              <td className="px-2 py-1.5">
                                <StatusLine inst={i} />
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    ))}
                </section>
              );
            })}
          </div>
        )}
      </div>

      <SelectionPanel
        inst={selected}
        jobs={jobs}
        runningIds={new Set(Object.values(games).filter((g) => g.state === "running").map((g) => g.instanceId))}
        onPlay={actions.play}
        onEdit={actions.edit}
        onInstall={actions.install}
        onOpenFolder={actions.openFolder}
        onDialog={setDialog}
      />

      <ContextMenu
        at={menu?.at ?? null}
        items={menuInstance ? menuItems(menuInstance) : []}
        onClose={() => setMenu(null)}
      />
      <ContextMenu
        at={groupMenu?.at ?? null}
        items={
          groupMenu
            ? [
                { label: t("groups.rename"), icon: <Pencil size={15} />, onSelect: () => setGroupDialog({ kind: "rename", group: groupMenu.group }) },
                { label: t("groups.delete"), icon: <Trash2 size={15} />, danger: true, onSelect: () => setGroupDialog({ kind: "delete", group: groupMenu.group }) },
              ]
            : []
        }
        onClose={() => setGroupMenu(null)}
      />
      <DragGhost drag={drag} label={drag ? (drag.item.type === "group" ? drag.item.group : drag.item.label) : ""} />
      <PromptDialog
        open={dialog === "newGroup"}
        title={t("groups.new")}
        initial=""
        placeholder={t("groups.namePlaceholder")}
        onSubmit={(name) => run(api.createGroup(name))}
        onClose={() => setDialog(null)}
      />
      <PromptDialog
        open={groupDialog?.kind === "rename"}
        title={t("groups.renameTitle", { name: groupDialog?.group ?? "" })}
        initial={groupDialog?.group ?? ""}
        hint={t("groups.renameHint")}
        onSubmit={(name) => groupDialog && name !== groupDialog.group && run(api.renameGroup(groupDialog.group, name))}
        onClose={() => setGroupDialog(null)}
      />
      <ConfirmDialog
        open={groupDialog?.kind === "delete"}
        title={t("groups.deleteTitle")}
        body={t("groups.deleteBody", { name: groupDialog?.group ?? "" })}
        confirmLabel={t("groups.delete")}
        danger
        onConfirm={() => groupDialog && run(api.deleteGroup(groupDialog.group))}
        onClose={() => setGroupDialog(null)}
      />
      <CloneDialog inst={selected} open={dialog === "clone"} onClose={() => setDialog(null)} onCloned={(c) => selectInstance(c.id)} />
      <InstanceExportDialog inst={selected} open={dialog === "export"} onClose={() => setDialog(null)} />
      <PromptDialog
        open={dialog === "rename"}
        title={t("renameDialog.title")}
        initial={selected?.name ?? ""}
        onSubmit={actions.rename}
        onClose={() => setDialog(null)}
      />
      <PromptDialog
        open={dialog === "group"}
        title={t("groupDialog.title")}
        initial={selected?.group ?? ""}
        placeholder={t("newInstance.groupPlaceholder")}
        hint={t("groupDialog.hint")}
        suggestions={groups}
        allowEmpty
        onSubmit={actions.setGroup}
        onClose={() => setDialog(null)}
      />
      <ConfirmDialog
        open={dialog === "delete"}
        title={t("selection.deleteTitle")}
        body={t("selection.deleteBody", { name: selected?.name ?? "" })}
        confirmLabel={t("selection.delete")}
        danger
        onConfirm={actions.delete}
        onClose={() => setDialog(null)}
      />
    </div>
  );
}

function SelectionPanel({
  inst,
  jobs,
  runningIds,
  onPlay,
  onEdit,
  onInstall,
  onOpenFolder,
  onDialog,
}: {
  inst: Instance | null;
  jobs: Record<number, JobInfo>;
  runningIds: Set<string>;
  onPlay: (i: Instance) => void;
  onEdit: (i: Instance) => void;
  onInstall: (i: Instance) => void;
  onOpenFolder: (i: Instance) => void;
  onDialog: (d: "rename" | "group" | "delete" | "clone" | "export") => void;
}) {
  const { t } = useTranslation();
  if (!inst) {
    return (
      <aside className="flex w-64 shrink-0 flex-col border-l border-border bg-surface p-4">
        <p className="text-sm text-muted">{t("selection.none")}</p>
      </aside>
    );
  }
  const job = jobForInstance(jobs, inst.id);
  const installing = isActive(job);
  const running = runningIds.has(inst.id);

  return (
    <aside className="flex w-64 shrink-0 flex-col border-l border-border bg-surface p-4">
      <div className="mb-4 flex flex-col items-center gap-2 text-center">
        <InstanceIcon name={inst.name} src={inst.modpack?.iconUrl} size={72} />
        <h2 className="text-base font-semibold break-words">{inst.name}</h2>
        <p className="text-xs text-muted">
          {t("selection.version", { version: inst.minecraft })}
          {inst.loader && <> · {loaderLabel(inst.minecraft, inst.loader)}</>}
          {inst.group && <> · {inst.group}</>}
        </p>
        <StatusLine inst={inst} />
        {installing && job?.stage && <p className="text-xs text-muted">{t(`jobs.stage.${job.stage}`)}</p>}
      </div>

      {running ? (
        <Button variant="danger" size="lg" icon={<Square size={16} />} onClick={() => onPlay(inst)}>
          {t("game.stop")}
        </Button>
      ) : (
        <Button variant="primary" size="lg" icon={<Play size={18} />} onClick={() => onPlay(inst)} disabled={installing} title="Enter">
          {t("selection.play")}
        </Button>
      )}

      <div className="mt-3 flex flex-col gap-0.5">
        {[
          { icon: <Settings2 size={15} />, label: t("selection.edit"), onClick: () => onEdit(inst) },
          { icon: <FolderOpen size={15} />, label: t("selection.openFolder"), onClick: () => onOpenFolder(inst) },
          { icon: <Pencil size={15} />, label: t("selection.rename"), onClick: () => onDialog("rename"), hint: "F2" },
          { icon: <FolderInput size={15} />, label: t("selection.setGroup"), onClick: () => onDialog("group") },
          {
            icon: inst.installed ? <ShieldCheck size={15} /> : <Download size={15} />,
            label: inst.installed ? t("selection.verify") : t("selection.install"),
            onClick: () => onInstall(inst),
            disabled: installing || running,
          },
          { icon: <Copy size={15} />, label: t("selection.copy"), onClick: () => onDialog("clone") },
          { icon: <PackageOpen size={15} />, label: t("instanceExport.open"), onClick: () => onDialog("export"), disabled: installing },
          { icon: <Trash2 size={15} />, label: t("selection.delete"), onClick: () => onDialog("delete"), disabled: installing || running, hint: "Del" },
        ].map((a) => (
          <Button
            key={a.label}
            variant="ghost"
            icon={a.icon}
            className="justify-start"
            disabled={a.disabled}
            onClick={a.onClick}
            title={a.hint}
          >
            {a.label}
          </Button>
        ))}
      </div>
    </aside>
  );
}
