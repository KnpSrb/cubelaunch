//! Правила (`rules`) из version JSON: условия по ОС, архитектуре, версии ОС и «фичам».
//!
//! Семантика Mojang: правила проверяются по порядку, каждое подходящее правило
//! перезаписывает результат своим `action`. Пустой список — разрешено.
//! Если ни одно правило не подошло — запрещено.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OsName {
    Windows,
    Linux,
    Osx,
}

impl OsName {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "windows" => OsName::Windows,
            "macos" => OsName::Osx,
            _ => OsName::Linux,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OsName::Windows => "windows",
            OsName::Linux => "linux",
            OsName::Osx => "osx",
        }
    }

    /// Имя ОС в новых классификаторах natives (`natives-macos`, а не `natives-osx`).
    pub fn natives_name(self) -> &'static str {
        match self {
            OsName::Windows => "windows",
            OsName::Linux => "linux",
            OsName::Osx => "macos",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    X86,
    X86_64,
    Arm64,
    Arm32,
}

impl Arch {
    pub fn current() -> Self {
        match std::env::consts::ARCH {
            "x86" => Arch::X86,
            "aarch64" => Arch::Arm64,
            "arm" => Arch::Arm32,
            _ => Arch::X86_64,
        }
    }

    /// Значение для подстановки `${arch}` в старых классификаторах natives.
    pub fn bits(self) -> &'static str {
        match self {
            Arch::X86 | Arch::Arm32 => "32",
            Arch::X86_64 | Arch::Arm64 => "64",
        }
    }
}

/// Окружение, относительно которого вычисляются правила.
#[derive(Debug, Clone)]
pub struct Env {
    pub os: OsName,
    pub arch: Arch,
    /// Версия ОС, например `10.0.22631` — для правил вида `"version": "^10\\."`.
    pub os_version: String,
    /// Включённые фичи (`is_demo_user`, `has_custom_resolution`, …).
    pub features: HashMap<String, bool>,
}

impl Env {
    pub fn current() -> Self {
        Self {
            os: OsName::current(),
            arch: Arch::current(),
            os_version: os_info::get().version().to_string(),
            features: HashMap::new(),
        }
    }

    pub fn with_feature(mut self, name: &str, on: bool) -> Self {
        self.features.insert(name.to_string(), on);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OsRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<HashMap<String, bool>>,
}

impl Rule {
    pub fn matches(&self, env: &Env) -> bool {
        if let Some(os) = &self.os {
            if let Some(name) = &os.name {
                // На всякий случай принимаем и «macos».
                let ok = name == env.os.as_str() || (env.os == OsName::Osx && name == "macos");
                if !ok {
                    return false;
                }
            }
            if let Some(arch) = &os.arch {
                let ok = match arch.as_str() {
                    "x86" => env.arch == Arch::X86,
                    "x86_64" | "amd64" => env.arch == Arch::X86_64,
                    "arm64" | "aarch64" => env.arch == Arch::Arm64,
                    "arm32" | "arm" => env.arch == Arch::Arm32,
                    _ => false,
                };
                if !ok {
                    return false;
                }
            }
            if let Some(ver) = &os.version {
                match regex::Regex::new(ver) {
                    Ok(re) if re.is_match(&env.os_version) => {}
                    _ => return false,
                }
            }
        }
        if let Some(features) = &self.features {
            for (name, expected) in features {
                let actual = env.features.get(name).copied().unwrap_or(false);
                if actual != *expected {
                    return false;
                }
            }
        }
        true
    }
}

pub fn rules_allow(rules: &[Rule], env: &Env) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule.matches(env) {
            allowed = rule.action == RuleAction::Allow;
        }
    }
    allowed
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn env(os: OsName, arch: Arch) -> Env {
        Env { os, arch, os_version: "10.0.22631".into(), features: HashMap::new() }
    }

    fn parse(json: &str) -> Vec<Rule> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn empty_rules_allow() {
        assert!(rules_allow(&[], &env(OsName::Linux, Arch::X86_64)));
    }

    #[test]
    fn allow_all_except_osx() {
        let rules = parse(
            r#"[{"action":"allow"},{"action":"disallow","os":{"name":"osx"}}]"#,
        );
        assert!(rules_allow(&rules, &env(OsName::Windows, Arch::X86_64)));
        assert!(rules_allow(&rules, &env(OsName::Linux, Arch::X86_64)));
        assert!(!rules_allow(&rules, &env(OsName::Osx, Arch::Arm64)));
    }

    #[test]
    fn only_osx() {
        let rules = parse(r#"[{"action":"allow","os":{"name":"osx"}}]"#);
        assert!(rules_allow(&rules, &env(OsName::Osx, Arch::X86_64)));
        assert!(!rules_allow(&rules, &env(OsName::Windows, Arch::X86_64)));
    }

    #[test]
    fn arch_rule() {
        let rules = parse(r#"[{"action":"allow","os":{"arch":"x86"}}]"#);
        assert!(rules_allow(&rules, &env(OsName::Windows, Arch::X86)));
        assert!(!rules_allow(&rules, &env(OsName::Windows, Arch::X86_64)));
    }

    #[test]
    fn os_version_regex() {
        let rules = parse(r#"[{"action":"allow","os":{"name":"windows","version":"^10\\."}}]"#);
        assert!(rules_allow(&rules, &env(OsName::Windows, Arch::X86_64)));
        let mut e = env(OsName::Windows, Arch::X86_64);
        e.os_version = "6.1.7601".into();
        assert!(!rules_allow(&rules, &e));
    }

    #[test]
    fn features() {
        let rules = parse(r#"[{"action":"allow","features":{"has_custom_resolution":true}}]"#);
        let e = env(OsName::Linux, Arch::X86_64);
        assert!(!rules_allow(&rules, &e));
        assert!(rules_allow(&rules, &e.with_feature("has_custom_resolution", true)));
    }
}
