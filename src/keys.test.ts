import { keyAction } from "./keys";
import type { PendingView } from "./types";

const perm: PendingView = { id: "p1", agent: "claude", sessionKey: "k", project: "shop", createdAt: 0,
  kind: "permission", tool: "Bash", input: { command: "ls" } };
const key = (k: string, extra: Partial<KeyboardEvent> = {}) =>
  ({ key: k, repeat: false, metaKey: false, ctrlKey: false, altKey: false, ...extra }) as KeyboardEvent;
const visible = { expanded: true, tab: "sessions" as const, first: perm };

test("a / d decide the visible first permission", () => {
  expect(keyAction(key("a"), visible)).toEqual({ id: "p1", decision: { type: "allow", always: false } });
  expect(keyAction(key("d"), visible)).toEqual({ id: "p1", decision: { type: "deny" } });
});

test("never decides what the user cannot see or did not mean", () => {
  expect(keyAction(key("a"), { ...visible, expanded: false })).toBeNull();
  expect(keyAction(key("a"), { ...visible, tab: "settings" })).toBeNull();
  expect(keyAction(key("a", { metaKey: true }), visible)).toBeNull();
  expect(keyAction(key("d", { ctrlKey: true }), visible)).toBeNull();
  expect(keyAction(key("a", { repeat: true }), visible)).toBeNull();
  expect(keyAction(key("a"), { ...visible, first: undefined })).toBeNull();
});

test("escape collapses", () => {
  expect(keyAction(key("Escape"), visible)).toBe("collapse");
});
