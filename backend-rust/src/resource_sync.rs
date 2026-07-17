//! 启动时从远程仓库增量同步 `resources/`（优先 Gitee，失败回退 GitHub；按 Git blob SHA 差异更新）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest::Client;
use serde::Deserialize;
use sha1::{Digest, Sha1};
use thiserror::Error;

const DEFAULT_GITEE_OWNER: &str = "floating-illusion-language";
const DEFAULT_GITEE_REPO: &str = "hsr-gacha-web";
const DEFAULT_GITEE_BRANCH: &str = "main";
const DEFAULT_GITHUB_OWNER: &str = "fushenghuanyu";
const DEFAULT_GITHUB_REPO: &str = "hsr-gacha-web";
const DEFAULT_GITHUB_BRANCH: &str = "main";
const RESOURCES_PREFIX: &str = "resources/";
const SKIP_ENV: &str = "GACHA_SKIP_RESOURCE_SYNC";
const GITEE_OWNER_ENV: &str = "GACHA_RESOURCE_GITEE_OWNER";
const GITEE_REPO_ENV: &str = "GACHA_RESOURCE_GITEE_REPO";
const GITEE_BRANCH_ENV: &str = "GACHA_RESOURCE_GITEE_BRANCH";
const GITHUB_OWNER_ENV: &str = "GACHA_RESOURCE_GITHUB_OWNER";
const GITHUB_REPO_ENV: &str = "GACHA_RESOURCE_GITHUB_REPO";
const GITHUB_BRANCH_ENV: &str = "GACHA_RESOURCE_GITHUB_BRANCH";
const MAX_CONCURRENT: usize = 8;

#[derive(Debug, Clone)]
pub enum ResourceSyncPhase {
    Idle,
    Checking,
    Downloading { done: u32, total: u32 },
    Done { updated: bool, message: String },
    Failed { message: String },
    Skipped { message: String },
}

#[derive(Debug, Clone)]
pub struct ResourceSyncStatus {
    pub phase: ResourceSyncPhase,
}

impl Default for ResourceSyncStatus {
    fn default() -> Self {
        Self {
            phase: ResourceSyncPhase::Idle,
        }
    }
}

pub struct ResourceSyncHandle {
    status: Arc<Mutex<ResourceSyncStatus>>,
    thread: Option<JoinHandle<()>>,
}

impl ResourceSyncHandle {
    pub fn status(&self) -> Arc<Mutex<ResourceSyncStatus>> {
        Arc::clone(&self.status)
    }

    pub fn join(self) {
        if let Some(h) = self.thread {
            let _ = h.join();
        }
    }
}

#[derive(Debug, Error)]
enum SyncError {
    #[error("HTTP 请求失败: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteHost {
    Gitee,
    GitHub,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteRepoSource {
    host: RemoteHost,
    owner: String,
    repo: String,
    branch: String,
}

impl RemoteRepoSource {
    fn label(&self) -> &'static str {
        match self.host {
            RemoteHost::Gitee => "Gitee",
            RemoteHost::GitHub => "GitHub",
        }
    }

    fn raw_url(&self, repo_path: &str) -> String {
        match self.host {
            RemoteHost::Gitee => format!(
                "https://gitee.com/{}/{}/raw/{}/{}",
                self.owner, self.repo, self.branch, repo_path
            ),
            RemoteHost::GitHub => format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}",
                self.owner, self.repo, self.branch, repo_path
            ),
        }
    }

    fn tree_api_url(&self) -> String {
        match self.host {
            RemoteHost::Gitee => format!(
                "https://gitee.com/api/v5/repos/{}/{}/git/trees/{}?recursive=1",
                self.owner, self.repo, self.branch
            ),
            RemoteHost::GitHub => format!(
                "https://api.github.com/repos/{}/{}/git/trees/{}?recursive=1",
                self.owner, self.repo, self.branch
            ),
        }
    }
}

fn remote_sources_from_env() -> Vec<RemoteRepoSource> {
    vec![
        RemoteRepoSource {
            host: RemoteHost::Gitee,
            owner: env_or(DEFAULT_GITEE_OWNER, GITEE_OWNER_ENV),
            repo: env_or(DEFAULT_GITEE_REPO, GITEE_REPO_ENV),
            branch: env_or(DEFAULT_GITEE_BRANCH, GITEE_BRANCH_ENV),
        },
        RemoteRepoSource {
            host: RemoteHost::GitHub,
            owner: env_or(DEFAULT_GITHUB_OWNER, GITHUB_OWNER_ENV),
            repo: env_or(DEFAULT_GITHUB_REPO, GITHUB_REPO_ENV),
            branch: env_or(DEFAULT_GITHUB_BRANCH, GITHUB_BRANCH_ENV),
        },
    ]
}

