//! Клиент Modrinth API v2 (<https://docs.modrinth.com/api/>): поиск, версии,
//! зависимости, распознавание файлов по SHA1 и проверка обновлений.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::ContentKind;
use crate::error::{AppError, AppResult};

pub const API: &str = "https://api.modrinth.com/v2";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub follows: u64,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub date_modified: Option<String>,
    #[serde(default)]
    pub project_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub total_hits: u64,
    pub offset: u64,
    pub limit: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Hashes {
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha512: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub hashes: Hashes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Dependency {
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    /// `required` | `optional` | `incompatible` | `embedded`
    pub dependency_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Version {
    pub id: String,
    pub project_id: String,
    #[serde(default)]
    pub name: String,
    pub version_number: String,
    /// `release` | `beta` | `alpha`
    #[serde(default)]
    pub version_type: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub files: Vec<VersionFile>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub date_published: Option<String>,
    #[serde(default)]
    pub downloads: u64,
}

impl Version {
    pub fn primary_file(&self) -> Option<&VersionFile> {
        self.files.iter().find(|f| f.primary).or(self.files.first())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub project_type: String,
}

/// Фильтр совместимости с инстансом.
#[derive(Debug, Clone)]
pub struct Target {
    pub minecraft: String,
    /// `None` — без фильтра по загрузчику (шейдеры).
    pub loaders: Option<Vec<String>>,
}

impl Target {
    /// Что подходит инстансу для этой категории. `Err` — моды без загрузчика.
    pub fn for_instance(kind: ContentKind, minecraft: &str, loader: Option<&str>) -> AppResult<Self> {
        let loaders = match kind {
            ContentKind::Mods => {
                let l = loader.ok_or_else(|| {
                    AppError::Content("для модов нужен загрузчик — выберите его на вкладке «Версия»".into())
                })?;
                Some(match l {
                    "quilt" => vec!["quilt".into(), "fabric".into()],
                    "neoforge" if minecraft == "1.20.1" => vec!["neoforge".into(), "forge".into()],
                    other => vec![other.to_string()],
                })
            }
            ContentKind::Resourcepacks => Some(vec!["minecraft".into()]),
            ContentKind::Shaderpacks => None,
        };
        Ok(Self { minecraft: minecraft.to_string(), loaders })
    }

    pub fn accepts(&self, v: &Version) -> bool {
        v.game_versions.iter().any(|g| g == &self.minecraft)
            && self.loaders.as_ref().is_none_or(|ls| v.loaders.iter().any(|l| ls.contains(l)))
    }
}

/// id/slug из UI идут в путь запроса — только безопасные символы.
fn validate_id(id: &str) -> AppResult<()> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::InvalidInput(format!("некорректный id Modrinth: {id}")));
    }
    Ok(())
}

pub struct Modrinth<'a> {
    pub client: &'a reqwest::Client,
    pub base: &'a str,
}

/// 404 → `None`, остальные ошибки HTTP — ошибки.
async fn json_or_none<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> AppResult<Option<T>> {
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    Ok(Some(resp.error_for_status()?.json().await?))
}

