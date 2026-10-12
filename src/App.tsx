import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "./api";
import type { AppState, ClaudexAction, ClaudexSnapshot, Decision, TranscriptRow } from "./types";
import { Pill } from "./components/Pill";
import { SessionList } from "./components/SessionList";
import { ApprovalCard } from "./components/ApprovalCard";
import { QuestionCard } from "./components/QuestionCard";
import { SettingsPanel } from "./components/SettingsPanel";
import { Ghost } from "./components/Ghost";
import { keyAction } from "./keys";
import { SessionsPanel } from "./components/SessionsPanel";
import { AccountsPanel } from "./components/AccountsPanel";

type Tab = "sessions" | "transcripts" | "accounts" | "settings";
const CLAUDEX_POLL_MS = 5 * 60_000;
const nowSecs = () => Math.floor(Date.now() / 1000);

const EMPTY: AppState = { sessions: [], pending: [], rules: [], hooks: [] };

export default function App() {
  const [state, setState] = useState<AppState>(EMPTY);
  const [expanded, setExpandedState] = useState(false);
  const [tab, setTab] = useState<Tab>("sessions");
  const [transcripts, setTranscripts] = useState<TranscriptRow[]>([]);
  const [claudex, setClaudex] = useState<ClaudexSnapshot | null>(null);
  const [claudexLoading, setClaudexLoading] = useState(false);
  const [claudexActing, setClaudexActing] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const expandedRef = useRef(false);
  const lastPending = useRef(0);

  const refresh = useCallback(() => {
    api.getState().then(setState).catch(console.error);
  }, []);
  const setExpanded = useCallback((v: boolean) => {
    expandedRef.current = v;
    setExpandedState(v);
    api.setExpanded(v).catch(console.error);
  }, []);

  // Pad content below the notch; the strip behind it stays black and blends into the hardware.
  useEffect(() => {
    api.getTopInset()
      .then((px) => document.documentElement.style.setProperty("--notch", `${px}px`))
      .catch(console.error);
  }, []);

  useEffect(() => {
    refresh();
    const subs = [api.onStateChanged(refresh), api.onToggle(() => setExpanded(!expandedRef.current))];
    return () => {
      subs.forEach((p) => p.then((un) => un()));
    };
  }, [refresh, setExpanded]);

  // Peek out whenever a new request arrives.
  useEffect(() => {
    if (state.pending.length > lastPending.current) setExpanded(true);
    lastPending.current = state.pending.length;
  }, [state.pending.length, setExpanded]);

  const loadTranscripts = useCallback(() => {
    api.listTranscripts().then(setTranscripts).catch((e) => setNotice(String(e)));
  }, []);
  const loadClaudex = useCallback((force: boolean) => {
    setClaudexLoading(true);
    api.claudexSnapshot(force).then(setClaudex).catch((e) => setNotice(String(e))).finally(() => setClaudexLoading(false));
  }, []);
  const actClaudex = (a: ClaudexAction) => {
    setClaudexActing(true);
    api.claudexAct(a)
      .then((out) => setNotice(out || "Done."))
      .catch((e) => setNotice(String(e)))
      .finally(() => {
        setClaudexActing(false);
        loadClaudex(true);
      });
  };

  // Usage for the pill: once at start, then every few minutes (claudex itself caches for 60 s).
  useEffect(() => {
    loadClaudex(false);
    const t = setInterval(() => loadClaudex(false), CLAUDEX_POLL_MS);
    return () => clearInterval(t);
  }, [loadClaudex]);

  // Session list refreshes when its tab is open and whenever hook activity changes state.
  useEffect(() => {
    if (expanded && tab === "transcripts") loadTranscripts();
  }, [expanded, tab, loadTranscripts]);
  useEffect(() => {
    if (!expanded || tab !== "transcripts") return;
    const t = setTimeout(loadTranscripts, 750);
    return () => clearTimeout(t);
  }, [expanded, tab, state, loadTranscripts]);

  const onDecide = useCallback((id: string) => (d: Decision) => {
    api.decide(id, d).catch(console.error).finally(refresh);
  }, [refresh]);
  const guard = (p: Promise<unknown>) => p.then(refresh).catch((e) => alert(String(e)));

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return;
      const action = keyAction(e, { expanded, tab: tab === "sessions" ? "sessions" : "settings", first: state.pending[0] });
      if (action === "collapse") setExpanded(false);
      else if (action) onDecide(action.id)(action.decision);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [state.pending, expanded, tab, setExpanded, onDecide]);

  const waiting = state.pending.length;
  const weekPct = claudex?.usage.data?.find((u) => u.active)?.week.pct ?? null;
  if (!expanded) return <Pill active={state.sessions.length} waiting={waiting} weekPct={weekPct} onOpen={() => setExpanded(true)} />;
  const tabButton = (t: Tab, label: string) => (
    <button className={tab === t ? "on" : ""} onClick={() => { setTab(t); setNotice(null); }}>{label}</button>
  );

  return (
    <div className="panel" onMouseLeave={() => waiting === 0 && setExpanded(false)}>
      <header className="row">
        <Ghost size={26} alert={waiting > 0} />
        <nav>
          {tabButton("sessions", "Agents")}
          {tabButton("transcripts", "Sessions")}
          {tabButton("accounts", "Accounts")}
          {tabButton("settings", "Settings")}
        </nav>
      </header>
      {notice && <p className="notice" onClick={() => setNotice(null)}>{notice}</p>}
      {tab === "transcripts" ? (
        <SessionsPanel
          sessions={transcripts}
          now={nowSecs()}
          onResume={(id) => api.resumeSession(id).then(() => setExpanded(false)).catch((e) => setNotice(String(e)))}
          onCopy={(cmd) => navigator.clipboard.writeText(cmd).then(() => setNotice("Copied resume command.")).catch((e) => setNotice(String(e)))}
        />
      ) : tab === "accounts" ? (
        <AccountsPanel snap={claudex} loading={claudexLoading || claudexActing} onRefresh={() => loadClaudex(true)} onAct={actClaudex} />
      ) : tab === "sessions" ? (
        <main>
          {state.pending.map((p) =>
            p.kind === "permission" ? (
              <ApprovalCard key={p.id} req={p} onDecide={onDecide(p.id)} />
            ) : (
              <QuestionCard key={p.id} req={p} onDecide={onDecide(p.id)} />
            ),
          )}
          <SessionList sessions={state.sessions} />
        </main>
      ) : (
        <SettingsPanel
          hooks={state.hooks}
          rules={state.rules}
          onInstall={(a) => guard(api.installHooks(a))}
          onUninstall={(a) => guard(api.uninstallHooks(a))}
          onRemoveRule={(r) => guard(api.removeRule(r))}
        />
      )}
    </div>
  );
}
