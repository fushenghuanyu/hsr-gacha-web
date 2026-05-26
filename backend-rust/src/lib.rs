//! HSR 抽卡历史 API：可被 `main` 与 `hsr-gacha-launcher` 共用。

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use bytes::Bytes;
use chrono::Local;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

mod analytics;
mod auto_url;
mod error;
mod hsr_client;
mod hsr_history;
mod paths;
mod user_data;

use analytics::{build_overview, build_pool_summary, sort_records_by_id};
use error::AppError;
use hsr_client::{fetch_all_records, resolve_uid_from_warp_url};

pub const APP_VERSION: &str = env!("APP_VERSION");

#[derive(Clone)]
pub(crate) struct AppState {
    project_root: PathBuf,
    user_data_dir: PathBuf,
}

pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .try_init();
}

/// 将 tracing 输出追加到 `log_path`（纯文本，无 ANSI）。供 GUI 启动器在无控制台时使用。
pub fn init_tracing_to_file(log_path: &Path) -> io::Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(log_path)?;
    let writer = Mutex::new(file);
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with_writer(writer)
        .try_init()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    Ok(())
}

/// 绑定 `addr`（如 `127.0.0.1:8000`）并启动 HTTP 服务，直至进程结束或出错。
pub async fn run_server(addr: &str) -> io::Result<()> {
    let project_root = paths::project_root();
    let user_data_dir = paths::user_data_dir(&project_root);
    let state = AppState {
        project_root: project_root.clone(),
        user_data_dir: user_data_dir.clone(),
    };
    let app = app_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("HSR Gacha API (Rust) 监听于 http://{addr}");
    tracing::info!("userData 目录: {}", user_data_dir.display());
    axum::serve(listener, app).await?;
    Ok(())
}

