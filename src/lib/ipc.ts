// Типизированные обёртки над Tauri-командами.
// Вне Tauri (обычный браузер, `npm run dev`) используется мок-бэкенд из ./mock,
// чтобы UI можно было разрабатывать и проверять без сборки Rust.

import { invoke as tauriInvoke, isTauri } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  UpdateInfo,
  Account,
  AccountsFile,
  AppInfo,
  DeviceCode,
  GameStatus,
  MsaStatus,
  SkinData,
  Instance,
  InstanceOverrides,
  JavaInstall,
  JobInfo,
  LauncherFolder,
  LoaderKind,
  LoaderSpec,
  LoaderVersion,
  AddResult,
  ContentFile,
  ContentKind,
  ContentUpdate,
  ModrinthInfo,
  ModrinthVersion,
  SearchResult,
  SearchSort,
  World,
  WorldBackup,
  ContentSource,
  CurseforgeStatus,
  ExportArgs,
  ExportCandidate,
  ModpackState,
  PackPreview,
  PackSource,
  LogLine,
  Settings,
  SystemInfo,
  VersionManifest,
} from "./types";
import { mockInvoke, mockListen } from "./mock";

export const inTauri = isTauri();

function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return inTauri ? tauriInvoke<T>(cmd, args) : mockInvoke<T>(cmd, args);
}

