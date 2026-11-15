import { useEffect, useMemo, useState } from "react";
import { useLocation } from "react-router";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import clsx from "clsx";
import { Check, Download, FolderOpen, Moon, Monitor, Sun } from "lucide-react";
import { Button, Field, Input, NumberInput, Section, Segmented, Select, Switch } from "@/components/ui";
import { useSettings } from "@/stores/settings";
import { notify } from "@/stores/toasts";
import { api, inTauri } from "@/lib/ipc";
import type { Settings } from "@/lib/types";
import { LANGUAGES } from "@/i18n";
import { ACCENT_PRESETS } from "@/app/theme";
import { useHotkeys } from "@/app/hotkeys";
import { JavaPicker } from "@/features/java/JavaPicker";
import { formatBytes } from "@/lib/format";
import { UpdateSettingsBlock } from "@/features/updates/updates";

const SECTIONS = ["general", "appearance", "java", "game", "network", "about"] as const;

/** Ключ CurseForge: сохраняется сразу (в системное хранилище секретов), не через черновик настроек. */
function CurseforgeKey() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const status = useQuery({ queryKey: ["curseforge-status"], queryFn: api.curseforgeStatus });
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const apply = async (value: string | null) => {
    setBusy(true);
    try {
      qc.setQueryData(["curseforge-status"], await api.setCurseforgeKey(value));
      setKey("");
      notify.success(value ? t("curseforge.keySaved") : t("curseforge.keyRemoved"));
    } catch (e) {
      notify.error(e);
    } finally {
      setBusy(false);
    }
  };
  const s = status.data;
  return (
    <Field
      label={t("curseforge.key")}
      hint={
        <>
          {t("curseforge.keyHint")}{" "}
          {s?.userKey ? t("curseforge.keyStored") : s?.builtIn ? t("curseforge.keyBuiltIn") : t("curseforge.keyNone")}
        </>
      }
    >
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (key.trim()) void apply(key.trim());
        }}
      >
        <Input
          type="password"
          autoComplete="off"
          value={key}
          placeholder={s?.userKey ? "••••••••••••" : "$2a$10$…"}
          onChange={(e) => setKey(e.target.value)}
          className="flex-1 font-mono"
          spellCheck={false}
        />
        <Button type="submit" disabled={busy || !key.trim()}>
          {t("curseforge.check")}
        </Button>
        {s?.userKey && (
          <Button type="button" variant="ghost" disabled={busy} onClick={() => void apply(null)}>
            {t("common.delete")}
          </Button>
        )}
      </form>
    </Field>
  );
}

/** Обновляет вложенный объект настроек иммутабельно. */
function patch<K extends keyof Settings>(s: Settings, key: K, value: Partial<Settings[K]>): Settings {
  return { ...s, [key]: { ...(s[key] as object), ...value } };
}

export function SettingsPage() {
  const saved = useSettings((s) => s.settings);
  if (!saved) return null;
  return <SettingsForm saved={saved} />;
}