fn app_router(state: AppState) -> Router {
    let st = Arc::new(state);
    let icon = paths::resources_dir(&st.project_root).join("icon");
    let dist = st.project_root.join("dist");
    let index_html = dist.join("index.html");

    let mut r = Router::new()
        .route("/api/health", get(health))
        .route("/api/history", get(history))
        .route("/api/gacha/fetch", post(fetch_gacha))
        .route("/api/gacha/fetch-auto", post(fetch_gacha_auto))
        .route("/api/gacha/fetch-stream", post(fetch_gacha_stream))
        .route("/api/gacha/fetch-auto-stream", post(fetch_gacha_auto_stream))
        .route("/api/user-data/bootstrap", get(user_data_bootstrap))
        .route("/api/user-data/save", post(user_data_save));

    if icon.is_dir() {
        r = r.nest_service("/icon", ServeDir::new(icon));
    }

    if dist.is_dir() && index_html.is_file() {
        let spa = ServeDir::new(dist.clone())
            .append_index_html_on_directories(true)
            .not_found_service(ServeFile::new(index_html));
        r = r.fallback_service(spa);
        tracing::info!("已挂载前端静态资源: {}", dist.display());
    } else {
        tracing::warn!(
            "未找到前端构建目录（期望 {}），请在 frontend 目录执行 npm run build（输出到项目根 dist/）",
            dist.display()
        );
    }

    r.with_state(st)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

async fn health() -> axum::Json<Value> {
    axum::Json(json!({ "status": "ok", "version": APP_VERSION }))
}

async fn history(State(st): State<Arc<AppState>>) -> Result<axum::Json<Value>, AppError> {
    let rows = hsr_history::load_history_rows(&st.project_root).map_err(|e| {
        if e.contains("未找到") {
            AppError::NotFound {
                message: e,
                logs: vec![],
            }
        } else {
            AppError::Server {
                message: e,
                logs: vec![],
            }
        }
    })?;
    Ok(axum::Json(Value::Array(rows)))
}

async fn user_data_bootstrap(State(st): State<Arc<AppState>>) -> Result<axum::Json<Value>, AppError> {
    let v = user_data::load_bootstrap(&st.user_data_dir).map_err(|e| AppError::Server {
        message: e,
        logs: vec![],
    })?;
    Ok(axum::Json(v))
}

async fn user_data_save(
    State(st): State<Arc<AppState>>,
    axum::Json(body): axum::Json<user_data::PersistBody>,
) -> Result<axum::Json<Value>, AppError> {
    user_data::persist(&st.user_data_dir, body).map_err(|e| AppError::Server {
        message: e,
        logs: vec![],
    })?;
    Ok(axum::Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct FetchRequest {
    #[serde(default = "default_fetch_full_history")]
    fetch_full_history: bool,
    warp_url: String,
}

#[derive(Deserialize)]
struct FetchAutoRequest {
    #[serde(default = "default_fetch_full_history")]
    fetch_full_history: bool,
}

fn default_fetch_full_history() -> bool {
    true
}

fn ts_log_line(msg: &str) -> String {
    format!("{} {msg}", Local::now().format("%Y-%m-%d %H:%M:%S"))
}

fn ndjson_chunk(obj: &Value) -> Bytes {
    let mut s = serde_json::to_string(obj).unwrap_or_else(|_| "{}".into());
    s.push('\n');
    Bytes::from(s)
}

fn records_to_response_value(
    warp_url: String,
    mut records: Vec<Value>,
    logs: Vec<String>,
) -> Result<Value, AppError> {
    sort_records_by_id(&mut records);
    let uid = records.first().and_then(|r| {
        r.get("uid").and_then(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.as_i64().map(|i| i.to_string()))
        })
    });
    let overview = build_overview(&records);
    let pool_summary = build_pool_summary(&records);
    let overview_v = serde_json::to_value(&overview).map_err(|e| AppError::Server {
        message: format!("序列化 overview: {e}"),
        logs: vec![],
    })?;
    let pool_v = serde_json::to_value(&pool_summary).map_err(|e| AppError::Server {
        message: format!("序列化 pool_summary: {e}"),
        logs: vec![],
    })?;
    Ok(json!({
        "uid": uid,
        "warp_url": warp_url,
        "overview": overview_v,
        "pool_summary": pool_v,
        "records": records,
        "logs": logs,
    }))
}

async fn fetch_gacha_core(
    user_data_dir: &Path,
    warp_url: String,
    fetch_full_history: bool,
    mut logs: Vec<String>,
) -> Result<Value, AppError> {
    let mut hsr_lines: Vec<String> = Vec::new();
    let local_latest_ids = if fetch_full_history {
        None
    } else {
        match resolve_uid_from_warp_url(&warp_url).await {
            Ok(Some(uid)) => match user_data::latest_ids_by_pool(user_data_dir, &uid) {
                Ok(m) if !m.is_empty() => {
                    logs.push(ts_log_line(&format!(
                        "增量模式：UID {uid} 已命中本地缓存，按卡池命中后提前停止翻页"
                    )));
                    Some(m)
                }
                Ok(_) => None,
                Err(e) => {
                    logs.push(ts_log_line(&format!("读取本地缓存失败，回退全量：{e}")));
                    None
                }
            },
            Ok(None) => {
                logs.push(ts_log_line("未能预解析 UID，回退全量拉取"));
                None
            }
            Err(e) => {
                logs.push(ts_log_line(&format!("预解析 UID 失败，回退全量：{e}")));
                None
            }
        }
    };
    let fetched = match fetch_all_records(
        &warp_url,
        fetch_full_history,
        local_latest_ids.as_ref(),
        |line| hsr_lines.push(line),
    )
    .await
    {
        Ok(f) => f,
        Err(e) => {
            logs.push(ts_log_line(&format!("错误: {e}")));
            return Err(AppError::BadRequest { message: e, logs });
        }
    };
    for line in hsr_lines {
        logs.push(ts_log_line(&line));
    }
    records_to_response_value(warp_url, fetched.records, logs)
}

async fn fetch_gacha(
    State(_st): State<Arc<AppState>>,
    axum::Json(body): axum::Json<FetchRequest>,
) -> Result<axum::Json<Value>, AppError> {
    let warp_url = body.warp_url.trim().to_string();
    if warp_url.len() < 10 {
        return Err(AppError::BadRequest {
            message: "warp_url 过短".into(),
            logs: vec![],
        });
    }
    let logs: Vec<String> = vec![ts_log_line("开始执行手动链接拉取")];
    let v = fetch_gacha_core(
        &_st.user_data_dir,
        warp_url,
        body.fetch_full_history,
        logs,
    )
    .await?;
    Ok(axum::Json(v))
}

async fn fetch_gacha_auto(
    State(_st): State<Arc<AppState>>,
    axum::Json(body): axum::Json<FetchAutoRequest>,
) -> Result<axum::Json<Value>, AppError> {
    let mut logs: Vec<String> = vec![ts_log_line("开始执行自动日志读取")];
    let warp = match auto_url::get_warp_url_from_local_logs(|m| {
        logs.push(ts_log_line(m));
    }) {
        Ok(u) => u,
        Err(e) => {
            logs.push(ts_log_line(&format!("错误: {e}")));
            return Err(AppError::BadRequest { message: e, logs });
        }
    };
    logs.push(ts_log_line("自动获取链接成功，开始在线拉取"));
    let v = fetch_gacha_core(
        &_st.user_data_dir,
        warp,
        body.fetch_full_history,
        logs,
    )
    .await?;
    Ok(axum::Json(v))
}

/// NDJSON 流：`{"type":"log","line":"..."}` 逐行，最后 `{"type":"complete","data":{...}}` 或 `{"type":"error",...}`。
fn ndjson_fetch_stream(
    user_data_dir: PathBuf,
    warp_url: String,
    fetch_full_history: bool,
    initial_log_lines: Vec<String>,
) -> impl futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    async_stream::stream! {
        for line in initial_log_lines {
            yield Ok(ndjson_chunk(&json!({ "type": "log", "line": line })));
        }

        let (log_tx, mut log_rx) = mpsc::unbounded_channel::<String>();
        let log_tx_fetch = log_tx.clone();
        let w = warp_url.clone();
        let ud = user_data_dir.clone();
        let handle = tokio::spawn(async move {
            let local_latest_ids = if fetch_full_history {
                None
            } else {
                match resolve_uid_from_warp_url(&w).await {
                    Ok(Some(uid)) => user_data::latest_ids_by_pool(&ud, &uid).ok().filter(|m| !m.is_empty()),
                    _ => None,
                }
            };
            fetch_all_records(&w, fetch_full_history, local_latest_ids.as_ref(), |msg| {
                let _ = log_tx_fetch.send(msg);
            })
            .await
        });
        drop(log_tx);

        while let Some(msg) = log_rx.recv().await {
            let line = ts_log_line(&msg);
            yield Ok(ndjson_chunk(&json!({ "type": "log", "line": line })));
        }

        let fetch_result = match handle.await {
            Ok(Ok(f)) => Ok(f.records),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(format!("拉取任务异常: {e}")),
        };

        match fetch_result {
            Ok(records) => match records_to_response_value(warp_url, records, vec![]) {
                Ok(data) => {
                    yield Ok(ndjson_chunk(&json!({ "type": "complete", "data": data })));
                }
                Err(e) => {
                    let (msg, logs) = match e {
                        AppError::BadRequest { message, logs } => (message, logs),
                        AppError::Server { message, logs } => (message, logs),
                        AppError::NotFound { message, logs } => (message, logs),
                    };
                    yield Ok(ndjson_chunk(&json!({
                        "type": "error",
                        "detail": { "message": msg, "logs": logs }
                    })));
                }
            },
            Err(message) => {
                let line = ts_log_line(&format!("错误: {message}"));
                yield Ok(ndjson_chunk(&json!({ "type": "log", "line": line })));
                yield Ok(ndjson_chunk(&json!({
                    "type": "error",
                    "detail": { "message": message, "logs": [] }
                })));
            }
        }
    }
}

