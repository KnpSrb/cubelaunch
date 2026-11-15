// Мок-бэкенд для запуска фронтенда в обычном браузере (без Tauri).
// Повторяет поведение Rust-команд в упрощённом виде; данные живут в localStorage.

import type {
  Account,
  AccountsFile,
  BackendError,
  GameStatus,
  Instance,
  JavaInstall,
  JobInfo,
  LoaderKind,
  LoaderSpec,
  LoaderVersion,
  LogLine,
  ContentFile,
  ContentKind,
  SearchHit,
  World,
  WorldBackup,
  Settings,
  VersionManifest,
} from "./types";
import { EVENTS } from "./events";

export const defaultSettings: Settings = {
  version: 1,
  language: "ru",
  appearance: { theme: "system", accent: "#4caf50", instanceView: "grid", iconSize: 64 },
  downloads: { concurrency: 8, retries: 3, timeoutSecs: 30, maxJobs: 2 },
  updates: { checkOnStartup: true },
  proxy: { mode: "none", host: "", port: 0, username: "" },
  java: { path: null, autoDownload: true, minMemoryMb: 512, maxMemoryMb: 4096, jvmArgs: "" },
  game: {
    windowWidth: 854,
    windowHeight: 480,
    fullscreen: false,
    onLaunch: "keep",
    showLogOnLaunch: true,
    recordPlaytime: true,
  },
  msaClientId: null,
};

const KEY = "cubelaunch.mock.settings";
const INST_KEY = "cubelaunch.mock.instances";

// ---- события ----
type Handler = (payload: never) => void;
const listeners = new Map<string, Set<Handler>>();

export async function mockListen<T>(event: string, handler: (payload: T) => void) {
  const set = listeners.get(event) ?? new Set();
  set.add(handler as Handler);
  listeners.set(event, set);
  return () => void set.delete(handler as Handler);
}

function emit(event: string, payload: unknown) {
  listeners.get(event)?.forEach((h) => (h as (p: unknown) => void)(payload));
}

// ---- хранилище ----
function load(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const s = JSON.parse(raw);
      return {
        ...defaultSettings,
        ...s,
        downloads: { ...defaultSettings.downloads, ...s.downloads },
        updates: { ...defaultSettings.updates, ...s.updates },
      };
    }
  } catch {
    /* ignore */
  }
  return structuredClone(defaultSettings);
}

function loadInstances(): Instance[] {
  try {
    return JSON.parse(localStorage.getItem(INST_KEY) ?? "[]");
  } catch {
    return [];
  }
}

const GROUPS_KEY = "cubelaunch.mock.groups";
function mockGroups(): string[] {
  let order: string[] = [];
  try {
    order = JSON.parse(localStorage.getItem(GROUPS_KEY) ?? "[]");
  } catch {
    /* ignore */
  }
  const fromList = [...new Set(loadInstances().map((i) => i.group).filter((g): g is string => !!g))].sort((x, y) => x.localeCompare(y));
  return [...order, ...fromList.filter((g) => !order.includes(g))];
}
function saveGroups(order: string[]) {
  localStorage.setItem(GROUPS_KEY, JSON.stringify([...new Set(order)]));
  emit(EVENTS.instancesChanged, null);
}

function saveInstances(list: Instance[]) {
  localStorage.setItem(INST_KEY, JSON.stringify(list));
  emit(EVENTS.instancesChanged, null);
}

function fail(kind: string, details: string): never {
  const msg: Record<string, string> = {
    invalid_settings: "Некорректные настройки",
    invalid_input: "Некорректные данные",
    not_found: "Не найдено",
  };
  throw { kind, message: `${msg[kind] ?? "Ошибка"}: ${details}`, details } satisfies BackendError;
}

