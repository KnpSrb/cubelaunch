//! Сборка командной строки запуска: аргументы JVM и игры, classpath,
//! подстановка плейсхолдеров `${...}`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::libraries::ResolvedLibrary;
use super::rules::{rules_allow, Env, OsName};
use super::version::{Argument, VersionJson};
use crate::error::{AppError, AppResult};

pub const LAUNCHER_NAME: &str = "cubelaunch";

/// Всё, что нужно для подстановки плейсхолдеров.
#[derive(Debug, Clone)]
pub struct LaunchVars {
    pub username: String,
    /// UUID без дефисов.
    pub uuid: String,
    pub access_token: String,
    /// `msa` для Microsoft, `legacy` для офлайна.
    pub user_type: String,
    pub xuid: String,
    pub client_id: String,
    pub version_name: String,
    pub version_type: String,
    pub game_dir: PathBuf,
    pub assets_root: PathBuf,
    /// Папка для старых версий (`virtual` / `map_to_resources`), иначе = assets_root.
    pub game_assets: PathBuf,
    pub assets_index: String,
    pub natives_dir: PathBuf,
    pub libraries_dir: PathBuf,
    pub classpath: String,
    pub width: u32,
    pub height: u32,
}

impl LaunchVars {
    pub fn map(&self, env: &Env) -> HashMap<&'static str, String> {
        let p = |p: &Path| p.display().to_string();
        HashMap::from([
            ("auth_player_name", self.username.clone()),
            ("auth_uuid", self.uuid.clone()),
            ("auth_access_token", self.access_token.clone()),
            ("auth_session", self.access_token.clone()),
            ("auth_xuid", self.xuid.clone()),
            ("clientid", self.client_id.clone()),
            ("user_type", self.user_type.clone()),
            ("user_properties", "{}".to_string()),
            ("version_name", self.version_name.clone()),
            ("version_type", self.version_type.clone()),
            ("game_directory", p(&self.game_dir)),
            ("assets_root", p(&self.assets_root)),
            ("game_assets", p(&self.game_assets)),
            ("assets_index_name", self.assets_index.clone()),
            ("natives_directory", p(&self.natives_dir)),
            ("library_directory", p(&self.libraries_dir)),
            ("classpath", self.classpath.clone()),
            ("classpath_separator", classpath_separator(env).to_string()),
            ("launcher_name", LAUNCHER_NAME.to_string()),
            ("launcher_version", env!("CARGO_PKG_VERSION").to_string()),
            ("resolution_width", self.width.to_string()),
            ("resolution_height", self.height.to_string()),
        ])
    }
}

pub fn classpath_separator(env: &Env) -> &'static str {
    if env.os == OsName::Windows {
        ";"
    } else {
        ":"
    }
}

/// Заменяет `${name}` известными значениями; неизвестные плейсхолдеры оставляет.
pub fn substitute(s: &str, vars: &HashMap<&'static str, String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match vars.get(key) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[start..start + 2 + end + 1]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Разворачивает список аргументов с учётом правил.
pub fn collect(args: &[Argument], env: &Env) -> Vec<String> {
    let mut out = Vec::new();
    for a in args {
        match a {
            Argument::Plain(s) => out.push(s.clone()),
            Argument::Conditional { rules, value } => {
                if rules_allow(rules, env) {
                    out.extend(value.as_slice().iter().cloned());
                }
            }
        }
    }
    out
}

/// Разбивает строку пользовательских аргументов, учитывая кавычки.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                has = true;
            }
            (None, c) if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Classpath: библиотеки (без старых natives, без дублей) + клиентский jar последним.
pub fn build_classpath(libs: &[ResolvedLibrary], libraries_dir: &Path, client_jar: &Path, env: &Env) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut parts: Vec<String> = libs
        .iter()
        .filter(|l| l.classpath)
        // Сборка через components() приводит разделители к родным для ОС.
        .map(|l| libraries_dir.join(l.path.components().collect::<PathBuf>()).display().to_string())
        .filter(|p| seen.insert(p.clone()))
        .collect();
    parts.push(client_jar.display().to_string());
    parts.join(classpath_separator(env))
}

