import { annotateUpByHistory } from "./poolHistory.js";
import { buildOverview, buildPoolSummary } from "./uigf.js";

/** 对完整记录列表重新标注 UP 并生成 overview / pool_summary */
export function finalizeGachaResult(result, normalizedHistory) {
  if (!result) return result;
  const records = annotateUpByHistory(result.records || [], normalizedHistory);
  return {
    ...result,
    records,
    overview: buildOverview(records),
    pool_summary: buildPoolSummary(records),
  };
}

/**
 * 与 star-rail-warp-export mergeData 中 mergeList 一致：先拼接 [本次, 本地]，
 * 同 id 仅保留先出现的（本次优先），再按 id 升序排序。
 * 对缺失 id 的条目使用 time+name+gacha 作为辅助去重键，避免空 id 条被误合并为一条。
 */
function recordDedupeKey(item) {
  const id = item?.id;
  if (id != null && `${id}`.length > 0) {
    return `id:${id}`;
  }
  return `fallback:${item?.time ?? ""}|${item?.name ?? ""}|${item?.gacha_type ?? ""}`;
}

function compareRecordId(m, n) {
  const sm = m?.id;
  const sn = n?.id;
  if (sm != null && `${sm}`.length > 0 && sn != null && `${sn}`.length > 0) {
    try {
      const num = BigInt(String(sm)) - BigInt(String(sn));
      if (num > 0n) return 1;
      if (num < 0n) return -1;
      return 0;
    } catch {
      /* fall through */
    }
  }
  return String(sm ?? "").localeCompare(String(sn ?? ""));
}

export function mergeRecordLists(local, origin) {
  const a = local || [];
  const b = origin || [];
  if (!a.length) return b.slice();
  if (!b.length) return a.slice();

  const list = [...b, ...a];
  const out = [];
  const seen = new Set();
  for (const item of list) {
    const k = recordDedupeKey(item);
    if (!seen.has(k)) {
      out.push(item);
    }
    seen.add(k);
  }
  return out.sort(compareRecordId);
}

/**
 * 将本地已缓存的 result 与本次拉取/导入的 result 按 UID 合并（同 UID 累加、去重）。
 * UID 不一致时返回本次数据，行为与 references 中 mergeData 一致。
 */
export function mergeGachaResult(local, incoming) {
  if (!local || !incoming) return incoming;
  const localUid = local.uid != null ? `${local.uid}`.trim() : "";
  const incomingUid = incoming.uid != null ? `${incoming.uid}`.trim() : "";
  if (!localUid || localUid !== incomingUid) {
    return incoming;
  }

  const localRecords = Array.isArray(local.records) ? local.records : [];
  const incomingRecords = Array.isArray(incoming.records) ? incoming.records : [];
  const mergedRecords = mergeRecordLists(localRecords, incomingRecords);
  const prevN = localRecords.length;
  const curN = incomingRecords.length;
  const mergedN = mergedRecords.length;

  const baseLogs = Array.isArray(incoming.logs) ? incoming.logs : [];
  const mergeLog =
    prevN > 0 && curN > 0
      ? `[合并] 本地 ${prevN} 条 + 本次 ${curN} 条 → 去重后 ${mergedN} 条（重复 ${prevN + curN - mergedN} 条）`
      : null;

  return {
    ...incoming,
    uid: incoming.uid,
    warp_url: incoming.warp_url != null && incoming.warp_url !== "" ? incoming.warp_url : local.warp_url,
    records: mergedRecords,
    logs: mergeLog ? [mergeLog, ...baseLogs] : baseLogs,
  };
}