function validate(s: Settings) {
  if (!/^#[0-9a-fA-F]{6}$/.test(s.appearance.accent)) fail("invalid_settings", "акцентный цвет должен быть в формате #rrggbb");
  if (s.downloads.concurrency < 1 || s.downloads.concurrency > 64)
    fail("invalid_settings", "число параллельных загрузок должно быть от 1 до 64");
  if ((s.proxy.mode === "http" || s.proxy.mode === "socks5") && (!s.proxy.host.trim() || !s.proxy.port))
    fail("invalid_settings", "для прокси нужно указать адрес и порт");
  if (s.java.minMemoryMb < 128) fail("invalid_settings", "минимальная память не может быть меньше 128 МБ");
  if (s.java.maxMemoryMb < s.java.minMemoryMb) fail("invalid_settings", "максимальная память меньше минимальной");
  if (s.game.windowWidth < 320 || s.game.windowHeight < 240) fail("invalid_settings", "размер окна игры слишком мал");
}

function slugify(name: string) {
  const s = name.trim().toLowerCase().replace(/[^\p{L}\p{N}._]+/gu, "-").replace(/^[-.]+|[-.]+$/g, "");
  return s.slice(0, 48) || "instance";
}

// ---- манифест ----
const manifest: VersionManifest = {
  latest: { release: "1.21.4", snapshot: "25w03a" },
  versions: [
    ["25w03a", "snapshot", "2025-01-15"],
    ["1.21.4", "release", "2024-12-03"],
    ["1.21.1", "release", "2024-08-08"],
    ["24w14potato", "snapshot", "2024-04-01"],
    ["1.20.1", "release", "2023-06-12"],
    ["1.19.2", "release", "2022-08-05"],
    ["1.16.5", "release", "2021-01-14"],
    ["1.12.2", "release", "2017-09-18"],
    ["1.8.9", "release", "2015-12-03"],
    ["1.7.10", "release", "2014-06-26"],
    ["b1.7.3", "old_beta", "2011-07-08"],
    ["a1.2.6", "old_alpha", "2010-12-03"],
    ["rd-132211", "old_alpha", "2009-05-13"],
  ].map(([id, type, date]) => ({
    id,
    type: type as VersionManifest["versions"][number]["type"],
    url: "",
    releaseTime: `${date}T00:00:00+00:00`,
  })),
};

// ---- задачи ----
let nextJob = 1;
const jobs = new Map<number, MockJob>();

type MockJob = JobInfo & { timer?: number; begin?: () => void; again?: () => number };
const QUEUED_KINDS = ["installVersion", "installContent", "updateContent", "downloadJava", "installModpack", "updateModpack", "importInstance"];

function publicJob(j: MockJob): JobInfo {
  const { timer: _t, begin: _b, again: _a, ...rest } = j;
  return rest;
}

/** Запускает ждущие задачи, если в очереди есть место. */
function pump() {
  const limit = load().downloads.maxJobs || 2;
  for (const j of [...jobs.values()].sort((a, b) => a.id - b.id)) {
    const running = [...jobs.values()].filter((x) => x.state === "running" && QUEUED_KINDS.includes(x.kind)).length;
    if (j.state !== "queued" || running >= limit) continue;
    j.state = "running";
    j.begin?.();
    emit(EVENTS.jobUpdate, publicJob(j));
  }
}

function finishJob(j: MockJob, state: JobInfo["state"]) {
  clearInterval(j.timer);
  j.state = state;
  j.finishedAt = new Date().toISOString();
  emit(EVENTS.jobUpdate, publicJob(j));
  setTimeout(pump, 0);
}

/** Регистрирует задачу: тяжёлые ставятся в очередь, остальные стартуют сразу. */
function enqueue(job: MockJob, begin: () => void, again: () => number) {
  job.begin = begin;
  job.again = again;
  job.retryable = job.kind !== "launch";
  job.state = QUEUED_KINDS.includes(job.kind) ? "queued" : "running";
  jobs.set(job.id, job);
  if (job.state === "running") begin();
  emit(EVENTS.jobUpdate, publicJob(job));
  pump();
}

function startInstall(inst: Instance): number {
  const id = nextJob++;
  const job: MockJob = {
    id,
    kind: "installVersion",
    title: `${inst.name} — Minecraft ${inst.minecraft}${inst.loader ? ` + ${inst.loader.kind} ${inst.loader.version}` : ""}`,
    instanceId: inst.id,
    state: "queued",
    stage: "files",
    progress: { filesDone: 0, filesTotal: 3800, bytesDone: 0, bytesTotal: 620_000_000 },
    startedAt: new Date().toISOString(),
    retryable: true,
  };
  enqueue(
    job,
    () => {
      job.timer = window.setInterval(() => {
        const p = job.progress;
        p.filesDone = Math.min(p.filesTotal, p.filesDone + 190);
        p.bytesDone = Math.min(p.bytesTotal, p.bytesDone + 31_000_000);
        if (p.filesDone >= p.filesTotal) {
          saveInstances(loadInstances().map((i) => (i.id === inst.id ? { ...i, installed: true } : i)));
          finishJob(job, "done");
        } else emit(EVENTS.jobUpdate, publicJob(job));
      }, 250);
    },
    () => startInstall(inst),
  );
  return id;
}

/** Правдоподобные списки версий загрузчиков (без сети). */
function mockLoaderVersions(kind: LoaderKind, mc: string): LoaderVersion[] {
  const old = /^1\.(\d+)/.exec(mc);
  const minor = old ? Number(old[1]) : 99;
  const plain = (v: string, i: number): LoaderVersion => ({ version: v, label: v, stable: !v.includes("beta"), recommended: i === 1 });
  switch (kind) {
    case "fabric":
      return minor < 14 ? [] : ["0.16.10", "0.16.9", "0.16.8", "0.15.11"].map((v, i) => ({ ...plain(v, i), stable: i > 0, recommended: i === 1 }));
    case "quilt":
      return minor < 14 ? [] : ["0.28.0-beta.1", "0.27.1", "0.27.0"].map(plain);
    case "forge":
      return minor < 6 ? [] : ["52.0.10", "52.0.9", "52.0.4"].map((v, i) => ({ version: `${mc}-${v}`, label: v, stable: true, recommended: i === 1 }));
    case "neoforge":
      return minor < 20 ? [] : ["21.1.80-beta", "21.1.77", "21.1.76"].map(plain);
  }
}

// ---- содержимое (моды, паки, миры) ----

const PIXEL =
  "data:image/svg+xml;utf8," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2"><rect width="2" height="2" fill="#4caf50"/><rect width="1" height="1" fill="#2e7d32"/></svg>',
  );

const MOCK_PROJECTS: SearchHit[] = (
  [
    ["AANobbMI", "sodium", "Sodium", "Мощный движок рендеринга, сильно повышающий FPS.", "jellysquid3", 82_000_000],
    ["P7dR8mSH", "fabric-api", "Fabric API", "Базовая библиотека для модов Fabric.", "modmuss50", 120_000_000],
    ["YL57xq9U", "iris", "Iris Shaders", "Шейдеры, совместимые с OptiFine, для Fabric.", "coderbot", 60_000_000],
    ["gvQqBUqZ", "lithium", "Lithium", "Оптимизация игровой логики без изменения поведения.", "jellysquid3", 40_000_000],
    ["mOgUt4GM", "modmenu", "Mod Menu", "Список модов в меню игры.", "Prospector", 70_000_000],
  ] as const
).map(([projectId, slug, title, description, author, downloads]) => ({
  projectId,
  slug,
  title,
  description,
  author,
  downloads,
  follows: 1000,
  categories: ["optimization"],
  iconUrl: null,
  dateModified: "2026-10-01T00:00:00Z",
  projectType: "mod",
}));

