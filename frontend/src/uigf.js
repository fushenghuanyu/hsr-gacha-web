const FIVE_STAR_RANK = "5";

export function buildOverview(records) {
  const total = records.length;
  const fiveStarCount = records.filter((item) => `${item.rank_type}` === FIVE_STAR_RANK).length;
  const fourStarCount = records.filter((item) => `${item.rank_type}` === "4").length;
  return {
    total,
    five_star_count: fiveStarCount,
    four_star_count: fourStarCount,
  };
}

export function buildPoolSummary(records) {
  const grouped = new Map();
  for (const item of records) {
    const type = `${item.gacha_type || ""}`;
    if (!grouped.has(type)) {
      grouped.set(type, []);
    }
    grouped.get(type).push(item);
  }

  const rows = [];
  for (const [gachaType, itemsRaw] of grouped.entries()) {
    const items = [...itemsRaw].sort((a, b) => `${a.id}`.localeCompare(`${b.id}`));
    const fiveStars = items.filter((x) => `${x.rank_type}` === FIVE_STAR_RANK);
    const upCount = fiveStars.filter((x) => x.is_up === true).length;
    const nonUpCount = fiveStars.filter((x) => x.is_up === false).length;

    const fiveIndexes = [];
    for (let i = 0; i < items.length; i += 1) {
      if (`${items[i].rank_type}` === FIVE_STAR_RANK) {
        fiveIndexes.push(i);
      }
    }

    const pityGaps = [];
    let prev = -1;
    for (const idx of fiveIndexes) {
      pityGaps.push(idx - prev);
      prev = idx;
    }

    // 小保底不歪率 = (UP数 - 歪次数) / (总金数 - 歪次数)，仅统计非大保底出金时的 50/50 结果
    const smallPityDenom = fiveStars.length - nonUpCount;
    const smallPityNum = upCount - nonUpCount;
    const smallPityNoMissRate =
      smallPityDenom > 0 && smallPityNum >= 0
        ? Number(((smallPityNum / smallPityDenom) * 100).toFixed(2))
        : null;

    rows.push({
      gacha_type: gachaType,
      total: items.length,
      five_star_count: fiveStars.length,
      up_count: upCount,
      avg_up_pity: upCount ? Number((items.length / upCount).toFixed(2)) : null,
      up_rate: fiveStars.length ? Number(((upCount / fiveStars.length) * 100).toFixed(2)) : 0,
      non_up_count: nonUpCount,
      small_pity_no_miss_rate: smallPityNoMissRate,
      avg_five_star_pity: pityGaps.length
        ? Number((pityGaps.reduce((s, n) => s + n, 0) / pityGaps.length).toFixed(2))
        : 0,
      latest_five_star: fiveStars.length ? fiveStars[fiveStars.length - 1].name : null,
    });
  }

  rows.sort((a, b) => a.gacha_type.localeCompare(b.gacha_type));
  return rows;
}

export function parseUigfToResult(text) {
  const payload = JSON.parse(text);
  const hkrpg = payload?.hkrpg;
  if (!Array.isArray(hkrpg) || !hkrpg.length) {
    throw new Error("不是有效的 UIGF 文件：缺少 hkrpg 列表。");
  }

  const account = hkrpg[0];
  if (!Array.isArray(account?.list)) {
    throw new Error("不是有效的 UIGF 文件：缺少 hkrpg[0].list。");
  }

  const records = account.list
    .map((item) => ({
      id: item.id ?? "",
      item_id: item.item_id ?? "",
      uid: item.uid ?? account.uid ?? "",
      name: item.name ?? "",
      item_type: item.item_type ?? "",
      rank_type: item.rank_type ?? "",
      time: item.time ?? "",
      gacha_type: item.gacha_type ?? "",
      is_up: typeof item.is_up === "boolean" ? item.is_up : null,
    }))
    .sort((a, b) => `${a.id}`.localeCompare(`${b.id}`));

  return {
    uid: account.uid ?? records[0]?.uid ?? null,
    warp_url: null,
    overview: buildOverview(records),
    pool_summary: buildPoolSummary(records),
    records,
    logs: [
      "已从本地 UIGF 文件加载数据",
      `UIGF版本: ${payload?.info?.version || "未知"}`,
      `账号数量: ${hkrpg.length}`,
      `当前解析UID: ${account.uid || "未知"}`,
      `总记录数: ${records.length}`,
    ],
  };
}

