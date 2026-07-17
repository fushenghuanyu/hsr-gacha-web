import { useEffect, useMemo, useRef, useState } from "react";
import { fetchGachaData, fetchGachaDataAuto, fetchHistoryData } from "./api";
import { parseUigfToResult } from "./uigf";
import {
  loadLocalState,
  saveAccountResult,
  setDefaultUid,
  loadFiveStarDisplaySettings,
  saveFiveStarDisplaySettings,
  mergeOrderWithData,
  mergeOrderWithRest,
} from "./localStore";
import { finalizeGachaResult } from "./mergeGachaResult";
import { normalizeHistoryRows } from "./poolHistory";

const typeNameMap = {
  "11": "角色活动跃迁",
  "12": "光锥活动跃迁",
  "21": "Fate联动卡池（角色）",
  "22": "Fate联动卡池（光锥）",
  "1": "常驻跃迁",
  "2": "新手跃迁",
};

/** 无 UP/小保底机制，不展示 UP 平均与小保底不歪率 */
const poolsWithoutUpPityStats = new Set(["1", "2"]);

function showUpPityStats(gachaType) {
  return !poolsWithoutUpPityStats.has(`${gachaType}`);
}

function StatCard({ label, value }) {
  return (
    <div className="stat-card">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
    </div>
  );
}

function StatSwipeCard({ row }) {
  return (
    <div className="summary-card">
      <div className="summary-card-title">{typeNameMap[row.gacha_type] || row.gacha_type}</div>
      <div className="summary-card-main">
        <span className="summary-card-number">{row.total}</span>
        <span className="summary-card-unit">抽</span>
      </div>
      <div className="summary-card-sub">
        <span>UP数/总金数</span>
        <span>
          {row.up_count ?? 0} / {row.five_star_count ?? 0}
        </span>
      </div>
      {showUpPityStats(row.gacha_type) ? (
        <>
          <div className="summary-card-sub">
            <span>UP平均</span>
            <span>{row.avg_up_pity ?? "-"}</span>
          </div>
          <div className="summary-card-sub">
            <span>小保底不歪率</span>
            <span>
              {row.small_pity_no_miss_rate != null ? `${row.small_pity_no_miss_rate}%` : "-"}
            </span>
          </div>
        </>
      ) : null}
      <div className="summary-card-sub">
        <span>平均出金</span>
        <span>{row.avg_five_star_pity}</span>
      </div>
    </div>
  );
}

function getPoolPityLimit(poolType) {
  const t = `${poolType}`;
  if (t === "12" || t === "22") return 80;
  if (t === "2") return 50;
  return 90;
}

function getPityClass(pity) {
  if (pity <= 40) return "pity-good";
  if (pity <= 70) return "pity-mid";
  return "pity-bad";
}

function getIconUrl(row) {
  const itemId = `${row.itemId || ""}`.trim();
  if (!itemId) return "";
  const itemType = `${row.itemType || ""}`.trim();
  if (itemType === "角色") {
    return `http://127.0.0.1:8000/icon/character/${itemId}.png`;
  }
  if (itemType === "光锥") {
    return `http://127.0.0.1:8000/icon/light_cone/${itemId}.png`;
  }
  return "";
}

function getIconRankClass(rankType) {
  const rank = `${rankType || ""}`.trim();
  if (rank === "5") return "rank-5-icon";
  if (rank === "4") return "rank-4-icon";
  return "";
}

function FiveStarIconWithPlaceholder({ iconUrl, name, rankType, alt = "" }) {
  const [failed, setFailed] = useState(false);
  const rankClass = getIconRankClass(rankType) || "rank-5-icon";
  const showPh = !`${iconUrl || ""}`.trim() || failed;
  const label = alt || name || "未知";
  if (showPh) {
    return (
      <div
        className={`five-star-icon five-star-icon--placeholder ${rankClass}`}
        role="img"
        aria-label={label}
      >
        ?
      </div>
    );
  }
  return (
    <img
      className={`five-star-icon ${rankClass}`}
      src={iconUrl}
      alt={name}
      loading="lazy"
      onError={() => setFailed(true)}
    />
  );
}

function CounterClockwiseClockIcon() {
  return (
    <svg
      className="history-fab-icon"
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
      <path d="M3 3v5h5" />
      <path d="M12 7v5l4 2" />
    </svg>
  );
}

