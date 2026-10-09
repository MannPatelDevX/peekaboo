import type { Session, SessionState } from "../types";

const LABEL: Record<SessionState, string> = {
  working: "Working",
  waitingApproval: "Waiting for you",
  waitingAnswer: "Waiting for you",
  idle: "Idle",
  ended: "Ended",
};

export function SessionList({ sessions }: { sessions: Session[] }) {
  if (sessions.length === 0) return <p className="empty">No active agents. Start Claude Code and Boo will peek in.</p>;
  return (
    <ul className="sessions">
      {sessions.map((s) => (
        <li key={s.key} className={`session ${s.state}`}>
          <div className="row">
            <strong>{s.project}</strong>
            <span className="badge">{LABEL[s.state]}</span>
          </div>
          <div className="meta">
            <span className="agent">{s.agent}</span>
            {s.lastAction && <span>{s.lastAction}</span>}
          </div>
        </li>
      ))}
    </ul>
  );
}
