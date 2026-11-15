//! Java: поиск установленных JVM, проверка, автоскачивание Temurin и выбор
//! подходящей Java под версию Minecraft.

pub mod detect;
pub mod download;

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::minecraft::rules::Arch;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstall {
    /// Путь к исполняемому файлу `java` / `java.exe`.
    pub path: String,
    /// Полная версия, например `17.0.9` или `1.8.0_392`.
    pub version: String,
    pub major: u32,
    /// Архитектура JVM: `x86_64`, `arm64`, `x86`, `arm32`.
    pub arch: Arch,
    pub vendor: String,
    /// Скачана лаунчером (лежит в `<data>/java`).
    pub managed: bool,
}

/// `1.8.0_392` → 8, `17.0.9` → 17, `21` → 21, `22-ea` → 22.
pub fn parse_major(version: &str) -> Option<u32> {
    let mut parts = version.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

pub fn parse_arch(os_arch: &str) -> Option<Arch> {
    match os_arch.trim() {
        "amd64" | "x86_64" | "x64" => Some(Arch::X86_64),
        "aarch64" | "arm64" => Some(Arch::Arm64),
        "x86" | "i386" | "i486" | "i586" | "i686" => Some(Arch::X86),
        "arm" | "arm32" | "aarch32" => Some(Arch::Arm32),
        _ => None,
    }
}

/// Разбор вывода `java -XshowSettings:properties -version` (строки `key = value`).
pub fn parse_properties(output: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut last_key: Option<String> = None;
    for line in output.lines() {
        if let Some((k, v)) = line.split_once(" = ") {
            let k = k.trim().to_string();
            map.insert(k.clone(), v.trim().to_string());
            last_key = Some(k);
        } else if line.starts_with("        ") && last_key.is_some() {
            // Продолжение многострочного значения — нам не нужно.
        } else {
            last_key = None;
        }
    }
    map
}

pub fn java_exe_name() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

/// Скрывает консольное окно дочернего процесса на Windows.
pub fn no_console(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Запускает java и читает её свойства. `managed_root` — папка Java лаунчера.
pub async fn probe(path: &Path, managed_root: Option<&Path>) -> AppResult<JavaInstall> {
    if !path.is_file() {
        return Err(AppError::JavaInvalid(format!("{}: файл не найден", path.display())));
    }
    let mut cmd = tokio::process::Command::new(path);
    cmd.arg("-XshowSettings:properties").arg("-version").kill_on_drop(true);
    no_console(&mut cmd);
    let out = tokio::time::timeout(Duration::from_secs(15), cmd.output())
        .await
        .map_err(|_| AppError::JavaInvalid(format!("{}: не ответила за 15 секунд", path.display())))?
        .map_err(|e| AppError::JavaInvalid(format!("{}: {e}", path.display())))?;
    let text = String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    let props = parse_properties(&text);
    let version = props
        .get("java.version")
        .cloned()
        .ok_or_else(|| AppError::JavaInvalid(format!("{}: не удалось определить версию", path.display())))?;
    let major = parse_major(&version)
        .ok_or_else(|| AppError::JavaInvalid(format!("{}: непонятная версия {version}", path.display())))?;
    let arch = props.get("os.arch").and_then(|a| parse_arch(a)).unwrap_or(Arch::current());
    let vendor = props
        .get("java.vendor")
        .or_else(|| props.get("java.vm.vendor"))
        .cloned()
        .unwrap_or_default();
    let managed = managed_root.is_some_and(|r| path.starts_with(r));
    Ok(JavaInstall { path: path.display().to_string(), version, major, arch, vendor, managed })
}

/// Выбор из найденных: точное совпадение мажорной версии; среди них — родная
/// архитектура, затем скачанная лаунчером, затем более новая сборка.
pub fn pick_best(installs: &[JavaInstall], major: u32, native: Arch) -> Option<&JavaInstall> {
    installs
        .iter()
        .filter(|j| j.major == major)
        .max_by_key(|j| (j.arch == native, j.managed, version_key(&j.version)))
}

/// Числовой ключ версии для сравнения (`17.0.10` > `17.0.9`).
fn version_key(v: &str) -> Vec<u32> {
    v.split(|c: char| !c.is_ascii_digit()).filter_map(|p| p.parse().ok()).collect()
}

/// Рекомендованный максимум памяти для ванильной игры, МБ.
pub fn recommended_max_memory_mb(total_mb: u64) -> u64 {
    let quarter = total_mb / 4;
    (quarter.clamp(2048, 8192) / 512 * 512).min(total_mb.saturating_sub(1024).max(1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn majors() {
        assert_eq!(parse_major("1.8.0_392"), Some(8));
        assert_eq!(parse_major("17.0.9"), Some(17));
        assert_eq!(parse_major("21"), Some(21));
        assert_eq!(parse_major("22-ea"), Some(22));
        assert_eq!(parse_major("25+36"), Some(25));
        assert_eq!(parse_major("abc"), None);
    }

    #[test]
    fn properties() {
        let out = "Property settings:\n    file.encoding = UTF-8\n    java.library.path = C:\\a\n        C:\\b\n    java.version = 17.0.9\n    os.arch = amd64\n    java.vendor = Eclipse Adoptium\n\nopenjdk version \"17.0.9\" 2023-10-17\n";
        let p = parse_properties(out);
        assert_eq!(p["java.version"], "17.0.9");
        assert_eq!(parse_arch(&p["os.arch"]), Some(Arch::X86_64));
        assert_eq!(p["java.vendor"], "Eclipse Adoptium");
    }

    fn j(major: u32, arch: Arch, managed: bool, version: &str) -> JavaInstall {
        JavaInstall { path: format!("/j/{version}"), version: version.into(), major, arch, vendor: String::new(), managed }
    }

    #[test]
    fn picks_matching_major_and_native_arch() {
        let list = vec![
            j(17, Arch::X86_64, false, "17.0.2"),
            j(17, Arch::Arm64, false, "17.0.1"),
            j(21, Arch::Arm64, true, "21.0.1"),
            j(17, Arch::Arm64, true, "17.0.0"),
        ];
        // Родная архитектура → скачанная лаунчером → более новая.
        assert_eq!(pick_best(&list, 17, Arch::Arm64).unwrap().version, "17.0.0");
        let unmanaged: Vec<_> = list.iter().filter(|j| !j.managed).cloned().collect();
        assert_eq!(pick_best(&unmanaged, 17, Arch::Arm64).unwrap().version, "17.0.1");
        assert_eq!(pick_best(&list, 17, Arch::X86_64).unwrap().version, "17.0.2");
        assert!(pick_best(&list, 8, Arch::X86_64).is_none());
    }

    #[test]
    fn memory_recommendation() {
        assert_eq!(recommended_max_memory_mb(4096), 2048);
        assert_eq!(recommended_max_memory_mb(16384), 4096);
        assert_eq!(recommended_max_memory_mb(65536), 8192);
        assert_eq!(recommended_max_memory_mb(2048), 1024);
    }
}
