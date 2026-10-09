export type SessionState = "working" | "waitingApproval" | "waitingAnswer" | "idle" | "ended";
export interface Session { key: string; agent: string; cwd: string | null; project: string; state: SessionState; lastAction: string | null; updatedAt: number }
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
