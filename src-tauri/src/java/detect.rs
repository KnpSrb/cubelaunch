//! Поиск установленных JVM: JAVA_HOME, PATH, типовые папки вендоров,
//! рантаймы официального лаунчера и Java, скачанные Cubelaunch.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use futures::StreamExt;

use super::{java_exe_name, probe, JavaInstall};

/// Ищет `bin/java` в `dir` и его подпапках до глубины `depth`.
/// Найдя JVM, внутрь неё не спускается (чтобы не дублировать `jre/` у JDK 8).
pub fn find_java_in(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let exe = dir.join("bin").join(java_exe_name());
    if exe.is_file() {
        out.push(exe);
        return;
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            find_java_in(&p, depth - 1, out);
        }
    }
}

fn candidate_roots() -> Vec<(PathBuf, usize)> {
    let mut roots: Vec<(PathBuf, usize)> = Vec::new();
    let home = dirs::home_dir();

    #[cfg(windows)]
    {
        let vendors = [
            "Java", "Eclipse Adoptium", "Eclipse Foundation", "AdoptOpenJDK", "Microsoft", "Zulu",
            "BellSoft", "Amazon Corretto", "Semeru", "Oracle", "OpenJDK", "Temurin",
        ];
        for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            if let Ok(pf) = std::env::var(var) {
                for v in vendors {
                    roots.push((PathBuf::from(&pf).join(v), 2));
                }
            }
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            roots.push((PathBuf::from(appdata).join(".minecraft").join("runtime"), 4));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            // Лаунчер Minecraft из Microsoft Store.
            roots.push((
                PathBuf::from(&local)
                    .join("Packages")
                    .join("Microsoft.4297127D64EC6_8wekyb3d8bbwe")
                    .join("LocalCache")
                    .join("Local")
                    .join("runtime"),
                4,
            ));
            roots.push((PathBuf::from(local).join("Programs"), 2));
        }
    }

    #[cfg(target_os = "macos")]
    {
        roots.push((PathBuf::from("/Library/Java/JavaVirtualMachines"), 3));
        roots.push((PathBuf::from("/opt/homebrew/opt"), 4));
        roots.push((PathBuf::from("/usr/local/opt"), 4));
        if let Some(h) = &home {
            roots.push((h.join("Library/Java/JavaVirtualMachines"), 3));
            roots.push((h.join("Library/Application Support/minecraft/runtime"), 5));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for d in ["/usr/lib/jvm", "/usr/lib64/jvm", "/usr/java", "/opt/java", "/opt/jdk", "/opt"] {
            roots.push((PathBuf::from(d), 2));
        }
        if let Some(h) = &home {
            roots.push((h.join(".minecraft/runtime"), 4));
        }
    }

    if let Some(h) = &home {
        roots.push((h.join(".jdks"), 1));
        roots.push((h.join(".sdkman/candidates/java"), 1));
    }
    roots
}

/// Все кандидаты в исполняемые файлы java (без проверки).
pub fn candidates(managed_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    find_java_in(managed_root, 3, &mut out);
    if let Ok(home) = std::env::var("JAVA_HOME") {
        find_java_in(Path::new(&home), 0, &mut out);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let exe = dir.join(java_exe_name());
            if exe.is_file() {
                out.push(exe);
            }
        }
    }
    for (root, depth) in candidate_roots() {
        find_java_in(&root, depth, &mut out);
    }

    // Дедупликация по каноническому пути (симлинки, /usr/bin/java → /usr/lib/jvm/...).
    let mut seen = HashSet::new();
    out.into_iter()
        .filter(|p| {
            let key = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            seen.insert(key)
        })
        .collect()
}

/// Находит и проверяет все JVM (параллельно).
pub async fn detect_all(managed_root: &Path) -> Vec<JavaInstall> {
    let root = managed_root.to_path_buf();
    let cands = tokio::task::spawn_blocking({
        let root = root.clone();
        move || candidates(&root)
    })
    .await
    .unwrap_or_default();

    let mut found: Vec<JavaInstall> = futures::stream::iter(cands)
        .map(|p| {
            let root = root.clone();
            async move { probe(&p, Some(&root)).await }
        })
        .buffer_unordered(8)
        .filter_map(|r| async move {
            match r {
                Ok(j) => Some(j),
                Err(e) => {
                    tracing::debug!("кандидат Java отклонён: {e}");
                    None
                }
            }
        })
        .collect()
        .await;
    found.sort_by(|a, b| b.major.cmp(&a.major).then(a.path.cmp(&b.path)));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nested_jvm_without_descending_into_it() {
        let dir = tempfile::tempdir().unwrap();
        let jdk = dir.path().join("vendor").join("jdk8");
        std::fs::create_dir_all(jdk.join("bin")).unwrap();
        std::fs::create_dir_all(jdk.join("jre").join("bin")).unwrap();
        std::fs::write(jdk.join("bin").join(java_exe_name()), b"").unwrap();
        std::fs::write(jdk.join("jre").join("bin").join(java_exe_name()), b"").unwrap();
        let mut out = Vec::new();
        find_java_in(dir.path(), 3, &mut out);
        assert_eq!(out, vec![jdk.join("bin").join(java_exe_name())]);
    }
}