impl Modrinth<'_> {
    pub fn new(client: &reqwest::Client) -> Modrinth<'_> {
        Modrinth { client, base: API }
    }

    pub async fn search(&self, query: &str, kind: ContentKind, target: &Target, sort: &str, offset: u64) -> AppResult<SearchResult> {
        let mut facets = vec![vec![format!("project_type:{}", kind.modrinth_type())], vec![format!("versions:{}", target.minecraft)]];
        if let (ContentKind::Mods, Some(ls)) = (kind, &target.loaders) {
            facets.push(ls.iter().map(|l| format!("categories:{l}")).collect());
        }
        let index = match sort {
            "downloads" | "follows" | "newest" | "updated" => sort,
            _ => "relevance",
        };
        let resp = self
            .client
            .get(format!("{}/search", self.base))
            .query(&[
                ("query", query.to_string()),
                ("facets", serde_json::to_string(&facets)?),
                ("index", index.to_string()),
                ("offset", offset.to_string()),
                ("limit", "20".to_string()),
            ])
            .send()
            .await?;
        Ok(resp.error_for_status()?.json().await?)
    }

    /// Поиск модпаков (без инстанса): необязательные фильтры по версии игры и загрузчику.
    pub async fn search_modpacks(&self, query: &str, minecraft: Option<&str>, loader: Option<&str>, sort: &str, offset: u64) -> AppResult<SearchResult> {
        let mut facets = vec![vec!["project_type:modpack".to_string()]];
        if let Some(mc) = minecraft {
            facets.push(vec![format!("versions:{mc}")]);
        }
        if let Some(l) = loader {
            facets.push(vec![format!("categories:{l}")]);
        }
        let index = match sort {
            "downloads" | "follows" | "newest" | "updated" => sort,
            _ => "relevance",
        };
        let resp = self
            .client
            .get(format!("{}/search", self.base))
            .query(&[
                ("query", query.to_string()),
                ("facets", serde_json::to_string(&facets)?),
                ("index", index.to_string()),
                ("offset", offset.to_string()),
                ("limit", "20".to_string()),
            ])
            .send()
            .await?;
        Ok(resp.error_for_status()?.json().await?)
    }

    /// Все версии проекта (новые сверху).
    pub async fn all_versions(&self, project: &str) -> AppResult<Vec<Version>> {
        validate_id(project)?;
        let resp = self.client.get(format!("{}/project/{project}/version", self.base)).send().await?;
        Ok(json_or_none(resp).await?.unwrap_or_default())
    }

    /// Версии проекта, подходящие инстансу (новые сверху).
    pub async fn versions(&self, project: &str, target: &Target) -> AppResult<Vec<Version>> {
        validate_id(project)?;
        let mut q = vec![("game_versions", serde_json::to_string(&[&target.minecraft])?)];
        if let Some(ls) = &target.loaders {
            q.push(("loaders", serde_json::to_string(ls)?));
        }
        let resp = self.client.get(format!("{}/project/{project}/version", self.base)).query(&q).send().await?;
        Ok(json_or_none(resp).await?.unwrap_or_default())
    }

    pub async fn version(&self, id: &str) -> AppResult<Option<Version>> {
        validate_id(id)?;
        json_or_none(self.client.get(format!("{}/version/{id}", self.base)).send().await?).await
    }

    pub async fn projects(&self, ids: &[String]) -> AppResult<Vec<Project>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let resp = self.client.get(format!("{}/projects", self.base)).query(&[("ids", serde_json::to_string(ids)?)]).send().await?;
        Ok(resp.error_for_status()?.json().await?)
    }

    /// Версии, которым принадлежат файлы с этими SHA1.
    pub async fn versions_by_hashes(&self, hashes: &[String]) -> AppResult<HashMap<String, Version>> {
        if hashes.is_empty() {
            return Ok(HashMap::new());
        }
        let resp = self
            .client
            .post(format!("{}/version_files", self.base))
            .json(&json!({ "hashes": hashes, "algorithm": "sha1" }))
            .send()
            .await?;
        Ok(resp.error_for_status()?.json().await?)
    }

    /// Последние совместимые версии для файлов с этими SHA1.
    pub async fn latest_by_hashes(&self, hashes: &[String], target: &Target) -> AppResult<HashMap<String, Version>> {
        if hashes.is_empty() {
            return Ok(HashMap::new());
        }
        let mut body = json!({ "hashes": hashes, "algorithm": "sha1", "game_versions": [target.minecraft] });
        if let Some(ls) = &target.loaders {
            body["loaders"] = json!(ls);
        }
        let resp = self.client.post(format!("{}/version_files/update", self.base)).json(&body).send().await?;
        Ok(resp.error_for_status()?.json().await?)
    }

    /// Лучшая совместимая версия проекта: свежий релиз, иначе самая новая.
    pub async fn best_version(&self, project: &str, target: &Target) -> AppResult<Option<Version>> {
        let list: Vec<Version> = self.versions(project, target).await?.into_iter().filter(|v| target.accepts(v)).collect();
        Ok(list.iter().find(|v| v.version_type == "release").or(list.first()).cloned())
    }

    /// План установки: выбранная версия + обязательные зависимости (рекурсивно).
    /// Проекты из `installed` пропускаются (кроме корневого — он заменяет старый файл).
    pub async fn plan(&self, root: Version, target: &Target, installed: &HashSet<String>, with_deps: bool) -> AppResult<Vec<Version>> {
        let mut plan = vec![];
        let mut seen: HashSet<String> = HashSet::from([root.project_id.clone()]);
        let mut queue = VecDeque::from([root]);
        while let Some(v) = queue.pop_front() {
            if with_deps {
                for dep in v.dependencies.iter().filter(|d| d.dependency_type == "required") {
                    let pinned = match &dep.version_id {
                        Some(id) => self.version(id).await?,
                        None => None,
                    };
                    let Some(project) = dep.project_id.clone().or_else(|| pinned.as_ref().map(|p| p.project_id.clone())) else {
                        continue;
                    };
                    if installed.contains(&project) || !seen.insert(project.clone()) {
                        continue;
                    }
                    // Закреплённую версию берём, только если она подходит инстансу.
                    let chosen = match pinned.filter(|p| target.accepts(p)) {
                        Some(p) => Some(p),
                        None => self.best_version(&project, target).await?,
                    };
                    match chosen {
                        Some(c) => queue.push_back(c),
                        None => {
                            let title = self.projects(std::slice::from_ref(&project)).await.ok().and_then(|p| p.into_iter().next()).map(|p| p.title).unwrap_or(project);
                            return Err(AppError::Content(format!(
                                "нет версии обязательной зависимости «{title}» для Minecraft {}",
                                target.minecraft
                            )));
                        }
                    }
                }
            }
            plan.push(v);
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn version(id: &str, project: &str, deps: serde_json::Value, game: &str, loader: &str) -> serde_json::Value {
        json!({
            "id": id, "project_id": project, "name": id, "version_number": format!("{id}-v"), "version_type": "release",
            "game_versions": [game], "loaders": [loader], "dependencies": deps,
            "files": [{ "url": format!("https://cdn.modrinth.com/{id}.jar"), "filename": format!("{id}.jar"), "primary": true, "size": 10, "hashes": { "sha1": format!("{id}sha") } }]
        })
    }

    fn target() -> Target {
        Target::for_instance(ContentKind::Mods, "1.21.1", Some("quilt")).unwrap()
    }

    #[test]
    fn targets() {
        assert_eq!(target().loaders.unwrap(), vec!["quilt", "fabric"]);
        assert!(matches!(Target::for_instance(ContentKind::Mods, "1.21.1", None), Err(AppError::Content(_))));
        assert!(Target::for_instance(ContentKind::Shaderpacks, "1.21.1", None).unwrap().loaders.is_none());
        let t = Target::for_instance(ContentKind::Mods, "1.20.1", Some("neoforge")).unwrap();
        assert_eq!(t.loaders.unwrap(), vec!["neoforge", "forge"]);
        let v: Version = serde_json::from_value(version("a", "p", json!([]), "1.21.1", "fabric")).unwrap();
        assert!(target().accepts(&v));
        let v2: Version = serde_json::from_value(version("a", "p", json!([]), "1.20.1", "fabric")).unwrap();
        assert!(!target().accepts(&v2));
    }

    #[tokio::test]
    async fn search_builds_facets() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .and(query_param("facets", r#"[["project_type:mod"],["versions:1.21.1"],["categories:quilt","categories:fabric"]]"#))
            .and(query_param("index", "downloads"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "hits": [{ "project_id": "AANobbMI", "slug": "sodium", "title": "Sodium", "description": "fast", "author": "jelly",
                           "icon_url": "https://cdn/x.png", "downloads": 100, "follows": 5, "categories": ["fabric"], "project_type": "mod" }],
                "total_hits": 1, "offset": 0, "limit": 20
            })))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let base = server.uri();
        let m = Modrinth { client: &client, base: &base };
        let r = m.search("sod", ContentKind::Mods, &target(), "downloads", 0).await.unwrap();
        assert_eq!(r.hits[0].title, "Sodium");
        // Наружу — camelCase.
        let s = serde_json::to_value(&r.hits[0]).unwrap();
        assert_eq!(s["projectId"], "AANobbMI");
    }

    #[tokio::test]
    async fn plans_required_dependencies() {
        let server = MockServer::start().await;
        // dep1 закреплён на версии для другой версии MC → берём лучшую совместимую.
        Mock::given(method("GET")).and(path("/version/dep1-old"))
            .respond_with(ResponseTemplate::new(200).set_body_json(version("dep1-old", "dep1", json!([]), "1.20.1", "fabric")))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/project/dep1/version"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                version("dep1-new", "dep1", json!([{ "project_id": "dep2", "dependency_type": "required" }]), "1.21.1", "fabric")
            ])))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/project/dep2/version"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([version("dep2-a", "dep2", json!([]), "1.21.1", "quilt")])))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/project/missing/version"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/projects"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "id": "missing", "slug": "m", "title": "Пропавшая" }])))
            .mount(&server).await;

        let client = reqwest::Client::new();
        let base = server.uri();
        let m = Modrinth { client: &client, base: &base };
        let root: Version = serde_json::from_value(version("root", "rootp", json!([
            { "version_id": "dep1-old", "project_id": "dep1", "dependency_type": "required" },
            { "project_id": "opt", "dependency_type": "optional" },
            { "project_id": "api", "dependency_type": "required" }
        ]), "1.21.1", "fabric")).unwrap();
        let installed = HashSet::from(["api".to_string()]);
        let plan = m.plan(root.clone(), &target(), &installed, true).await.unwrap();
        assert_eq!(plan.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), vec!["root", "dep1-new", "dep2-a"]);

        let no_deps = m.plan(root, &target(), &installed, false).await.unwrap();
        assert_eq!(no_deps.len(), 1);

        let broken: Version = serde_json::from_value(version("b", "bp", json!([{ "project_id": "missing", "dependency_type": "required" }]), "1.21.1", "fabric")).unwrap();
        let e = m.plan(broken, &target(), &HashSet::new(), true).await.unwrap_err();
        assert!(e.to_string().contains("Пропавшая"), "{e}");
    }

    #[tokio::test]
    async fn hash_lookups() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/version_files/update"))
            .and(body_partial_json(json!({ "algorithm": "sha1", "game_versions": ["1.21.1"], "loaders": ["quilt", "fabric"] })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "aaa": version("v2", "p", json!([]), "1.21.1", "fabric") })))
            .mount(&server).await;
        Mock::given(method("POST")).and(path("/version_files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "aaa": version("v1", "p", json!([]), "1.21.1", "fabric") })))
            .mount(&server).await;
        let client = reqwest::Client::new();
        let base = server.uri();
        let m = Modrinth { client: &client, base: &base };
        let hashes = vec!["aaa".to_string(), "bbb".to_string()];
        assert_eq!(m.versions_by_hashes(&hashes).await.unwrap()["aaa"].id, "v1");
        assert_eq!(m.latest_by_hashes(&hashes, &target()).await.unwrap()["aaa"].id, "v2");
        assert!(m.versions_by_hashes(&[]).await.unwrap().is_empty());
    }
}
