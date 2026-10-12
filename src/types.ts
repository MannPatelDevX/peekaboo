export type SessionState = "working" | "waitingApproval" | "waitingAnswer" | "idle" | "ended";
export interface Session { key: string; agent: string; cwd: string | null; project: string; state: SessionState; lastAction: string | null; updatedAt: number; title?: string | null; tty?: string | null; queued?: number }
export type PromptRoute = "terminal" | "queued";
export interface QuestionOption { label: string; description?: string | null }
export interface Question { question: string; header?: string | null; options: QuestionOption[]; multiSelect: boolean }
interface PendingBase { id: string; agent: string; sessionKey: string; project: string; createdAt: number }
export type PendingView =
  | (PendingBase & { kind: "permission"; tool: string; input: Record<string, unknown> | null })
  | (PendingBase & { kind: "question"; questions: Question[] });
export interface AlwaysRule { agent: string; tool: string; cwd: string | null; command: string | null }
export interface HookStatus { agent: string; installed: boolean; stale: boolean; configPath: string; error: string | null }
export interface AppState { sessions: Session[]; pending: PendingView[]; rules: AlwaysRule[]; hooks: HookStatus[] }
export interface Answer { question: string; answer: string }
export type Decision =
  | { type: "allow"; always: boolean }
  | { type: "deny"; reason?: string | null }
  | { type: "passthrough" }
  | { type: "answer"; answers: Answer[] };

export interface TranscriptRow {
  id: string; cwd: string | null; project: string; branch: string | null; title: string;
  lastActivity: number; model: string | null; contextTokens: number | null; contextWindow: number;
  live: SessionState | null; resumeCommand: string | null;
}
export interface Gauge { pct: number | null; at: string | null }
export interface UsageRow { account: string; active: boolean; tier: string; session: Gauge; week: Gauge; updated: string }
export interface AccountRow { account: string; active: boolean; email: string; org: string; plan: string; saved: string }
export interface Section<T> { data: T | null; raw: string; error: string | null }
export interface ClaudexSnapshot {
  installed: boolean; installUrl: string;
  usage: Section<UsageRow[]>; accounts: Section<AccountRow[]>;
  current: Section<{ account: string; email: string; org: string; plan: string }>;
  pool: Section<{ consuming: { on: boolean; detail: string }; sharing: { on: boolean; detail: string } }>;
  autoswitch: boolean | null;
}
export type ClaudexAction =
  | { type: "switch"; account: string }
  | { type: "pool"; on: boolean }
  | { type: "autoswitch"; on: boolean };

export interface Group { id: string; name: string }
export interface GroupData { groups: Group[]; assignments: Record<string, string> }
export type DateRange = "today" | "yesterday" | "7d" | "30d";
