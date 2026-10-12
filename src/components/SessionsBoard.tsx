import { useMemo, useState, type DragEvent } from "react";
import type { DateRange, GroupData, SessionState, TranscriptRow } from "../types";
import { usageLevel } from "./Pill";
import { BranchIcon } from "./icons";

const LIVE: Partial<Record<SessionState, string>> = {
  working: "Running",
  waitingApproval: "Waiting for you",
  waitingAnswer: "Waiting for you",
  idle: "Open",
};
const DRAG_TYPE = "text/plain";
const UNGROUPED = "ungrouped";

export function ago(now: number, then: number): string {
  const s = Math.max(0, now - then);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86_400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86_400)}d ago`;
}

/** Inclusive lower / exclusive upper bound in epoch seconds, using local calendar days. */
function dateBounds(range: DateRange, now: number): [number, number] {
  const midnight = new Date(now * 1000);
  midnight.setHours(0, 0, 0, 0);
  const today = Math.floor(midnight.getTime() / 1000);
  switch (range) {
    case "today": return [today, Infinity];
    case "yesterday": return [today - 86_400, today];
    case "7d": return [now - 7 * 86_400, Infinity];
    case "30d": return [0, Infinity];
  }
}

interface Props {
  sessions: TranscriptRow[];
  groups: GroupData;
  now: number;
  onResume: (id: string) => void;
  onCopy: (command: string) => void;
  onAssign: (session: string, group: string | null) => void;
  onCreateGroup: (name: string) => void;
  onRenameGroup: (id: string, name: string) => void;
  onDeleteGroup: (id: string) => void;
  onAutoGroup: () => void;
}

function Card({ s, now, onResume, onCopy }: { s: TranscriptRow; now: number; onResume: Props["onResume"]; onCopy: Props["onCopy"] }) {
  const pct = s.contextTokens === null ? null : Math.min(100, Math.round((s.contextTokens / s.contextWindow) * 100));
  const live = s.live ? LIVE[s.live] : undefined;
  return (
    <li className="session transcript" draggable
      onDragStart={(e) => { e.dataTransfer.setData(DRAG_TYPE, s.id); e.dataTransfer.effectAllowed = "move"; }}>
      <div className="row">
        <strong className="title" title={s.title}>{s.title}</strong>
        {live && <span className={`badge live ${s.live}`}>{live}</span>}
      </div>
      <div className="meta">
        <span>{s.project}</span>
        {s.branch && <span><BranchIcon />{s.branch}</span>}
        <span>{ago(now, s.lastActivity)}</span>
      </div>
      {pct !== null && (
        <div className="gauge" title={`${s.contextTokens?.toLocaleString()} / ${s.contextWindow.toLocaleString()} tokens`}>
          <div className={`fill ${usageLevel(pct)}`} style={{ width: `${pct}%` }} />
          <span>{pct}%</span>
        </div>
      )}
      <div className="actions">
        <button className="primary small" disabled={!s.cwd} onClick={() => onResume(s.id)}>Resume</button>
        {s.resumeCommand && <button className="ghost-btn small" onClick={() => onCopy(s.resumeCommand!)}>Copy command</button>}
      </div>
    </li>
  );
}

function ColumnTitle({ name, onRename }: { name: string; onRename?: (name: string) => void }) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(name);
  if (editing && onRename) {
    const finish = (save: boolean) => {
      setEditing(false);
      if (save && value.trim() && value.trim() !== name) onRename(value.trim());
      else setValue(name);
    };
    return (
      <input className="rename" autoFocus value={value} onChange={(e) => setValue(e.target.value)}
        onBlur={() => finish(true)} onKeyDown={(e) => { if (e.key === "Enter") finish(true); if (e.key === "Escape") finish(false); }} />
    );
  }
  return <strong onDoubleClick={() => onRename && setEditing(true)} title={onRename ? "Double-click to rename" : undefined}>{name}</strong>;
}

export function SessionsBoard(p: Props) {
  const [q, setQ] = useState("");
  const [groupFilter, setGroupFilter] = useState<string>("all");
  const [range, setRange] = useState<DateRange>("30d");
  const [over, setOver] = useState<string | null>(null);
  const [newName, setNewName] = useState("");

  const visible = useMemo(() => {
    const needle = q.trim().toLowerCase();
    const [from, to] = dateBounds(range, p.now);
    return p.sessions.filter((s) =>
      s.lastActivity >= from && s.lastActivity < to &&
      (!needle || [s.title, s.project, s.branch ?? ""].some((f) => f.toLowerCase().includes(needle))));
  }, [q, range, p.sessions, p.now]);

  const known = new Set(p.groups.groups.map((g) => g.id));
  const groupOf = (s: TranscriptRow) => {
    const g = p.groups.assignments[s.id];
    return g && known.has(g) ? g : UNGROUPED;
  };
  const columns = [...p.groups.groups.map((g) => ({ id: g.id, name: g.name })), { id: UNGROUPED, name: "Ungrouped" }]
    .filter((c) => groupFilter === "all" || groupFilter === c.id);

  const dropProps = (id: string) => ({
    onDragOver: (e: DragEvent) => { e.preventDefault(); e.dataTransfer.dropEffect = "move"; setOver(id); },
    onDragLeave: () => setOver((o) => (o === id ? null : o)),
    onDrop: (e: DragEvent) => {
      e.preventDefault();
      setOver(null);
      const session = e.dataTransfer.getData(DRAG_TYPE);
      if (session) p.onAssign(session, id === UNGROUPED ? null : id);
    },
  });

  if (p.sessions.length === 0) return <p className="empty">No Claude Code sessions in the last 30 days.</p>;

  return (
    <div className="board-wrap">
      <div className="board-toolbar">
        <input className="search" placeholder="Search sessions…" value={q} onChange={(e) => setQ(e.target.value)} />
        <label className="select"><span>Group</span>
          <select aria-label="Group" value={groupFilter} onChange={(e) => setGroupFilter(e.target.value)}>
            <option value="all">All groups</option>
            {p.groups.groups.map((g) => <option key={g.id} value={g.id}>{g.name}</option>)}
            <option value={UNGROUPED}>Ungrouped</option>
          </select>
        </label>
        <label className="select"><span>Date</span>
          <select aria-label="Date" value={range} onChange={(e) => setRange(e.target.value as DateRange)}>
            <option value="today">Today</option>
            <option value="yesterday">Yesterday</option>
            <option value="7d">Last 7 days</option>
            <option value="30d">Last 30 days</option>
          </select>
        </label>
        <button className="ghost-btn small" onClick={p.onAutoGroup} title="Put every ungrouped session into a group named after its project">
          Auto-group by project
        </button>
      </div>
      <div className="board">
        {columns.map((c) => {
          const items = visible.filter((s) => groupOf(s) === c.id);
          const isGroup = c.id !== UNGROUPED;
          return (
            <section key={c.id} role="region" aria-label={`${c.name} (${items.length})`}
              className={`column ${isGroup ? "" : "ungrouped"} ${over === c.id ? "drop-over" : ""}`} {...dropProps(c.id)}>
              <header className="column-head">
                <ColumnTitle name={c.name} onRename={isGroup ? (n) => p.onRenameGroup(c.id, n) : undefined} />
                <span className="count">{items.length}</span>
                {isGroup && (
                  <button className="ghost-btn icon" aria-label={`Delete group ${c.name}`} title="Delete group (sessions become ungrouped)"
                    onClick={() => p.onDeleteGroup(c.id)}>×</button>
                )}
              </header>
              <ul className="sessions">
                {items.map((s) => <Card key={s.id} s={s} now={p.now} onResume={p.onResume} onCopy={p.onCopy} />)}
              </ul>
              {items.length === 0 && <p className="drop-hint">Drop sessions here</p>}
            </section>
          );
        })}
        {groupFilter === "all" && (
          <section className="column new-group">
            <input placeholder="New group…" value={newName} onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter" && newName.trim()) { p.onCreateGroup(newName.trim()); setNewName(""); } }} />
            <p className="drop-hint">Press Enter to add a column</p>
          </section>
        )}
      </div>
    </div>
  );
}
