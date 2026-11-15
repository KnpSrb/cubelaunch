//! Модель version JSON (`versions/<id>/<id>.json`).
//!
//! Поддерживаются оба формата аргументов: современный `arguments { game, jvm }`
//! (1.13+) и старый `minecraftArguments` (строка). Неизвестные поля игнорируются,
//! поэтому файлы от загрузчиков модов тоже читаются.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::rules::Rule;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_class: Option<String>,
    /// Родительская версия (у профилей загрузчиков модов).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Arguments>,
    /// Старый формат аргументов игры (до 1.13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,
    /// `client`, `server`, `client_mappings`, …
    #[serde(default)]
    pub downloads: HashMap<String, Download>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<Logging>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_version: Option<JavaVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compliance_level: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional { rules: Vec<Rule>, value: ArgValue },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

impl ArgValue {
    pub fn as_slice(&self) -> &[String] {
        match self {
            ArgValue::One(s) => std::slice::from_ref(s),
            ArgValue::Many(v) => v,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexRef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Library {
    /// Maven-координаты `group:artifact:version[:classifier][@ext]`.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<LibraryDownloads>,
    /// Старый формат natives: ОС → классификатор (может содержать `${arch}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<Extract>,
    /// Базовый URL maven-репозитория (у библиотек загрузчиков без `downloads`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Download>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifiers: Option<HashMap<String, Download>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Logging {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<LoggingConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Аргумент JVM с плейсхолдером `${path}`.
    pub argument: String,
    pub file: LoggingFile,
    #[serde(rename = "type", default)]
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoggingFile {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    #[serde(default)]
    pub component: String,
    pub major_version: u32,
}

impl VersionJson {
    /// Требуемая мажорная версия Java. Для старых JSON без поля — 8.
    pub fn required_java(&self) -> u32 {
        self.java_version.as_ref().map(|j| j.major_version).unwrap_or(8)
    }

    /// Идентификатор индекса assets (`assetIndex.id`, иначе `assets`, иначе `legacy`).
    pub fn assets_id(&self) -> &str {
        self.asset_index
            .as_ref()
            .map(|a| a.id.as_str())
            .or(self.assets.as_deref())
            .unwrap_or("legacy")
    }

    /// Накладывает профиль загрузчика (`self`, с `inheritsFrom`) на родительскую версию.
    ///
    /// Правила как у официального лаунчера:
    /// - библиотеки профиля идут первыми и вытесняют родительские с тем же
    ///   `group:artifact[:classifier]` (загрузчик может поднять версию, например ASM);
    /// - `arguments.game/jvm` складываются (родитель, затем профиль);
    /// - `minecraftArguments` и `mainClass` профиля заменяют родительские;
    /// - остальные поля берутся из профиля, если заданы, иначе от родителя.
    pub fn merge_onto(self, parent: VersionJson) -> VersionJson {
        let child = self;
        let keys: std::collections::HashSet<String> = child.libraries.iter().map(|l| library_key(&l.name)).collect();
        let mut libraries = child.libraries;
        libraries.extend(parent.libraries.into_iter().filter(|l| !keys.contains(&library_key(&l.name))));

        let arguments = match (parent.arguments, child.arguments) {
            (Some(mut p), Some(c)) => {
                p.game.extend(c.game);
                p.jvm.extend(c.jvm);
                Some(p)
            }
            (p, c) => c.or(p),
        };

        let mut downloads = parent.downloads;
        downloads.extend(child.downloads);

        VersionJson {
            id: child.id,
            kind: child.kind.or(parent.kind),
            main_class: child.main_class.or(parent.main_class),
            inherits_from: None,
            arguments,
            minecraft_arguments: child.minecraft_arguments.or(parent.minecraft_arguments),
            asset_index: child.asset_index.or(parent.asset_index),
            assets: child.assets.or(parent.assets),
            downloads,
            libraries,
            logging: child.logging.or(parent.logging),
            java_version: child.java_version.or(parent.java_version),
            release_time: parent.release_time.or(child.release_time),
            compliance_level: parent.compliance_level.or(child.compliance_level),
        }
    }
}

/// Ключ библиотеки без версии: `group:artifact[:classifier]`.
pub fn library_key(name: &str) -> String {
    let main = name.split('@').next().unwrap_or(name);
    let parts: Vec<&str> = main.split(':').collect();
    match parts.as_slice() {
        [g, a, _, c, ..] => format!("{g}:{a}:{c}"),
        [g, a, ..] => format!("{g}:{a}"),
        _ => main.to_string(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Сокращённый, но структурно точный фрагмент version JSON 1.20.x.
    pub const MODERN: &str = r#"{
      "id": "1.20.1",
      "type": "release",
      "mainClass": "net.minecraft.client.main.Main",
      "assets": "5",
      "assetIndex": { "id": "5", "sha1": "aa", "size": 1, "totalSize": 2, "url": "https://piston-meta.mojang.com/v1/packages/aa/5.json" },
      "complianceLevel": 1,
      "javaVersion": { "component": "java-runtime-gamma", "majorVersion": 17 },
      "downloads": {
        "client": { "sha1": "bb", "size": 100, "url": "https://piston-data.mojang.com/v1/objects/bb/client.jar" }
      },
      "arguments": {
        "game": [
          "--username", "${auth_player_name}", "--version", "${version_name}",
          { "rules": [{ "action": "allow", "features": { "is_demo_user": true } }], "value": "--demo" },
          { "rules": [{ "action": "allow", "features": { "has_custom_resolution": true } }],
            "value": ["--width", "${resolution_width}", "--height", "${resolution_height}"] }
        ],
        "jvm": [
          { "rules": [{ "action": "allow", "os": { "name": "osx" } }], "value": ["-XstartOnFirstThread"] },
          { "rules": [{ "action": "allow", "os": { "name": "windows" } }],
            "value": "-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump" },
          "-Djava.library.path=${natives_directory}", "-cp", "${classpath}"
        ]
      },
      "libraries": [
        { "name": "com.mojang:logging:1.1.1",
          "downloads": { "artifact": { "path": "com/mojang/logging/1.1.1/logging-1.1.1.jar", "sha1": "c1", "size": 10,
            "url": "https://libraries.minecraft.net/com/mojang/logging/1.1.1/logging-1.1.1.jar" } } },
        { "name": "org.lwjgl:lwjgl:3.3.1:natives-windows",
          "downloads": { "artifact": { "path": "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar", "sha1": "c2", "size": 10,
            "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar" } },
          "rules": [{ "action": "allow", "os": { "name": "windows" } }] },
        { "name": "org.lwjgl:lwjgl:3.3.1:natives-windows-arm64",
          "downloads": { "artifact": { "path": "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows-arm64.jar", "sha1": "c3", "size": 10,
            "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows-arm64.jar" } },
          "rules": [{ "action": "allow", "os": { "name": "windows" } }] },
        { "name": "org.lwjgl:lwjgl:3.3.1:natives-windows-x86",
          "downloads": { "artifact": { "path": "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows-x86.jar", "sha1": "c4", "size": 10,
            "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows-x86.jar" } },
          "rules": [{ "action": "allow", "os": { "name": "windows" } }] },
        { "name": "org.lwjgl:lwjgl:3.3.1:natives-macos-arm64",
          "downloads": { "artifact": { "path": "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-macos-arm64.jar", "sha1": "c5", "size": 10,
            "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-macos-arm64.jar" } },
          "rules": [{ "action": "allow", "os": { "name": "osx" } }] }
      ],
      "logging": { "client": { "argument": "-Dlog4j.configurationFile=${path}",
        "file": { "id": "client-1.12.xml", "sha1": "dd", "size": 888, "url": "https://piston-data.mojang.com/v1/objects/dd/client-1.12.xml" },
        "type": "log4j2-xml" } },
      "releaseTime": "2023-06-12T13:25:51+00:00"
    }"#;

    /// Фрагмент version JSON 1.8.9 со старыми natives и `minecraftArguments`.
    pub const LEGACY: &str = r#"{
      "id": "1.8.9",
      "type": "release",
      "mainClass": "net.minecraft.client.main.Main",
      "minecraftArguments": "--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory}",
      "assets": "1.8",
      "assetIndex": { "id": "1.8", "sha1": "ee", "size": 1, "totalSize": 2, "url": "https://launchermeta.mojang.com/v1/packages/ee/1.8.json" },
      "downloads": { "client": { "sha1": "ff", "size": 100, "url": "https://launcher.mojang.com/v1/objects/ff/client.jar" } },
      "libraries": [
        { "name": "org.lwjgl.lwjgl:lwjgl-platform:2.9.4-nightly-20150209",
          "downloads": { "classifiers": {
              "natives-linux": { "path": "org/lwjgl/lwjgl/lwjgl-platform/2.9.4-nightly-20150209/lwjgl-platform-2.9.4-nightly-20150209-natives-linux.jar", "sha1": "n1", "size": 1, "url": "https://libraries.minecraft.net/l.jar" },
              "natives-osx": { "path": "org/lwjgl/lwjgl/lwjgl-platform/2.9.4-nightly-20150209/lwjgl-platform-2.9.4-nightly-20150209-natives-osx.jar", "sha1": "n2", "size": 1, "url": "https://libraries.minecraft.net/o.jar" },
              "natives-windows": { "path": "org/lwjgl/lwjgl/lwjgl-platform/2.9.4-nightly-20150209/lwjgl-platform-2.9.4-nightly-20150209-natives-windows.jar", "sha1": "n3", "size": 1, "url": "https://libraries.minecraft.net/w.jar" } } },
          "extract": { "exclude": ["META-INF/"] },
          "natives": { "linux": "natives-linux", "osx": "natives-osx", "windows": "natives-windows" } },
        { "name": "tv.twitch:twitch-platform:6.5:natives-${arch}",
          "downloads": { "classifiers": {
              "natives-windows-32": { "path": "tv/twitch/twitch-platform/6.5/twitch-platform-6.5-natives-windows-32.jar", "sha1": "t32", "size": 1, "url": "https://libraries.minecraft.net/t32.jar" },
              "natives-windows-64": { "path": "tv/twitch/twitch-platform/6.5/twitch-platform-6.5-natives-windows-64.jar", "sha1": "t64", "size": 1, "url": "https://libraries.minecraft.net/t64.jar" } } },
          "natives": { "windows": "natives-windows-${arch}" },
          "rules": [{ "action": "allow" }, { "action": "disallow", "os": { "name": "linux" } }] },
        { "name": "net.java.jinput:jinput:2.0.5",
          "downloads": { "artifact": { "path": "net/java/jinput/jinput/2.0.5/jinput-2.0.5.jar", "sha1": "j1", "size": 1, "url": "https://libraries.minecraft.net/net/java/jinput/jinput/2.0.5/jinput-2.0.5.jar" } } }
      ]
    }"#;

    #[test]
    fn parses_modern() {
        let v: VersionJson = serde_json::from_str(MODERN).unwrap();
        assert_eq!(v.id, "1.20.1");
        assert_eq!(v.required_java(), 17);
        assert_eq!(v.assets_id(), "5");
        let args = v.arguments.as_ref().unwrap();
        assert_eq!(args.game.len(), 6);
        assert!(matches!(&args.game[5], Argument::Conditional { value: ArgValue::Many(v), .. } if v.len() == 4));
        assert!(matches!(&args.jvm[1], Argument::Conditional { value: ArgValue::One(_), .. }));
        assert_eq!(v.libraries.len(), 5);
        assert_eq!(v.logging.unwrap().client.unwrap().file.id, "client-1.12.xml");
        assert!(v.downloads.contains_key("client"));
    }

    #[test]
    fn parses_legacy() {
        let v: VersionJson = serde_json::from_str(LEGACY).unwrap();
        assert!(v.arguments.is_none());
        assert!(v.minecraft_arguments.as_ref().unwrap().contains("${auth_player_name}"));
        assert_eq!(v.required_java(), 8);
        assert_eq!(v.libraries[0].natives.as_ref().unwrap()["osx"], "natives-osx");
        assert_eq!(v.libraries[0].extract.as_ref().unwrap().exclude, vec!["META-INF/"]);
    }

    #[test]
    fn library_keys() {
        assert_eq!(library_key("org.ow2.asm:asm:9.7.1"), "org.ow2.asm:asm");
        assert_eq!(library_key("org.lwjgl:lwjgl:3.3.1:natives-linux"), "org.lwjgl:lwjgl:natives-linux");
        assert_eq!(library_key("de.oceanlabs.mcp:mcp_config:1.20.1@zip"), "de.oceanlabs.mcp:mcp_config");
    }

    #[test]
    fn merges_fabric_profile_onto_vanilla() {
        let parent: VersionJson = serde_json::from_str(MODERN).unwrap();
        let child: VersionJson = serde_json::from_str(
            r#"{ "id": "fabric-loader-0.16.9-1.20.1", "inheritsFrom": "1.20.1", "type": "release",
                 "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                 "arguments": { "game": [], "jvm": ["-DFabricMcEmu= net.minecraft.client.main.Main "] },
                 "libraries": [
                   { "name": "net.fabricmc:fabric-loader:0.16.9", "url": "https://maven.fabricmc.net/" },
                   { "name": "com.mojang:logging:1.2.0", "url": "https://maven.fabricmc.net/" }
                 ] }"#,
        )
        .unwrap();
        let m = child.merge_onto(parent.clone());
        assert_eq!(m.id, "fabric-loader-0.16.9-1.20.1");
        assert!(m.inherits_from.is_none());
        assert_eq!(m.main_class.as_deref(), Some("net.fabricmc.loader.impl.launch.knot.KnotClient"));
        // Библиотеки профиля первыми; logging 1.1.1 вытеснен версией из профиля.
        let names: Vec<_> = m.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names[0], "net.fabricmc:fabric-loader:0.16.9");
        assert_eq!(names[1], "com.mojang:logging:1.2.0");
        assert!(!names.contains(&"com.mojang:logging:1.1.1"));
        assert_eq!(m.libraries.len(), parent.libraries.len() + 1);
        // Аргументы сложены, всё прочее — от родителя.
        let a = m.arguments.as_ref().unwrap();
        assert_eq!(a.game.len(), parent.arguments.as_ref().unwrap().game.len());
        assert_eq!(a.jvm.len(), parent.arguments.as_ref().unwrap().jvm.len() + 1);
        assert_eq!(m.required_java(), 17);
        assert_eq!(m.assets_id(), "5");
        assert!(m.downloads.contains_key("client"));
        assert!(m.logging.is_some());
    }

    #[test]
    fn legacy_forge_replaces_minecraft_arguments() {
        let parent: VersionJson = serde_json::from_str(LEGACY).unwrap();
        let child: VersionJson = serde_json::from_str(
            r#"{ "id": "forge", "inheritsFrom": "1.8.9", "mainClass": "net.minecraft.launchwrapper.Launch",
                 "minecraftArguments": "--username ${auth_player_name} --tweakClass net.minecraftforge.fml.common.launcher.FMLTweaker",
                 "libraries": [ { "name": "net.minecraft:launchwrapper:1.12" } ] }"#,
        )
        .unwrap();
        let m = child.merge_onto(parent);
        assert!(m.minecraft_arguments.unwrap().contains("FMLTweaker"));
        assert!(m.arguments.is_none());
        assert_eq!(m.libraries[0].name, "net.minecraft:launchwrapper:1.12");
        assert_eq!(m.kind.as_deref(), Some("release"));
    }

    #[test]
    fn roundtrip_preserves_structure() {
        let v: VersionJson = serde_json::from_str(MODERN).unwrap();
        let again: VersionJson = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(v, again);
    }
}