/** Подписка на событие бэкенда; возвращает функцию отписки. */
export function listen<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return inTauri ? tauriListen<T>(event, (e) => handler(e.payload)) : mockListen(event, handler);
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  installUpdate: () => invoke<number>("install_update"),
  openLauncherFolder: (folder: LauncherFolder) => invoke<void>("open_launcher_folder", { folder }),

  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  resetSettings: () => invoke<Settings>("reset_settings"),
  /** Возвращает true, если для применения нужен перезапуск. */
  setDataDir: (path: string | null) => invoke<boolean>("set_data_dir", { path }),

  getVersionManifest: (force = false) => invoke<VersionManifest>("get_version_manifest", { force }),

  listJobs: () => invoke<JobInfo[]>("list_jobs"),
  cancelJob: (id: number) => invoke<boolean>("cancel_job", { id }),
  dismissJob: (id: number) => invoke<void>("dismiss_job", { id }),
  /** Повторяет упавшую/отменённую задачу; возвращает id новой. */
  retryJob: (id: number) => invoke<number>("retry_job", { id }),

  listGroups: () => invoke<string[]>("list_groups"),
  setGroupOrder: (order: string[]) => invoke<string[]>("set_group_order", { order }),
  createGroup: (name: string) => invoke<string[]>("create_group", { name }),
  renameGroup: (old: string, next: string) => invoke<string[]>("rename_group", { old, new: next }),
  deleteGroup: (name: string) => invoke<string[]>("delete_group", { name }),
  cloneInstance: (id: string, name: string, group: string | null, copySaves: boolean) =>
    invoke<Instance>("clone_instance", { id, name, group, copySaves }),
  instanceExportCandidates: (id: string) => invoke<ExportCandidate[]>("instance_export_candidates", { id }),
  /** Экспорт инстанса целиком в zip; возвращает id задачи. */
  exportInstance: (id: string, path: string, include: string[]) => invoke<number>("export_instance", { id, path, include }),

  listInstances: () => invoke<Instance[]>("list_instances"),
  createInstance: (args: { name: string; group: string | null; minecraft: string; loader: LoaderSpec | null }) =>
    invoke<Instance>("create_instance", { args }),
  loaderVersions: (kind: LoaderKind, minecraft: string) =>
    invoke<LoaderVersion[]>("loader_versions", { kind, minecraft }),
  /** Меняет версию Minecraft и/или загрузчик и запускает установку. */
  setInstanceVersion: (id: string, minecraft: string, loader: LoaderSpec | null) =>
    invoke<Instance>("set_instance_version", { id, minecraft, loader }),
  installInstance: (id: string) => invoke<number>("install_instance", { id }),
  renameInstance: (id: string, name: string) => invoke<Instance>("rename_instance", { id, name }),
  setInstanceGroup: (id: string, group: string | null) => invoke<Instance>("set_instance_group", { id, group }),
  deleteInstance: (id: string) => invoke<void>("delete_instance", { id }),
  openInstanceFolder: (id: string) => invoke<void>("open_instance_folder", { id }),
  getInstance: (id: string) => invoke<Instance>("get_instance", { id }),
  updateInstanceOverrides: (id: string, overrides: InstanceOverrides) =>
    invoke<Instance>("update_instance_overrides", { id, overrides }),

  listContent: (id: string, kind: ContentKind) => invoke<ContentFile[]>("list_content", { id, kind }),
  setContentEnabled: (id: string, kind: ContentKind, fileName: string, enabled: boolean) =>
    invoke<string>("set_content_enabled", { id, kind, fileName, enabled }),
  deleteContent: (id: string, kind: ContentKind, fileNames: string[]) =>
    invoke<void>("delete_content", { id, kind, fileNames }),
  addContentFiles: (id: string, kind: ContentKind, paths: string[]) =>
    invoke<AddResult>("add_content_files", { id, kind, paths }),
  /** mods | resourcepacks | shaderpacks | saves | saves/<мир> | screenshots | backups */
  openContentFolder: (id: string, folder: string) => invoke<void>("open_content_folder", { id, folder }),
  contentModrinthInfo: (id: string, kind: ContentKind) =>
    invoke<Record<string, ModrinthInfo>>("content_modrinth_info", { id, kind }),
  modrinthSearch: (id: string, kind: ContentKind, query: string, sort: SearchSort, offset: number) =>
    invoke<SearchResult>("modrinth_search", { id, kind, query, sort, offset }),
  modrinthVersions: (id: string, kind: ContentKind, project: string) =>
    invoke<ModrinthVersion[]>("modrinth_versions", { id, kind, project }),
  installModrinth: (id: string, kind: ContentKind, versionId: string, title: string, withDeps: boolean) =>
    invoke<number>("install_modrinth", { id, kind, versionId, title, withDeps }),
  checkContentUpdates: (id: string, kind: ContentKind) =>
    invoke<ContentUpdate[]>("check_content_updates", { id, kind }),
  updateContent: (id: string, kind: ContentKind, items: { fileName: string; versionId: string }[]) =>
    invoke<number>("update_content", { id, kind, items }),
  /** Поиск/версии/установка из выбранного источника (Modrinth или CurseForge). */
  contentSearch: (source: ContentSource, id: string, kind: ContentKind, query: string, sort: SearchSort, offset: number) =>
    invoke<SearchResult>(source === "curseforge" ? "curseforge_search" : "modrinth_search", { id, kind, query, sort, offset }),
  contentVersions: (source: ContentSource, id: string, kind: ContentKind, project: string) =>
    invoke<ModrinthVersion[]>(source === "curseforge" ? "curseforge_versions" : "modrinth_versions", { id, kind, project }),
  installCurseforge: (id: string, kind: ContentKind, projectId: string, fileId: string, title: string, withDeps: boolean) =>
    invoke<number>("install_curseforge", { id, kind, projectId, fileId, title, withDeps }),

  curseforgeStatus: () => invoke<CurseforgeStatus>("curseforge_status"),
  /** Проверяет ключ и сохраняет его в системном хранилище; null — удалить. */
  setCurseforgeKey: (key: string | null) => invoke<CurseforgeStatus>("set_curseforge_key", { key }),
  modpackSearch: (source: ContentSource, query: string, sort: SearchSort, offset: number, minecraft: string | null, loader: LoaderKind | null) =>
    invoke<SearchResult>("modpack_search", { source, query, sort, offset, minecraft, loader }),
  modpackVersions: (source: PackSource, project: string) => invoke<ModrinthVersion[]>("modpack_versions", { source, project }),
  inspectModpack: (path: string) => invoke<PackPreview>("inspect_modpack", { path }),
  importModpack: (path: string, name: string | null, group: string | null) =>
    invoke<Instance>("import_modpack", { path, name, group }),
  installModpack: (a: { source: ContentSource; projectId: string; versionId: string; title: string; name: string | null; group: string | null; iconUrl: string | null }) =>
    invoke<Instance>("install_modpack", a),
  /** Обновление/откат: версия из источника или файл. Возвращает id задачи. */
  updateModpack: (id: string, versionId: string | null, path: string | null) => invoke<number>("update_modpack", { id, versionId, path }),
  modpackState: (id: string) => invoke<ModpackState>("modpack_state", { id }),
  /** Ищет вручную скачанные файлы в «Загрузках» и среди paths; возвращает, сколько ещё не хватает. */
  modpackAddMissing: (id: string, paths: string[]) => invoke<number>("modpack_add_missing", { id, paths }),
  exportCandidates: (id: string) => invoke<ExportCandidate[]>("export_candidates", { id }),
  exportModpack: (id: string, args: ExportArgs) => invoke<number>("export_modpack", { id, args }),

  listWorlds: (id: string) => invoke<{ worlds: World[]; backups: WorldBackup[] }>("list_worlds", { id }),
  backupWorld: (id: string, folder: string) => invoke<number>("backup_world", { id, folder }),
  deleteWorld: (id: string, folder: string) => invoke<void>("delete_world", { id, folder }),

  launchInstance: (id: string) => invoke<number>("launch_instance", { id }),
  killGame: (id: string) => invoke<void>("kill_game", { id }),
  getGameLog: (id: string) => invoke<{ status: GameStatus | null; lines: LogLine[] }>("get_game_log", { id }),
  runningGames: () => invoke<GameStatus[]>("running_games"),
  saveGameLog: (id: string, path: string) => invoke<void>("save_game_log", { id, path }),

  detectJava: (force = false) => invoke<JavaInstall[]>("detect_java", { force }),
  testJava: (path: string) => invoke<JavaInstall>("test_java", { path }),
  downloadJava: (major: number) => invoke<number>("download_java", { major }),
  systemInfo: () => invoke<SystemInfo>("system_info"),

  listAccounts: () => invoke<AccountsFile>("list_accounts"),
  addOfflineAccount: (username: string) => invoke<Account>("add_offline_account", { username }),
  removeAccount: (id: string) => invoke<void>("remove_account", { id }),
  setDefaultAccount: (id: string) => invoke<void>("set_default_account", { id }),
  msaStatus: () => invoke<MsaStatus>("msa_status"),
  msaBegin: () => invoke<DeviceCode>("msa_begin"),
  /** Долгий вызов: ждёт ввода кода и проходит цепочку Xbox → Minecraft. */
  msaFinish: () => invoke<Account>("msa_finish"),
  msaCancel: () => invoke<void>("msa_cancel"),
  refreshAccount: (id: string) => invoke<Account>("refresh_account", { id }),
  accountSkin: (id: string) => invoke<SkinData | null>("account_skin", { id }),
};
