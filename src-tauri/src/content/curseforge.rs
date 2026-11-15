//! Клиент CurseForge API (<https://docs.curseforge.com/rest-api/>). Нужен ключ API:
//! без него CurseForge отключён. Ответы приводятся к тем же структурам, что и у
//! Modrinth (`SearchHit`, `Version`), чтобы интерфейс работал с обоими источниками.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::json;

use super::modrinth::{Dependency, Hashes, SearchHit, SearchResult, Target, Version, VersionFile};
use super::ContentKind;
use crate::error::{AppError, AppResult};

pub const API: &str = "https://api.curseforge.com/v1";
/// Minecraft в CurseForge.
pub const GAME_ID: u32 = 432;
pub const CLASS_MODS: u32 = 6;
pub const CLASS_RESOURCEPACKS: u32 = 12;
pub const CLASS_SHADERS: u32 = 6552;
pub const CLASS_MODPACKS: u32 = 4471;

/// Ключ, вшитый при сборке (`CUBELAUNCH_CURSEFORGE_KEY`).
pub const BUILT_IN_KEY: Option<&str> = option_env!("CUBELAUNCH_CURSEFORGE_KEY");

pub fn class_for(kind: ContentKind) -> u32 {
    match kind {
        ContentKind::Mods => CLASS_MODS,
        ContentKind::Resourcepacks => CLASS_RESOURCEPACKS,
        ContentKind::Shaderpacks => CLASS_SHADERS,
    }
}

/// Папка инстанса для класса проекта CurseForge (`None` — не поддерживается).
pub fn folder_for_class(class_id: u32) -> Option<&'static str> {
    match class_id {
        CLASS_MODS => Some("mods"),
        CLASS_RESOURCEPACKS => Some("resourcepacks"),
        CLASS_SHADERS => Some("shaderpacks"),
        _ => None,
    }
}

