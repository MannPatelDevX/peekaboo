import { useState } from "react";
import type { ClaudexAction, ClaudexSnapshot, Gauge } from "../types";
import { usageLevel } from "./Pill";
import { RefreshIcon } from "./icons";

interface Props {
  snap: ClaudexSnapshot | null;
  loading: boolean;
  error?: string | null;
  onRefresh: () => void;
  onAct: (a: ClaudexAction) => void;
}

interface Pending { action: ClaudexAction; message: string }

function Meter({ label, g }: { label: string; g: Gauge }) {
  if (g.pct === null) return <div className="meter"><span>{label}</span><span className="meta">no reading</span></div>;
  return (
    <div className="meter">
      <span>{label}</span>
      <div className="gauge"><div className={`fill ${usageLevel(g.pct)}`} style={{ width: `${g.pct}%` }} /><span>{g.pct}%</span></div>
      {g.at && <span className="meta">resets {g.at}</span>}
    </div>
  );
}

export function AccountsPanel({ snap, loading, error, onRefresh, onAct }: Props) {
  const [pending, setPending] = useState<Pending | null>(null);

  if (!snap) return <p className="empty">{loading ? "Reading claudex…" : error ?? "No data yet."}</p>;
  if (!snap.installed) {
    return (
      <div className="accounts">
        <p className="empty">claudex is not installed, so account switching and usage aren't available.</p>
        <a href={snap.installUrl} target="_blank" rel="noreferrer">How to install claudex</a>
      </div>
    );
  }

  const ask = (action: ClaudexAction, message: string) => setPending({ action, message });
  const pool = snap.pool.data;
  return (
    <div className="accounts">
      <div className="row toolbar">
        {pool && (pool.consuming.on
          ? <button disabled={loading} onClick={() => ask({ type: "pool", on: false }, "Stop borrowing from the pool and go back to your own token?")}>Stop pool</button>
          : <button disabled={loading} onClick={() => ask({ type: "pool", on: true }, "Borrow from the shared pool instead of your own token?")}>Start pool</button>)}
        {snap.autoswitch !== null && (snap.autoswitch
          ? <button disabled={loading} onClick={() => ask({ type: "autoswitch", on: false }, "Turn off automatic account switching?")}>Disable autoswitch</button>
          : <button disabled={loading} onClick={() => ask({ type: "autoswitch", on: true }, "Let claudex switch accounts automatically when usage gets high?")}>Enable autoswitch</button>)}
        <button className="ghost-btn" aria-label="Refresh" disabled={loading} onClick={onRefresh}><span className={loading ? "spin" : ""}><RefreshIcon /></span></button>
      </div>

      {pending && (
        <div className="card confirm">
          <span>{pending.message}</span>
          <div className="actions">
            <button className="primary" disabled={loading} onClick={() => { onAct(pending.action); setPending(null); }}>Confirm</button>
            <button className="ghost-btn" onClick={() => setPending(null)}>Cancel</button>
          </div>
        </div>
      )}
      {error && <p className="error">{error}</p>}

      {snap.usage.data ? (
        <ul className="sessions">
          {snap.usage.data.map((u) => (
            <li key={u.account} className={`session account ${u.active ? "active" : ""}`}>
              <div className="row">
                <strong>{u.account}</strong>
                <span className="meta">{u.tier}</span>
                {u.active
                  ? <span className="badge live">Active</span>
                  : <button disabled={loading} onClick={() => ask({ type: "switch", account: u.account }, `Switch this machine to ${u.account}?`)}>Switch</button>}
              </div>
              <Meter label="Session" g={u.session} />
              <Meter label="Week" g={u.week} />
            </li>
          ))}
        </ul>
      ) : (
        <pre className="raw">{snap.usage.error ?? snap.usage.raw}</pre>
      )}
    </div>
  );
}
