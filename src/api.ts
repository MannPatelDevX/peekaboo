import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AlwaysRule, AppState, ClaudexAction, ClaudexSnapshot, Decision, TranscriptRow } from "./types";

export const getState = () => invoke<AppState>("get_state");
export const decide = (id: string, decision: Decision) => invoke<void>("decide", { id, decision });
export const installHooks = (agent: string) => invoke<void>("install_hooks", { agent });
export const uninstallHooks = (agent: string) => invoke<void>("uninstall_hooks", { agent });
export const removeRule = (rule: AlwaysRule) => invoke<void>("remove_rule", { rule });
export const getNotch = () => invoke<{ height: number; width: number }>("get_notch");
export const setExpanded = (expanded: boolean) => invoke<void>("set_expanded", { expanded });
export const onStateChanged = (cb: () => void) => listen("state:changed", () => cb());
export const onToggle = (cb: () => void) => listen("overlay:toggle", () => cb());
export const listTranscripts = () => invoke<TranscriptRow[]>("list_transcripts");
export const resumeSession = (id: string) => invoke<void>("resume_session", { id });
export const claudexSnapshot = (force: boolean) => invoke<ClaudexSnapshot>("claudex_snapshot", { force });
export const claudexAct = (action: ClaudexAction) => invoke<string>("claudex_act", { action });
