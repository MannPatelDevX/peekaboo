import { useState } from "react";
import type { PromptRoute, Session, SessionState } from "../types";

const LABEL: Record<SessionState, string> = {
  working: "Working",
  waitingApproval: "Waiting for you",
  waitingAnswer: "Waiting for you",
  idle: "Idle",
  ended: "Ended",
};

const ROUTE_NOTE: Record<PromptRoute, string> = {
  terminal: "Typed into Terminal",
  queued: "Queued — sent when Claude finishes its current turn",
};

interface Props {
  sessions: Session[];
  onShowOnBoard: (sessionId: string) => void;
  onSend: (key: string, text: string) => Promise<PromptRoute>;
}

function Row({ s, onShowOnBoard, onSend }: { s: Session } & Omit<Props, "sessions">) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [note, setNote] = useState<{ text: string; error: boolean } | null>(null);
  const id = s.key.startsWith("claude:") ? s.key.slice("claude:".length) : null;
  const title = s.title || s.project;

  const send = () => {
    const msg = text.trim();
    if (!msg || sending) return;
    setSending(true);
    onSend(s.key, msg)
      .then((route) => { setText(""); setNote({ text: ROUTE_NOTE[route], error: false }); })
      .catch((e) => setNote({ text: String(e), error: true }))
      .finally(() => setSending(false));
  };

  return (
    <li className={`session ${s.state}`}>
      <div className="row">
        <h4 className="title" title={title}>{title}</h4>
        <span className="badge">{LABEL[s.state]}</span>
      </div>
      <div className="meta">
        {s.title && <span>{s.project}</span>}
        {s.lastAction && <span className="last-action">{s.lastAction}</span>}
        {!!s.queued && <span className="badge queued">{s.queued} queued</span>}
        {id && <button className="ghost-btn small link" onClick={() => onShowOnBoard(id)}>Show on board</button>}
      </div>
      <div className="prompt-box">
        <input
          placeholder="Message Claude…"
          value={text}
          disabled={sending}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") send(); }}
        />
        <button className="primary small" disabled={!text.trim() || sending} onClick={send}>Send</button>
      </div>
      {note && <p className={note.error ? "note error-text" : "note"}>{note.text}</p>}
    </li>
  );
}

export function SessionList({ sessions, ...rest }: Props) {
  if (sessions.length === 0) return <p className="empty">No active agents. Start Claude Code and Boo will peek in.</p>;
  return (
    <ul className="sessions">
      {sessions.map((s) => <Row key={s.key} s={s} {...rest} />)}
    </ul>
  );
}
