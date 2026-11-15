//! Перенос инстанса целиком: экспорт в zip Cubelaunch, импорт своего zip и zip
//! инстанса Prism Launcher / MultiMC, копирование файлов при клонировании.
//!
//! Zip Cubelaunch:
//! ```text
//! instance.json   — описание инстанса (без времени игры и пути к Java)
//! modpack.json    — учёт файлов модпака (если инстанс из модпака)
//! .minecraft/...  — игровая папка (выбранные папки и файлы)
//! ```
//! Файлы игры (клиент, библиотеки, assets) в архив не входят — после импорта
//! они скачиваются с серверов Mojang заново.

use std::io::{Read, Write};
use std::path::Path;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::{Instance, InstanceOverrides, LoaderKind, GAME_DIR, INSTANCE_FILE};
use crate::error::{AppError, AppResult};
use crate::modpacks::export::{walk, ExportCandidate, ExportFile};
use crate::modpacks::{join_rel, safe_rel, ManagedState, STATE_FILE};

/// Никогда не переносятся: временное и логи.
const NEVER: [&str; 6] = ["logs", "crash-reports", ".fabric", ".mixin.out", "natives", ".modpack-staging"];
/// По умолчанию выключены при экспорте.
const OFF_BY_DEFAULT: [&str; 1] = ["screenshots"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveKind {
    Cubelaunch,
    Prism,
}

#[derive(Debug, Clone)]
pub struct InstanceArchive {
    pub kind: ArchiveKind,
    pub name: String,
    pub minecraft: String,
    /// Загрузчик и версия (у Prism — как в mmc-pack.json, у Forge короткая).
    pub loader: Option<(LoaderKind, String)>,
    /// Описание инстанса Cubelaunch целиком.
    pub instance: Option<Instance>,
    pub managed: Option<ManagedState>,
    /// Настройки из instance.cfg Prism (память, аргументы JVM).
    pub overrides: InstanceOverrides,
    /// Префикс игровой папки внутри архива (`.minecraft/`, `x/minecraft/`…).
    pub game_prefix: String,
    pub files: usize,
    pub has_saves: bool,
    /// Что из архива не переносится (jar-моды Prism и т. п.) — для предупреждения.
    pub warnings: Vec<String>,
}

fn bad(msg: impl Into<String>) -> AppError {
    AppError::Content(msg.into())
}

fn zerr(e: zip::result::ZipError) -> AppError {
    bad(format!("архив повреждён: {e}"))
}

/// Имя записи с `/`: архивы, собранные старыми инструментами Windows
/// (например, Compress-Archive в PowerShell 5), пишут пути через `\`.
fn norm(name: &str) -> String {
    name.replace('\\', "/")
}

fn read_text(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> AppResult<Option<String>> {
    let found = zip.index_for_name(name).or_else(|| zip.index_for_name(&name.replace('/', "\\")));
    let Some(i) = found else { return Ok(None) };
    let mut e = zip.by_index(i).map_err(zerr)?;
    let mut s = String::new();
    e.read_to_string(&mut s).map_err(|_| bad(format!("{name}: не текст UTF-8")))?;
    Ok(Some(s.trim_start_matches('\u{feff}').to_string()))
}

/// Корень инстанса в архиве: папка, где лежит `marker` (в корне или на один
/// уровень глубже — Prism кладёт инстанс в папку с его именем).
fn find_root(names: &[String], marker: &str) -> Option<String> {
    names
        .iter()
        .filter_map(|n| {
            let root = n.strip_suffix(marker)?;
            (root.is_empty() || (root.ends_with('/') && root.matches('/').count() == 1)).then(|| root.to_string())
        })
        .min_by_key(|r| r.len())
}

/// Значения из instance.cfg (формат QSettings INI).
fn parse_cfg(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            let v = v.trim();
            let v = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(v);
            Some((k.trim().to_string(), v.replace("\\\"", "\"").replace("\\\\", "\\")))
        })
        .collect()
}

