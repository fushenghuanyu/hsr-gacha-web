use indexmap::IndexMap;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;
use url::Url;

const DEFAULT_TYPES: [&str; 4] = ["11", "12", "1", "2"];
const RATE_LIMIT_MESSAGE: &str = "visit too frequently";

/// 与前端 `App.jsx` 中 `typeNameMap` 一致，用于日志展示
fn gacha_type_bracket_name(gacha_type: &str) -> String {
    let name = match gacha_type {
        "11" => "角色活动跃迁",
        "12" => "光锥活动跃迁",
        "21" => "Fate联动卡池（角色）",
        "22" => "Fate联动卡池（光锥）",
        "1" => "常驻跃迁",
        "2" => "新手跃迁",
        _ => gacha_type,
    };
    format!("【{name}】")
}

fn detect_api_domain(host: &str) -> &'static str {
    if host.contains("webstatic-sea")
        || host.contains("hkrpg-api-os")
        || host.contains("api-os-takumi")
        || host.contains("hoyoverse.com")
    {
        "https://public-operation-hkrpg-sg.hoyoverse.com"
    } else {
        "https://public-operation-hkrpg.mihoyo.com"
    }
}

/// 与 Python `_query_from_warp_url` 一致。
pub fn query_from_warp_url(warp_url: &str) -> Result<(String, IndexMap<String, String>), String> {
    let parsed = Url::parse(warp_url).map_err(|e| e.to_string())?;
    let host = parsed.host_str().unwrap_or("").to_string();

    let mut query: IndexMap<String, String> = IndexMap::new();
    for (k, v) in parsed.query_pairs() {
        query.insert(k.into_owned(), v.into_owned());
    }

    let auth = query.get("authkey").cloned().unwrap_or_default();
    if auth.is_empty() {
        return Err("链接中缺少 authkey，请重新从游戏跃迁历史页面复制。".to_string());
    }
    for k in ["page", "size", "gacha_type", "end_id"] {
        query.shift_remove(k);
    }
    Ok((host, query))
}

async fn request_with_retry<F>(
    client: &Client,
    base_url: &str,
    params: &IndexMap<String, String>,
    gacha_type: &str,
    log_line: &mut F,
) -> Result<Value, String>
where
    F: FnMut(String),
{
    for attempt in 0..4 {
        let q: Vec<(&str, &str)> = params
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let response = client
            .get(base_url)
            .query(&q)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let response = response.error_for_status().map_err(|e| e.to_string())?;
        let payload: Value = response.json().await.map_err(|e| e.to_string())?;

        let retcode = payload
            .get("retcode")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        let message = payload
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if retcode == 0 {
            return Ok(payload);
        }
        if message == "authkey timeout" {
            return Err("authkey 已过期，请在游戏内重新打开跃迁历史后再复制链接。".to_string());
        }
        if message.contains(RATE_LIMIT_MESSAGE) && attempt < 3 {
            let g = gacha_type_bracket_name(gacha_type);
            log_line(format!("卡池{g} 请求过频，第 {} 次退避重试", attempt + 1));
            sleep(Duration::from_millis(1000 * (1u64 << attempt))).await;
            continue;
        }
        return Err(format!(
            "接口返回错误: {}",
            if message.is_empty() {
                "接口请求失败"
            } else {
                message
            }
        ));
    }
    Err("接口请求失败".to_string())
}

fn record_value_from_item(item: &Value) -> Value {
    json!({
        "id": item.get("id"),
        "gacha_id": item.get("gacha_id"),
        "item_id": item.get("item_id"),
        "uid": item.get("uid"),
        "name": item.get("name"),
        "item_type": item.get("item_type"),
        "rank_type": item.get("rank_type"),
        "time": item.get("time"),
        "gacha_type": item.get("gacha_type"),
    })
}

