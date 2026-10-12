import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SessionsBoard } from "./SessionsBoard";
import type { GroupData, TranscriptRow } from "../types";

// 2026-10-12 15:00 local
const NOW = Math.floor(new Date(2026, 9, 12, 15, 0).getTime() / 1000);
const H = 3600;

const row = (over: Partial<TranscriptRow>): TranscriptRow => ({
  id: "a", cwd: "/w/shop", project: "shop", branch: "main", title: "Fix checkout", lastActivity: NOW - 60,
  model: "claude-opus-5-5", contextTokens: 50_000, contextWindow: 200_000, live: null,
  resumeCommand: "cd '/w/shop' && claude --resume a", ...over,
});

const sessions = [
  row({}),
  row({ id: "b", title: "CMS schema", project: "admin", lastActivity: NOW - 20 * H, live: "waitingApproval", contextTokens: 190_000 }),
  row({ id: "c", title: "Old docs", project: "site", branch: "release", lastActivity: NOW - 3 * 24 * H }),
];
const groups: GroupData = { groups: [{ id: "g1", name: "Foxtale" }, { id: "g2", name: "Empty" }], assignments: { b: "g1" } };

function setup(over: Partial<React.ComponentProps<typeof SessionsBoard>> = {}) {
  const p = {
    sessions, groups, now: NOW, onResume: vi.fn(), onCopy: vi.fn(), onAssign: vi.fn(),
    onCreateGroup: vi.fn(), onRenameGroup: vi.fn(), onDeleteGroup: vi.fn(), onAutoGroup: vi.fn(), ...over,
  };
  render(<SessionsBoard {...p} />);
  return p;
}
const column = (name: string) => screen.getByRole("region", { name: new RegExp(`^${name}`) });

test("one column per group plus Ungrouped", () => {
  setup();
  expect(within(column("Foxtale")).getByText("CMS schema")).toBeInTheDocument();
  expect(within(column("Ungrouped")).getByText("Fix checkout")).toBeInTheDocument();
  expect(within(column("Ungrouped")).getByText("Old docs")).toBeInTheDocument();
  expect(within(column("Empty")).getByText(/Drop sessions here/)).toBeInTheDocument();
  const order = screen.getAllByRole("region").map((r) => r.getAttribute("aria-label"));
  expect(order).toEqual(["Foxtale (1)", "Empty (0)", "Ungrouped (2)"]);
});

test("cards keep gauge, live badge, resume and copy", async () => {
  const p = setup();
  expect(screen.getByText("95%").previousSibling).toHaveClass("danger");
  expect(screen.getByText("Waiting for you")).toBeInTheDocument();
  await userEvent.click(within(column("Ungrouped")).getAllByRole("button", { name: "Resume" })[0]);
  expect(p.onResume).toHaveBeenCalledWith("a");
  await userEvent.click(within(column("Ungrouped")).getAllByRole("button", { name: "Copy command" })[0]);
  expect(p.onCopy).toHaveBeenCalledWith("cd '/w/shop' && claude --resume a");
});

test("drag a card onto a column to group or ungroup it", () => {
  const p = setup();
  const data: Record<string, string> = {};
  const dataTransfer = { setData: (k: string, v: string) => { data[k] = v; }, getData: (k: string) => data[k], effectAllowed: "", dropEffect: "" };
  fireEvent.dragStart(screen.getByText("Fix checkout").closest("li")!, { dataTransfer });
  fireEvent.dragOver(column("Empty"), { dataTransfer });
  fireEvent.drop(column("Empty"), { dataTransfer });
  expect(p.onAssign).toHaveBeenCalledWith("a", "g2");
  fireEvent.dragStart(screen.getByText("CMS schema").closest("li")!, { dataTransfer });
  fireEvent.drop(column("Ungrouped"), { dataTransfer });
  expect(p.onAssign).toHaveBeenLastCalledWith("b", null);
});

test("filter by group", async () => {
  setup();
  await userEvent.selectOptions(screen.getByLabelText("Group"), "g1");
  expect(screen.getAllByRole("region").map((r) => r.getAttribute("aria-label"))).toEqual(["Foxtale (1)"]);
  await userEvent.selectOptions(screen.getByLabelText("Group"), "ungrouped");
  expect(screen.getAllByRole("region").map((r) => r.getAttribute("aria-label"))).toEqual(["Ungrouped (2)"]);
});

test("filter by date", async () => {
  setup();
  await userEvent.selectOptions(screen.getByLabelText("Date"), "today");
  expect(screen.queryByText("CMS schema")).not.toBeInTheDocument(); // 20h ago = yesterday
  expect(screen.getByText("Fix checkout")).toBeInTheDocument();
  await userEvent.selectOptions(screen.getByLabelText("Date"), "yesterday");
  expect(screen.getByText("CMS schema")).toBeInTheDocument();
  expect(screen.queryByText("Fix checkout")).not.toBeInTheDocument();
  await userEvent.selectOptions(screen.getByLabelText("Date"), "7d");
  expect(screen.getByText("Old docs")).toBeInTheDocument();
});

test("search still filters", async () => {
  setup();
  await userEvent.type(screen.getByPlaceholderText("Search sessions…"), "release");
  expect(screen.queryByText("Fix checkout")).not.toBeInTheDocument();
  expect(screen.getByText("Old docs")).toBeInTheDocument();
});

test("create, rename, delete and auto-group", async () => {
  const p = setup();
  await userEvent.type(screen.getByPlaceholderText("New group…"), "Peekaboo{Enter}");
  expect(p.onCreateGroup).toHaveBeenCalledWith("Peekaboo");
  await userEvent.dblClick(within(column("Foxtale")).getByText("Foxtale"));
  const input = within(column("Foxtale")).getByDisplayValue("Foxtale");
  await userEvent.clear(input);
  await userEvent.type(input, "Foxtale CMS{Enter}");
  expect(p.onRenameGroup).toHaveBeenCalledWith("g1", "Foxtale CMS");
  await userEvent.click(screen.getByRole("button", { name: "Delete group Empty" }));
  expect(p.onDeleteGroup).toHaveBeenCalledWith("g2");
  await userEvent.click(screen.getByRole("button", { name: /Auto-group by project/ }));
  expect(p.onAutoGroup).toHaveBeenCalled();
});

test("empty state", () => {
  setup({ sessions: [], groups: { groups: [], assignments: {} } });
  expect(screen.getByText(/No Claude Code sessions/)).toBeInTheDocument();
});

test("focusing a session clears filters and highlights its card", async () => {
  const p = { sessions, groups, now: NOW, onResume: vi.fn(), onCopy: vi.fn(), onAssign: vi.fn(),
    onCreateGroup: vi.fn(), onRenameGroup: vi.fn(), onDeleteGroup: vi.fn(), onAutoGroup: vi.fn() };
  const { rerender } = render(<SessionsBoard {...p} />);
  await userEvent.selectOptions(screen.getByLabelText("Date"), "today");
  expect(screen.queryByText("Old docs")).not.toBeInTheDocument();
  rerender(<SessionsBoard {...p} focusId="c" />);
  expect(screen.getByText("Old docs").closest("li")).toHaveClass("focused");
  expect(screen.getByLabelText("Date")).toHaveValue("30d");
});
