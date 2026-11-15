//! Разрешение списка библиотек под конкретную ОС и архитектуру.
//!
//! Учитывает:
//! - `rules` библиотеки;
//! - старые natives (`natives` + `downloads.classifiers`, подстановка `${arch}`);
//! - новые natives (1.19+): отдельные библиотеки с классификатором `natives-<os>[-<arch>]`.
//!   У Mojang их правила проверяют только ОС, поэтому архитектуру фильтруем по классификатору;
//! - библиотеки загрузчиков без `downloads` (путь из maven-координат + `url` репозитория).

use std::path::PathBuf;

use super::rules::{rules_allow, Arch, Env};
use super::version::{Download, Library};

pub const MOJANG_LIBRARIES: &str = "https://libraries.minecraft.net/";

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLibrary {
    /// Исходные координаты (для отладки и дедупликации).
    pub name: String,
    /// Путь относительно папки `libraries`.
    pub path: PathBuf,
    /// Откуда качать. `None` — файл должен появиться локально (например, от установщика Forge).
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    /// Нативная библиотека: распаковывается в папку natives при запуске.
    pub native: bool,
    /// Входит в classpath. Старые natives (`natives` + classifiers) — нет;
    /// новые (`...:natives-windows`) — да, LWJGL 3 умеет грузить их из classpath.
    pub classpath: bool,
    pub extract_exclude: Vec<String>,
}

/// Maven-координаты `group:artifact:version[:classifier][@ext]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenCoord {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub ext: String,
}

impl MavenCoord {
    pub fn parse(s: &str) -> Option<Self> {
        let (main, ext) = match s.split_once('@') {
            Some((m, e)) => (m, e.to_string()),
            None => (s, "jar".to_string()),
        };
        let parts: Vec<&str> = main.split(':').collect();
        if parts.len() < 3 || parts.len() > 4 || parts.iter().any(|p| p.is_empty()) {
            return None;
        }
        Some(Self {
            group: parts[0].into(),
            artifact: parts[1].into(),
            version: parts[2].into(),
            classifier: parts.get(3).map(|c| c.to_string()),
            ext,
        })
    }

    pub fn with_classifier(&self, classifier: &str) -> Self {
        Self { classifier: Some(classifier.into()), ..self.clone() }
    }

    /// Путь внутри maven-репозитория (всегда с `/`).
    pub fn path(&self) -> String {
        let file = match &self.classifier {
            Some(c) => format!("{}-{}-{}.{}", self.artifact, self.version, c, self.ext),
            None => format!("{}-{}.{}", self.artifact, self.version, self.ext),
        };
        format!("{}/{}/{}/{}", self.group.replace('.', "/"), self.artifact, self.version, file)
    }
}

/// Подходит ли классификатор нового формата (`natives-windows-arm64`) под архитектуру.
fn native_classifier_matches(classifier: &str, env: &Env) -> bool {
    let Some(rest) = classifier.strip_prefix("natives-") else { return true };
    let os = env.os.natives_name();
    let Some(suffix) = rest.strip_prefix(os) else {
        // Классификатор для другой ОС (правила должны были отсечь, но перестрахуемся).
        return !["windows", "linux", "macos", "osx"].iter().any(|o| rest.starts_with(o));
    };
    match suffix {
        // Классификатор без суффикса — x86_64.
        "" => env.arch == Arch::X86_64,
        "-arm64" | "-aarch64" => env.arch == Arch::Arm64,
        "-x86" => env.arch == Arch::X86,
        "-arm32" => env.arch == Arch::Arm32,
        "-x86_64" | "-x64" => env.arch == Arch::X86_64,
        _ => false,
    }
}

fn from_download(name: &str, d: &Download, fallback_path: String, native: bool, exclude: &[String]) -> ResolvedLibrary {
    let path = d.path.clone().unwrap_or(fallback_path);
    ResolvedLibrary {
        name: name.to_string(),
        path: PathBuf::from(path),
        url: (!d.url.is_empty()).then(|| d.url.clone()),
        sha1: d.sha1.clone(),
        size: d.size,
        native,
        classpath: true,
        extract_exclude: exclude.to_vec(),
    }
}

fn from_maven(lib: &Library, coord: &MavenCoord, native: bool) -> ResolvedLibrary {
    let path = coord.path();
    let base = lib.url.as_deref().unwrap_or(MOJANG_LIBRARIES);
    let base = if base.ends_with('/') { base.to_string() } else { format!("{base}/") };
    ResolvedLibrary {
        name: lib.name.clone(),
        url: Some(format!("{base}{path}")),
        path: PathBuf::from(path),
        sha1: lib.sha1.clone(),
        size: lib.size,
        native,
        classpath: true,
        extract_exclude: lib.extract.as_ref().map(|e| e.exclude.clone()).unwrap_or_default(),
    }
}

