import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "./api";
import type { AppState, Decision } from "./types";
import { Pill } from "./components/Pill";
import { SessionList } from "./components/SessionList";
import { ApprovalCard } from "./components/ApprovalCard";
import { QuestionCard } from "./components/QuestionCard";
import { SettingsPanel } from "./components/SettingsPanel";
import { Ghost } from "./components/Ghost";
import { keyAction } from "./keys";

const EMPTY: AppState = { sessions: [], pending: [], rules: [], hooks: [] };

export default function App() {
  const [state, setState] = useState<AppState>(EMPTY);
  const [expanded, setExpandedState] = useState(false);
  const [tab, setTab] = useState<"sessions" | "settings">("sessions");
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

  const onDecide = useCallback((id: string) => (d: Decision) => {
    api.decide(id, d).catch(console.error).finally(refresh);
  }, [refresh]);
  const guard = (p: Promise<unknown>) => p.then(refresh).catch((e) => alert(String(e)));

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return;
      const action = keyAction(e, { expanded, tab, first: state.pending[0] });
      if (action === "collapse") setExpanded(false);
      else if (action) onDecide(action.id)(action.decision);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [state.pending, expanded, tab, setExpanded, onDecide]);

  const waiting = state.pending.length;
  if (!expanded) return <Pill active={state.sessions.length} waiting={waiting} onOpen={() => setExpanded(true)} />;

  return (
    <div className="panel" onMouseLeave={() => waiting === 0 && setExpanded(false)}>
      <header className="row">
        <Ghost size={26} alert={waiting > 0} />
        <nav>
          <button className={tab === "sessions" ? "on" : ""} onClick={() => setTab("sessions")}>Agents</button>
          <button className={tab === "settings" ? "on" : ""} onClick={() => setTab("settings")}>Settings</button>
        </nav>
      </header>
      {tab === "sessions" ? (
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
