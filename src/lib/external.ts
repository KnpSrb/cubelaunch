import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

/** Открывает ссылку в системном браузере (в браузерном режиме — в новой вкладке). */
export async function openExternal(url: string) {
  if (isTauri()) await openUrl(url);
  else window.open(url, "_blank", "noopener");
}

/** Страница проекта CurseForge. kind — раздел сайта. */
export function curseforgeUrl(kind: "mods" | "resourcepacks" | "shaderpacks" | "modpacks", slug: string) {
  const section = { mods: "mc-mods", resourcepacks: "texture-packs", shaderpacks: "shaders", modpacks: "modpacks" }[kind];
  return `https://www.curseforge.com/minecraft/${section}/${slug}`;
}

export function modrinthUrl(projectType: string, slug: string) {
  const type = ["mod", "resourcepack", "shader", "modpack", "datapack", "plugin"].includes(projectType) ? projectType : "project";
  return `https://modrinth.com/${type}/${slug}`;
}