#[derive(Debug, Clone)]
struct RemoteResourceFile {
    repo_path: String,
    blob_sha: String,
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RemoteTreeResponse {
    tree: Vec<RemoteTreeEntry>,
}

#[derive(Debug, Deserialize)]
struct RemoteTreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
    size: Option<u64>,
}

fn env_or(default: &str, key: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Git 对象 blob 的 SHA-1（与 Git Trees API 返回的 `sha` 一致）。
fn git_blob_sha(content: &[u8]) -> String {
    let header = format!("blob {}\0", content.len());
    let mut hasher = Sha1::new();
    hasher.update(header.as_bytes());
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

/// 跳过自动同步的原因（用于状态文案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceSyncSkipReason {
    Env,
    DevMode,
    UserDisabled,
}

impl ResourceSyncSkipReason {
    fn message(self) -> String {
        match self {
            Self::Env => format!("已跳过资源同步（{SKIP_ENV}=1）"),
            Self::DevMode => "开发模式，已跳过远程资源同步".into(),
            Self::UserDisabled => "已关闭启动时自动同步远程资源".into(),
        }
    }
}

/// 若不应自动同步则返回原因。
pub fn resource_sync_skip_reason() -> Option<ResourceSyncSkipReason> {
    if std::env::var(SKIP_ENV).ok().as_deref() == Some("1") {
        return Some(ResourceSyncSkipReason::Env);
    }
    if let Ok(exe) = std::env::current_exe() {
        if crate::paths::is_dev_launcher_exe(&exe) {
            return Some(ResourceSyncSkipReason::DevMode);
        }
    }
    let settings = crate::launcher_settings::load_launcher_settings(&crate::paths::user_data_dir(
        &crate::paths::project_root(),
    ));
    if !settings.auto_sync_resources {
        return Some(ResourceSyncSkipReason::UserDisabled);
    }
    None
}

fn http_client() -> Result<Client, SyncError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&format!("hsr-gacha-launcher/{}", crate::APP_VERSION))
            .map_err(|e| SyncError::Message(e.to_string()))?,
    );
    Client::builder()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(SyncError::from)
}

fn parse_remote_tree(resp: RemoteTreeResponse) -> Vec<RemoteResourceFile> {
    let mut files: Vec<RemoteResourceFile> = resp
        .tree
        .into_iter()
        .filter(|e| e.kind == "blob" && e.path.starts_with(RESOURCES_PREFIX))
        .filter(|e| e.path != "resources/VERSION")
        .filter(|e| !e.path.contains("/.sync-"))
        .map(|e| RemoteResourceFile {
            repo_path: e.path,
            blob_sha: e.sha,
            size: e.size,
        })
        .collect();
    files.sort_unstable_by(|a, b| a.repo_path.cmp(&b.repo_path));
    files
}

async fn list_remote_resource_files(
    client: &Client,
    source: &RemoteRepoSource,
) -> Result<Vec<RemoteResourceFile>, SyncError> {
    let resp: RemoteTreeResponse = client
        .get(source.tree_api_url())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(parse_remote_tree(resp))
}

async fn resolve_remote_source(
    client: &Client,
    sources: &[RemoteRepoSource],
) -> Result<(RemoteRepoSource, Vec<RemoteResourceFile>), SyncError> {
    let mut last_err: Option<SyncError> = None;

    for source in sources {
        match list_remote_resource_files(client, source).await {
            Ok(files) if !files.is_empty() => {
                tracing::info!(
                    "资源源：{}（{} 个文件，{}/{}/{})",
                    source.label(),
                    files.len(),
                    source.owner,
                    source.repo,
                    source.branch
                );
                return Ok((source.clone(), files));
            }
            Ok(_) => {
                let err = SyncError::Message(format!("{} 未找到 resources/ 文件", source.label()));
                tracing::warn!("{err}");
                last_err = Some(err);
            }
            Err(e) => {
                tracing::warn!("{} 资源列表获取失败: {e}", source.label());
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| SyncError::Message("无可用的远程资源源".into())))
}

fn local_file_matches_remote(local_path: &Path, remote: &RemoteResourceFile) -> bool {
    let Ok(meta) = std::fs::metadata(local_path) else {
        return false;
    };
    if let Some(size) = remote.size {
        if meta.len() != size {
            return false;
        }
    }
    let Ok(content) = std::fs::read(local_path) else {
        return false;
    };
    git_blob_sha(&content) == remote.blob_sha
}

fn collect_files_to_update(
    project_root: &Path,
    remote_files: &[RemoteResourceFile],
) -> Vec<RemoteResourceFile> {
    remote_files
        .iter()
        .filter(|remote| {
            let local_path = repo_path_to_local(project_root, &remote.repo_path);
            !local_file_matches_remote(&local_path, remote)
        })
        .cloned()
        .collect()
}

fn collect_orphan_local_files(
    project_root: &Path,
    remote_files: &[RemoteResourceFile],
) -> Vec<PathBuf> {
    let resources_root = crate::paths::resources_dir(project_root);
    let remote_local: HashSet<PathBuf> = remote_files
        .iter()
        .map(|f| repo_path_to_local(project_root, &f.repo_path))
        .collect();

    let mut orphans = Vec::new();
    collect_orphans_recursive(&resources_root, &resources_root, &remote_local, &mut orphans);
    orphans
}

fn collect_orphans_recursive(
    resources_root: &Path,
    dir: &Path,
    remote_local: &HashSet<PathBuf>,
    orphans: &mut Vec<PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(".sync-"))
        {
            continue;
        }
        if path.is_dir() {
            collect_orphans_recursive(resources_root, &path, remote_local, orphans);
            continue;
        }
        if !remote_local.contains(&path) {
            orphans.push(path);
        }
    }
}