function SettingsForm({ saved }: { saved: Settings }) {
  const { t } = useTranslation();
  const { hash } = useLocation();
  const { save, reset, preview } = useSettings();
  const [draft, setDraft] = useState<Settings>(saved);
  const [busy, setBusy] = useState(false);
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo });
  const sys = useQuery({ queryKey: ["system-info"], queryFn: api.systemInfo, staleTime: Infinity });
  const msa = useQuery({ queryKey: ["msa-status"], queryFn: api.msaStatus });

  const dirty = useMemo(() => JSON.stringify(draft) !== JSON.stringify(saved), [draft, saved]);

  // Живой предпросмотр темы, акцента и языка; при уходе со страницы — откат к сохранённым.
  useEffect(() => preview(draft), [draft, preview]);
  useEffect(() => () => preview(useSettings.getState().settings ?? saved), [preview, saved]);

  useEffect(() => {
    if (hash) document.getElementById(hash.slice(1))?.scrollIntoView();
  }, [hash]);

  const onSave = async () => {
    setBusy(true);
    try {
      const s = await save(draft);
      setDraft(s);
      notify.success(t("settings.saved"));
    } catch (e) {
      notify.error(e);
    } finally {
      setBusy(false);
    }
  };

  useHotkeys({ "mod+s": () => dirty && !busy && void onSave() });

  const onReset = async () => {
    if (!window.confirm(t("settings.resetConfirm"))) return;
    try {
      setDraft(await reset());
    } catch (e) {
      notify.error(e);
    }
  };

  const changeDataDir = async (useDefault: boolean) => {
    try {
      let path: string | null = null;
      if (!useDefault) {
        const picked = await openDialog({ directory: true, multiple: false });
        if (typeof picked !== "string") return;
        path = picked;
      }
      if (await api.setDataDir(path)) notify.info(t("settings.restartRequired"));
    } catch (e) {
      notify.error(e);
    }
  };

  const d = draft;

  return (
    <div className="flex h-full">
      <nav className="w-52 shrink-0 border-r border-border bg-surface p-3">
        <h1 className="mb-3 px-2 text-lg font-semibold">{t("settings.title")}</h1>
        {SECTIONS.map((s) => (
          <a
            key={s}
            href={`#${s}`}
            onClick={(e) => {
              e.preventDefault();
              document.getElementById(s)?.scrollIntoView({ behavior: "smooth" });
            }}
            className="block rounded-md px-2 py-1.5 text-sm text-muted hover:bg-surface-2 hover:text-text"
          >
            {t(`settings.sections.${s}`)}
          </a>
        ))}
      </nav>

      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex-1 overflow-y-auto">
          <div className="mx-auto flex max-w-3xl flex-col gap-5 p-6">
            <Section id="general" title={t("settings.sections.general")}>
              <Field label={t("settings.general.language")}>
                <Select
                  className="w-56"
                  value={d.language}
                  onValueChange={(language) => setDraft({ ...d, language })}
                  options={LANGUAGES.map((l) => ({ value: l.code, label: l.name }))}
                />
              </Field>
              <Field
                label={t("settings.general.dataDir")}
                hint={info.data?.portable ? t("settings.general.portable") : t("settings.general.dataDirHint")}
              >
                <code className="rounded bg-surface-2 px-2 py-1.5 text-xs break-all select-text">
                  {info.data?.dataDir}
                </code>
                <div className="flex gap-2">
                  <Button
                    size="sm"
                    icon={<FolderOpen size={14} />}
                    onClick={() => api.openLauncherFolder("data").catch(notify.error)}
                  >
                    {t("settings.general.open")}
                  </Button>
                  {inTauri && !info.data?.portable && (
                    <>
                      <Button size="sm" onClick={() => changeDataDir(false)}>
                        {t("settings.general.change")}
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => changeDataDir(true)}>
                        {t("settings.general.useDefault")}
                      </Button>
                    </>
                  )}
                </div>
              </Field>
            </Section>

            <Section id="microsoft" title={t("accounts.msaTitle")}>
              <Field
                label={t("settings.general.msaClientId")}
                hint={
                  <>
                    {t("settings.general.msaClientIdHint")}
                    {msa.data?.builtIn && <> {t("settings.general.msaClientIdBuiltIn")}</>}
                  </>
                }
              >
                <Input
                  value={d.msaClientId ?? ""}
                  placeholder="00000000-0000-0000-0000-000000000000"
                  onChange={(e) => setDraft({ ...d, msaClientId: e.target.value.trim() || null })}
                  className="font-mono"
                  spellCheck={false}
                />
              </Field>
            </Section>

            <Section id="curseforge" title="CurseForge">
              <CurseforgeKey />
            </Section>

            <Section id="appearance" title={t("settings.sections.appearance")}>
              <Field label={t("settings.appearance.theme")}>
                <Segmented
                  value={d.appearance.theme}
                  onValueChange={(theme) => setDraft(patch(d, "appearance", { theme }))}
                  options={[
                    { value: "system", label: <><Monitor size={14} />{t("settings.appearance.themeSystem")}</> },
                    { value: "dark", label: <><Moon size={14} />{t("settings.appearance.themeDark")}</> },
                    { value: "light", label: <><Sun size={14} />{t("settings.appearance.themeLight")}</> },
                  ]}
                />
              </Field>
              <Field label={t("settings.appearance.accent")}>
                <div className="flex flex-wrap items-center gap-2">
                  {ACCENT_PRESETS.map((c) => (
                    <button
                      key={c}
                      type="button"
                      aria-label={c}
                      onClick={() => setDraft(patch(d, "appearance", { accent: c }))}
                      className={clsx(
                        "flex h-7 w-7 items-center justify-center rounded-full ring-offset-2 ring-offset-surface",
                        d.appearance.accent.toLowerCase() === c && "ring-2 ring-text",
                      )}
                      style={{ background: c }}
                    >
                      {d.appearance.accent.toLowerCase() === c && <Check size={14} className="text-white" />}
                    </button>
                  ))}
                  <label className="ml-2 flex items-center gap-2 text-sm text-muted">
                    <input
                      type="color"
                      value={d.appearance.accent}
                      onChange={(e) => setDraft(patch(d, "appearance", { accent: e.target.value }))}
                      className="h-7 w-9 cursor-pointer rounded border border-border bg-transparent"
                    />
                    {t("settings.appearance.accentCustom")}
                  </label>
                </div>
              </Field>
              <Field label={t("settings.appearance.instanceView")}>
                <Segmented
                  value={d.appearance.instanceView}
                  onValueChange={(instanceView) => setDraft(patch(d, "appearance", { instanceView }))}
                  options={[
                    { value: "grid", label: t("instances.viewGrid") },
                    { value: "list", label: t("instances.viewList") },
                  ]}
                />
              </Field>
              <Field label={t("settings.appearance.iconSize")}>
                <div className="flex items-center gap-3">
                  <input
                    type="range"
                    min={32}
                    max={128}
                    step={8}
                    value={d.appearance.iconSize}
                    onChange={(e) => setDraft(patch(d, "appearance", { iconSize: Number(e.target.value) }))}
                    className="w-56 accent-[var(--accent)]"
                  />
                  <span className="text-sm text-muted">{d.appearance.iconSize} px</span>
                </div>
              </Field>
            </Section>

            <Section id="java" title={t("settings.sections.java")}>
              <Field label={t("settings.java.path")}>
                <JavaPicker value={d.java.path} onChange={(path) => setDraft(patch(d, "java", { path }))} />
              </Field>
              <Field label="" hint={t("javaSettings.downloadHint")}>
                <div className="flex flex-wrap gap-2">
                  {[8, 17, 21, 25].map((major) => (
                    <Button
                      key={major}
                      size="sm"
                      icon={<Download size={13} />}
                      onClick={() => api.downloadJava(major).catch(notify.error)}
                    >
                      {t("javaSettings.download", { major })}
                    </Button>
                  ))}
                </div>
              </Field>
              <Field
                label={t("settings.java.minMemory")}
                hint={
                  sys.data &&
                  t("javaSettings.memoryHint", {
                    total: formatBytes(sys.data.totalMemoryMb * 1024 * 1024),
                    rec: formatBytes(sys.data.recommendedMaxMemoryMb * 1024 * 1024),
                  })
                }
              >
                <NumberInput
                  min={128}
                  step={256}
                  value={d.java.minMemoryMb}
                  suffix={t("common.mb")}
                  onValueChange={(minMemoryMb) => setDraft(patch(d, "java", { minMemoryMb }))}
                />
              </Field>
              <Field
                label={t("settings.java.maxMemory")}
                hint={
                  sys.data && d.java.maxMemoryMb > sys.data.totalMemoryMb - 2048 ? (
                    <span className="text-warning">{t("javaSettings.memoryTooHigh")}</span>
                  ) : undefined
                }
              >
                <NumberInput
                  min={256}
                  step={256}
                  value={d.java.maxMemoryMb}
                  suffix={t("common.mb")}
                  onValueChange={(maxMemoryMb) => setDraft(patch(d, "java", { maxMemoryMb }))}
                />
              </Field>
              <Field label={t("settings.java.jvmArgs")}>
                <Input
                  value={d.java.jvmArgs}
                  placeholder={t("settings.java.jvmArgsPlaceholder")}
                  onChange={(e) => setDraft(patch(d, "java", { jvmArgs: e.target.value }))}
                  className="font-mono"
                />
              </Field>
              <div className="py-2.5">
                <Switch
                  checked={d.java.autoDownload}
                  onCheckedChange={(autoDownload) => setDraft(patch(d, "java", { autoDownload }))}
                  label={t("settings.java.autoDownload")}
                />
              </div>
            </Section>

            <Section id="game" title={t("settings.sections.game")}>
              <Field label={t("settings.game.windowSize")}>
                <div className="flex items-center gap-2">
                  <NumberInput
                    aria-label={t("settings.game.width")}
                    value={d.game.windowWidth}
                    onValueChange={(windowWidth) => setDraft(patch(d, "game", { windowWidth }))}
                  />
                  <span className="text-muted">×</span>
                  <NumberInput
                    aria-label={t("settings.game.height")}
                    value={d.game.windowHeight}
                    onValueChange={(windowHeight) => setDraft(patch(d, "game", { windowHeight }))}
                  />
                </div>
              </Field>
              <Field label={t("settings.game.onLaunch")}>
                <Select
                  className="w-72"
                  value={d.game.onLaunch}
                  onValueChange={(onLaunch) => setDraft(patch(d, "game", { onLaunch }))}
                  options={[
                    { value: "keep", label: t("settings.game.onLaunchKeep") },
                    { value: "minimize", label: t("settings.game.onLaunchMinimize") },
                    { value: "hide", label: t("settings.game.onLaunchHide") },
                    { value: "close", label: t("settings.game.onLaunchClose") },
                  ]}
                />
              </Field>
              <div className="flex flex-col py-2.5">
                <Switch
                  checked={d.game.fullscreen}
                  onCheckedChange={(fullscreen) => setDraft(patch(d, "game", { fullscreen }))}
                  label={t("settings.game.fullscreen")}
                />
                <Switch
                  checked={d.game.showLogOnLaunch}
                  onCheckedChange={(showLogOnLaunch) => setDraft(patch(d, "game", { showLogOnLaunch }))}
                  label={t("settings.game.showLog")}
                />
                <Switch
                  checked={d.game.recordPlaytime}
                  onCheckedChange={(recordPlaytime) => setDraft(patch(d, "game", { recordPlaytime }))}
                  label={t("settings.game.playtime")}
                />
              </div>
            </Section>

            <Section id="network" title={t("settings.sections.network")}>
              <Field label={t("settings.network.concurrency")}>
                <NumberInput
                  min={1}
                  max={64}
                  value={d.downloads.concurrency}
                  onValueChange={(concurrency) => setDraft(patch(d, "downloads", { concurrency }))}
                />
              </Field>
              <Field label={t("settings.network.maxJobs")} hint={t("settings.network.maxJobsHint")}>
                <NumberInput
                  min={1}
                  max={8}
                  value={d.downloads.maxJobs}
                  onValueChange={(maxJobs) => setDraft(patch(d, "downloads", { maxJobs }))}
                />
              </Field>
              <Field label={t("settings.network.retries")}>
                <NumberInput
                  min={0}
                  max={10}
                  value={d.downloads.retries}
                  onValueChange={(retries) => setDraft(patch(d, "downloads", { retries }))}
                />
              </Field>
              <Field label={t("settings.network.timeout")}>
                <NumberInput
                  min={5}
                  max={600}
                  value={d.downloads.timeoutSecs}
                  suffix={t("common.seconds")}
                  onValueChange={(timeoutSecs) => setDraft(patch(d, "downloads", { timeoutSecs }))}
                />
              </Field>
              <Field label={t("settings.network.proxy")}>
                <Segmented
                  value={d.proxy.mode}
                  onValueChange={(mode) => setDraft(patch(d, "proxy", { mode }))}
                  options={[
                    { value: "none", label: t("settings.network.proxyNone") },
                    { value: "system", label: t("settings.network.proxySystem") },
                    { value: "http", label: t("settings.network.proxyHttp") },
                    { value: "socks5", label: t("settings.network.proxySocks5") },
                  ]}
                />
              </Field>
              {(d.proxy.mode === "http" || d.proxy.mode === "socks5") && (
                <Field label="" hint={t("settings.network.passwordHint")}>
                  <div className="flex gap-2">
                    <Input
                      className="flex-1"
                      placeholder={t("settings.network.host")}
                      value={d.proxy.host}
                      onChange={(e) => setDraft(patch(d, "proxy", { host: e.target.value }))}
                    />
                    <NumberInput
                      aria-label={t("settings.network.port")}
                      min={1}
                      max={65535}
                      value={d.proxy.port}
                      onValueChange={(port) => setDraft(patch(d, "proxy", { port }))}
                    />
                  </div>
                  <Input
                    placeholder={t("settings.network.username")}
                    value={d.proxy.username}
                    onChange={(e) => setDraft(patch(d, "proxy", { username: e.target.value }))}
                  />
                </Field>
              )}
            </Section>

            <Section id="about" title={t("settings.sections.about")}>
              <Field label={t("settings.about.version")}>
                <span className="pt-1.5 text-sm">{info.data?.version}</span>
              </Field>
              <Field label={t("settings.about.platform")}>
                <span className="pt-1.5 text-sm">
                  {info.data?.os} / {info.data?.arch}
                </span>
              </Field>
              <Field label={t("updates.title")}>
                <UpdateSettingsBlock />
              </Field>
              <div className="flex flex-col py-2.5">
                <Switch
                  checked={d.updates.checkOnStartup}
                  onCheckedChange={(checkOnStartup) => setDraft(patch(d, "updates", { checkOnStartup }))}
                  label={t("updates.checkOnStartup")}
                />
              </div>
              <p className="pt-3 text-xs text-muted">{t("settings.about.disclaimer")}</p>
            </Section>
          </div>
        </div>

        <div className="flex h-14 shrink-0 items-center gap-2 border-t border-border bg-surface px-6">
          <Button variant="ghost" onClick={onReset}>
            {t("settings.resetAll")}
          </Button>
          <span className={clsx("ml-auto text-sm text-muted", !dirty && "invisible")}>
            {t("settings.unsaved")}
          </span>
          <Button disabled={!dirty || busy} onClick={() => setDraft(saved)}>
            {t("settings.discard")}
          </Button>
          <Button variant="primary" disabled={!dirty || busy} onClick={onSave} title="Ctrl+S">
            {t("common.save")}
          </Button>
        </div>
      </div>
    </div>
  );
}
