import { useState } from "react";
import type { HookStatus } from "../types";

interface Props {
  /** Claude Code hook status, if known. */
  hook: HookStatus | undefined;
  /** Idle sessions Peekaboo can't reach yet (started before setup). */
  unreachable: number;
  onSetup: () => Promise<unknown>;
}

/** One global switch for messaging every Claude session from Peekaboo. */
export function SetupBanner({ hook, unreachable, onSetup }: Props) {
  const [busy, setBusy] = useState(false);
  if (!hook) return null;

  if (hook.error) {
    return <div className="banner warn">Peekaboo can't read your Claude settings: {hook.error}</div>;
  }
  if (!hook.installed || hook.stale) {
    return (
      <div className="banner">
        <span>
          {hook.installed
            ? "Messaging Claude from Peekaboo needs an update."
            : "Messaging Claude from Peekaboo isn't set up."}{" "}
          One click turns it on for every session.
        </span>
        <button className="primary small" disabled={busy}
          onClick={() => { setBusy(true); onSetup().finally(() => setBusy(false)); }}>
          {busy ? "Setting up…" : "Set up messaging"}
        </button>
      </div>
    );
  }
  if (unreachable > 0) {
    return (
      <div className="banner info">
        {unreachable} idle {unreachable === 1 ? "session" : "sessions"} started before setup. Each becomes reachable after its
        next turn — send one prompt in its terminal, or restart it.
      </div>
    );
  }
  return null;
}