pub struct LaunchOptions {
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub extra_jvm_args: Vec<String>,
    /// Аргумент логирования из version JSON с уже подставленным путём.
    pub logging_arg: Option<String>,
    pub fullscreen: bool,
}

/// Полный список аргументов после исполняемого файла java.
pub fn build_command(version: &VersionJson, env: &Env, vars: &LaunchVars, opts: &LaunchOptions) -> AppResult<Vec<String>> {
    let main_class = version
        .main_class
        .clone()
        .ok_or_else(|| AppError::Launch(format!("в описании версии {} нет mainClass", version.id)))?;
    let map = vars.map(env);
    // Разрешение окна задаём всегда, поэтому фича включена.
    let env = env.clone().with_feature("has_custom_resolution", true);

    let mut args = vec![format!("-Xms{}M", opts.min_memory_mb), format!("-Xmx{}M", opts.max_memory_mb)];
    args.extend(opts.extra_jvm_args.iter().cloned());

    let described: Vec<String> = version.arguments.as_ref().map(|a| collect(&a.jvm, &env)).unwrap_or_default();
    let jvm: Vec<String> = if described.iter().any(|a| a.contains("${classpath}")) {
        described
    } else {
        // Старые версии: аргументы JVM не описаны, задаём стандартный набор
        // (и добавляем то, что дописал профиль загрузчика).
        {
            let mut v = Vec::new();
            if env.os == OsName::Osx {
                v.push("-XstartOnFirstThread".into());
            }
            if env.os == OsName::Windows {
                v.push("-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump".into());
            }
            v.extend([
                "-Djava.library.path=${natives_directory}".to_string(),
                "-Dminecraft.launcher.brand=${launcher_name}".to_string(),
                "-Dminecraft.launcher.version=${launcher_version}".to_string(),
                "-cp".to_string(),
                "${classpath}".to_string(),
            ]);
            // Дополнительные аргументы профиля — до -cp, чтобы не разорвать пару.
            let at = v.len() - 2;
            v.splice(at..at, described);
            v
        }
    };
    args.extend(jvm.iter().map(|a| substitute(a, &map)));
    if let Some(l) = &opts.logging_arg {
        args.push(l.clone());
    }

    args.push(main_class);

    let legacy = version.arguments.as_ref().is_none_or(|a| a.game.is_empty());
    let game: Vec<String> = if legacy {
        version
            .minecraft_arguments
            .as_deref()
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_string)
            .collect()
    } else {
        collect(&version.arguments.as_ref().unwrap().game, &env)
    };
    args.extend(game.iter().map(|a| substitute(a, &map)));
    if legacy {
        args.extend(["--width".into(), vars.width.to_string(), "--height".into(), vars.height.to_string()]);
    }
    if opts.fullscreen {
        args.push("--fullscreen".into());
    }
    Ok(args)
}

