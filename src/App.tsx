import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import * as api from "./api";
import type { Preferences, AppState, ClaudexAction, ClaudexSnapshot, Decision, GroupData, TranscriptRow } from "./types";
import { Pill } from "./components/Pill";
import { SessionList } from "./components/SessionList";
import { SetupBanner } from "./components/SetupBanner";
import { ApprovalCard } from "./components/ApprovalCard";
import { QuestionCard } from "./components/QuestionCard";
import { SettingsPanel } from "./components/SettingsPanel";
import { keyAction } from "./keys";
import { SessionsBoard } from "./components/SessionsBoard";
import { AccountsPanel } from "./components/AccountsPanel";
import { AccountsIcon, AgentsIcon, SessionsIcon, SettingsIcon } from "./components/icons";

type Tab = "sessions" | "transcripts" | "accounts" | "settings";
const CLAUDEX_POLL_MS = 5 * 60_000;
const nowSecs = () => Math.floor(Date.now() / 1000);
/** Matches the island-close animation in styles.css. */
const CLOSE_MS = 180;

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
  const [notchWidth, setNotchWidth] = useState(0);
  const [groups, setGroups] = useState<GroupData>({ groups: [], assignments: {} });
  const [focusId, setFocusId] = useState<string | null>(null);
  const [prefs, setPrefs] = useState<Preferences | null>(null);
  const [closing, setClosing] = useState(false);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const tabRef = useRef<Tab>("sessions");
  tabRef.current = tab;
  const openSize = (t: Tab): api.WindowSize => (t === "transcripts" ? "board" : "panel");
  const expandedRef = useRef(false);
  const lastPending = useRef(0);

  const refresh = useCallback(() => {
    api.getState().then(setState).catch(console.error);
  }, []);
  // Opening resizes the window first and lets CSS spring the island open; closing plays the
  // shrink animation first and only then shrinks the window.
  const setExpanded = useCallback((v: boolean) => {
    if (v === expandedRef.current) return;
    expandedRef.current = v;
    clearTimeout(closeTimer.current);
    if (v) {
      setClosing(false);
      setExpandedState(true);
      api.setSize(openSize(tabRef.current)).catch(console.error);
    } else {
      setClosing(true);
      closeTimer.current = setTimeout(() => {
        setClosing(false);
        setExpandedState(false);
        api.setSize("collapsed").catch(console.error);
      }, CLOSE_MS);
    }
  }, []);

  // Lay the UI out around the camera: content goes in the "ears" and below the notch band.
  useEffect(() => {
    api.getNotch()
      .then((n) => {
        document.documentElement.style.setProperty("--notch", `${n.height}px`);
        setNotchWidth(n.width);
      })
      .catch(console.error);
  }, []);

  useEffect(() => {
    refresh();
    const subs = [api.onStateChanged(refresh), api.onToggle(() => setExpanded(!expandedRef.current))];
    return () => {
      subs.forEach((p) => p.then((un) => un()));
    };
  }, [refresh, setExpanded]);

  // Peek out on the Agents tab whenever a new request arrives, whichever tab was open.
  useEffect(() => {
    if (state.pending.length > lastPending.current) {
      setTab("sessions");
      setNotice(null);
      setExpanded(true);
    }
    lastPending.current = state.pending.length;
  }, [state.pending.length, setExpanded]);

  // The board needs a wider window than the other tabs.
  useEffect(() => {
    if (expanded) api.setSize(openSize(tab)).catch(console.error);
  }, [expanded, tab]);

  useEffect(() => {
    if (expanded && tab === "settings") api.getPreferences().then(setPrefs).catch(console.error);
  }, [expanded, tab]);

  const groupOp = (p: Promise<GroupData>) => p.then(setGroups).catch((e) => setNotice(String(e)));
  useEffect(() => {
    if (expanded && tab === "transcripts") groupOp(api.getGroups());
  }, [expanded, tab]);

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
  if (!expanded) {
    return <Pill active={state.sessions.length} waiting={waiting} weekPct={weekPct} notchWidth={notchWidth} onOpen={() => setExpanded(true)} />;
  }
  const tabButton = (t: Tab, label: string, icon: ReactNode, badge = 0) => (
    <button role="tab" aria-selected={tab === t} className={tab === t ? "on" : ""} onClick={() => { setTab(t); setNotice(null); setFocusId(null); }}>
      {icon}
      <span>{label}</span>
      {badge > 0 && <span className="tab-badge">{badge}</span>}
    </button>
  );

  return (
    <div className={`panel ${closing ? "closing" : "opening"}`} onMouseLeave={() => waiting === 0 && setExpanded(false)}>
      <div className="notch-band" />
      <nav className="tabs" role="tablist">
        {tabButton("sessions", "Agents", <AgentsIcon />, waiting)}
        {tabButton("transcripts", "Sessions", <SessionsIcon />)}
        {tabButton("accounts", "Accounts", <AccountsIcon />)}
        {tabButton("settings", "Settings", <SettingsIcon />)}
      </nav>
      {notice && <p className="notice" onClick={() => setNotice(null)}>{notice}</p>}
      {tab === "transcripts" ? (
        <SessionsBoard
          sessions={transcripts}
          groups={groups}
          focusId={focusId}
          now={nowSecs()}
          onResume={(id) => api.resumeSession(id).then(() => setExpanded(false)).catch((e) => setNotice(String(e)))}
          onCopy={(cmd) => navigator.clipboard.writeText(cmd).then(() => setNotice("Copied resume command.")).catch((e) => setNotice(String(e)))}
          onAssign={(session, group) => {
            setGroups((g) => {
              const assignments = { ...g.assignments };
              if (group) assignments[session] = group;
              else delete assignments[session];
              return { ...g, assignments };
            });
            groupOp(api.assignSession(session, group));
          }}
          onCreateGroup={(name) => groupOp(api.createGroup(name))}
          onRenameGroup={(id, name) => groupOp(api.renameGroup(id, name))}
          onDeleteGroup={(id) => groupOp(api.deleteGroup(id))}
          onAutoGroup={() => groupOp(api.autoGroup())}
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
          <SetupBanner
            hook={state.hooks.find((h) => h.agent === "claude")}
            unreachable={state.sessions.filter((x) => x.state === "idle" && !x.reachable && x.terminal !== "Terminal" && x.terminal !== "iTerm2").length}
            onSetup={() => api.installHooks("claude").then(refresh).catch((e) => setNotice(String(e)))}
          />
          <SessionList
            sessions={state.sessions}
            onShowOnBoard={(id) => { setFocusId(id); setTab("transcripts"); }}
            onSend={(key, text) => api.sendPrompt(key, text).finally(refresh)}
          />
        </main>
      ) : (
        <SettingsPanel
          hooks={state.hooks}
          rules={state.rules}
          onInstall={(a) => guard(api.installHooks(a))}
          onUninstall={(a) => guard(api.uninstallHooks(a))}
          onRemoveRule={(r) => guard(api.removeRule(r))}
          openIn={prefs?.openIn}
          itermInstalled={prefs?.itermInstalled}
          onOpenIn={(v) => api.setOpenIn(v).then(setPrefs).catch((e) => setNotice(String(e)))}
        />
      )}
    </div>
  );
}