async fn download_repo_file(
    client: &Client,
    source: &RemoteRepoSource,
    repo_path: &str,
) -> Result<Vec<u8>, SyncError> {
    let url = source.raw_url(repo_path);
    let bytes = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(bytes.to_vec())
}

async fn download_repo_file_with_fallback(
    client: &Client,
    sources: &[RemoteRepoSource],
    primary: &RemoteRepoSource,
    repo_path: &str,
) -> Result<Vec<u8>, SyncError> {
    let mut try_order: Vec<&RemoteRepoSource> = vec![primary];
    for source in sources {
        if source != primary {
            try_order.push(source);
        }
    }

    let mut last_err: Option<SyncError> = None;
    for source in try_order {
        match download_repo_file(client, source, repo_path).await {
            Ok(data) => {
                if source != primary {
                    tracing::info!(
                        "{} 下载失败，已由 {} 获取 {repo_path}",
                        primary.label(),
                        source.label()
                    );
                }
                return Ok(data);
            }
            Err(e) => {
                tracing::warn!("{} 下载 {repo_path} 失败: {e}", source.label());
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| SyncError::Message(format!("无法下载 {repo_path}"))))
}

fn write_file_atomic(dest: &Path, data: &[u8]) -> Result<(), SyncError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    if dest.exists() {
        std::fs::remove_file(dest)?;
    }
    std::fs::rename(tmp, dest)?;
    Ok(())
}

fn repo_path_to_local(project_root: &Path, repo_path: &str) -> PathBuf {
    let rel = repo_path
        .strip_prefix(RESOURCES_PREFIX)
        .unwrap_or(repo_path);
    crate::paths::resources_dir(project_root).join(rel)
}

fn set_phase(status: &Arc<Mutex<ResourceSyncStatus>>, phase: ResourceSyncPhase) {
    if let Ok(mut s) = status.lock() {
        s.phase = phase;
    }
}

async fn sync_resources_inner(
    project_root: PathBuf,
    status: Arc<Mutex<ResourceSyncStatus>>,
) -> Result<(bool, String), SyncError> {
    let sources = remote_sources_from_env();
    let client = http_client()?;

    set_phase(&status, ResourceSyncPhase::Checking);

    let (active_source, remote_files) = resolve_remote_source(&client, &sources).await?;

    let to_update = collect_files_to_update(&project_root, &remote_files);
    let orphans = collect_orphan_local_files(&project_root, &remote_files);
    let update_count = to_update.len();
    let orphan_count = orphans.len();

    if to_update.is_empty() && orphans.is_empty() {
        tracing::info!(
            "资源增量检查完成，本地与远程一致（{} 个文件，源：{}）",
            remote_files.len(),
            active_source.label()
        );
        return Ok((false, format!("资源已是最新（{}）", active_source.label())));
    }

    tracing::info!(
        "资源差异（{}）：待更新 {} 个，待删除 {} 个（远程共 {} 个）",
        active_source.label(),
        update_count,
        orphan_count,
        remote_files.len()
    );

    let total = (update_count + orphan_count) as u32;
    let mut done = 0u32;

    for chunk in to_update.chunks(MAX_CONCURRENT) {
        let mut tasks = Vec::with_capacity(chunk.len());
        for remote in chunk {
            let client = client.clone();
            let sources = sources.clone();
            let primary = active_source.clone();
            let repo_path = remote.repo_path.clone();
            tasks.push(tokio::spawn(async move {
                download_repo_file_with_fallback(&client, &sources, &primary, &repo_path)
                    .await
                    .map(|data| (repo_path, data))
            }));
        }

        for task in tasks {
            let (repo_path, data) = task
                .await
                .map_err(|e| SyncError::Message(format!("下载任务失败: {e}")))?
                .map_err(|e| e)?;
            let dest = repo_path_to_local(&project_root, &repo_path);
            write_file_atomic(&dest, &data)?;
            done += 1;
            set_phase(
                &status,
                ResourceSyncPhase::Downloading { done, total },
            );
        }
    }

    for orphan in orphans {
        if orphan.is_file() {
            std::fs::remove_file(&orphan)?;
        }
        done += 1;
        set_phase(
            &status,
            ResourceSyncPhase::Downloading { done, total },
        );
    }

    prune_empty_dirs(&crate::paths::resources_dir(&project_root));

    let message = format!(
        "已增量更新 {} 个文件（{}）{}",
        update_count,
        active_source.label(),
        if orphan_count == 0 {
            String::new()
        } else {
            format!("，删除 {} 个过期文件", orphan_count)
        }
    );
    tracing::info!("{message}");
    Ok((true, message))
}