const contentStore = new Map<string, ContentFile[]>();
function contentList(id: string, kind: ContentKind): ContentFile[] {
  const key = `${id}:${kind}`;
  if (!contentStore.has(key)) {
    const seed: ContentFile[] =
      kind === "mods"
        ? [
            {
              fileName: "sodium-fabric-0.6.0.jar",
              enabled: true,
              isDir: false,
              size: 1_200_000,
              sha1: "s1",
              incompatible: false,
              meta: { id: "sodium", name: "Sodium", version: "0.6.0", description: "Мощный движок рендеринга", authors: ["jellysquid3"], icon: PIXEL, loaders: ["fabric"] },
            },
            {
              fileName: "jei-forge-15.2.jar.disabled",
              enabled: false,
              isDir: false,
              size: 900_000,
              sha1: "s2",
              incompatible: true,
              meta: { id: "jei", name: "Just Enough Items", version: "15.2.0", description: "Список предметов", loaders: ["forge"] },
            },
            { fileName: "unknown-mod.jar", enabled: true, isDir: false, size: 40_000, sha1: "s3", incompatible: false },
          ]
        : kind === "resourcepacks"
          ? [{ fileName: "Faithful 32x.zip", enabled: true, isDir: false, size: 12_000_000, sha1: "r1", incompatible: false, meta: { description: "Faithful 32x", icon: PIXEL } }]
          : [];
    contentStore.set(key, seed);
  }
  return contentStore.get(key)!;
}

function startContentJob(inst: Instance, kind: string, title: string, after: () => void): number {
  const id = nextJob++;
  const job: MockJob = {
    id,
    kind,
    title,
    instanceId: inst.id,
    state: "queued",
    stage: "files",
    progress: { filesDone: 0, filesTotal: 4, bytesDone: 0, bytesTotal: 4_000_000 },
    startedAt: new Date().toISOString(),
    retryable: true,
  };
  enqueue(
    job,
    () => {
      job.timer = window.setInterval(() => {
        job.progress.filesDone++;
        job.progress.bytesDone += 1_000_000;
        // Имя с «сбой» — задача падает (проверка «Повторить»).
        if (job.progress.filesDone === 2 && /сбой/i.test(inst.name) && !job.title.includes("↻")) {
          job.error = { kind: "network", message: "Ошибка сети", details: "соединение сброшено (мок)" };
          finishJob(job, "failed");
          return;
        }
        if (job.progress.filesDone >= job.progress.filesTotal) {
          after();
          finishJob(job, "done");
        } else emit(EVENTS.jobUpdate, publicJob(job));
      }, 300);
    },
    () => startContentJob(inst, kind, title + " ↻", after),
  );
  return id;
}

// ---- модпаки и CurseForge ----
let cfKey: string | null = null;
const cfStatus = () => ({ configured: !!cfKey, userKey: !!cfKey, builtIn: false });
const MOCK_PACKS: SearchHit[] = (
  [
    ["1KVo5zza", "fabulously-optimized", "Fabulously Optimized", "Улучшенная графика и производительность.", "robotkoer", 9_000_000],
    ["ZYgyPyfq", "cobblemon-modpack", "Cobblemon Official Modpack", "Покемоны в Minecraft.", "Cobblemon", 3_000_000],
    ["Ab1Cd2Ef", "better-mc", "Better MC [FABRIC]", "Ванилла, но лучше.", "Luna", 5_500_000],
  ] as const
).map(([projectId, slug, title, description, author, downloads]) => ({
  projectId, slug, title, description, author, downloads, follows: 100, categories: ["optimization"], iconUrl: null,
  dateModified: "2026-10-20T00:00:00Z", projectType: "modpack",
}));
const modpackStates = new Map<string, { files: number; missing: { name: string; fileName: string; pageUrl: string; path: string; size: number }[] }>();
function packVersions(project: string) {
  return ["6.2.0", "6.1.0", "6.0.0-beta.3"].map((v, i) => ({
    id: `${project}-${v}`, projectId: project, name: v, versionNumber: v, versionType: i === 2 ? "beta" : "release",
    gameVersions: [i === 0 ? "1.21.1" : "1.21"], loaders: ["fabric"], files: [], dependencies: [], downloads: 10,
  }));
}
function createPackInstance(name: string, group: string | null, minecraft: string, modpack: Instance["modpack"], missing = 0): Instance {
  const list = loadInstances();
  const base = slugify(name);
  let id = base;
  for (let n = 2; list.some((i) => i.id === id); n++) id = `${base}-${n}`;
  const inst: Instance = {
    id, format: 1, name: name.trim(), group: group?.trim() || null, minecraft, loader: { kind: "fabric", version: "0.16.5" },
    createdAt: new Date().toISOString(), playtimeSecs: 0, installed: false, overrides: {}, modpack,
  };
  saveInstances([...list, inst]);
  modpackStates.set(id, {
    files: 87,
    missing: Array.from({ length: missing }, (_, i) => ({
      name: `Blocked Mod ${i + 1}`, fileName: `blocked-${i + 1}.jar`, pageUrl: "https://www.curseforge.com/minecraft/mc-mods/x/files/1", path: `mods/blocked-${i + 1}.jar`, size: 1000,
    })),
  });
  const job = startContentJob(inst, "installModpack", `Модпак «${inst.name}»`, () =>
    saveInstances(loadInstances().map((i) => (i.id === inst.id ? { ...i, installed: true } : i))),
  );
  jobs.get(job)!.stage = "modpackApply";
  return inst;
}

