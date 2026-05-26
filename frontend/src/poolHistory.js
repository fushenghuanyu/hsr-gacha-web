const DATETIME_RE = /^\d{4}\/\d{2}\/\d{2}\s+\d{2}:\d{2}$/;



function parseDateTime(text) {

  const v = `${text || ""}`.trim();

  if (!DATETIME_RE.test(v)) return null;

  const iso = v.replace(/\//g, "-").replace(" ", "T") + ":00";

  const d = new Date(iso);

  return Number.isNaN(d.getTime()) ? null : d;

}



function splitTimer(timer) {

  const text = `${timer || ""}`;

  const idx = text.indexOf("~");

  if (idx < 0) return [text.trim(), ""];

  return [text.slice(0, idx).trim(), text.slice(idx + 1).trim()];

}



export function normalizeHistoryRows(rows) {

  const safeRows = Array.isArray(rows) ? rows : [];

  return safeRows.map((row) => {

    const [left, right] = splitTimer(row.timer);

    const startAt = parseDateTime(left);

    const endAt = right === "长期" || right === "长期开放" ? null : parseDateTime(right);

    return {

      title: row.title || "",

      type: row.type || "",

      version: row.version || "",

      s: row.s || "",

      a: Array.isArray(row.a) ? row.a : [],

      startAt,

      endAt,

    };

  });

}



function parseDrawTime(text) {

  const raw = `${text || ""}`.trim();

  if (!raw) return null;

  const iso = raw.replace(" ", "T");

  const d = new Date(iso);

  return Number.isNaN(d.getTime()) ? null : d;

}



export function annotateUpByHistory(records, normalizedHistory) {

  const rows = Array.isArray(normalizedHistory) ? normalizedHistory : [];

  return (records || []).map((item) => {

    const gachaType = `${item?.gacha_type || ""}`;

    // 11/12 为当前游戏内活动 ID；21/22 为部分导出版本/区服使用的同机制卡池 ID

    const targetType =

      gachaType === "11" || gachaType === "21"

        ? "角色"

        : gachaType === "12" || gachaType === "22"

          ? "武器"

          : null;

    if (!targetType) return { ...item, is_up: null };



    const drawTime = parseDrawTime(item?.time);

    if (!drawTime) return { ...item, is_up: null };



    const candidates = rows.filter((row) => {

      if (row.type !== targetType) return false;

      if (row.startAt && drawTime < row.startAt) return false;

      if (row.endAt && drawTime > row.endAt) return false;

      return true;

    });

    if (!candidates.length) return { ...item, is_up: null };



    const rankType = `${item?.rank_type || ""}`;

    const name = item?.name || "";

    let isUp = false;

    if (rankType === "5") {

      isUp = candidates.some((c) => name === c.s);

    } else if (rankType === "4") {

      isUp = candidates.some((c) => c.a.includes(name));

    }

    return { ...item, is_up: isUp };

  });

}