fn ndjson_stream_response(
    user_data_dir: PathBuf,
    warp_url: String,
    fetch_full_history: bool,
    initial_log_lines: Vec<String>,
) -> Response {
    let stream = ndjson_fetch_stream(user_data_dir, warp_url, fetch_full_history, initial_log_lines);
    let body = Body::from_stream(stream);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(body)
        .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "stream build failed").into_response())
}

async fn fetch_gacha_stream(
    State(_st): State<Arc<AppState>>,
    axum::Json(body): axum::Json<FetchRequest>,
) -> Result<Response, AppError> {
    let warp_url = body.warp_url.trim().to_string();
    if warp_url.len() < 10 {
        return Err(AppError::BadRequest {
            message: "warp_url 过短".into(),
            logs: vec![],
        });
    }
    let initial = vec![ts_log_line("开始执行手动链接拉取")];
    Ok(ndjson_stream_response(
        _st.user_data_dir.clone(),
        warp_url,
        body.fetch_full_history,
        initial,
    ))
}

async fn fetch_gacha_auto_stream(
    State(_st): State<Arc<AppState>>,
    axum::Json(body): axum::Json<FetchAutoRequest>,
) -> Response {
    let mut initial: Vec<String> = vec![ts_log_line("开始执行自动日志读取")];
    let warp = match auto_url::get_warp_url_from_local_logs(|m| {
        initial.push(ts_log_line(m));
    }) {
        Ok(u) => u,
        Err(e) => {
            initial.push(ts_log_line(&format!("错误: {e}")));
            let stream = async_stream::stream! {
                for line in initial {
                    yield Ok::<Bytes, io::Error>(ndjson_chunk(&json!({ "type": "log", "line": line })));
                }
                yield Ok::<Bytes, io::Error>(ndjson_chunk(&json!({
                    "type": "error",
                    "detail": { "message": e, "logs": [] }
                })));
            };
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/x-ndjson; charset=utf-8")
                .header(header::CACHE_CONTROL, "no-store")
                .body(Body::from_stream(stream))
                .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR).into_response());
        }
    };
    initial.push(ts_log_line("自动获取链接成功，开始在线拉取"));
    ndjson_stream_response(
        _st.user_data_dir.clone(),
        warp,
        body.fetch_full_history,
        initial,
    )
}