function buildFiveStarRows(records, poolType) {
  const list = getPoolRecordList(records, poolType);
  const rows = [];
  let lastFiveIndex = -1;
  for (let i = 0; i < list.length; i += 1) {
    const item = list[i];
    if (`${item.rank_type}` === "5") {
      const pity = i - lastFiveIndex;
      rows.push({
        id: item.id,
        itemId: item.item_id,
        name: item.name,
        itemType: item.item_type,
        rankType: item.rank_type,
        isUp: item.is_up,
        time: item.time,
        pity,
        fourStars: aggregateFourStarsInRange(list, lastFiveIndex, i - 1),
      });
      lastFiveIndex = i;
    }
  }
  return rows.reverse();
}

function getPoolRecordList(records, poolType) {
  return (records || [])
    .filter((item) => `${item.gacha_type}` === `${poolType}`)
    .sort((a, b) => `${a.id}`.localeCompare(`${b.id}`));
}

function fourStarKey(item) {
  const id = `${item.item_id || ""}`.trim();
  if (id) return id;
  return `${item.item_type || ""}:${item.name || ""}`;
}

function aggregateFourStarsInRange(list, fromExclusive, toInclusive) {
  if (toInclusive <= fromExclusive) return [];
  const counts = new Map();
  const order = [];
  for (let i = fromExclusive + 1; i <= toInclusive; i += 1) {
    const item = list[i];
    if (`${item.rank_type}` !== "4") continue;
    const key = fourStarKey(item);
    if (!counts.has(key)) {
      counts.set(key, {
        key,
        id: item.id,
        itemId: item.item_id,
        name: item.name,
        itemType: item.item_type,
        rankType: item.rank_type,
        isUp: item.is_up,
        time: item.time,
        count: 0,
      });
      order.push(key);
    }
    counts.get(key).count += 1;
  }
  return order.map((key) => counts.get(key));
}

function buildPendingFourStars(records, poolType) {
  const list = getPoolRecordList(records, poolType);
  if (!list.length) return [];

  let lastFiveIndex = -1;
  for (let i = 0; i < list.length; i += 1) {
    if (`${list[i].rank_type}` === "5") {
      lastFiveIndex = i;
    }
  }
  return aggregateFourStarsInRange(list, lastFiveIndex, list.length - 1);
}

function FourStarGapRow({ items, flush = false }) {
  if (!items?.length) return null;
  return (
    <div className={`four-star-gap-row${flush ? " four-star-gap-row--flush" : ""}`}>
      {items.map((item) => (
        <div className="four-star-gap-item" key={item.key || item.id}>
          <FiveStarIconWithPlaceholder
            iconUrl={getIconUrl(item)}
            name={item.name}
            rankType="4"
          />
          {item.count > 1 ? (
            <span className="four-star-gap-count">{item.count}</span>
          ) : null}
        </div>
      ))}
    </div>
  );
}

function buildSinceLastFiveStar(records, poolType) {
  const list = getPoolRecordList(records, poolType);
  if (!list.length) return 0;

  let lastFiveIndex = -1;
  for (let i = 0; i < list.length; i += 1) {
    const item = list[i];
    if (`${item.rank_type}` === "5") {
      lastFiveIndex = i;
    }
  }
  if (lastFiveIndex < 0) return list.length;
  return list.length - 1 - lastFiveIndex;
}

