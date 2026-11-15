// Типы, зеркалирующие структуры Rust (src-tauri/src/settings/mod.rs и commands).

export type ThemeMode = "system" | "dark" | "light";
export type InstanceView = "grid" | "list";
export type ProxyMode = "none" | "system" | "http" | "socks5";
export type LauncherVisibility = "keep" | "minimize" | "hide" | "close";

export interface Settings {
  version: number;
  language: string;
  appearance: {
    theme: ThemeMode;
    accent: string;
    instanceView: InstanceView;
    iconSize: number;
  };
  downloads: {
    concurrency: number;
    retries: number;
    timeoutSecs: number;
    /** Сколько задач с загрузками идёт одновременно. */
    maxJobs: number;
  };
  proxy: {
    mode: ProxyMode;
    host: string;
    port: number;
    username: string;
  };
  java: {
    path: string | null;
    autoDownload: boolean;
    minMemoryMb: number;
    maxMemoryMb: number;
    jvmArgs: string;
  };
  game: {
    windowWidth: number;
    windowHeight: number;
    fullscreen: boolean;
    onLaunch: LauncherVisibility;
    showLogOnLaunch: boolean;
    recordPlaytime: boolean;
  };
  updates: {
    /** Проверять обновления лаунчера при запуске. */
    checkOnStartup: boolean;
  };
  /** Client ID приложения Azure; пусто — вшитый при сборке. */
  msaClientId?: string | null;
}

/** Почему обновление ставится вручную: отладочная сборка, портативный режим, не из установщика. */
export type UpdateManualReason = "dev" | "portable" | "unpackaged";

export interface UpdateInfo {
  current: string;
  version: string;
  notes: string | null;
  date: string | null;
  manual: UpdateManualReason | null;
  releaseUrl: string | null;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  portable: boolean;
  os: string;
  arch: string;
}

export type LauncherFolder = "data" | "instances" | "logs" | "java";

export type VersionType = "release" | "snapshot" | "old_beta" | "old_alpha";

export interface ManifestVersion {
  id: string;
  type: VersionType;
  url: string;
  releaseTime: string;
  sha1?: string | null;
}

export interface VersionManifest {
  latest: { release: string; snapshot: string };
  versions: ManifestVersion[];
}

export type LoaderKind = "fabric" | "quilt" | "forge" | "neoforge";
export const LOADER_KINDS: LoaderKind[] = ["fabric", "quilt", "forge", "neoforge"];
export const LOADER_NAMES: Record<LoaderKind, string> = {
  fabric: "Fabric",
  quilt: "Quilt",
  forge: "Forge",
  neoforge: "NeoForge",
};

export interface LoaderSpec {
  kind: LoaderKind;
  version: string;
}

export interface LoaderVersion {
  /** Полная версия для репозитория загрузчика. */
  version: string;
  /** Для показа (у Forge — без префикса версии Minecraft). */
  label: string;
  stable: boolean;
  recommended: boolean;
}

/** Короткая версия загрузчика для показа: у Forge в maven она с префиксом версии Minecraft. */
export function loaderLabel(minecraft: string, spec: LoaderSpec): string {
  let v = spec.version;
  if (v.startsWith(`${minecraft}-`)) v = v.slice(minecraft.length + 1);
  if (v.endsWith(`-${minecraft}`)) v = v.slice(0, -minecraft.length - 1);
  return `${LOADER_NAMES[spec.kind]} ${v}`;
}

export interface Instance {
  id: string;
  format: number;
  name: string;
  group?: string | null;
  icon?: string | null;
  minecraft: string;
  loader?: LoaderSpec | null;
  createdAt: string;
  lastPlayed?: string | null;
  playtimeSecs: number;
  installed: boolean;
  overrides: InstanceOverrides;
  modpack?: ModpackLink | null;
}

export type PackSource = "modrinth" | "curseforge" | "file";
export type PackFormat = "mrpack" | "curseforge";
/** Источник содержимого в браузере модов. */
export type ContentSource = "modrinth" | "curseforge";

export interface ModpackLink {
  source: PackSource;
  projectId?: string | null;
  versionId?: string | null;
  name: string;
  version: string;
  iconUrl?: string | null;
}

/** Формат импортируемого файла: модпак или инстанс целиком. */
export type ImportFormat = PackFormat | "cubelaunch" | "prism";

export interface PackPreview {
  format: ImportFormat;
  name: string;
  version: string;
  summary: string | null;
  minecraft: string;
  loader: LoaderKind | null;
  loaderVersion: string | null;
  files: number;
  overrides: number;
  /** Архив инстанса содержит миры. */
  hasSaves: boolean;
  /** Что из архива не переносится. */
  warnings: string[];
}

/** Файл пака, который нужно скачать вручную (CurseForge запретил сторонние лаунчеры). */
export interface MissingFile {
  name: string;
  fileName: string;
  pageUrl: string;
  path: string;
  sha1?: string | null;
  size: number;
}

export interface ModpackState {
  files: number;
  missing: MissingFile[];
}

export interface CurseforgeStatus {
  configured: boolean;
  userKey: boolean;
  builtIn: boolean;
}

export interface ExportCandidate {
  name: string;
  isDir: boolean;
  default: boolean;
}

