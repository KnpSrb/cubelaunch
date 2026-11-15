import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import clsx from "clsx";
import { Button, Field, Input, NumberInput, Section, Switch } from "@/components/ui";
import { api } from "@/lib/ipc";
import type { Instance, InstanceOverrides } from "@/lib/types";
import { useSettings } from "@/stores/settings";
import { notify } from "@/stores/toasts";
import { JavaPicker } from "@/features/java/JavaPicker";

/** Строка формы: флажок «своё значение» + редактор, неактивный без флажка. */
function OverrideRow({
  label,
  enabled,
  onToggle,
  globalText,
  children,
}: {
  label: string;
  enabled: boolean;
  onToggle: (on: boolean) => void;
  globalText: string;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <Field
      label={
        <span className="flex flex-col gap-1">
          {label}
          <label className="flex items-center gap-1.5 text-xs font-normal text-muted">
            <input type="checkbox" checked={enabled} onChange={(e) => onToggle(e.target.checked)} className="accent-[var(--accent)]" />
            {t("overrides.override")}
          </label>
        </span>
      }
      hint={!enabled ? t("overrides.global", { value: globalText }) : undefined}
    >
      <div className={clsx(!enabled && "pointer-events-none opacity-50")} aria-disabled={!enabled}>
        {children}
      </div>
    </Field>
  );
}

function envToText(env: Record<string, string> | undefined) {
  return Object.entries(env ?? {})
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
}

function textToEnv(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const i = line.indexOf("=");
    if (i > 0) out[line.slice(0, i).trim()] = line.slice(i + 1);
  }
  return out;
}

export function OverridesForm({ inst }: { inst: Instance }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const g = useSettings((s) => s.settings)!;
  const [o, setO] = useState<InstanceOverrides>(inst.overrides ?? {});
  const [envText, setEnvText] = useState(envToText(inst.overrides?.env));
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    setO(inst.overrides ?? {});
    setEnvText(envToText(inst.overrides?.env));
  }, [inst.overrides]);

  const draft: InstanceOverrides = useMemo(() => ({ ...o, env: textToEnv(envText) }), [o, envText]);
  const dirty = JSON.stringify(draft) !== JSON.stringify({ ...inst.overrides, env: inst.overrides?.env ?? {} });

  const set = (patch: Partial<InstanceOverrides>) => setO((prev) => ({ ...prev, ...patch }));
  const has = (v: unknown) => v !== undefined && v !== null;

  const save = async () => {
    setBusy(true);
    try {
      // Убираем пустые значения, чтобы в instance.json оставались только реальные переопределения.
      const clean = Object.fromEntries(
        Object.entries(draft).filter(([, v]) => has(v) && v !== "" && !(typeof v === "object" && Object.keys(v as object).length === 0)),
      ) as InstanceOverrides;
      const updated = await api.updateInstanceOverrides(inst.id, clean);
      qc.setQueryData(["instance", inst.id], updated);
      void qc.invalidateQueries({ queryKey: ["instances"] });
      notify.success(t("overrides.saved"));
    } catch (e) {
      notify.error(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-muted">{t("overrides.intro")}</p>
      <Section title={t("settings.sections.java")}>
        <OverrideRow
          label={t("overrides.javaPath")}
          enabled={has(o.javaPath)}
          onToggle={(on) => set({ javaPath: on ? (g.java.path ?? "") : null })}
          globalText={g.java.path ?? t("javaSettings.auto")}
        >
          <JavaPicker value={o.javaPath || null} onChange={(p) => set({ javaPath: p ?? "" })} />
        </OverrideRow>
        <OverrideRow
          label={t("overrides.memory")}
          enabled={has(o.minMemoryMb)}
          onToggle={(on) =>
            set(on ? { minMemoryMb: g.java.minMemoryMb, maxMemoryMb: g.java.maxMemoryMb } : { minMemoryMb: null, maxMemoryMb: null })
          }
          globalText={`${g.java.minMemoryMb}–${g.java.maxMemoryMb} ${t("common.mb")}`}
        >
          <div className="flex items-center gap-2">
            <NumberInput aria-label="Xms" step={256} value={o.minMemoryMb ?? g.java.minMemoryMb} onValueChange={(v) => set({ minMemoryMb: v })} />
            <span className="text-muted">–</span>
            <NumberInput
              aria-label="Xmx"
              step={256}
              value={o.maxMemoryMb ?? g.java.maxMemoryMb}
              suffix={t("common.mb")}
              onValueChange={(v) => set({ maxMemoryMb: v })}
            />
          </div>
        </OverrideRow>
        <OverrideRow
          label={t("overrides.jvmArgs")}
          enabled={has(o.jvmArgs)}
          onToggle={(on) => set({ jvmArgs: on ? g.java.jvmArgs : null })}
          globalText={g.java.jvmArgs || "—"}
        >
          <Input className="w-full font-mono" value={o.jvmArgs ?? ""} onChange={(e) => set({ jvmArgs: e.target.value })} />
        </OverrideRow>
      </Section>

      <Section title={t("settings.sections.game")}>
        <OverrideRow
          label={t("overrides.window")}
          enabled={has(o.windowWidth)}
          onToggle={(on) =>
            set(on ? { windowWidth: g.game.windowWidth, windowHeight: g.game.windowHeight } : { windowWidth: null, windowHeight: null })
          }
          globalText={`${g.game.windowWidth}×${g.game.windowHeight}`}
        >
          <div className="flex items-center gap-2">
            <NumberInput aria-label={t("settings.game.width")} value={o.windowWidth ?? g.game.windowWidth} onValueChange={(v) => set({ windowWidth: v })} />
            <span className="text-muted">×</span>
            <NumberInput aria-label={t("settings.game.height")} value={o.windowHeight ?? g.game.windowHeight} onValueChange={(v) => set({ windowHeight: v })} />
          </div>
        </OverrideRow>
        <OverrideRow
          label={t("overrides.fullscreen")}
          enabled={has(o.fullscreen)}
          onToggle={(on) => set({ fullscreen: on ? g.game.fullscreen : null })}
          globalText={g.game.fullscreen ? "✓" : "—"}
        >
          <Switch checked={o.fullscreen ?? false} onCheckedChange={(v) => set({ fullscreen: v })} label={t("overrides.fullscreen")} />
        </OverrideRow>
      </Section>

      <Section title={t("overrides.env")}>
        <Field label={t("overrides.env")} hint={t("overrides.envHint")}>
          <textarea
            value={envText}
            onChange={(e) => setEnvText(e.target.value)}
            rows={3}
            className="rounded-md border border-border bg-surface p-2 font-mono text-xs outline-none focus:border-accent"
          />
        </Field>
        <Field label={t("overrides.preLaunch")}>
          <Input className="w-full font-mono" value={o.preLaunchCommand ?? ""} onChange={(e) => set({ preLaunchCommand: e.target.value || null })} />
        </Field>
        <Field label={t("overrides.postExit")} hint={t("overrides.commandsHint")}>
          <Input className="w-full font-mono" value={o.postExitCommand ?? ""} onChange={(e) => set({ postExitCommand: e.target.value || null })} />
        </Field>
      </Section>

      <div className="sticky bottom-0 flex justify-end gap-2 border-t border-border bg-bg py-3">
        <Button
          disabled={!dirty || busy}
          onClick={() => {
            setO(inst.overrides ?? {});
            setEnvText(envToText(inst.overrides?.env));
          }}
        >
          {t("settings.discard")}
        </Button>
        <Button variant="primary" disabled={!dirty || busy} onClick={save}>
          {t("common.save")}
        </Button>
      </div>
    </div>
  );
}
