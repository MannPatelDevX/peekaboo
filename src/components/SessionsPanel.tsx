import { useMemo, useState } from "react";
import type { SessionState, TranscriptRow } from "../types";
import { usageLevel } from "./Pill";

const LIVE: Partial<Record<SessionState, string>> = {
  working: "Running",
  waitingApproval: "Waiting for you",
  waitingAnswer: "Waiting for you",
  idle: "Open",
};

export function ago(now: number, then: number): string {
  const s = Math.max(0, now - then);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86_400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86_400)}d ago`;
}

interface Props {
  sessions: TranscriptRow[];
  now: number;
  onResume: (id: string) => void;
  onCopy: (command: string) => void;
}

export function SessionsPanel({ sessions, now, onResume, onCopy }: Props) {
  const [q, setQ] = useState("");
  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    if (!needle) return sessions;
    return sessions.filter((s) => [s.title, s.project, s.branch ?? ""].some((f) => f.toLowerCase().includes(needle)));
  }, [q, sessions]);

  return (
    <div className="transcripts">
      <input placeholder="Search sessions…" value={q} onChange={(e) => setQ(e.target.value)} />
      {sessions.length === 0 && <p className="empty">No Claude Code sessions in the last 30 days.</p>}
      <ul className="sessions">
        {shown.map((s) => {
          const pct = s.contextTokens === null ? null : Math.min(100, Math.round((s.contextTokens / s.contextWindow) * 100));
          const live = s.live ? LIVE[s.live] : undefined;
          return (
            <li key={s.id} className="session transcript">
              <div className="row">
                <strong className="title">{s.title}</strong>
                {live && <span className={`badge live ${s.live}`}>{live}</span>}
              </div>
              <div className="meta">
                <span>{s.project}</span>
                {s.branch && <span>⎇ {s.branch}</span>}
                <span>{ago(now, s.lastActivity)}</span>
              </div>
              {pct !== null && (
                <div className="gauge" title={`${s.contextTokens?.toLocaleString()} / ${s.contextWindow.toLocaleString()} tokens`}>
                  <div className={`fill ${usageLevel(pct)}`} style={{ width: `${pct}%` }} />
                  <span>{pct}%</span>
                </div>
              )}
              <div className="actions">
                <button className="primary" disabled={!s.cwd} onClick={() => onResume(s.id)}>Resume</button>
                {s.resumeCommand && <button className="ghost-btn" onClick={() => onCopy(s.resumeCommand!)}>Copy command</button>}
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