export interface ExportArgs {
  format: PackFormat;
  path: string;
  name: string;
  version: string;
  summary: string | null;
  include: string[];
}

/** Переопределения глобальных настроек; отсутствующее поле — брать глобальное. */
export interface InstanceOverrides {
  javaPath?: string | null;
  minMemoryMb?: number | null;
  maxMemoryMb?: number | null;
  jvmArgs?: string | null;
  windowWidth?: number | null;
  windowHeight?: number | null;
  fullscreen?: boolean | null;
  env?: Record<string, string>;
  preLaunchCommand?: string | null;
  postExitCommand?: string | null;
}

export type Arch = "x86_64" | "arm64" | "x86" | "arm32";

export interface JavaInstall {
  path: string;
  version: string;
  major: number;
  arch: Arch;
  vendor: string;
  managed: boolean;
}

export interface SystemInfo {
  totalMemoryMb: number;
  recommendedMaxMemoryMb: number;
  cpus: number;
}

export interface Account {
  id: string;
  kind: "offline" | "microsoft";
  username: string;
  uuid: string;
  addedAt: string;
  skin?: { url: string; variant: string; state: string } | null;
  cape?: { url: string; state: string; alias: string } | null;
}

export interface DeviceCode {
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
}

export interface MsaStatus {
  configured: boolean;
  builtIn: boolean;
}

export interface SkinData {
  /** data:image/png;base64,... */
  skin: string;
  model: "classic" | "slim";
  cape?: string;
}

export type LoginStage = "waiting" | "xbox" | "xsts" | "minecraft" | "profile";

export interface AccountsFile {
  accounts: Account[];
  defaultId: string | null;
}

export type LogLevel = "trace" | "debug" | "info" | "warn" | "error" | "fatal";

export interface LogLine {
  level: LogLevel;
  time?: string;
  thread?: string;
  text: string;
}

export type GameState = "running" | "exited" | "crashed" | "killed";

export interface GameStatus {
  instanceId: string;
  state: GameState;
  pid: number | null;
  startedAt: string;
  endedAt?: string;
  exitCode?: number;
  crashReport?: { path: string; text: string };
}

export interface Progress {
  filesDone: number;
  filesTotal: number;
  bytesDone: number;
  bytesTotal: number;
}

export type JobState = "queued" | "running" | "done" | "failed" | "cancelled";

export interface JobInfo {
  id: number;
  kind: string;
  title: string;
  instanceId?: string;
  state: JobState;
  stage?: string;
  progress: Progress;
  error?: BackendError;
  startedAt: string;
  finishedAt?: string;
  /** Упавшую/отменённую задачу можно повторить. */
  retryable: boolean;
}

/** Задача ещё не завершена (идёт или ждёт в очереди). */
export const isActive = (j: { state: JobState } | undefined | null) => j?.state === "running" || j?.state === "queued";

/** Ошибка, приходящая из Rust (`AppError`). */
export interface BackendError {
  kind: string;
  message: string;
  details: string | null;
}

// ---- содержимое инстанса ----

export type ContentKind = "mods" | "resourcepacks" | "shaderpacks";

export interface ContentMeta {
  id?: string;
  name?: string;
  version?: string;
  description?: string;
  authors?: string[];
  /** data:-URL. */
  icon?: string;
  loaders?: string[];
}

export interface ContentFile {
  /** Имя на диске (с .disabled, если выключен). */
  fileName: string;
  enabled: boolean;
  isDir: boolean;
  size: number;
  modified?: string;
  sha1?: string;
  meta?: ContentMeta;
  incompatible: boolean;
}

export interface AddResult {
  added: string[];
  skipped: string[];
}

export interface ModrinthInfo {
  projectId: string;
  versionId: string;
  versionNumber: string;
  title: string;
  slug: string;
  projectType: string;
  iconUrl?: string;
}

export interface SearchHit {
  projectId: string;
  slug: string;
  title: string;
  description: string;
  author: string;
  iconUrl?: string | null;
  downloads: number;
  follows: number;
  categories: string[];
  dateModified?: string | null;
  projectType: string;
}

export interface SearchResult {
  hits: SearchHit[];
  totalHits: number;
  offset: number;
  limit: number;
}

export type SearchSort = "relevance" | "downloads" | "follows" | "newest" | "updated";

export interface ModrinthVersion {
  id: string;
  projectId: string;
  name: string;
  versionNumber: string;
  versionType: "release" | "beta" | "alpha" | string;
  gameVersions: string[];
  loaders: string[];
  files: { url: string; filename: string; primary: boolean; size: number; hashes: { sha1?: string | null } }[];
  dependencies: { versionId?: string | null; projectId?: string | null; dependencyType: string }[];
  datePublished?: string | null;
  downloads: number;
}

export interface ContentUpdate {
  fileName: string;
  projectId: string;
  title: string;
  currentVersion: string;
  versionId: string;
  versionNumber: string;
}

export interface World {
  folder: string;
  name: string;
  lastPlayed?: string;
  gameMode?: "survival" | "creative" | "adventure" | "spectator";
  hardcore: boolean;
  version?: string;
  icon?: string;
  size: number;
}

export interface WorldBackup {
  fileName: string;
  size: number;
  created?: string;
}