pub fn resolve_libraries(libs: &[Library], env: &Env) -> Vec<ResolvedLibrary> {
    let mut out = Vec::new();
    for lib in libs {
        if !rules_allow(&lib.rules, env) {
            continue;
        }
        let Some(coord) = MavenCoord::parse(&lib.name) else {
            tracing::warn!("пропущена библиотека с некорректным именем: {}", lib.name);
            continue;
        };
        let exclude = lib.extract.as_ref().map(|e| e.exclude.clone()).unwrap_or_default();

        // Старый формат natives.
        if let Some(natives) = &lib.natives {
            if let Some(classifier) = natives.get(env.os.as_str()) {
                let classifier = classifier.replace("${arch}", env.arch.bits());
                let dl = lib
                    .downloads
                    .as_ref()
                    .and_then(|d| d.classifiers.as_ref())
                    .and_then(|c| c.get(&classifier));
                let fallback = coord.with_classifier(&classifier);
                let mut r = match dl {
                    Some(d) => from_download(&lib.name, d, fallback.path(), true, &exclude),
                    None => from_maven(lib, &fallback, true),
                };
                r.classpath = false;
                out.push(r);
            }
            // Иногда у natives-библиотеки есть и обычный артефакт.
            if let Some(a) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
                out.push(from_download(&lib.name, a, coord.path(), false, &[]));
            }
            continue;
        }

        // Новый формат natives — отдельная библиотека с классификатором.
        let native = coord.classifier.as_deref().is_some_and(|c| c.starts_with("natives-"));
        if native && !native_classifier_matches(coord.classifier.as_deref().unwrap(), env) {
            continue;
        }

        match lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
            Some(a) => out.push(from_download(&lib.name, a, coord.path(), native, &exclude)),
            None if lib.downloads.is_some() => {
                // Есть только classifiers без natives — нечего качать для classpath.
            }
            None => out.push(from_maven(lib, &coord, native)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minecraft::rules::{tests::env, Arch, OsName};
    use crate::minecraft::version::{tests::*, VersionJson};

    fn names(libs: &[ResolvedLibrary]) -> Vec<String> {
        libs.iter().map(|l| l.path.to_string_lossy().rsplit('/').next().unwrap().to_string()).collect()
    }

    #[test]
    fn maven_coords() {
        let c = MavenCoord::parse("net.fabricmc:fabric-loader:0.15.0").unwrap();
        assert_eq!(c.path(), "net/fabricmc/fabric-loader/0.15.0/fabric-loader-0.15.0.jar");
        let c = MavenCoord::parse("org.lwjgl:lwjgl:3.3.1:natives-linux").unwrap();
        assert_eq!(c.path(), "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-linux.jar");
        let c = MavenCoord::parse("de.oceanlabs.mcp:mcp_config:1.20.1@zip").unwrap();
        assert_eq!(c.path(), "de/oceanlabs/mcp/mcp_config/1.20.1/mcp_config-1.20.1.zip");
        assert!(MavenCoord::parse("broken").is_none());
    }

    #[test]
    fn modern_windows_x64_picks_only_matching_natives() {
        let v: VersionJson = serde_json::from_str(MODERN).unwrap();
        let libs = resolve_libraries(&v.libraries, &env(OsName::Windows, Arch::X86_64));
        assert_eq!(names(&libs), vec!["logging-1.1.1.jar", "lwjgl-3.3.1-natives-windows.jar"]);
        assert!(!libs[0].native);
        assert!(libs[1].native && libs[1].classpath);
        assert_eq!(libs[1].sha1.as_deref(), Some("c2"));
    }

    #[test]
    fn modern_windows_arm64() {
        let v: VersionJson = serde_json::from_str(MODERN).unwrap();
        let libs = resolve_libraries(&v.libraries, &env(OsName::Windows, Arch::Arm64));
        assert_eq!(names(&libs), vec!["logging-1.1.1.jar", "lwjgl-3.3.1-natives-windows-arm64.jar"]);
    }

    #[test]
    fn modern_macos_arm64() {
        let v: VersionJson = serde_json::from_str(MODERN).unwrap();
        let libs = resolve_libraries(&v.libraries, &env(OsName::Osx, Arch::Arm64));
        assert_eq!(names(&libs), vec!["logging-1.1.1.jar", "lwjgl-3.3.1-natives-macos-arm64.jar"]);
    }

    #[test]
    fn modern_linux_has_no_natives_in_fixture() {
        let v: VersionJson = serde_json::from_str(MODERN).unwrap();
        let libs = resolve_libraries(&v.libraries, &env(OsName::Linux, Arch::X86_64));
        assert_eq!(names(&libs), vec!["logging-1.1.1.jar"]);
    }

    #[test]
    fn legacy_natives_with_arch_substitution() {
        let v: VersionJson = serde_json::from_str(LEGACY).unwrap();
        let libs = resolve_libraries(&v.libraries, &env(OsName::Windows, Arch::X86_64));
        assert_eq!(
            names(&libs),
            vec![
                "lwjgl-platform-2.9.4-nightly-20150209-natives-windows.jar",
                "twitch-platform-6.5-natives-windows-64.jar",
                "jinput-2.0.5.jar"
            ]
        );
        assert!(libs[0].native && libs[1].native && !libs[2].native);
        assert!(!libs[0].classpath && !libs[1].classpath && libs[2].classpath);
        assert_eq!(libs[0].extract_exclude, vec!["META-INF/"]);

        let libs32 = resolve_libraries(&v.libraries, &env(OsName::Windows, Arch::X86));
        assert_eq!(libs32[1].sha1.as_deref(), Some("t32"));
    }

    #[test]
    fn legacy_rules_exclude_linux() {
        let v: VersionJson = serde_json::from_str(LEGACY).unwrap();
        let libs = resolve_libraries(&v.libraries, &env(OsName::Linux, Arch::X86_64));
        assert_eq!(
            names(&libs),
            vec!["lwjgl-platform-2.9.4-nightly-20150209-natives-linux.jar", "jinput-2.0.5.jar"]
        );
    }

    #[test]
    fn maven_library_without_downloads() {
        let lib: Library = serde_json::from_str(
            r#"{ "name": "net.fabricmc:intermediary:1.20.1", "url": "https://maven.fabricmc.net" }"#,
        )
        .unwrap();
        let libs = resolve_libraries(&[lib], &env(OsName::Linux, Arch::X86_64));
        assert_eq!(
            libs[0].url.as_deref(),
            Some("https://maven.fabricmc.net/net/fabricmc/intermediary/1.20.1/intermediary-1.20.1.jar")
        );
    }
}
