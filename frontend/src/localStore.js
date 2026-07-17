import { finalizeGachaResult, mergeGachaResult } from "./mergeGachaResult.js";
import { fetchUserDataBootstrap, persistUserData } from "./api.js";

function emptyState() {
  return { defaultUid: null, accounts: {}, history: [] };
}

/** 从服务端 userData 加载；失败时返回空状态 */
export async function loadLocalState() {
  try {
    const data = await fetchUserDataBootstrap();
    return {
      defaultUid: data?.defaultUid || null,
      accounts: data?.accounts || {},
      history: Array.isArray(data?.history) ? data.history : [],
    };
  } catch {
    return emptyState();
  }
}

export async function saveAccountResult(result, source, { normalizedHistory } = {}) {
  const state = await loadLocalState();
  const uid = `${result?.uid || ""}`.trim();
  if (!uid) return state;

  const previous = state.accounts[uid];
  let mergedResult = previous?.result ? mergeGachaResult(previous.result, result) : result;
  if (Array.isArray(normalizedHistory)) {
    mergedResult = finalizeGachaResult(mergedResult, normalizedHistory);
  }

  state.accounts[uid] = {
    uid,
    source,
    updatedAt: Date.now(),
    result: mergedResult,
  };
  state.defaultUid = uid;
  state.history.unshift({
    uid,
    source,
    time: Date.now(),
    total: mergedResult?.overview?.total || 0,
  });
  state.history = state.history.slice(0, 100);

  try {
    await persistUserData({
      defaultUid: state.defaultUid,
      history: state.history,
      accounts: state.accounts,
    });
  } catch {
    /* 持久化失败仍返回内存状态 */
  }
  return state;
}

export async function setDefaultUid(uid) {
  const state = await loadLocalState();
  if (state.accounts[uid]) {
    state.defaultUid = uid;
    try {
      await persistUserData({
        defaultUid: state.defaultUid,
        history: state.history,
        accounts: state.accounts,
      });
    } catch {
      /* ignore */
    }
  }
  return state;
}

const FIVE_STAR_DISPLAY_KEY = "hsr_gacha_five_star_display_v1";

const DEFAULT_FIVE_STAR_ORDER = ["11", "12", "21", "22", "1", "2"];
const DEFAULT_FIVE_STAR_VISIBLE = {
  "11": true,
  "12": true,
  "21": true,
  "22": true,
  "1": true,
  "2": true,
};

export function loadFiveStarDisplaySettings() {
  try {
    const raw = localStorage.getItem(FIVE_STAR_DISPLAY_KEY);
    if (!raw) {
      return {
        order: [...DEFAULT_FIVE_STAR_ORDER],
        visible: { ...DEFAULT_FIVE_STAR_VISIBLE },
        showFourStar: false,
      };
    }
    const parsed = JSON.parse(raw);
    const order = Array.isArray(parsed?.order) ? parsed.order.map((t) => `${t}`) : [...DEFAULT_FIVE_STAR_ORDER];
    const vis = { ...DEFAULT_FIVE_STAR_VISIBLE, ...(typeof parsed?.visible === "object" && parsed?.visible ? parsed.visible : {}) };
    for (const k of Object.keys(vis)) {
      if (vis[k] !== true && vis[k] !== false) vis[k] = true;
    }
    return {
      order,
      visible: vis,
      showFourStar: parsed?.showFourStar === true,
    };
  } catch {
    return {
      order: [...DEFAULT_FIVE_STAR_ORDER],
      visible: { ...DEFAULT_FIVE_STAR_VISIBLE },
      showFourStar: false,
    };
  }
}

export function saveFiveStarDisplaySettings(settings) {
  try {
    localStorage.setItem(FIVE_STAR_DISPLAY_KEY, JSON.stringify(settings));
  } catch {
    /* ignore */
  }
}

export function mergeOrderWithData(storedOrder, dataTypes) {
  const st = new Set(dataTypes.map((t) => `${t}`));
  const out = [];
  const used = new Set();
  for (const t of (storedOrder || []).map((x) => `${x}`)) {
    if (st.has(t) && !used.has(t)) {
      out.push(t);
      used.add(t);
    }
  }
  for (const t of dataTypes.map((x) => `${x}`)) {
    if (!used.has(t)) {
      out.push(t);
      used.add(t);
    }
  }
  return out;
}

export function mergeOrderWithRest(mergedReordered, previousOrder) {
  const m = new Set(mergedReordered);
  const rest = (previousOrder || []).map((t) => `${t}`).filter((t) => !m.has(t));
  return [...mergedReordered, ...rest];
}
