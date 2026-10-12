import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SessionsPanel } from "./SessionsPanel";
import type { TranscriptRow } from "../types";

const row = (over: Partial<TranscriptRow>): TranscriptRow => ({
  id: "a", cwd: "/w/shop", project: "shop", branch: "main", title: "Fix checkout", lastActivity: 1000,
  model: "claude-opus-5-5", contextTokens: 50_000, contextWindow: 200_000, live: null,
  resumeCommand: "cd '/w/shop' && claude --resume a", ...over,
});

test("lists sessions with context gauge, live badge and actions", async () => {
  const p = { onResume: vi.fn(), onCopy: vi.fn() };
  render(<SessionsPanel now={1060} sessions={[row({}), row({ id: "b", title: "Docs", project: "site", live: "waitingApproval", contextTokens: 190_000 })]} {...p} />);
  expect(screen.getByText("Fix checkout")).toBeInTheDocument();
  expect(screen.getByText("25%")).toBeInTheDocument();
  expect(screen.getByText("95%").previousSibling).toHaveClass("danger");
  expect(screen.getByText("Waiting for you")).toBeInTheDocument();
  expect(screen.getAllByText(/1m ago/).length).toBeGreaterThan(0);
  await userEvent.click(screen.getAllByRole("button", { name: "Resume" })[0]);
  expect(p.onResume).toHaveBeenCalledWith("a");
  await userEvent.click(screen.getAllByRole("button", { name: "Copy command" })[0]);
  expect(p.onCopy).toHaveBeenCalledWith("cd '/w/shop' && claude --resume a");
});

test("search filters by title, project and branch", async () => {
  render(<SessionsPanel now={1000} sessions={[row({}), row({ id: "b", title: "Docs", project: "site", branch: "release" })]} onResume={vi.fn()} onCopy={vi.fn()} />);
  await userEvent.type(screen.getByPlaceholderText("Search sessions…"), "release");
  expect(screen.queryByText("Fix checkout")).not.toBeInTheDocument();
  expect(screen.getByText("Docs")).toBeInTheDocument();
});

test("empty state", () => {
  render(<SessionsPanel now={0} sessions={[]} onResume={vi.fn()} onCopy={vi.fn()} />);
  expect(screen.getByText(/No Claude Code sessions/)).toBeInTheDocument();
});