/// Читает архив инстанса. `Ok(None)` — это не архив инстанса (может быть модпаком).
pub fn read(path: &Path) -> AppResult<Option<InstanceArchive>> {
    let f = std::fs::File::open(path)?;
    let Ok(mut zip) = zip::ZipArchive::new(f) else { return Ok(None) };
    let names: Vec<String> = zip.file_names().map(norm).collect();

    let count = |prefix: &str| names.iter().filter(|n| n.starts_with(prefix) && !n.ends_with('/')).count();
    let saves = |prefix: &str| names.iter().any(|n| n.starts_with(&format!("{prefix}saves/")));

    if let Some(root) = find_root(&names, INSTANCE_FILE) {
        let text = read_text(&mut zip, &format!("{root}{INSTANCE_FILE}"))?.unwrap_or_default();
        // Посторонний instance.json (не наш формат) — не считаем архивом инстанса.
        let Ok(inst) = serde_json::from_str::<Instance>(&text) else { return Ok(None) };
        let managed = match read_text(&mut zip, &format!("{root}{STATE_FILE}"))? {
            Some(t) => serde_json::from_str(&t).map_err(|e| bad(format!("modpack.json: {e}")))?,
            None => None,
        };
        let game_prefix = format!("{root}{GAME_DIR}/");
        return Ok(Some(InstanceArchive {
            kind: ArchiveKind::Cubelaunch,
            name: inst.name.clone(),
            minecraft: inst.minecraft.clone(),
            loader: inst.loader.as_ref().map(|l| (l.kind, l.version.clone())),
            overrides: inst.overrides.clone(),
            instance: Some(inst),
            managed,
            files: count(&game_prefix),
            has_saves: saves(&game_prefix),
            game_prefix,
            warnings: vec![],
        }));
    }

    let Some(root) = find_root(&names, "instance.cfg") else { return Ok(None) };
    let cfg = parse_cfg(&read_text(&mut zip, &format!("{root}instance.cfg"))?.unwrap_or_default());
    let pack = read_text(&mut zip, &format!("{root}mmc-pack.json"))?
        .ok_or_else(|| bad("в архиве Prism нет mmc-pack.json (очень старый формат MultiMC не поддерживается)"))?;
    let pack: serde_json::Value = serde_json::from_str(&pack).map_err(|e| bad(format!("mmc-pack.json: {e}")))?;
    let mut minecraft = None;
    let mut loader = None;
    let mut warnings = Vec::new();
    for c in pack["components"].as_array().into_iter().flatten() {
        let uid = c["uid"].as_str().unwrap_or_default();
        let version = c["version"].as_str().unwrap_or_default().to_string();
        let kind = match uid {
            "net.minecraft" => {
                minecraft = Some(version);
                continue;
            }
            "net.fabricmc.fabric-loader" => LoaderKind::Fabric,
            "org.quiltmc.quilt-loader" => LoaderKind::Quilt,
            "net.minecraftforge" => LoaderKind::Forge,
            "net.neoforged" => LoaderKind::NeoForge,
            "org.lwjgl" | "org.lwjgl3" | "net.fabricmc.intermediary" | "org.quiltmc.hashed" => continue,
            other => {
                warnings.push(format!("компонент {other}"));
                continue;
            }
        };
        if !version.is_empty() {
            loader = Some((kind, version));
        }
    }
    let minecraft = minecraft.filter(|v| !v.is_empty()).ok_or_else(|| bad("в mmc-pack.json не указана версия Minecraft"))?;
    if names.iter().any(|n| n.starts_with(&format!("{root}jarmods/")) && !n.ends_with('/')) {
        warnings.push("jar-моды".into());
    }
    let game_prefix = [".minecraft/", "minecraft/"]
        .iter()
        .map(|g| format!("{root}{g}"))
        .find(|p| names.iter().any(|n| n.starts_with(p.as_str())))
        .unwrap_or_else(|| format!("{root}.minecraft/"));

    let mut overrides = InstanceOverrides::default();
    let yes = |k: &str| cfg.get(k).is_some_and(|v| v == "true");
    let num = |k: &str| cfg.get(k).and_then(|v| v.parse::<u32>().ok()).filter(|v| *v >= 128);
    if yes("OverrideMemory") {
        overrides.min_memory_mb = num("MinMemAlloc");
        overrides.max_memory_mb = num("MaxMemAlloc");
        if let (Some(a), Some(b)) = (overrides.min_memory_mb, overrides.max_memory_mb) {
            if b < a {
                overrides.min_memory_mb = None;
            }
        }
    }
    if yes("OverrideJavaArgs") {
        overrides.jvm_args = cfg.get("JvmArgs").filter(|v| !v.trim().is_empty()).cloned();
    }
    let name = cfg
        .get("name")
        .filter(|n| !n.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| root.trim_end_matches('/').to_string());
    let name = if name.is_empty() { "Инстанс Prism".to_string() } else { name };
    Ok(Some(InstanceArchive {
        kind: ArchiveKind::Prism,
        name,
        minecraft,
        loader,
        instance: None,
        managed: None,
        overrides,
        files: count(&game_prefix),
        has_saves: saves(&game_prefix),
        game_prefix,
        warnings,
    }))
}