fn prune_empty_dirs(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            prune_empty_dirs(&path);
            if std::fs::read_dir(&path).is_ok_and(|mut it| it.next().is_none()) {
                let _ = std::fs::remove_dir(&path);
            }
        }
    }
}

/// 在后台线程中检查并同步资源；GUI 可通过 [`ResourceSyncHandle::status`] 读取进度。
pub fn start_background_sync(project_root: PathBuf) -> ResourceSyncHandle {
    let status = Arc::new(Mutex::new(ResourceSyncStatus::default()));

    if let Some(reason) = resource_sync_skip_reason() {
        set_phase(
            &status,
            ResourceSyncPhase::Skipped {
                message: reason.message(),
            },
        );
        return ResourceSyncHandle {
            status,
            thread: None,
        };
    }

    let status_thread = Arc::clone(&status);
    let thread = std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                set_phase(
                    &status_thread,
                    ResourceSyncPhase::Failed {
                        message: format!("无法创建同步运行时: {e}"),
                    },
                );
                return;
            }
        };

        match rt.block_on(sync_resources_inner(project_root, Arc::clone(&status_thread))) {
            Ok((updated, message)) => {
                set_phase(
                    &status_thread,
                    ResourceSyncPhase::Done { updated, message },
                );
            }
            Err(e) => {
                tracing::warn!("资源同步失败，将使用本地资源: {e}");
                set_phase(
                    &status_thread,
                    ResourceSyncPhase::Failed {
                        message: format!("同步失败，使用本地资源（{e}）"),
                    },
                );
            }
        }
    });

    ResourceSyncHandle {
        status,
        thread: Some(thread),
    }
}

pub fn phase_label(phase: &ResourceSyncPhase) -> Option<String> {
    match phase {
        ResourceSyncPhase::Idle => None,
        ResourceSyncPhase::Checking => Some("正在对比远程资源差异（优先 Gitee）…".into()),
        ResourceSyncPhase::Downloading { done, total } => {
            Some(format!("正在应用更新 {done}/{total}…"))
        }
        ResourceSyncPhase::Done { message, .. } => Some(message.clone()),
        ResourceSyncPhase::Failed { message } => Some(message.clone()),
        ResourceSyncPhase::Skipped { message } => Some(message.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_blob_sha_matches_git() {
        assert_eq!(
            git_blob_sha(b""),
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
        );
        assert_eq!(
            git_blob_sha(b"hello"),
            "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0"
        );
    }

    #[test]
    fn local_match_uses_blob_sha() {
        let dir = std::env::temp_dir().join(format!("hsr-sync-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("character.json");
        std::fs::write(&file, br#"{"id":1}"#).unwrap();

        let remote = RemoteResourceFile {
            repo_path: "resources/gacha/character.json".into(),
            blob_sha: git_blob_sha(br#"{"id":1}"#),
            size: Some(8),
        };
        assert!(local_file_matches_remote(&file, &remote));

        let remote_old = RemoteResourceFile {
            repo_path: "resources/gacha/character.json".into(),
            blob_sha: git_blob_sha(br#"{"id":2}"#),
            size: Some(8),
        };
        assert!(!local_file_matches_remote(&file, &remote_old));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gitee_source_urls() {
        let source = RemoteRepoSource {
            host: RemoteHost::Gitee,
            owner: "floating-illusion-language".into(),
            repo: "hsr-gacha-web".into(),
            branch: "main".into(),
        };
        assert_eq!(
            source.raw_url("resources/gacha/character.json"),
            "https://gitee.com/floating-illusion-language/hsr-gacha-web/raw/main/resources/gacha/character.json"
        );
        assert!(source.tree_api_url().contains("gitee.com/api/v5/repos"));
    }

    #[test]
    fn remote_sources_order_gitee_first() {
        let sources = remote_sources_from_env();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].host, RemoteHost::Gitee);
        assert_eq!(sources[0].repo, "hsr-gacha-web");
        assert_eq!(sources[1].host, RemoteHost::GitHub);
        assert_eq!(sources[1].repo, "hsr-gacha-web");
    }
}
