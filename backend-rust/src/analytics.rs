use serde::Serialize;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;

const FIVE_STAR_RANK: &str = "5";
const FOUR_STAR_RANK: &str = "4";

static UP_ITEM_TYPE: [&str; 2] = ["角色", "光锥"];

/// 与 Python `str(x.get("rank_type"))` 用于比较时等价的字符串形式
fn rank_type_str(x: &Value) -> String {
    match x.get("rank_type") {
        None => String::new(),
        Some(v) if v.is_null() => String::new(),
        Some(v) if v.is_string() => v.as_str().unwrap_or("").to_string(),
        Some(v) if v.is_i64() || v.is_u64() => v
            .as_i64()
            .or_else(|| v.as_u64().map(|u| u as i64))
            .map(|n| n.to_string())
            .unwrap_or_else(|| v.to_string()),
        Some(v) if v.is_f64() => v.as_f64().map(|f| f as i64).map(|i| i.to_string()).unwrap_or_else(|| v.to_string()),
        Some(v) => v.to_string(),
    }
}

fn is_five_star(x: &Value) -> bool {
    rank_type_str(x) == FIVE_STAR_RANK
}

fn is_four_star(x: &Value) -> bool {
    rank_type_str(x) == FOUR_STAR_RANK
}

fn is_up_type(x: &Value) -> bool {
    x.get("item_type")
        .and_then(|v| v.as_str())
        .is_some_and(|t| UP_ITEM_TYPE.contains(&t))
}

/// 与全量 `records` 的排序键 `x.get("id", "0")` 的字符串形式一致
fn id_sort_key(x: &Value) -> String {
    match x.get("id") {
        None => "0".to_string(),
        Some(v) if v.is_string() => v.as_str().unwrap_or("0").to_string(),
        Some(v) if v.is_i64() || v.is_u64() => v
            .as_i64()
            .or_else(|| v.as_u64().map(|u| u as i64))
            .map(|n| n.to_string())
            .unwrap_or_else(|| "0".to_string()),
        Some(v) if v.is_f64() => v.as_f64().map(|f| f as i64).map(|i| i.to_string()).unwrap_or_else(|| "0".to_string()),
        Some(v) => v.to_string(),
    }
}

fn compare_id(a: &Value, b: &Value) -> Ordering {
    id_sort_key(a).cmp(&id_sort_key(b))
}

#[derive(Serialize)]
pub struct PoolSummary {
    pub gacha_type: String,
    pub total: usize,
    pub five_star_count: usize,
    pub up_count: usize,
    pub up_rate: f64,
    pub avg_five_star_pity: f64,
    pub latest_five_star: Option<String>,
}

#[derive(Serialize)]
pub struct Overview {
    pub total: usize,
    pub five_star_count: usize,
    pub four_star_count: usize,
}

/// 与 Python `build_pool_summary` 对齐。
pub fn build_pool_summary(records: &[Value]) -> Vec<PoolSummary> {
    let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();
    for item in records {
        if let Some(gt) = item.get("gacha_type").and_then(|v| v.as_str()) {
            grouped
                .entry(gt.to_string())
                .or_default()
                .push(item.clone());
        }
    }

    let mut result = Vec::new();
    for (gacha_type, mut items) in grouped {
        items.sort_by(compare_id);

        let five_star_indexes: Vec<usize> = items
            .iter()
            .enumerate()
            .filter_map(|(idx, x)| if is_five_star(x) { Some(idx) } else { None })
            .collect();

        let mut pity_gaps: Vec<usize> = Vec::new();
        let mut prev: isize = -1;
        for idx in five_star_indexes {
            pity_gaps.push((idx as isize - prev) as usize);
            prev = idx as isize;
        }

        let five_star_items: Vec<&Value> = items.iter().filter(|x| is_five_star(x)).collect();
        let up_count = five_star_items
            .iter()
            .filter(|x| is_up_type(x))
            .count();
        let total = items.len();
        let five_star_count = five_star_items.len();

        let up_rate = if five_star_count > 0 {
            ((up_count as f64 / five_star_count as f64) * 100.0 * 100.0).round() / 100.0
        } else {
            0.0
        };
        let avg_five_star_pity = if !pity_gaps.is_empty() {
            let s: f64 = pity_gaps.iter().map(|&x| x as f64).sum();
            let n = pity_gaps.len() as f64;
            ((s / n) * 100.0).round() / 100.0
        } else {
            0.0
        };
        let latest_five_star = five_star_items
            .last()
            .and_then(|x| x.get("name").and_then(|v| v.as_str().map(String::from)));

        result.push(PoolSummary {
            gacha_type,
            total,
            five_star_count,
            up_count,
            up_rate,
            avg_five_star_pity,
            latest_five_star,
        });
    }

    result.sort_by(|a, b| a.gacha_type.cmp(&b.gacha_type));
    result
}

pub fn build_overview(records: &[Value]) -> Overview {
    let total = records.len();
    let five_star_count = records.iter().filter(|x| is_five_star(x)).count();
    let four_star_count = records.iter().filter(|x| is_four_star(x)).count();
    Overview {
        total,
        five_star_count,
        four_star_count,
    }
}

/// 全量拉取后按 `id` 字符串排序，与 Python `x.get("id", "0")` 比较行为接近。
pub fn sort_records_by_id(records: &mut [Value]) {
    records.sort_by(|a, b| id_sort_key(a).cmp(&id_sort_key(b)));
}