async fn fetch_one_pool<F>(
    client: &Client,
    base_url: &str,
    query: &IndexMap<String, String>,
    gacha_type: &str,
    stop_when_hit_local_id: Option<&str>,
    log_line: &mut F,
) -> Result<Vec<Value>, String>
where
    F: FnMut(String),
{
    let mut all_items: Vec<Value> = Vec::new();
    let g = gacha_type_bracket_name(gacha_type);
    log_line(format!("开始拉取卡池{g}"));
    let mut page: i64 = 1;
    let mut end_id = String::from("0");
    loop {
        log_line(format!("卡池{g} 第 {page} 页请求中"));
        let mut params = query.clone();
        params.insert("gacha_type".into(), gacha_type.to_string());
        params.insert("page".into(), page.to_string());
        params.insert("size".into(), "20".into());
        params.insert("end_id".into(), end_id.clone());

        let payload = request_with_retry(client, base_url, &params, gacha_type, log_line).await?;
        let data = payload.get("data").unwrap_or(&Value::Null);
        let list = data
            .get("list")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if list.is_empty() {
            log_line(format!("卡池{g} 拉取完成，共 {} 条", all_items.len()));
            break;
        }
        for item in &list {
            all_items.push(record_value_from_item(item));
        }
        if let Some(local_latest_id) = stop_when_hit_local_id {
            if list.iter().any(|item| {
                item.get("id")
                    .map(|v| v.to_string().trim_matches('"').to_string())
                    .is_some_and(|id| id == local_latest_id)
            }) {
                log_line(format!(
                    "卡池{g} 命中本地已存在记录（id={local_latest_id}），停止继续翻页"
                ));
                log_line(format!("卡池{g} 增量拉取完成，共 {} 条", all_items.len()));
                break;
            }
        }
        let last = list.last();
        end_id = last
            .and_then(|x| x.get("id"))
            .map(|v| v.to_string().trim_matches('"').to_string())
            .unwrap_or_else(|| "0".to_string());
        page += 1;
        sleep(Duration::from_millis(600)).await;
    }
    Ok(all_items)
}

/// 与 Python `fetch_all_records` 的拉取/日志部分一致。排序在 [`crate::analytics::sort_records_by_id`] 中完成。
pub struct FetchedGacha {
    pub records: Vec<Value>,
}

pub async fn resolve_uid_from_warp_url(warp_url: &str) -> Result<Option<String>, String> {
    let (host, query) = query_from_warp_url(warp_url)?;
    let api_domain = detect_api_domain(&host);
    let base_url = format!("{api_domain}/common/gacha_record/api/getGachaLog");
    let client = Client::new();

    let mut params = query.clone();
    params.insert("gacha_type".into(), "1".into());
    params.insert("page".into(), "1".into());
    params.insert("size".into(), "5".into());
    params.insert("end_id".into(), "0".into());

    let mut noop = |_msg: String| {};
    let payload = request_with_retry(&client, &base_url, &params, "1", &mut noop).await?;
    let list = payload
        .get("data")
        .and_then(|v| v.get("list"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let uid = list.first().and_then(|x| {
        x.get("uid")
            .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|i| i.to_string())))
    });
    Ok(uid)
}

/// `log_line` 在每一页请求前后等节点同步调用，便于流式输出日志。
pub async fn fetch_all_records<F>(
    warp_url: &str,
    fetch_full_history: bool,
    local_latest_ids_by_pool: Option<&HashMap<String, String>>,
    mut log_line: F,
) -> Result<FetchedGacha, String>
where
    F: FnMut(String) + Send,
{
    let (host, query) = query_from_warp_url(warp_url)?;
    let api_domain = detect_api_domain(&host);
    let base_url = format!("{api_domain}/common/gacha_record/api/getGachaLog");
    log_line(format!("已识别接口域名: {api_domain}"));

    let client = Client::new();
    let mut pools: Vec<Vec<Value>> = Vec::new();
    for gacha_type in DEFAULT_TYPES {
        let stop_id = if fetch_full_history {
            None
        } else {
            local_latest_ids_by_pool.and_then(|m| m.get(gacha_type)).map(String::as_str)
        };
        let pool_items = fetch_one_pool(
            &client,
            &base_url,
            &query,
            gacha_type,
            stop_id,
            &mut log_line,
        )
        .await?;
        pools.push(pool_items);
        sleep(Duration::from_millis(800)).await;
    }

    let mut records: Vec<Value> = Vec::new();
    for p in pools {
        records.extend(p);
    }
    Ok(FetchedGacha { records })
}