/// Код загрузчика в CurseForge.
pub fn loader_type(loader: &str) -> Option<u32> {
    match loader {
        "forge" => Some(1),
        "fabric" => Some(4),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfAuthor {
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfLogo {
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfLinks {
    #[serde(default)]
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfMod {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub download_count: f64,
    #[serde(default)]
    pub class_id: Option<u32>,
    #[serde(default)]
    pub logo: Option<CfLogo>,
    #[serde(default)]
    pub authors: Vec<CfAuthor>,
    #[serde(default)]
    pub links: CfLinks,
    #[serde(default)]
    pub date_modified: Option<String>,
}

impl CfMod {
    pub fn page_url(&self) -> String {
        self.links
            .website_url
            .clone()
            .unwrap_or_else(|| format!("https://www.curseforge.com/minecraft/mc-mods/{}", self.slug))
    }

    pub fn into_hit(self) -> SearchHit {
        SearchHit {
            project_id: self.id.to_string(),
            slug: self.slug,
            title: self.name,
            description: self.summary,
            author: self.authors.first().map(|a| a.name.clone()).unwrap_or_default(),
            icon_url: self.logo.and_then(|l| l.thumbnail_url),
            downloads: self.download_count as u64,
            follows: 0,
            categories: vec![],
            date_modified: self.date_modified,
            project_type: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CfHash {
    pub value: String,
    /// 1 — SHA1, 2 — MD5.
    pub algo: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfDependency {
    pub mod_id: u64,
    /// 3 — обязательная зависимость.
    pub relation_type: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfFile {
    pub id: u64,
    pub mod_id: u64,
    #[serde(default)]
    pub display_name: String,
    pub file_name: String,
    /// 1 — release, 2 — beta, 3 — alpha.
    #[serde(default)]
    pub release_type: u32,
    #[serde(default)]
    pub file_date: Option<String>,
    #[serde(default)]
    pub file_length: u64,
    #[serde(default)]
    pub download_count: f64,
    /// `None` — автор запретил скачивание из сторонних лаунчеров.
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<CfDependency>,
    #[serde(default)]
    pub hashes: Vec<CfHash>,
    #[serde(default)]
    pub file_fingerprint: u64,
}

const LOADER_NAMES: [&str; 4] = ["forge", "fabric", "quilt", "neoforge"];

impl CfFile {
    pub fn sha1(&self) -> Option<String> {
        self.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.to_lowercase())
    }

    /// Версии игры (без названий загрузчиков и «Client/Server»).
    pub fn minecraft_versions(&self) -> Vec<String> {
        self.game_versions
            .iter()
            .filter(|g| g.chars().next().is_some_and(|c| c.is_ascii_digit()))
            .cloned()
            .collect()
    }

    pub fn loaders(&self) -> Vec<String> {
        self.game_versions
            .iter()
            .map(|g| g.to_lowercase())
            .filter(|g| LOADER_NAMES.contains(&g.as_str()))
            .collect()
    }

    /// В формате версии Modrinth (для общего интерфейса).
    pub fn into_version(self) -> Version {
        let sha1 = self.sha1();
        let game_versions = self.minecraft_versions();
        let loaders = self.loaders();
        Version {
            id: self.id.to_string(),
            project_id: self.mod_id.to_string(),
            name: self.display_name.clone(),
            version_number: if self.display_name.is_empty() { self.file_name.clone() } else { self.display_name },
            version_type: match self.release_type {
                2 => "beta",
                3 => "alpha",
                _ => "release",
            }
            .into(),
            game_versions,
            loaders,
            files: vec![VersionFile {
                url: self.download_url.unwrap_or_default(),
                filename: self.file_name,
                primary: true,
                size: self.file_length,
                hashes: Hashes { sha1, sha512: None },
            }],
            dependencies: self
                .dependencies
                .into_iter()
                .map(|d| Dependency {
                    version_id: None,
                    project_id: Some(d.mod_id.to_string()),
                    file_name: None,
                    dependency_type: if d.relation_type == 3 { "required" } else { "optional" }.into(),
                })
                .collect(),
            date_published: self.file_date,
            downloads: self.download_count as u64,
        }
    }
}

/// Подходит ли файл CurseForge инстансу. У ресурспаков и шейдеров загрузчика нет.
pub fn accepts(target: &Target, kind: ContentKind, v: &Version) -> bool {
    v.game_versions.iter().any(|g| g == &target.minecraft)
        && (kind != ContentKind::Mods || target.loaders.as_ref().is_none_or(|ls| v.loaders.iter().any(|l| ls.contains(l))))
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    index: u64,
    page_size: u64,
    total_count: u64,
}

#[derive(Deserialize)]
struct Page<T> {
    data: Vec<T>,
    pagination: Pagination,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintMatch {
    pub id: u64,
    pub file: CfFile,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FingerprintResult {
    #[serde(default)]
    exact_matches: Vec<FingerprintMatch>,
}

/// Фильтр поиска модпаков (без инстанса).
#[derive(Debug, Clone, Default)]
pub struct PackFilter {
    pub minecraft: Option<String>,
    pub loader: Option<String>,
}

pub struct CurseForge<'a> {
    pub client: &'a reqwest::Client,
    pub base: &'a str,
    pub key: &'a str,
}

impl<'a> CurseForge<'a> {
    pub fn new(client: &'a reqwest::Client, key: &'a str) -> Self {
        Self { client, base: API, key }
    }

    async fn send<T: serde::de::DeserializeOwned>(&self, req: reqwest::RequestBuilder) -> AppResult<Option<T>> {
        let resp = req.header("x-api-key", self.key).header("accept", "application/json").send().await?;
        match resp.status().as_u16() {
            401 | 403 => Err(AppError::CurseforgeKey("ключ CurseForge API недействителен — проверьте его в настройках".into())),
            404 => Ok(None),
            _ => Ok(Some(resp.error_for_status()?.json().await?)),
        }
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> AppResult<Option<T>> {
        self.send(self.client.get(format!("{}{path}", self.base)).query(query)).await
    }

    async fn post<T: serde::de::DeserializeOwned>(&self, path: &str, body: serde_json::Value) -> AppResult<Option<T>> {
        self.send(self.client.post(format!("{}{path}", self.base)).json(&body)).await
    }

    /// Проверка ключа: простой запрос к API.
    pub async fn check_key(&self) -> AppResult<()> {
        self.get::<serde_json::Value>(&format!("/games/{GAME_ID}"), &[]).await?;
        Ok(())
    }

    async fn search_raw(&self, class_id: u32, query: &str, sort: &str, offset: u64, minecraft: Option<&str>, loader: Option<u32>) -> AppResult<SearchResult> {
        // 1 Featured, 2 Popularity, 3 LastUpdated, 6 TotalDownloads, 11 ReleasedDate.
        let sort_field = match sort {
            "downloads" => "6",
            "follows" => "2",
            "updated" => "3",
            "newest" => "11",
            _ => "1",
        };
        let mut q = vec![
            ("gameId", GAME_ID.to_string()),
            ("classId", class_id.to_string()),
            ("searchFilter", query.to_string()),
            ("sortField", sort_field.to_string()),
            ("sortOrder", "desc".to_string()),
            ("index", offset.to_string()),
            ("pageSize", "20".to_string()),
        ];
        if let Some(mc) = minecraft {
            q.push(("gameVersion", mc.to_string()));
        }
        if let Some(l) = loader {
            q.push(("modLoaderType", l.to_string()));
        }
        // CurseForge отдаёт не больше 10 000 результатов.
        if offset + 20 > 10_000 {
            return Ok(SearchResult { hits: vec![], total_hits: 0, offset, limit: 20 });
        }
        let page: Page<CfMod> = self.get("/mods/search", &q).await?.unwrap_or(Page {
            data: vec![],
            pagination: Pagination { index: offset, page_size: 20, total_count: 0 },
        });
        Ok(SearchResult {
            hits: page.data.into_iter().map(CfMod::into_hit).collect(),
            total_hits: page.pagination.total_count.min(10_000),
            offset: page.pagination.index,
            limit: page.pagination.page_size,
        })
    }

    pub async fn search(&self, query: &str, kind: ContentKind, target: &Target, sort: &str, offset: u64) -> AppResult<SearchResult> {
        // У Quilt своих модов на CurseForge мало — ищем Fabric-моды, они совместимы.
        let loader = match (kind, &target.loaders) {
            (ContentKind::Mods, Some(ls)) => ls.iter().find(|l| *l != "quilt").or(ls.first()).and_then(|l| loader_type(l)),
            _ => None,
        };
        self.search_raw(class_for(kind), query, sort, offset, Some(&target.minecraft), loader).await
    }

    pub async fn search_modpacks(&self, query: &str, filter: &PackFilter, sort: &str, offset: u64) -> AppResult<SearchResult> {
        let loader = filter.loader.as_deref().and_then(loader_type);
        self.search_raw(CLASS_MODPACKS, query, sort, offset, filter.minecraft.as_deref(), loader).await
    }

    /// Файлы проекта (новые сверху), не больше 200.
    pub async fn files(&self, mod_id: &str, minecraft: Option<&str>) -> AppResult<Vec<CfFile>> {
        validate_id(mod_id)?;
        let mut out = Vec::new();
        let mut index = 0;
        loop {
            let mut q = vec![("index", index.to_string()), ("pageSize", "50".to_string())];
            if let Some(mc) = minecraft {
                q.push(("gameVersion", mc.to_string()));
            }
            let Some(page) = self.get::<Page<CfFile>>(&format!("/mods/{mod_id}/files"), &q).await? else { break };
            let n = page.data.len() as u64;
            out.extend(page.data);
            index += n;
            if n == 0 || index >= page.pagination.total_count || index >= 200 {
                break;
            }
        }
        out.sort_by(|a, b| b.file_date.cmp(&a.file_date));
        Ok(out)
    }

    pub async fn file(&self, mod_id: &str, file_id: &str) -> AppResult<Option<CfFile>> {
        validate_id(mod_id)?;
        validate_id(file_id)?;
        Ok(self.get::<Data<CfFile>>(&format!("/mods/{mod_id}/files/{file_id}"), &[]).await?.map(|d| d.data))
    }

    pub async fn mod_info(&self, mod_id: &str) -> AppResult<Option<CfMod>> {
        validate_id(mod_id)?;
        Ok(self.get::<Data<CfMod>>(&format!("/mods/{mod_id}"), &[]).await?.map(|d| d.data))
    }

    pub async fn files_by_ids(&self, ids: &[u64]) -> AppResult<Vec<CfFile>> {
        let mut out = Vec::new();
        for chunk in ids.chunks(500) {
            let d: Option<Data<Vec<CfFile>>> = self.post("/mods/files", json!({ "fileIds": chunk })).await?;
            out.extend(d.map(|d| d.data).unwrap_or_default());
        }
        Ok(out)
    }

    pub async fn mods_by_ids(&self, ids: &[u64]) -> AppResult<Vec<CfMod>> {
        let mut out = Vec::new();
        for chunk in ids.chunks(500) {
            let d: Option<Data<Vec<CfMod>>> = self.post("/mods", json!({ "modIds": chunk })).await?;
            out.extend(d.map(|d| d.data).unwrap_or_default());
        }
        Ok(out)
    }

    /// Файлы по отпечаткам (murmur2): отпечаток → совпадение.
    pub async fn by_fingerprints(&self, prints: &[u32]) -> AppResult<HashMap<u32, FingerprintMatch>> {
        let mut out = HashMap::new();
        for chunk in prints.chunks(500) {
            let d: Option<Data<FingerprintResult>> = self.post(&format!("/fingerprints/{GAME_ID}"), json!({ "fingerprints": chunk })).await?;
            for m in d.map(|d| d.data.exact_matches).unwrap_or_default() {
                out.insert(m.file.file_fingerprint as u32, m);
            }
        }
        Ok(out)
    }

    /// Лучший совместимый файл проекта: свежий релиз, иначе самый новый.
    pub async fn best_file(&self, mod_id: &str, kind: ContentKind, target: &Target) -> AppResult<Option<Version>> {
        let list: Vec<Version> = self
            .files(mod_id, Some(&target.minecraft))
            .await?
            .into_iter()
            .map(CfFile::into_version)
            .filter(|v| accepts(target, kind, v))
            .collect();
        Ok(list.iter().find(|v| v.version_type == "release").or(list.first()).cloned())
    }
}

/// id из UI или манифеста — только цифры.
fn validate_id(id: &str) -> AppResult<()> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::InvalidInput(format!("некорректный id CurseForge: {id}")));
    }
    Ok(())
}

/// Отпечаток файла CurseForge: MurmurHash2 (seed 1) по содержимому без пробельных
/// байтов (9, 10, 13, 32).
pub fn fingerprint(data: &[u8]) -> u32 {
    let buf: Vec<u8> = data.iter().copied().filter(|b| !matches!(b, 9 | 10 | 13 | 32)).collect();
    murmur2(&buf, 1)
}

pub fn murmur2(data: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let mut h = seed ^ data.len() as u32;
    let mut chunks = data.chunks_exact(4);
    for c in &mut chunks {
        let mut k = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M) ^ k;
    }
    let rem = chunks.remainder();
    if !rem.is_empty() {
        for (i, b) in rem.iter().enumerate() {
            h ^= (*b as u32) << (8 * i);
        }
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^ (h >> 15)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Проверочное значение SMHasher для MurmurHash2 (32 бита): хеши ключей 0..i
    /// с seed 256-i, затем хеш от всех хешей.
    #[test]
    fn murmur2_matches_smhasher() {
        let key: Vec<u8> = (0..=255u8).collect();
        let mut hashes = Vec::new();
        for i in 0..256usize {
            hashes.extend_from_slice(&murmur2(&key[..i], 256 - i as u32).to_le_bytes());
        }
        assert_eq!(murmur2(&hashes, 0), 0x2786_4C1E);
    }

    #[test]
    fn fingerprint_ignores_whitespace() {
        assert_eq!(fingerprint(b"a b\r\nc\td"), fingerprint(b"abcd"));
        assert_ne!(fingerprint(b"abcd"), fingerprint(b"abce"));
    }

    fn file_json(id: u64, mod_id: u64, versions: &[&str], url: Option<&str>) -> serde_json::Value {
        json!({
            "id": id, "modId": mod_id, "displayName": format!("File {id}"), "fileName": format!("f{id}.jar"),
            "releaseType": 1, "fileDate": format!("2024-01-0{}T00:00:00Z", id % 9 + 1), "fileLength": 100,
            "downloadUrl": url, "gameVersions": versions,
            "dependencies": [{ "modId": 7, "relationType": 3 }, { "modId": 8, "relationType": 2 }],
            "hashes": [{ "value": "ABC", "algo": 1 }, { "value": "m", "algo": 2 }]
        })
    }

    #[test]
    fn converts_files() {
        let f: CfFile = serde_json::from_value(file_json(5, 3, &["1.20.1", "Forge", "Client", "NeoForge"], None)).unwrap();
        let v = f.into_version();
        assert_eq!(v.game_versions, vec!["1.20.1"]);
        assert_eq!(v.loaders, vec!["forge", "neoforge"]);
        assert_eq!(v.files[0].hashes.sha1.as_deref(), Some("abc"));
        assert_eq!(v.files[0].url, "");
        assert_eq!(v.dependencies.iter().filter(|d| d.dependency_type == "required").count(), 1);
        let t = Target::for_instance(ContentKind::Mods, "1.20.1", Some("neoforge")).unwrap();
        assert!(accepts(&t, ContentKind::Mods, &v));
        let rp = Target::for_instance(ContentKind::Resourcepacks, "1.20.1", None).unwrap();
        assert!(accepts(&rp, ContentKind::Resourcepacks, &v));
    }

    #[tokio::test]
    async fn api_calls_and_bad_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/mods/search"))
            .and(header("x-api-key", "k"))
            .and(query_param("classId", "6"))
            .and(query_param("modLoaderType", "4"))
            .and(query_param("gameVersion", "1.21.1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{ "id": 1, "name": "JEI", "slug": "jei", "summary": "items", "downloadCount": 5.0,
                           "authors": [{ "name": "mezz" }], "logo": { "thumbnailUrl": "https://x/l.png" }, "links": {} }],
                "pagination": { "index": 0, "pageSize": 20, "resultCount": 1, "totalCount": 1 }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/mods/files"))
            .and(body_partial_json(json!({ "fileIds": [5] })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [file_json(5, 3, &["1.21.1", "Fabric"], Some("https://edge/f5.jar"))] })))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let base = server.uri();
        let cf = CurseForge { client: &client, base: &base, key: "k" };
        let t = Target::for_instance(ContentKind::Mods, "1.21.1", Some("quilt")).unwrap();
        let r = cf.search("jei", ContentKind::Mods, &t, "relevance", 0).await.unwrap();
        assert_eq!(r.hits[0].title, "JEI");
        assert_eq!(r.hits[0].project_id, "1");
        assert_eq!(r.hits[0].icon_url.as_deref(), Some("https://x/l.png"));
        let files = cf.files_by_ids(&[5]).await.unwrap();
        assert_eq!(files[0].download_url.as_deref(), Some("https://edge/f5.jar"));

        let bad = CurseForge { client: &client, base: &base, key: "wrong" };
        Mock::given(method("GET")).and(path("/games/432")).and(header("x-api-key", "wrong"))
            .respond_with(ResponseTemplate::new(403)).mount(&server).await;
        assert!(matches!(bad.check_key().await, Err(AppError::CurseforgeKey(_))));
        assert!(cf.files("../x", None).await.is_err());
    }
}
