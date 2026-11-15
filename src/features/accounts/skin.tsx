import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { UserRound } from "lucide-react";
import { Switch } from "@/components/ui";
import { api } from "@/lib/ipc";
import type { Account, SkinData } from "@/lib/types";

export function useSkin(account: Account | undefined) {
  return useQuery({
    queryKey: ["skin", account?.id, account?.skin?.url, account?.cape?.url],
    queryFn: () => api.accountSkin(account!.id),
    enabled: !!account?.skin,
    staleTime: Infinity,
  });
}

/** Лицо персонажа (8×8 + слой шапки) из текстуры скина, пиксели без сглаживания. */
export function SkinHead({ skin, size = 20 }: { skin?: string; size?: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    if (!skin || !ref.current) return;
    const img = new Image();
    img.onload = () => {
      const g = ref.current?.getContext("2d");
      if (!g) return;
      g.imageSmoothingEnabled = false;
      g.clearRect(0, 0, 8, 8);
      g.drawImage(img, 8, 8, 8, 8, 0, 0, 8, 8);
      g.drawImage(img, 40, 8, 8, 8, 0, 0, 8, 8);
    };
    img.src = skin;
  }, [skin]);
  if (!skin) return <UserRound size={size * 0.8} className="text-muted" />;
  return (
    <canvas
      ref={ref}
      width={8}
      height={8}
      aria-hidden="true"
      style={{ width: size, height: size, imageRendering: "pixelated" }}
      className="rounded-sm"
    />
  );
}

/** 3D-модель игрока (skinview3d загружается лениво — отдельным чанком). */
export function SkinViewer3D({ data, width = 220, height = 300 }: { data: SkinData; width?: number; height?: number }) {
  const { t } = useTranslation();
  const canvas = useRef<HTMLCanvasElement>(null);
  const viewer = useRef<import("skinview3d").SkinViewer | null>(null);
  const [walk, setWalk] = useState(true);
  const [showCape, setShowCape] = useState(true);
  // Счётчик пересоздания просмотрщика — чтобы эффекты ниже применились к новому.
  const [ready, setViewerReady] = useState(0);

  useEffect(() => {
    let disposed = false;
    void import("skinview3d").then((sv) => {
      if (disposed || !canvas.current) return;
      const v = new sv.SkinViewer({ canvas: canvas.current, width, height, zoom: 0.85 });
      v.autoRotate = true;
      v.autoRotateSpeed = 0.6;
      viewer.current = v;
      setViewerReady((n) => n + 1);
    });
    return () => {
      disposed = true;
      viewer.current?.dispose();
      viewer.current = null;
    };
  }, [width, height]);

  useEffect(() => {
    const v = viewer.current;
    if (!v) return;
    void v.loadSkin(data.skin, { model: data.model === "slim" ? "slim" : "default" });
    if (data.cape && showCape) void v.loadCape(data.cape);
    else v.resetCape();
  }, [data, showCape, ready]);

  useEffect(() => {
    const v = viewer.current;
    if (!v) return;
    void import("skinview3d").then((sv) => {
      v.animation = walk ? new sv.WalkingAnimation() : null;
    });
  }, [walk, ready]);

  return (
    <div className="flex flex-col items-center gap-2">
      <canvas ref={canvas} className="rounded-lg bg-surface-2" aria-label={t("accounts.skin")} />
      <div className="flex gap-4">
        <Switch checked={walk} onCheckedChange={setWalk} label={t("accounts.walk")} />
        {data.cape && <Switch checked={showCape} onCheckedChange={setShowCape} label={t("accounts.cape")} />}
      </div>
    </div>
  );
}
