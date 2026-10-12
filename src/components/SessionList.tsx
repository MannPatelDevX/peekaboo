import { useState } from "react";
import type { PromptRoute, Session, SessionState } from "../types";

const LABEL: Record<SessionState, string> = {
  working: "Working",
  waitingApproval: "Waiting for you",
  waitingAnswer: "Waiting for you",
  idle: "Idle",
  ended: "Ended",
};

/** Terminals Peekaboo can type into directly (unknown → try). */
const typable = (terminal?: string | null) => !terminal || terminal === "Terminal" || terminal === "iTerm2";

const ROUTE_NOTE: Record<PromptRoute, string> = {
  terminal: "Typed into Terminal",
  queued: "Queued — sent when Claude finishes its current turn",
  delivered: "Sent — Claude is working on it",
};

interface Props {
  sessions: Session[];
  onShowOnBoard: (sessionId: string) => void;
  onSend: (key: string, text: string) => Promise<PromptRoute>;
  onListen: (key: string, on: boolean) => void;
}

function Row({ s, onShowOnBoard, onSend, onListen }: { s: Session } & Omit<Props, "sessions">) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [note, setNote] = useState<{ text: string; error: boolean } | null>(null);
  const id = s.key.startsWith("claude:") ? s.key.slice("claude:".length) : null;
  const title = s.title || s.project;
  const direct = typable(s.terminal);
  const unreachable = !s.held && !direct && s.state === "idle";
  const placeholder = s.held || direct
    ? "Message Claude…"
    : unreachable
      ? s.listening
        ? `Listening — you can message Claude here after Claude's next turn`
        : `Claude is idle in ${s.terminal} — type there`
      : `Message Claude… (sent when Claude finishes this turn)`;

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
        <span className={`badge ${s.held ? "held" : ""}`}>{s.held ? "Waiting for you here" : LABEL[s.state]}</span>
      </div>
      <div className="meta">
        {s.title && <span>{s.project}</span>}
        {s.terminal && <span className="terminal-chip">{s.terminal}</span>}
        {s.lastAction && <span className="last-action">{s.lastAction}</span>}
        {!!s.queued && <span className="badge queued">{s.queued} queued</span>}
        {id && <button className="ghost-btn small link" onClick={() => onShowOnBoard(id)}>Show on board</button>}
        {s.held ? (
          <button className="small" title="Let Claude stop so you can type in the terminal" onClick={() => onListen(s.key, false)}>Release</button>
        ) : (
          <button className={`small listen ${s.listening ? "on" : ""}`} aria-pressed={!!s.listening}
            title="When Claude finishes a turn, keep it waiting for your next prompt from Peekaboo"
            onClick={() => onListen(s.key, !s.listening)}>Listen</button>
        )}
      </div>
      <div className="prompt-box">
        <input
          placeholder={placeholder}
          value={text}
          disabled={sending || unreachable}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") send(); }}
        />
        <button className="primary small" disabled={!text.trim() || sending || unreachable} onClick={send}>Send</button>
      </div>
      {unreachable && !s.listening && (
        <p className="note">Turn on Listen to message this session from Peekaboo after Claude's next turn.</p>
      )}
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
