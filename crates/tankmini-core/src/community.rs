//! The M-VAVE community site (community.m-vave.com): users share amp models
//! (`.am4data`) and cabinet IRs (`.wav`). The API is public; listing and
//! downloading need no login. Endpoints found in the site's own script
//! (see `docs/protocol/app-findings.md` §13):
//!
//! - `POST /bbs/api/getlist` `{search, select: [], sort, currentPage, pageSize}`
//! - `GET /bbs/api/download?articleId=<id>&file=<stored file name>`
//!
//! Blocking calls: run them off the UI thread.

use serde::Deserialize;
use std::io::Read;
use std::time::Duration;

const BASE: &str = "https://community.m-vave.com/bbs/api";
/// Files above this are not models or IRs we can use.
const MAX_DOWNLOAD: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Latest,
    Popular,
}

impl Sort {
    fn code(self) -> u8 {
        match self {
            Sort::Latest => 1,
            Sort::Popular => 2,
        }
    }
}

/// What a file is good for on this pedal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `.am2data`, `.am3data`, `.am4data`: importable as an amp.
    Amp,
    /// `.wav`: importable as a cab IR.
    Ir,
    /// Anything else (`.nam`, …): shown, not importable.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Name on the server (`<uuid>_<name>.am4data`), used to download it.
    pub stored: String,
    /// Name to show and to save as.
    pub display: String,
    pub size: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    pub id: u64,
    pub title: String,
    pub author: String,
    pub downloads: u64,
    pub likes: u64,
    pub files: Vec<File>,
}

impl Post {
    /// Whether any file of the post can be imported.
    pub fn importable(&self) -> bool {
        self.files.iter().any(|f| f.kind != Kind::Other)
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    code: i64,
    msg: Option<String>,
    data: Option<T>,
}

#[derive(Deserialize)]
struct Page {
    records: Vec<RawPost>,
    total: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPost {
    id: u64,
    title: Option<String>,
    create_user_name: Option<String>,
    download_num: Option<u64>,
    like_num: Option<u64>,
    file_path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFile {
    file_name: String,
    original_name: Option<String>,
    file_size: Option<String>,
}

fn kind_of(name: &str) -> Kind {
    let lower = name.to_ascii_lowercase();
    if crate::amp::Format::from_file_name(&lower).is_some() {
        Kind::Amp
    } else if lower.ends_with(".wav") {
        Kind::Ir
    } else {
        Kind::Other
    }
}

/// `"<uuid>_Name.ext"` → `"Name.ext"`; uploads converted on the server keep
/// only the uuid, so the original name's stem is used with the real extension.
fn display_name(f: &RawFile) -> String {
    let ext = f.file_name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    if let Some(orig) = &f.original_name {
        let stem = orig.rsplit_once('.').map(|(s, _)| s).unwrap_or(orig);
        return format!("{stem}.{}", ext.to_ascii_lowercase());
    }
    match f.file_name.split_once('_') {
        Some((uuid, rest)) if uuid.len() == 36 => rest.to_string(),
        _ => f.file_name.clone(),
    }
}

fn parse_post(raw: RawPost) -> Post {
    let files: Vec<RawFile> = raw.file_path.as_deref().and_then(|p| serde_json::from_str(p).ok()).unwrap_or_default();
    Post {
        id: raw.id,
        title: raw.title.unwrap_or_default().trim().to_string(),
        author: raw.create_user_name.unwrap_or_default(),
        downloads: raw.download_num.unwrap_or(0),
        likes: raw.like_num.unwrap_or(0),
        files: files
            .iter()
            .map(|f| File {
                stored: f.file_name.clone(),
                display: display_name(f),
                size: f.file_size.clone().unwrap_or_default(),
                kind: kind_of(&f.file_name),
            })
            .collect(),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("tankmini-linux/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// One page of posts, newest or most downloaded first. Returns the posts and
/// the total count for the query.
pub fn search(query: &str, sort: Sort, page: u32, page_size: u32) -> Result<(Vec<Post>, u64), String> {
    let body = serde_json::json!({
        "search": query.trim(),
        "select": [],
        "sort": sort.code(),
        "currentPage": page.max(1),
        "pageSize": page_size.clamp(1, 50),
    });
    let resp = agent()
        .post(&format!("{BASE}/getlist"))
        .send_json(body)
        .map_err(|e| format!("sem resposta da comunidade: {e}"))?;
    let env: Envelope<Page> = resp.into_json().map_err(|e| format!("resposta inesperada: {e}"))?;
    if env.code != 200 {
        return Err(env.msg.unwrap_or_else(|| format!("erro {}", env.code)));
    }
    let page = env.data.ok_or("resposta vazia")?;
    Ok((page.records.into_iter().map(parse_post).collect(), page.total))
}

/// Download one file of a post.
pub fn download(post: u64, file: &File) -> Result<Vec<u8>, String> {
    let resp = agent()
        .get(&format!("{BASE}/download"))
        .query("articleId", &post.to_string())
        .query("file", &file.stored)
        .call()
        .map_err(|e| format!("download falhou: {e}"))?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(MAX_DOWNLOAD + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("download falhou: {e}"))?;
    if bytes.is_empty() {
        return Err("o servidor não devolveu o arquivo".into());
    }
    if bytes.len() as u64 > MAX_DOWNLOAD {
        return Err("arquivo grande demais para um modelo ou IR".into());
    }
    Ok(bytes)
}

/// Where downloaded files are kept, so they can be imported again later.
pub fn download_dir() -> std::path::PathBuf {
    crate::paths::data_dir().join("community")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_listing_record() {
        let raw: RawPost = serde_json::from_str(
            r#"{"id":11876,"title":" SLAMMIN ","createUserName":"MesaBoogie","downloadNum":56,"likeNum":0,
            "filePath":"[{\"originalName\":\"SLAMMIN_JHS.nam\",\"fileName\":\"fb1842cd-f207-485b-9a20-75edaf62dbc2.am3Data\",\"fileSize\":\"6KB\"},{\"fileName\":\"331be756-6c97-4904-a7ed-f052f2073256_Fender_Tweed.am4data\",\"fileSize\":\"8KB\"},{\"fileName\":\"0bd8f9f4-a1fc-4dc8-8149-4fb9280f1595.wav\",\"originalName\":\"Clean IR.wav\"}]"}"#,
        )
        .unwrap();
        let p = parse_post(raw);
        assert_eq!(p.title, "SLAMMIN");
        assert_eq!(p.files[0].display, "SLAMMIN_JHS.am3data");
        assert_eq!(p.files[0].kind, Kind::Amp);
        assert_eq!(p.files[1].display, "Fender_Tweed.am4data");
        assert_eq!(p.files[1].kind, Kind::Amp);
        assert_eq!(p.files[2].display, "Clean IR.wav");
        assert_eq!(p.files[2].kind, Kind::Ir);
        assert!(p.importable());
    }

    #[test]
    fn a_post_without_files_is_not_importable() {
        let raw: RawPost = serde_json::from_str(r#"{"id":1,"title":"x","filePath":null}"#).unwrap();
        assert!(!parse_post(raw).importable());
    }
}
