import type { Decision, PendingView } from "./types";

export interface KeyContext {
  expanded: boolean;
  tab: "sessions" | "settings";
  first: PendingView | undefined;
}

/**
 * Keyboard shortcuts for the overlay. Only acts on a card the user can actually see,
 * and ignores modifier combos and auto-repeat so Cmd+A or a held key never approves anything.
 */
export function keyAction(e: KeyboardEvent, ctx: KeyContext): { id: string; decision: Decision } | "collapse" | null {
  if (e.key === "Escape") return "collapse";
  if (e.repeat || e.metaKey || e.ctrlKey || e.altKey) return null;
  if (!ctx.expanded || ctx.tab !== "sessions" || ctx.first?.kind !== "permission") return null;
  if (e.key === "a") return { id: ctx.first.id, decision: { type: "allow", always: false } };
  if (e.key === "d") return { id: ctx.first.id, decision: { type: "deny" } };
  return null;
}