/// Распаковывает игровую папку из архива в `dest`.
pub fn extract_game(
    path: &Path,
    prefix: &str,
    dest: &Path,
    cancel: &CancellationToken,
    on_progress: impl Fn(usize, usize),
) -> AppResult<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?).map_err(zerr)?;
    let total = zip.file_names().map(norm).filter(|n| n.starts_with(prefix) && !n.ends_with('/')).count();
    let mut done = 0;
    for i in 0..zip.len() {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let mut e = zip.by_index(i).map_err(zerr)?;
        let Some(rest) = norm(e.name()).strip_prefix(prefix).map(str::to_string) else { continue };
        if e.is_dir() || rest.is_empty() || rest.ends_with('/') {
            continue;
        }
        let rel = safe_rel(&rest)?;
        if NEVER.contains(&rel.split('/').next().unwrap_or_default()) {
            continue;
        }
        let target = join_rel(dest, &rel);
        if let Some(p) = target.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::io::copy(&mut e, &mut std::fs::File::create(&target)?)?;
        done += 1;
        if done % 16 == 0 || done == total {
            on_progress(done, total);
        }
    }
    Ok(())
}

/// Верхний уровень игровой папки — что можно включить в экспорт инстанса.
pub fn candidates(game: &Path) -> Vec<ExportCandidate> {
    let mut out: Vec<ExportCandidate> = std::fs::read_dir(game)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if NEVER.contains(&name.as_str()) {
                return None;
            }
            Some(ExportCandidate { default: !OFF_BY_DEFAULT.contains(&name.as_str()), is_dir: e.path().is_dir(), name })
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

/// Файлы выбранных записей верхнего уровня (выключенные моды тоже переносятся).
pub fn collect(game: &Path, include: &[String]) -> AppResult<Vec<ExportFile>> {
    let mut out = Vec::new();
    for name in include {
        if name.contains(['/', '\\']) || name == ".." || name == "." || name.is_empty() || NEVER.contains(&name.as_str()) {
            return Err(AppError::InvalidInput(format!("некорректная папка для экспорта: {name}")));
        }
        walk(&game.join(name), name, &mut out, true)?;
    }
    Ok(out)
}

/// Описание инстанса для переноса: без личной статистики и пути к Java
/// (на другой машине он другой).
pub fn portable(inst: &Instance) -> Instance {
    let mut i = inst.clone();
    i.installed = false;
    i.last_played = None;
    i.playtime_secs = 0;
    i.overrides.java_path = None;
    i
}

/// Пишет zip инстанса (сначала во временный файл).
pub fn write(
    out: &Path,
    inst: &Instance,
    managed: Option<&ManagedState>,
    files: &[ExportFile],
    cancel: &CancellationToken,
    on_progress: impl Fn(usize, usize),
) -> AppResult<()> {
    let tmp = out.with_extension("part");
    let result = (|| -> AppResult<()> {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let werr = |e: zip::result::ZipError| AppError::Other(format!("ошибка записи архива: {e}"));
        z.start_file(INSTANCE_FILE, opts).map_err(werr)?;
        z.write_all(&serde_json::to_vec_pretty(&portable(inst))?)?;
        if let Some(m) = managed {
            z.start_file(STATE_FILE, opts).map_err(werr)?;
            z.write_all(&serde_json::to_vec_pretty(m)?)?;
        }
        for (i, f) in files.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            if i % 16 == 0 {
                on_progress(i, files.len());
            }
            z.start_file(format!("{GAME_DIR}/{}", f.rel), opts).map_err(werr)?;
            std::io::copy(&mut std::fs::File::open(&f.abs)?, &mut z)?;
        }
        z.finish().map_err(werr)?;
        on_progress(files.len(), files.len());
        Ok(())
    })();
    match result {
        Ok(()) => {
            std::fs::rename(&tmp, out)?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Копирует игровую папку при клонировании. `skip` — записи верхнего уровня,
/// которые не копируются (например, `saves`).
pub fn copy_game(src: &Path, dst: &Path, skip: &[&str], cancel: &CancellationToken, on_progress: impl Fn(usize, usize)) -> AppResult<()> {
    let mut files = Vec::new();
    for e in std::fs::read_dir(src).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if NEVER.contains(&name.as_str()) || skip.contains(&name.as_str()) {
            continue;
        }
        walk(&e.path(), &name, &mut files, true)?;
    }
    for (i, f) in files.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let target = join_rel(dst, &f.rel);
        if let Some(p) = target.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(&f.abs, &target)?;
        if i % 16 == 0 {
            on_progress(i, files.len());
        }
    }
    on_progress(files.len(), files.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modpacks::format::tests::make_zip;

    fn sample() -> Instance {
        serde_json::from_str(
            r#"{ "name": "Мой", "minecraft": "1.20.1", "createdAt": "2026-01-01T00:00:00Z", "playtimeSecs": 99,
                 "installed": true, "loader": { "kind": "fabric", "version": "0.15.0" },
                 "overrides": { "javaPath": "C:/java/bin/java.exe", "maxMemoryMb": 6000 } }"#,
        )
        .unwrap()
    }

    #[test]
    fn export_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("game");
        for (p, d) in [("mods/a.jar", "a"), ("mods/b.jar.disabled", "b"), ("saves/w/level.dat", "w"), ("logs/latest.log", "l"), ("options.txt", "o")] {
            let f = join_rel(&game, p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, d).unwrap();
        }
        let cands = candidates(&game);
        assert!(!cands.iter().any(|c| c.name == "logs"));
        let include: Vec<String> = cands.iter().filter(|c| c.default).map(|c| c.name.clone()).collect();
        let files = collect(&game, &include).unwrap();
        assert!(files.iter().any(|f| f.rel == "mods/b.jar.disabled"), "выключенные моды переносятся");

        let out = dir.path().join("x.zip");
        let mut managed = ManagedState::default();
        managed.files.insert("mods/a.jar".into(), "00".into());
        write(&out, &sample(), Some(&managed), &files, &CancellationToken::new(), |_, _| {}).unwrap();

        let a = read(&out).unwrap().expect("архив инстанса");
        assert_eq!(a.kind, ArchiveKind::Cubelaunch);
        assert_eq!(a.files, 4);
        assert!(a.has_saves);
        let inst = a.instance.unwrap();
        assert_eq!(inst.playtime_secs, 0);
        assert!(!inst.installed);
        assert!(inst.overrides.java_path.is_none());
        assert_eq!(inst.overrides.max_memory_mb, Some(6000));
        assert_eq!(a.managed.unwrap().files.len(), 1);

        let dest = dir.path().join("dest");
        extract_game(&out, &a.game_prefix, &dest, &CancellationToken::new(), |_, _| {}).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("saves/w/level.dat")).unwrap(), "w");
        assert!(!dest.join("logs").exists());
    }

    #[test]
    fn reads_prism_zip() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("p.zip");
        let cfg = b"[General]\nname=\"Prism \\\"pack\\\"\"\nOverrideMemory=true\nMinMemAlloc=1024\nMaxMemAlloc=8192\nOverrideJavaArgs=false\nJvmArgs=-Xss4M\n";
        let pack = br#"{"components":[{"uid":"org.lwjgl3","version":"3.3.1"},{"uid":"net.minecraft","version":"1.20.1"},{"uid":"net.minecraftforge","version":"47.2.0"},{"uid":"com.example.thing","version":"1"}],"formatVersion":1}"#;
        make_zip(
            &out,
            &[
                ("My Pack/instance.cfg", cfg),
                ("My Pack/mmc-pack.json", pack),
                ("My Pack/minecraft/mods/x.jar", b"x"),
                ("My Pack/minecraft/../../evil", b"e"),
                ("My Pack/jarmods/j.jar", b"j"),
            ],
        );
        let a = read(&out).unwrap().unwrap();
        assert_eq!(a.kind, ArchiveKind::Prism);
        assert_eq!(a.name, "Prism \"pack\"");
        assert_eq!(a.minecraft, "1.20.1");
        assert_eq!(a.loader, Some((LoaderKind::Forge, "47.2.0".into())));
        assert_eq!(a.game_prefix, "My Pack/minecraft/");
        assert_eq!((a.overrides.min_memory_mb, a.overrides.max_memory_mb), (Some(1024), Some(8192)));
        assert!(a.overrides.jvm_args.is_none());
        assert_eq!(a.warnings.len(), 2);
        // Путь с `..` отвергается при распаковке.
        assert!(extract_game(&out, &a.game_prefix, &dir.path().join("d"), &CancellationToken::new(), |_, _| {}).is_err());
    }

    #[test]
    fn backslash_names() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("w.zip");
        let pack = br#"{"components":[{"uid":"net.minecraft","version":"1.20.1"},{"uid":"net.fabricmc.fabric-loader","version":"0.16.14"}]}"#;
        let cfg = "\u{feff}name=Тест\n".as_bytes();
        make_zip(&out, &[("P\\instance.cfg", cfg), ("P\\mmc-pack.json", pack), ("P\\.minecraft\\saves\\", b""), ("P\\.minecraft\\config\\a.txt", b"a")]);
        let a = read(&out).unwrap().unwrap();
        assert_eq!((a.name.as_str(), a.game_prefix.as_str()), ("Тест", "P/.minecraft/"));
        assert_eq!(a.loader, Some((LoaderKind::Fabric, "0.16.14".into())));
        let dest = dir.path().join("d");
        extract_game(&out, &a.game_prefix, &dest, &CancellationToken::new(), |_, _| {}).unwrap();
        assert!(dest.join("config/a.txt").exists());
    }

    #[test]
    fn not_an_instance() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("m.mrpack");
        make_zip(&out, &[("modrinth.index.json", b"{}"), ("overrides/instance.json", b"{}")]);
        assert!(read(&out).unwrap().is_none());
        let txt = dir.path().join("t.txt");
        std::fs::write(&txt, "x").unwrap();
        assert!(read(&txt).unwrap().is_none());
    }

    #[test]
    fn copy_skips() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("s");
        for p in ["mods/a.jar", "saves/w/level.dat", "natives/x.dll", "config/c.toml"] {
            let f = join_rel(&src, p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, p).unwrap();
        }
        let dst = dir.path().join("d");
        copy_game(&src, &dst, &["saves"], &CancellationToken::new(), |_, _| {}).unwrap();
        assert!(dst.join("mods/a.jar").exists() && dst.join("config/c.toml").exists());
        assert!(!dst.join("saves").exists() && !dst.join("natives").exists());
    }
}