const worldsStore = new Map<string, World[]>();
function worldsList(id: string): World[] {
  if (!worldsStore.has(id))
    worldsStore.set(id, [
      { folder: "New World", name: "Мой первый мир", lastPlayed: "2026-11-14T18:00:00Z", gameMode: "survival", hardcore: false, version: "1.21.1", size: 48_000_000, icon: PIXEL },
      { folder: "Hardcore", name: "Хардкор", lastPlayed: "2026-11-01T10:00:00Z", gameMode: "survival", hardcore: true, version: "1.21.1", size: 12_000_000 },
    ]);
  return worldsStore.get(id)!;
}
const backupsStore = new Map<string, WorldBackup[]>();

// ---- аккаунты ----
const ACC_KEY = "cubelaunch.mock.accounts";

function loadAccounts(): AccountsFile {
  try {
    return JSON.parse(localStorage.getItem(ACC_KEY) ?? "") as AccountsFile;
  } catch {
    return { accounts: [], defaultId: null };
  }
}

function saveAccounts(d: AccountsFile) {
  localStorage.setItem(ACC_KEY, JSON.stringify(d));
  emit(EVENTS.accountsChanged, null);
}

let msaCancelled = false;

/** Синтетический скин 64×64 (раскрашенные области), чтобы проверить 3D-просмотр без сети. */
function mockSkin(): string {
  const c = document.createElement("canvas");
  c.width = c.height = 64;
  const g = c.getContext("2d")!;
  const fill = (color: string, x: number, y: number, w: number, h: number) => {
    g.fillStyle = color;
    g.fillRect(x, y, w, h);
  };
  fill("#c58c5c", 0, 0, 32, 16); // голова
  fill("#4a2f1b", 8, 0, 8, 8); // макушка
  fill("#ffffff", 9, 12, 2, 1); // глаза
  fill("#ffffff", 13, 12, 2, 1);
  fill("#2e9bf0", 16, 16, 24, 16); // торс
  fill("#c58c5c", 40, 16, 16, 16); // правая рука
  fill("#3b3b8f", 0, 16, 16, 16); // правая нога
  fill("#3b3b8f", 16, 48, 16, 16); // левая нога
  fill("#c58c5c", 32, 48, 16, 16); // левая рука
  return c.toDataURL("image/png");
}

// ---- игра ----
interface MockGame {
  status: GameStatus;
  lines: LogLine[];
  stop: (state: GameStatus["state"]) => void;
}
const games = new Map<string, MockGame>();

function mockLaunch(id: string) {
  const status: GameStatus = { instanceId: id, state: "running", pid: 4242, startedAt: new Date().toISOString() };
  const lines: LogLine[] = [];
  const now = () => new Date().toTimeString().slice(0, 8);
  const script: [LogLine["level"], string, string][] = [
    ["info", "Cubelaunch", "Запуск (браузерный режим, мок)"],
    ["info", "main", "Setting user: Steve"],
    ["info", "Render thread", "Backend library: LWJGL version 3.3.1"],
    ["warn", "Render thread", "Missing sound for event: minecraft:item.goat_horn.play"],
    ["debug", "Worker-Main-1", "Loaded 7 recipes"],
    ["error", "Render thread", "Failed to load texture\njava.io.FileNotFoundException: missing.png\n\tat net.minecraft.Foo.bar(Foo.java:42)"],
    ["info", "Render thread", "Sound engine started"],
  ];
  let i = 0;
  const timer = window.setInterval(() => {
    const [level, thread, text] = script[i % script.length];
    const line = { level, thread, text, time: now() };
    lines.push(line);
    emit(EVENTS.gameLog, { instanceId: id, lines: [line] });
    i++;
  }, 300);
  const game: MockGame = {
    status,
    lines,
    stop: (state) => {
      clearInterval(timer);
      game.status = { ...status, state, endedAt: new Date().toISOString(), exitCode: state === "crashed" ? 1 : 0 };
      if (state === "crashed")
        game.status.crashReport = { path: "crash-reports/crash-mock.txt", text: "---- Minecraft Crash Report ----\n// Oops.\n\nDescription: Mock crash" };
      emit(EVENTS.gameState, game.status);
    },
  };
  games.set(id, game);
  emit(EVENTS.gameState, status);
}