export default function App() {
  const [warpUrl, setWarpUrl] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState(null);
  const [logs, setLogs] = useState([]);
  const [accountMap, setAccountMap] = useState({});
  const [selectedUid, setSelectedUid] = useState("");
  const [historyLogs, setHistoryLogs] = useState([]);
  const [showUrlDialog, setShowUrlDialog] = useState(false);
  const [showHistoryDialog, setShowHistoryDialog] = useState(false);
  const [showFiveStarSettings, setShowFiveStarSettings] = useState(false);
  const [poolDragGacha, setPoolDragGacha] = useState(null);
  const [poolDragOverGacha, setPoolDragOverGacha] = useState(null);
  const [fiveStarDisplay, setFiveStarDisplay] = useState(() => loadFiveStarDisplaySettings());
  const [fetchFullHistory, setFetchFullHistory] = useState(false);
  const [urlDraft, setUrlDraft] = useState("");
  const [normalizedHistory, setNormalizedHistory] = useState([]);
  const logBoxRef = useRef(null);

  useEffect(() => {
    const el = logBoxRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [logs]);

  const appendLog = (message) => {
    const time = new Date().toLocaleTimeString();
    setLogs((prev) => [...prev, `[${time}] ${message}`]);
  };

  const recentRecords = useMemo(() => {
    if (!result?.records) return [];
    return [...result.records].reverse().slice(0, 40);
  }, [result]);

  const poolTypesFromData = useMemo(() => {
    if (!result?.pool_summary?.length) return [];
    return result.pool_summary.map((p) => `${p.gacha_type}`);
  }, [result]);

  const displayPoolOrder = useMemo(() => {
    const merged = mergeOrderWithData(fiveStarDisplay.order, poolTypesFromData);
    return merged.filter((t) => fiveStarDisplay.visible[t] !== false);
  }, [fiveStarDisplay.order, fiveStarDisplay.visible, poolTypesFromData]);

  const poolByType = useMemo(() => {
    const m = {};
    if (result?.pool_summary) {
      for (const p of result.pool_summary) {
        m[`${p.gacha_type}`] = p;
      }
    }
    return m;
  }, [result]);

  const persistFiveStarDisplay = (next) => {
    setFiveStarDisplay(next);
    saveFiveStarDisplaySettings(next);
  };

  const closeFiveStarSettings = () => {
    setShowFiveStarSettings(false);
    setPoolDragGacha(null);
    setPoolDragOverGacha(null);
  };

  const onTogglePoolVisible = (gachaType) => {
    const t = `${gachaType}`;
    const wasShown = fiveStarDisplay.visible[t] !== false;
    persistFiveStarDisplay({
      ...fiveStarDisplay,
      visible: { ...fiveStarDisplay.visible, [t]: !wasShown },
    });
  };

  const onToggleShowFourStar = () => {
    persistFiveStarDisplay({
      ...fiveStarDisplay,
      showFourStar: fiveStarDisplay.showFourStar !== true,
    });
  };

  const applyPoolOrderAfterDrag = (fromGacha, toGacha) => {
    if (!fromGacha || !toGacha || fromGacha === toGacha) return;
    const merged = mergeOrderWithData(fiveStarDisplay.order, poolTypesFromData);
    const from = merged.indexOf(fromGacha);
    const to = merged.indexOf(toGacha);
    if (from < 0 || to < 0) return;
    if (from === to) return;
    const next = merged.slice();
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    persistFiveStarDisplay({
      ...fiveStarDisplay,
      order: mergeOrderWithRest(next, fiveStarDisplay.order),
    });
  };

  const accountOptions = useMemo(() => {
    return Object.values(accountMap).sort((a, b) => (b.updatedAt || 0) - (a.updatedAt || 0));
  }, [accountMap]);

  const applyResult = async (data, source) => {
    const next = await saveAccountResult(data, source, { normalizedHistory });
    const uid = `${data?.uid || ""}`.trim();
    const finalResult =
      uid && next.accounts?.[uid]?.result
        ? next.accounts[uid].result
        : finalizeGachaResult(data, normalizedHistory);
    setResult(finalResult);
    setAccountMap(next.accounts || {});
    setHistoryLogs(next.history || []);
    setSelectedUid(`${data?.uid || ""}`);
  };

  useEffect(() => {
    let cancelled = false;
    fetchHistoryData()
      .then((rows) => {
        if (cancelled) return;
        setNormalizedHistory(normalizeHistoryRows(rows));
      })
      .catch(() => {
        if (cancelled) return;
        setNormalizedHistory([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const state = await loadLocalState();
      if (cancelled) return;
      const nextAccounts = {};
      for (const [uid, account] of Object.entries(state.accounts || {})) {
        const baseResult = account?.result;
        if (!baseResult?.records) {
          nextAccounts[uid] = account;
          continue;
        }
        nextAccounts[uid] = {
          ...account,
          result: finalizeGachaResult(baseResult, normalizedHistory),
        };
      }
      setAccountMap(nextAccounts);
      setHistoryLogs(state.history || []);

      const uid = state.defaultUid;
      if (uid && nextAccounts?.[uid]?.result) {
        setResult(nextAccounts[uid].result);
        setSelectedUid(uid);
        setLogs([`[userData] 已加载默认账号 ${uid} 的抽卡记录`]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [normalizedHistory]);

  const onFetchByUrl = async (inputUrl) => {
    if (!inputUrl.trim()) {
      setError("请先粘贴跃迁链接");
      return;
    }

    try {
      setLoading(true);
      setError("");
      setLogs([]);
      appendLog("开始手动拉取");
      const appendServerLog = (line) => {
        setLogs((prev) => [...prev, `[服务端] ${line}`]);
      };
      const data = await fetchGachaData(inputUrl.trim(), fetchFullHistory, appendServerLog);
      await applyResult(data, "manual");
      setWarpUrl(inputUrl.trim());
      appendLog("拉取完成");
    } catch (err) {
      setError(err.message || "获取数据失败");
      setLogs((prev) => [
        ...prev,
        ...(err.logs || []).map((item) => `[服务端] ${item}`),
        `[${new Date().toLocaleTimeString()}] 请求失败: ${err.message || "未知错误"}`,
      ]);
    } finally {
      setLoading(false);
    }
  };

  const onAutoFetch = async () => {
    try {
      setLoading(true);
      setError("");
      setLogs([]);
      appendLog("开始自动读取日志并拉取");
      const appendServerLog = (line) => {
        setLogs((prev) => [...prev, `[服务端] ${line}`]);
      };
      const data = await fetchGachaDataAuto(fetchFullHistory, appendServerLog);
      await applyResult(data, "auto");
      if (data.warp_url) {
        setWarpUrl(data.warp_url);
        setLogs((prev) => [...prev, `[自动提取] 抽卡分析链接: ${data.warp_url}`]);
      }
      appendLog("自动流程完成");
    } catch (err) {
      setError(err.message || "自动读取失败");
      setLogs((prev) => [
        ...prev,
        ...(err.logs || []).map((item) => `[服务端] ${item}`),
        `[${new Date().toLocaleTimeString()}] 自动流程失败: ${err.message || "未知错误"}`,
      ]);
    } finally {
      setLoading(false);
    }
  };

  const onImportUigf = async (event) => {
    const file = event.target.files?.[0];
    if (!file) return;
    try {
      setLoading(true);
      setError("");
      setLogs([]);
      appendLog(`开始读取本地文件: ${file.name}`);
      const text = await file.text();
      const data = parseUigfToResult(text);
      await applyResult(data, "uigf");
      setLogs((prev) => [...prev, ...(data.logs || []).map((item) => `[本地解析] ${item}`)]);
      appendLog("本地 UIGF 解析完成");
    } catch (err) {
      setError(err.message || "UIGF 文件解析失败");
      setLogs((prev) => [...prev, `[${new Date().toLocaleTimeString()}] 文件解析失败: ${err.message || "未知错误"}`]);
    } finally {
      event.target.value = "";
      setLoading(false);
    }
  };

  const onSwitchAccount = async (uid) => {
    setSelectedUid(uid);
    const account = accountMap[uid];
    if (!account?.result) return;
    await setDefaultUid(uid);
    setResult(account.result);
    setLogs([`[userData] 已切换到账号 ${uid}`]);
  };

  const onConfirmUrlDialog = async () => {
    setShowUrlDialog(false);
    await onFetchByUrl(urlDraft);
  };

  return (
    <main className="page">
      <button
        type="button"
        className="history-fab"
        onClick={() => setShowHistoryDialog(true)}
        title="本地读取记录"
      >
        <CounterClockwiseClockIcon />
      </button>

      <section className="panel">
        <h1>星穹铁道抽卡分析（Web）</h1>
        <p className="subtitle">前端 React 展示，支持手动URL/自动日志/UIGF导入 · v{__APP_VERSION__}</p>
        <div className="action-buttons">
          <button type="button" className="small-btn" disabled={loading} onClick={() => setShowUrlDialog(true)}>
            URL 获取抽卡分析
          </button>
          <button type="button" className="small-btn" disabled={loading} onClick={onAutoFetch}>
            自动读取日志
          </button>
          <label className="file-input-label">
            导入UIGF JSON
            <input
              type="file"
              accept=".json,application/json"
              onChange={onImportUigf}
              disabled={loading}
            />
          </label>
          <label className="inline-check">
            <input
              type="checkbox"
              checked={fetchFullHistory}
              onChange={(e) => setFetchFullHistory(e.target.checked)}
              disabled={loading}
            />
            <span>全量拉取</span>
          </label>
          {warpUrl ? <span className="current-url">当前URL已缓存</span> : null}
        </div>
        <div className="account-switch-row">
          <label htmlFor="uid-select">账号切换：</label>
          <select
            id="uid-select"
            value={selectedUid}
            onChange={(e) => onSwitchAccount(e.target.value)}
            disabled={!accountOptions.length}
          >
            <option value="">{accountOptions.length ? "请选择账号" : "暂无本地账号记录"}</option>
            {accountOptions.map((acc) => (
              <option value={acc.uid} key={`uid-${acc.uid}`}>
                {acc.uid}（{acc.source}，{new Date(acc.updatedAt).toLocaleString()}）
              </option>
            ))}
          </select>
        </div>
        {error ? <div className="error">{error}</div> : null}
      </section>

      {showUrlDialog ? (
        <div className="dialog-mask">
          <div className="dialog">
            <h3>输入跃迁历史链接</h3>
            <textarea
              placeholder="粘贴游戏跃迁历史链接（含 authkey）"
              value={urlDraft}
              onChange={(e) => setUrlDraft(e.target.value)}
              rows={5}
            />
            <div className="dialog-actions">
              <button type="button" className="small-btn gray" onClick={() => setShowUrlDialog(false)}>
                取消
              </button>
              <button type="button" className="small-btn" onClick={onConfirmUrlDialog}>
                确定并分析
              </button>
            </div>
          </div>
        </div>
      ) : null}

      <section className="panel">
        <h2>运行日志</h2>
        <div className="log-box" ref={logBoxRef}>
          {logs.length ? (
            logs.map((line, idx) => (
              <div className="log-line" key={`${line}-${idx}`}>
                {line}
              </div>
            ))
          ) : (
            <div className="log-empty">暂无日志，点击按钮开始拉取后会显示执行过程。</div>
          )}
        </div>
      </section>

      {showHistoryDialog ? (
        <div className="dialog-mask" onClick={() => setShowHistoryDialog(false)}>
          <div className="dialog history-dialog" onClick={(e) => e.stopPropagation()}>
            <h3>本地读取记录（按 UID）</h3>
            <div className="history-list">
              {historyLogs.length ? (
                historyLogs.slice(0, 20).map((item, idx) => (
                  <div className="history-item" key={`${item.uid}-${item.time}-${idx}`}>
                    <span>{item.uid}</span>
                    <span>{item.source}</span>
                    <span>{item.total} 抽</span>
                    <span>{new Date(item.time).toLocaleString()}</span>
                  </div>
                ))
              ) : (
                <div className="log-empty">暂无本地读取记录。</div>
              )}
            </div>
            <div className="dialog-actions">
              <button type="button" className="small-btn gray" onClick={() => setShowHistoryDialog(false)}>
                关闭
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {result ? (
        <>
          <section className="grid">
            <StatCard label="UID" value={result.uid || "-"} />
            <StatCard label="总抽数" value={result.overview.total} />
            <StatCard label="五星数" value={result.overview.five_star_count} />
            <StatCard label="四星数" value={result.overview.four_star_count} />
          </section>

          <section className="panel">
            <div className="panel-heading-row">
              <h2>卡池统计</h2>
              <button
                type="button"
                className="small-btn gray"
                onClick={() => setShowFiveStarSettings(true)}
              >
                显示设置
              </button>
            </div>
            {poolTypesFromData.length === 0 ? (
              <div className="log-empty">暂无卡池数据。</div>
            ) : displayPoolOrder.length === 0 ? (
              <div className="log-empty">未选择任何要显示的卡池。请点击「显示设置」勾选要展示的项。</div>
            ) : (
              <div className="summary-swiper">
                {displayPoolOrder.map((gachaType) => {
                  const row = poolByType[gachaType];
                  if (!row) return null;
                  return <StatSwipeCard key={gachaType} row={row} />;
                })}
              </div>
            )}
          </section>

          <section className="panel">
            <div className="panel-heading-row">
              <h2>跃迁记录</h2>
              <div className="panel-heading-actions">
                <label className="inline-check">
                  <input
                    type="checkbox"
                    checked={fiveStarDisplay.showFourStar === true}
                    onChange={onToggleShowFourStar}
                  />
                  显示四星
                </label>
                <button
                  type="button"
                  className="small-btn gray"
                  onClick={() => setShowFiveStarSettings(true)}
                >
                  显示设置
                </button>
              </div>
            </div>
            {displayPoolOrder.length ? (
              <div className="five-star-groups">
                {displayPoolOrder.map((gachaType) => {
                  const pool = poolByType[gachaType];
                  if (!pool) return null;
                  const poolRows = buildFiveStarRows(result.records, gachaType);
                  const gt = `${gachaType}`;
                  const sinceLastFive = buildSinceLastFiveStar(result.records, gachaType);
                  const pendingFourStars = buildPendingFourStars(result.records, gachaType);
                  return (
                    <div className="five-star-group" key={`group-${gachaType}`}>
                      <h3>{typeNameMap[gachaType] || gachaType}</h3>
                      <div className="five-star-list">
                        <div className="five-star-row five-star-row-pending">
                          <div className="five-star-meta five-star-meta--pending">
                            <div className="five-star-time">距上一五星</div>
                          </div>
                          <div className="five-star-bar-wrap">
                            <div
                              className="five-star-icon five-star-icon--placeholder rank-5-icon"
                              aria-hidden
                            >
                              ?
                            </div>
                            <div className="five-star-track">
                              <div
                                className={`five-star-bar ${getPityClass(sinceLastFive)}`}
                                style={{
                                  width: `${Math.min(
                                    100,
                                    (sinceLastFive / getPoolPityLimit(gachaType)) * 100,
                                  )}%`,
                                }}
                              />
                            </div>
                            <div className="five-star-count">已垫{sinceLastFive}抽</div>
                          </div>
                          {fiveStarDisplay.showFourStar ? (
                            <FourStarGapRow items={pendingFourStars} />
                          ) : null}
                        </div>
                        {poolRows.length ? (
                          poolRows.map((row) => {
                            const limit = getPoolPityLimit(gachaType);
                            const width = Math.min(100, (row.pity / limit) * 100);
                            const iconUrl = getIconUrl(row);
                            return (
                              <div className="five-star-row" key={`five-${gachaType}-${row.id}`}>
                                <div className="five-star-meta">
                                  <div className="five-star-name">{row.name}</div>
                                  <div className="five-star-time">{row.time}</div>
                                </div>
                                <div className="five-star-bar-wrap">
                                  <FiveStarIconWithPlaceholder
                                    iconUrl={iconUrl}
                                    name={row.name}
                                    rankType={row.rankType}
                                  />
                                  <div className="five-star-track">
                                    <div
                                      className={`five-star-bar ${getPityClass(row.pity)}`}
                                      style={{ width: `${width}%` }}
                                    />
                                  </div>
                                  {row.isUp === false ? <span className="non-up-badge">歪</span> : null}
                                  <div className="five-star-count">{row.pity} 抽</div>
                                </div>
                                {fiveStarDisplay.showFourStar ? (
                                  <FourStarGapRow items={row.fourStars} />
                                ) : null}
                              </div>
                            );
                          })
                        ) : (
                          <div className="log-empty">当前卡池暂无五星记录。</div>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            ) : (
              <div className="log-empty">未选择任何要显示的卡池。请点击「显示设置」勾选要展示的项。</div>
            )}
          </section>

          {showFiveStarSettings ? (
            <div className="dialog-mask" onClick={closeFiveStarSettings}>
              <div className="dialog five-star-settings-dialog" onClick={(e) => e.stopPropagation()}>
                <h3>卡池与跃迁：显示与顺序</h3>
                <p className="five-star-settings-hint">
                  本设置同时用于「卡池统计」与「跃迁记录」的展示顺序与显隐。勾选要展示的卡池；按住左侧拖动手柄上下拖动，松手放到某一行上即可改变顺序。默认顺序为：角色/光锥活动（含 11/12/21/22）、常驻跃迁、新手跃迁。此项设置保存在本机浏览器；账号、跃迁记录、resources/ 与 dist/ 在运行时会优先从可执行文件同层目录读取（与 userData/ 同根；本机从 target/ 直接运行时若同层无资源，则回退到 hsr-gacha-web 根目录）。
                </p>
                <div className="five-star-settings-list">
                  {mergeOrderWithData(fiveStarDisplay.order, poolTypesFromData).map((gachaType) => {
                    const label = typeNameMap[gachaType] || gachaType;
                    const checked = fiveStarDisplay.visible[gachaType] !== false;
                    const isDragSource = poolDragGacha === gachaType;
                    const isDragOver = poolDragOverGacha === gachaType && poolDragGacha && poolDragGacha !== gachaType;
                    return (
                      <div
                        className={[
                          "five-star-settings-row",
                          isDragSource ? " five-star-settings-row--source" : "",
                          isDragOver ? " five-star-settings-row--over" : "",
                        ].join("")}
                        key={gachaType}
                        onDragOver={(e) => {
                          e.preventDefault();
                          e.dataTransfer.dropEffect = "move";
                          if (poolDragGacha && poolDragGacha !== gachaType) {
                            setPoolDragOverGacha(gachaType);
                          }
                        }}
                        onDragLeave={(e) => {
                          if (e.currentTarget.contains(e.relatedTarget)) return;
                          setPoolDragOverGacha((prev) => (prev === gachaType ? null : prev));
                        }}
                        onDrop={(e) => {
                          e.preventDefault();
                          const fromGacha = e.dataTransfer.getData("text/plain");
                          applyPoolOrderAfterDrag(fromGacha, gachaType);
                          setPoolDragGacha(null);
                          setPoolDragOverGacha(null);
                        }}
                      >
                        <span
                          className="five-star-settings-handle"
                          draggable
                          onDragStart={(e) => {
                            e.dataTransfer.setData("text/plain", gachaType);
                            e.dataTransfer.effectAllowed = "move";
                            setPoolDragGacha(gachaType);
                            setPoolDragOverGacha(null);
                          }}
                          onDragEnd={() => {
                            setPoolDragGacha(null);
                            setPoolDragOverGacha(null);
                          }}
                          title="按住拖动以排序"
                          aria-label="拖动排序"
                        >
                          <svg
                            className="five-star-settings-grip"
                            viewBox="0 0 14 16"
                            width="12"
                            height="16"
                            aria-hidden
                          >
                            <circle cx="3.5" cy="3" r="1.4" fill="currentColor" />
                            <circle cx="10.5" cy="3" r="1.4" fill="currentColor" />
                            <circle cx="3.5" cy="8" r="1.4" fill="currentColor" />
                            <circle cx="10.5" cy="8" r="1.4" fill="currentColor" />
                            <circle cx="3.5" cy="13" r="1.4" fill="currentColor" />
                            <circle cx="10.5" cy="13" r="1.4" fill="currentColor" />
                          </svg>
                        </span>
                        <label className="five-star-settings-check">
                          <input
                            type="checkbox"
                            checked={checked}
                            onChange={() => onTogglePoolVisible(gachaType)}
                          />
                          <span>{label}</span>
                        </label>
                      </div>
                    );
                  })}
                </div>
                {poolTypesFromData.length ? null : (
                  <div className="log-empty">暂无卡池数据，请先在上方拉取到抽卡结果后再试。</div>
                )}
                <div className="dialog-actions">
                  <button type="button" className="small-btn" onClick={closeFiveStarSettings}>
                    完成
                  </button>
                </div>
              </div>
            </div>
          ) : null}

          <section className="panel">
            <h2>最近记录（最多 40 条）</h2>
            <div className="records">
              {recentRecords.map((item) => (
                <div className={`record rank-${item.rank_type}`} key={item.id}>
                  <div className="record-main">
                    <strong>{item.name}</strong>
                    <span>{item.item_type}</span>
                  </div>
                  <div className="record-sub">
                    <span>{typeNameMap[item.gacha_type] || item.gacha_type}</span>
                    <span>{item.time}</span>
                  </div>
                </div>
              ))}
            </div>
          </section>
        </>
      ) : null}
    </main>
  );
}