/// Скрывает токен доступа при записи командной строки в лог.
pub fn redact(args: &[String], token: &str) -> Vec<String> {
    args.iter()
        .map(|a| if !token.is_empty() && token.len() > 4 && a.contains(token) { a.replace(token, "<скрыто>") } else { a.clone() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minecraft::libraries::resolve_libraries;
    use crate::minecraft::rules::tests::env;
    use crate::minecraft::rules::{Arch, OsName};
    use crate::minecraft::version::tests::{LEGACY, MODERN};

    fn vars() -> LaunchVars {
        LaunchVars {
            username: "Steve".into(),
            uuid: "0123".into(),
            access_token: "secret-token".into(),
            user_type: "legacy".into(),
            xuid: "0".into(),
            client_id: "".into(),
            version_name: "1.20.1".into(),
            version_type: "release".into(),
            game_dir: PathBuf::from("/g"),
            assets_root: PathBuf::from("/a"),
            game_assets: PathBuf::from("/a"),
            assets_index: "5".into(),
            natives_dir: PathBuf::from("/n"),
            libraries_dir: PathBuf::from("/l"),
            classpath: "CP".into(),
            width: 1280,
            height: 720,
        }
    }

    fn opts() -> LaunchOptions {
        LaunchOptions {
            min_memory_mb: 512,
            max_memory_mb: 4096,
            extra_jvm_args: vec!["-XX:+UseG1GC".into()],
            logging_arg: Some("-Dlog4j.configurationFile=/a/log_configs/client-1.12.xml".into()),
            fullscreen: false,
        }
    }

    #[test]
    fn substitution() {
        let m = vars().map(&env(OsName::Linux, Arch::X86_64));
        assert_eq!(substitute("--user ${auth_player_name}!", &m), "--user Steve!");
        assert_eq!(substitute("${unknown} ${version_name}", &m), "${unknown} 1.20.1");
        assert_eq!(substitute("tail ${broken", &m), "tail ${broken");
        assert_eq!(substitute("${classpath_separator}", &m), ":");
    }

    #[test]
    fn modern_windows_command() {
        let v: VersionJson = serde_json::from_str(MODERN).unwrap();
        let args = build_command(&v, &env(OsName::Windows, Arch::X86_64), &vars(), &opts()).unwrap();
        let main = args.iter().position(|a| a == "net.minecraft.client.main.Main").unwrap();
        let (jvm, game) = (&args[..main], &args[main + 1..]);
        assert_eq!(&jvm[..3], &["-Xms512M", "-Xmx4096M", "-XX:+UseG1GC"]);
        assert!(jvm.iter().any(|a| a.starts_with("-XX:HeapDumpPath")), "windows-правило");
        assert!(!jvm.iter().any(|a| a == "-XstartOnFirstThread"), "osx-правило не должно сработать");
        assert!(jvm.contains(&"-Djava.library.path=/n".to_string()));
        assert_eq!(jvm[jvm.iter().position(|a| a == "-cp").unwrap() + 1], "CP");
        assert_eq!(jvm.last().unwrap(), "-Dlog4j.configurationFile=/a/log_configs/client-1.12.xml");
        assert_eq!(&game[..4], &["--username", "Steve", "--version", "1.20.1"]);
        assert!(!game.contains(&"--demo".to_string()));
        assert!(game.ends_with(&["--width".into(), "1280".into(), "--height".into(), "720".into()]));
    }

    #[test]
    fn modern_osx_first_thread() {
        let v: VersionJson = serde_json::from_str(MODERN).unwrap();
        let args = build_command(&v, &env(OsName::Osx, Arch::Arm64), &vars(), &opts()).unwrap();
        assert!(args.contains(&"-XstartOnFirstThread".to_string()));
    }

    #[test]
    fn legacy_command() {
        let v: VersionJson = serde_json::from_str(LEGACY).unwrap();
        let mut o = opts();
        o.fullscreen = true;
        o.logging_arg = None;
        let args = build_command(&v, &env(OsName::Linux, Arch::X86_64), &vars(), &o).unwrap();
        let main = args.iter().position(|a| a == "net.minecraft.client.main.Main").unwrap();
        assert!(args[..main].contains(&"-Djava.library.path=/n".to_string()));
        assert!(args[..main].contains(&"-Dminecraft.launcher.brand=cubelaunch".to_string()));
        assert_eq!(
            &args[main + 1..],
            &["--username", "Steve", "--version", "1.20.1", "--gameDir", "/g", "--width", "1280", "--height", "720", "--fullscreen"]
        );
    }

    #[test]
    fn classpath_excludes_legacy_natives_and_puts_client_last() {
        let v: VersionJson = serde_json::from_str(LEGACY).unwrap();
        let e = env(OsName::Windows, Arch::X86_64);
        let libs = resolve_libraries(&v.libraries, &e);
        let cp = build_classpath(&libs, Path::new("L"), Path::new("client.jar"), &e);
        let parts: Vec<&str> = cp.split(';').collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].ends_with("jinput-2.0.5.jar"));
        assert_eq!(parts[1], "client.jar");
    }

    #[test]
    fn splits_user_args() {
        assert_eq!(split_args(" -Xss1M  -Dfoo=\"a b\" '-Dbar=c d' "), vec!["-Xss1M", "-Dfoo=a b", "-Dbar=c d"]);
        assert!(split_args("   ").is_empty());
    }

    #[test]
    fn redacts_token() {
        let r = redact(&["--accessToken".into(), "secret-token".into()], "secret-token");
        assert_eq!(r[1], "<скрыто>");
    }
}
