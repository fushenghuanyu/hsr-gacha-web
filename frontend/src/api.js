const API_BASE = "http://127.0.0.1:8000";

async function handleResponse(resp) {
  const data = await resp.json();
  if (!resp.ok) {
    const detail = data?.detail;
    const message = typeof detail === "string" ? detail : detail?.message || "请求失败";
    const error = new Error(message);
    error.logs = Array.isArray(detail?.logs) ? detail.logs : [];
    throw error;
  }
  return data;
}

/** 解析 NDJSON 流：逐行 `onLogLine(完整一行)`，结束时返回 complete.data */
export async function consumeGachaNdjsonStream(resp, onLogLine) {
  const reader = resp.body?.getReader();
  if (!reader) {
    throw new Error("响应不支持流式读取");
  }
  const decoder = new TextDecoder();
  let buffer = "";
  let completeData = null;

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    for (;;) {
      const nl = buffer.indexOf("\n");
      if (nl < 0) break;
      const raw = buffer.slice(0, nl).trim();
      buffer = buffer.slice(nl + 1);
      if (!raw) continue;
      let obj;
      try {
        obj = JSON.parse(raw);
      } catch {
        continue;
      }
      if (obj.type === "log" && obj.line != null) {
        onLogLine(String(obj.line));
      }
      if (obj.type === "complete" && obj.data != null) {
        completeData = obj.data;
      }
      if (obj.type === "error") {
        const d = obj.detail || {};
        const err = new Error(d.message || "请求失败");
        err.logs = Array.isArray(d.logs) ? d.logs : [];
        throw err;
      }
    }
  }

  if (!completeData) {
    throw new Error("流式响应未返回完整数据");
  }
  return completeData;
}

async function postNdjsonStream(path, body, onLogLine) {
  const resp = await fetch(`${API_BASE}${path}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "application/x-ndjson, application/json",
    },
    body: JSON.stringify(body || {}),
  });
  if (!resp.ok) {
    const ct = resp.headers.get("content-type") || "";
    if (ct.includes("application/json")) {
      return handleResponse(resp);
    }
    const text = await resp.text();
    throw new Error(text || `HTTP ${resp.status}`);
  }
  const ct = resp.headers.get("content-type") || "";
  if (!ct.includes("ndjson") && !ct.includes("x-ndjson")) {
    return handleResponse(resp);
  }
  return consumeGachaNdjsonStream(resp, onLogLine);
}

export async function fetchGachaData(warpUrl, fetchFullHistory, onServerLogLine) {
  const resp = await fetch(`${API_BASE}/api/gacha/fetch-stream`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "application/x-ndjson, application/json",
    },
    body: JSON.stringify({
      warp_url: warpUrl,
      fetch_full_history: fetchFullHistory,
    }),
  });
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}));
    const detail = data?.detail;
    const message = typeof detail === "string" ? detail : detail?.message || "请求失败";
    const error = new Error(message);
    error.logs = Array.isArray(detail?.logs) ? detail.logs : [];
    throw error;
  }
  const ct = resp.headers.get("content-type") || "";
  if (!ct.includes("ndjson") && !ct.includes("x-ndjson")) {
    return handleResponse(resp);
  }
  return consumeGachaNdjsonStream(resp, onServerLogLine || (() => {}));
}

export async function fetchGachaDataAuto(fetchFullHistory, onServerLogLine) {
  return postNdjsonStream(
    "/api/gacha/fetch-auto-stream",
    { fetch_full_history: fetchFullHistory },
    onServerLogLine || (() => {}),
  );
}

export async function fetchHistoryData() {
  const resp = await fetch(`${API_BASE}/api/history`);
  return handleResponse(resp);
}

export async function fetchUserDataBootstrap() {
  const resp = await fetch(`${API_BASE}/api/user-data/bootstrap`);
  return handleResponse(resp);
}

export async function persistUserData(payload) {
  const resp = await fetch(`${API_BASE}/api/user-data/save`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  return handleResponse(resp);
}