export async function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  await new Promise((r) => setTimeout(r, 30));
  const a = (args ?? {}) as Record<string, never>;
  switch (cmd) {
    case "check_update":
      // В браузерном режиме показываем «найденное» обновление, чтобы было видно баннер.
      return {
        current: "0.1.0-dev",
        version: "0.2.0",
        notes: "## Что нового\n- Автообновление лаунчера\n- Установщики для Windows, macOS и Linux",
        date: new Date().toISOString(),
        manual: "dev",
        releaseUrl: "https://github.com/OWNER/REPO/releases/latest",
      } as T;
    case "install_update":
      throw { kind: "update", message: "Обновление лаунчера: в браузерном режиме недоступно", details: null };
    case "app_info":
      return { version: "0.1.0-dev", dataDir: "(браузерный режим)", portable: false, os: "web", arch: "-" } as T;
    case "get_settings":
      return load() as T;
    case "save_settings": {
      const s = a.settings as Settings;
      validate(s);
      localStorage.setItem(KEY, JSON.stringify(s));
      return s as T;
    }
    case "reset_settings":
      localStorage.removeItem(KEY);
      return structuredClone(defaultSettings) as T;
    case "set_data_dir":
      return true as T;
    case "open_launcher_folder":
    case "open_instance_folder":
      return undefined as T;

    case "get_version_manifest":
      return manifest as T;

    case "list_jobs":
      return [...jobs.values()].map(publicJob) as T;
    case "cancel_job": {
      const j = jobs.get(a.id);
      if (j && (j.state === "running" || j.state === "queued")) finishJob(j, "cancelled");
      return true as T;
    }
    case "retry_job": {
      const j = jobs.get(a.id) ?? fail("not_found", `задача ${a.id}`);
      if (!j.again || (j.state !== "failed" && j.state !== "cancelled")) fail("invalid_input", "повторить можно только упавшую или отменённую задачу");
      jobs.delete(j.id);
      return j.again() as T;
    }
    case "list_groups":
      return mockGroups() as T;
    case "set_group_order": {
      const all = mockGroups();
      const order = (a.order as string[]).filter((g) => all.includes(g));
      saveGroups([...order, ...all.filter((g) => !order.includes(g))]);
      return mockGroups() as T;
    }
    case "create_group": {
      const name = (a.name as string).trim();
      if (!name) fail("invalid_input", "имя группы не может быть пустым");
      if (mockGroups().includes(name)) fail("invalid_input", `группа «${name}» уже есть`);
      saveGroups([...mockGroups(), name]);
      return mockGroups() as T;
    }
    case "rename_group": {
      const [old, next] = [a.old as string, (a.new as string).trim()];
      if (!next) fail("invalid_input", "имя группы не может быть пустым");
      const all = mockGroups();
      saveInstances(loadInstances().map((i) => (i.group === old ? { ...i, group: next } : i)));
      saveGroups(all.includes(next) ? all.filter((g) => g !== old) : all.map((g) => (g === old ? next : g)));
      return mockGroups() as T;
    }
    case "delete_group": {
      const all = mockGroups();
      saveInstances(loadInstances().map((i) => (i.group === a.name ? { ...i, group: null } : i)));
      saveGroups(all.filter((g) => g !== a.name));
      return mockGroups() as T;
    }
    case "clone_instance": {
      const src = loadInstances().find((i) => i.id === a.id) ?? fail("not_found", `инстанс ${a.id}`);
      const list = loadInstances();
      const base = slugify(a.name as string);
      let id = base;
      for (let n = 2; list.some((i) => i.id === id); n++) id = `${base}-${n}`;
      const inst: Instance = { ...src, id, name: (a.name as string).trim(), group: (a.group as string | null)?.trim() || null, createdAt: new Date().toISOString(), playtimeSecs: 0, lastPlayed: undefined };
      saveInstances([...list, inst]);
      startContentJob(inst, "cloneInstance", `Копия «${src.name}» → «${inst.name}»`, () => {});
      return inst as T;
    }
    case "instance_export_candidates":
      return [
        { name: "config", isDir: true, default: true },
        { name: "mods", isDir: true, default: true },
        { name: "saves", isDir: true, default: true },
        { name: "screenshots", isDir: true, default: false },
        { name: "options.txt", isDir: false, default: true },
      ] as T;
    case "export_instance": {
      const inst = loadInstances().find((i) => i.id === a.id) ?? fail("not_found", `инстанс ${a.id}`);
      return startContentJob(inst, "exportInstance", `Экспорт инстанса «${inst.name}»`, () => {}) as T;
    }
    case "dismiss_job":
      jobs.delete(a.id);
      return undefined as T;

    case "list_instances":
      return loadInstances().sort((x, y) => x.name.localeCompare(y.name)) as T;
    case "create_instance": {
      const { name, group, minecraft, loader } = a.args as { name: string; group: string | null; minecraft: string; loader: LoaderSpec | null };
      if (!name.trim()) fail("invalid_input", "имя инстанса не может быть пустым");
      if (!minecraft) fail("invalid_input", "не выбрана версия Minecraft");
      const list = loadInstances();
      const base = slugify(name);
      let id = base;
      for (let n = 2; list.some((i) => i.id === id); n++) id = `${base}-${n}`;
      const inst: Instance = {
        id,
        format: 1,
        name: name.trim(),
        group: group?.trim() || null,
        minecraft,
        loader,
        createdAt: new Date().toISOString(),
        playtimeSecs: 0,
        installed: false,
        overrides: {},
      };
      saveInstances([...list, inst]);
      startInstall(inst);
      return inst as T;
    }
    case "loader_versions":
      await new Promise((r) => setTimeout(r, 300));
      return mockLoaderVersions(a.kind as LoaderKind, a.minecraft as string) as T;
    case "set_instance_version": {
      const list = loadInstances().map((i) =>
        i.id === a.id ? { ...i, minecraft: a.minecraft as string, loader: a.loader as LoaderSpec | null, installed: false } : i,
      );
      saveInstances(list);
      const inst = list.find((i) => i.id === a.id) ?? fail("not_found", `инстанс ${a.id}`);
      startInstall(inst);
      return inst as T;
    }
    case "install_instance": {
      const inst = loadInstances().find((i) => i.id === a.id) ?? fail("not_found", `инстанс ${a.id}`);
      return startInstall(inst) as T;
    }
    case "rename_instance": {
      if (!(a.name as string).trim()) fail("invalid_input", "имя инстанса не может быть пустым");
      const list = loadInstances().map((i) => (i.id === a.id ? { ...i, name: (a.name as string).trim() } : i));
      saveInstances(list);
      return list.find((i) => i.id === a.id) as T;
    }
    case "set_instance_group": {
      const g = (a.group as string | null)?.trim() || null;
      const list = loadInstances().map((i) => (i.id === a.id ? { ...i, group: g } : i));
      saveInstances(list);
      return list.find((i) => i.id === a.id) as T;
    }
    case "delete_instance":
      saveInstances(loadInstances().filter((i) => i.id !== a.id));
      return undefined as T;

    case "get_instance":
      return (loadInstances().find((i) => i.id === a.id) ?? fail("not_found", `инстанс ${a.id}`)) as T;
    case "update_instance_overrides": {
      const list = loadInstances().map((i) => (i.id === a.id ? { ...i, overrides: a.overrides } : i));
      saveInstances(list);
      return list.find((i) => i.id === a.id) as T;
    }

    // ---- аккаунты ----
    case "list_accounts":
      return loadAccounts() as T;
    case "add_offline_account": {
      const name = (a.username as string).trim();
      if (!/^[A-Za-z0-9_]{3,16}$/.test(name))
        fail("invalid_input", "ник должен содержать 3–16 символов: латинские буквы, цифры и «_»");
      const d = loadAccounts();
      const acc: Account = { id: crypto.randomUUID(), kind: "offline", username: name, uuid: "0".repeat(32), addedAt: new Date().toISOString() };
      d.accounts.push(acc);
      d.defaultId ??= acc.id;
      saveAccounts(d);
      return acc as T;
    }
    case "remove_account": {
      const d = loadAccounts();
      d.accounts = d.accounts.filter((x) => x.id !== a.id);
      if (!d.accounts.some((x) => x.id === d.defaultId)) d.defaultId = d.accounts[0]?.id ?? null;
      saveAccounts(d);
      return undefined as T;
    }
    case "set_default_account": {
      const d = loadAccounts();
      d.defaultId = a.id;
      saveAccounts(d);
      return undefined as T;
    }

    // ---- Microsoft ----
    case "msa_status":
      return { configured: true, builtIn: false } as T;
    case "msa_begin":
      msaCancelled = false;
      return { userCode: "CUBE-2024", verificationUri: "https://www.microsoft.com/link", expiresIn: 900, interval: 5 } as T;
    case "msa_finish": {
      for (const stage of ["waiting", "xbox", "xsts", "minecraft", "profile"]) {
        emit(EVENTS.loginStage, stage);
        await new Promise((r) => setTimeout(r, stage === "waiting" ? 1500 : 300));
        if (msaCancelled) throw { kind: "cancelled", message: "Операция отменена", details: null } satisfies BackendError;
      }
      const d = loadAccounts();
      const acc: Account = {
        id: "msa-mock",
        kind: "microsoft",
        username: "Notch",
        uuid: "069a79f444e94726a5befca90e38aaf5",
        addedAt: new Date().toISOString(),
        skin: { url: "mock", variant: "CLASSIC", state: "ACTIVE" },
      };
      d.accounts = [...d.accounts.filter((x) => x.id !== acc.id), acc];
      d.defaultId ??= acc.id;
      saveAccounts(d);
      return acc as T;
    }
    case "msa_cancel":
      msaCancelled = true;
      return undefined as T;
    case "refresh_account":
      return (loadAccounts().accounts.find((x) => x.id === a.id) ?? fail("not_found", `аккаунт ${a.id}`)) as T;
    case "account_skin": {
      const acc = loadAccounts().accounts.find((x) => x.id === a.id);
      return (acc?.skin ? { skin: mockSkin(), model: "classic" } : null) as T;
    }

    // ---- Java ----
    case "detect_java":
      return [
        { path: "C:\\Program Files\\Eclipse Adoptium\\jdk-21\\bin\\java.exe", version: "21.0.5", major: 21, arch: "x86_64", vendor: "Eclipse Adoptium", managed: false },
        { path: "C:\\Cubelaunch\\java\\temurin-17-x64\\bin\\java.exe", version: "17.0.13", major: 17, arch: "x86_64", vendor: "Eclipse Adoptium", managed: true },
        { path: "C:\\Program Files\\Java\\jre1.8.0_421\\bin\\java.exe", version: "1.8.0_421", major: 8, arch: "x86_64", vendor: "Oracle Corporation", managed: false },
      ] satisfies JavaInstall[] as T;
    case "test_java":
      if (!(a.path as string).toLowerCase().includes("java")) fail("java_invalid", `${a.path}: файл не найден`);
      return { path: a.path, version: "17.0.13", major: 17, arch: "x86_64", vendor: "Test", managed: false } as T;
    case "download_java":
      return 0 as T;
    case "system_info":
      return { totalMemoryMb: 16384, recommendedMaxMemoryMb: 4096, cpus: 8 } as T;

    // ---- запуск ----
    case "list_content":
      return contentList(a.id as string, a.kind as ContentKind).map((f) => ({ ...f })) as T;
    case "set_content_enabled": {
      const list = contentList(a.id as string, a.kind as ContentKind);
      const f = list.find((x) => x.fileName === a.fileName) ?? fail("not_found", a.fileName as string);
      const base = f.fileName.replace(/\.disabled$/, "");
      f.enabled = a.enabled as boolean;
      f.fileName = f.enabled ? base : base + ".disabled";
      return f.fileName as T;
    }
    case "delete_content": {
      const names = a.fileNames as string[];
      contentStore.set(
        `${a.id}:${a.kind}`,
        contentList(a.id as string, a.kind as ContentKind).filter((f) => !names.includes(f.fileName)),
      );
      return undefined as T;
    }
    case "add_content_files":
      return { added: [], skipped: [] } as T;
    case "content_modrinth_info": {
      const out: Record<string, unknown> = {};
      for (const f of contentList(a.id as string, a.kind as ContentKind)) {
        const p =
          f.sha1 === "s1" || f.sha1 === "s1-new"
            ? MOCK_PROJECTS[0]
            : f.sha1?.startsWith("mr-")
              ? MOCK_PROJECTS.find((x) => x.projectId === f.sha1!.slice(3))
              : undefined;
        if (p)
          out[f.fileName] = {
            projectId: p.projectId,
            versionId: "v1",
            versionNumber: f.meta?.version ?? "1.0.0",
            title: p.title,
            slug: p.slug,
            projectType: p.projectType,
            iconUrl: p.iconUrl,
          };
      }
      return out as T;
    }
    case "modrinth_search": {
      const inst = loadInstances().find((i) => i.id === a.id) ?? fail("not_found", `инстанс ${a.id}`);
      if (a.kind === "mods" && !inst.loader) fail("content", "для модов нужен загрузчик — выберите его на вкладке «Версия»");
      await new Promise((r) => setTimeout(r, 250));
      const q = (a.query as string).toLowerCase();
      const hits = MOCK_PROJECTS.filter((p) => !q || p.title.toLowerCase().includes(q) || p.description.toLowerCase().includes(q));
      return { hits, totalHits: hits.length, offset: 0, limit: 20 } as T;
    }
    case "modrinth_versions":
      await new Promise((r) => setTimeout(r, 200));
      return ["1.2.0", "1.1.0-beta", "1.0.0"].map((v, i) => ({
        id: `${a.project}-${v}`,
        projectId: a.project,
        name: v,
        versionNumber: v,
        versionType: i === 1 ? "beta" : "release",
        gameVersions: ["1.21.1"],
        loaders: ["fabric"],
        files: [],
        dependencies: [],
        downloads: 10,
      })) as T;
    case "install_modrinth": {
      const inst = loadInstances().find((i) => i.id === a.id) ?? fail("not_found", `инстанс ${a.id}`);
      const [project, ...rest] = (a.versionId as string).split("-");
      return startContentJob(inst, "installContent", `${a.title} → ${inst.name}`, () => {
        const p = MOCK_PROJECTS.find((x) => x.projectId === project);
        contentList(inst.id, a.kind as ContentKind).push({
          fileName: `${p?.slug ?? project}.jar`,
          enabled: true,
          isDir: false,
          size: 500_000,
          sha1: `mr-${project}`,
          incompatible: false,
          meta: { name: p?.title, version: rest.join("-"), description: p?.description, loaders: ["fabric"] },
        });
      }) as T;
    }
    case "check_content_updates":
      await new Promise((r) => setTimeout(r, 400));
      return contentList(a.id as string, a.kind as ContentKind)
        .filter((f) => f.sha1 === "s1")
        .map((f) => ({
          fileName: f.fileName,
          projectId: "AANobbMI",
          title: "Sodium",
          currentVersion: "0.6.0",
          versionId: "AANobbMI-0.6.5",
          versionNumber: "0.6.5",
        })) as T;
    case "update_content": {
      const inst = loadInstances().find((i) => i.id === a.id) ?? fail("not_found", `инстанс ${a.id}`);
      return startContentJob(inst, "updateContent", `${inst.name} — обновление`, () => {
        for (const it of a.items as { fileName: string }[]) {
          const f = contentList(inst.id, a.kind as ContentKind).find((x) => x.fileName === it.fileName);
          if (f) {
            f.sha1 = "s1-new";
            f.fileName = "sodium-fabric-0.6.5.jar";
            if (f.meta) f.meta.version = "0.6.5";
          }
        }
      }) as T;
    }
    case "curseforge_status":
      return cfStatus() as T;
    case "set_curseforge_key": {
      await new Promise((r) => setTimeout(r, 400));
      const k = (a.key as string | null)?.trim() || null;
      if (k && k.length < 8) fail("curseforge_key", "ключ CurseForge API недействителен — проверьте его в настройках");
      cfKey = k;
      return cfStatus() as T;
    }
    case "curseforge_search":
    case "curseforge_versions":
    case "install_curseforge": {
      if (!cfKey) fail("curseforge_key", "CurseForge не настроен: укажите ключ API в настройках");
      if (cmd === "curseforge_search") return mockInvoke<T>("modrinth_search", a);
      if (cmd === "curseforge_versions") return mockInvoke<T>("modrinth_versions", a);
      return mockInvoke<T>("install_modrinth", { ...a, versionId: `${a.projectId}-${a.fileId}` });
    }
    case "modpack_search": {
      if (a.source === "curseforge" && !cfKey) fail("curseforge_key", "CurseForge не настроен: укажите ключ API в настройках");
      await new Promise((r) => setTimeout(r, 250));
      const q = (a.query as string).toLowerCase();
      const hits = MOCK_PACKS.filter((p) => !q || p.title.toLowerCase().includes(q));
      return { hits, totalHits: hits.length, offset: 0, limit: 20 } as T;
    }
    case "modpack_versions":
      await new Promise((r) => setTimeout(r, 200));
      return packVersions(a.project as string) as T;
    case "install_modpack": {
      const v = packVersions(a.projectId as string).find((x) => x.id === a.versionId) ?? fail("not_found", `версия ${a.versionId}`);
      return createPackInstance((a.name as string) || (a.title as string), a.group as string | null, v.gameVersions[0], {
        source: a.source as "modrinth", projectId: a.projectId as string, versionId: a.versionId as string, name: a.title as string,
        version: v.versionNumber, iconUrl: (a.iconUrl as string | null) ?? null,
      }, a.source === "curseforge" ? 2 : 0) as T;
    }
    case "inspect_modpack": {
      await new Promise((r) => setTimeout(r, 300));
      const p = a.path as string;
      if (!/\.(mrpack|zip)$/i.test(p)) fail("content", "в архиве нет modrinth.index.json или manifest.json — это не модпак Modrinth или CurseForge");
      if (/prism/i.test(p))
        return { format: "prism", name: "Prism Pack", version: "", summary: null, minecraft: "1.20.1", loader: "forge", loaderVersion: "47.2.0", files: 0, overrides: 340, hasSaves: true, warnings: ["jar-моды"] } as T;
      if (/instance/i.test(p))
        return { format: "cubelaunch", name: "Перенесённый инстанс", version: "", summary: null, minecraft: "1.21.1", loader: "fabric", loaderVersion: "0.16.5", files: 0, overrides: 120, hasSaves: true, warnings: [] } as T;
      const cf = p.endsWith(".zip");
      return { hasSaves: false, warnings: [], format: cf ? "curseforge" : "mrpack", name: "Example Pack", version: "1.2", summary: cf ? null : "Тестовый модпак", minecraft: "1.21.1", loader: "fabric", loaderVersion: "0.16.5", files: 42, overrides: 120 } as T;
    }
    case "import_modpack": {
      if (/prism|instance/i.test(a.path as string)) {
        const list = loadInstances();
        const name = (a.name as string) || (/prism/i.test(a.path as string) ? "Prism Pack" : "Перенесённый инстанс");
        const base = slugify(name);
        let id = base;
        for (let n = 2; list.some((i) => i.id === id); n++) id = `${base}-${n}`;
        const inst: Instance = { id, format: 1, name, group: (a.group as string | null)?.trim() || null, minecraft: "1.20.1", loader: { kind: "forge", version: "1.20.1-47.2.0" }, createdAt: new Date().toISOString(), playtimeSecs: 0, installed: false, overrides: {} };
        saveInstances([...list, inst]);
        startContentJob(inst, "importInstance", `Импорт «${name}»`, () => saveInstances(loadInstances().map((i) => (i.id === id ? { ...i, installed: true } : i))));
        return inst as T;
      }
      const cf = (a.path as string).endsWith(".zip");
      return createPackInstance((a.name as string) || "Example Pack", a.group as string | null, "1.21.1", {
        source: "file", name: "Example Pack", version: "1.2",
      }, cf ? 1 : 0) as T;
    }
    case "update_modpack": {
      const inst = loadInstances().find((i) => i.id === a.id) ?? fail("not_found", `инстанс ${a.id}`);
      if (!inst.modpack) fail("invalid_input", "инстанс не создан из модпака");
      return startContentJob(inst, "updateModpack", `Обновление «${inst.name}»`, () => {
        const v = a.versionId ? packVersions(inst.modpack!.projectId ?? "").find((x) => x.id === a.versionId) : null;
        saveInstances(loadInstances().map((i) => (i.id === inst.id ? { ...i, modpack: { ...i.modpack!, versionId: (a.versionId as string) ?? i.modpack!.versionId, version: v?.versionNumber ?? "1.3" } } : i)));
        emit(EVENTS.instancesChanged, null);
      }) as T;
    }
    case "modpack_state":
      return (modpackStates.get(a.id as string) ?? { files: 0, missing: [] }) as T;
    case "modpack_add_missing": {
      const s = modpackStates.get(a.id as string);
      if (!s) return 0 as T;
      s.missing = s.missing.slice(1);
      return s.missing.length as T;
    }
    case "export_candidates":
      return [
        { name: "config", isDir: true, default: true },
        { name: "mods", isDir: true, default: true },
        { name: "resourcepacks", isDir: true, default: true },
        { name: "saves", isDir: true, default: false },
        { name: "options.txt", isDir: false, default: true },
      ] as T;
    case "export_modpack": {
      const inst = loadInstances().find((i) => i.id === a.id) ?? fail("not_found", `инстанс ${a.id}`);
      return startContentJob(inst, "exportModpack", `Экспорт «${inst.name}»`, () => {}) as T;
    }
    case "list_worlds":
      return { worlds: worldsList(a.id as string).map((w) => ({ ...w })), backups: backupsStore.get(a.id as string) ?? [] } as T;
    case "backup_world": {
      const inst = loadInstances().find((i) => i.id === a.id) ?? fail("not_found", `инстанс ${a.id}`);
      return startContentJob(inst, "backupWorld", `Резервная копия «${a.folder}»`, () => {
        const list = backupsStore.get(inst.id) ?? [];
        list.unshift({ fileName: `${a.folder}-2026-11-15_12-00-00.zip`, size: 30_000_000, created: new Date().toISOString() });
        backupsStore.set(inst.id, list);
      }) as T;
    }
    case "delete_world":
      worldsStore.set(
        a.id as string,
        worldsList(a.id as string).filter((w) => w.folder !== a.folder),
      );
      return undefined as T;
    case "open_content_folder":
      return undefined as T;

    case "launch_instance": {
      if (!loadAccounts().defaultId) throw { kind: "no_account", message: "Не выбран аккаунт", details: null } satisfies BackendError;
      mockLaunch(a.id as string);
      return 0 as T;
    }
    case "kill_game": {
      const g = games.get(a.id as string);
      if (g?.status.state === "running") g.stop("killed");
      return undefined as T;
    }
    case "get_game_log": {
      const g = games.get(a.id as string);
      return { status: g?.status ?? null, lines: [...(g?.lines ?? [])] } as T;
    }
    case "running_games":
      return [...games.values()].filter((g) => g.status.state === "running").map((g) => g.status) as T;
    case "save_game_log":
      return undefined as T;

    default:
      throw { kind: "other", message: `Команда ${cmd} недоступна в браузерном режиме`, details: null } satisfies BackendError;
  }
}
