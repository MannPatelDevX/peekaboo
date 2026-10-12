import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SessionList } from "./SessionList";
import type { Session } from "../types";

const s = (over: Partial<Session> = {}): Session => ({
  key: "claude:abc", agent: "claude", cwd: "/w/shop", project: "shop", state: "working",
  lastAction: "Bash: npm test", updatedAt: 1, title: "Fix the checkout bug", tty: "ttys004", queued: 0, ...over,
});
const props = () => ({ onShowOnBoard: vi.fn(), onSend: vi.fn().mockResolvedValue("terminal") });

test("empty state", () => {
  render(<SessionList sessions={[]} {...props()} />);
  expect(screen.getByText(/No active agents/)).toBeInTheDocument();
});

test("shows the real title, falls back to the project", () => {
  const { rerender } = render(<SessionList sessions={[s()]} {...props()} />);
  expect(screen.getByText("Fix the checkout bug")).toBeInTheDocument();
  expect(screen.getByText("shop")).toBeInTheDocument();
  expect(screen.getByText("Bash: npm test")).toBeInTheDocument();
  rerender(<SessionList sessions={[s({ title: null, state: "waitingApproval" })]} {...props()} />);
  expect(screen.getByRole("heading", { name: "shop" })).toBeInTheDocument();
  expect(screen.getByText("Waiting for you")).toBeInTheDocument();
});

test("show on board passes the session id", async () => {
  const p = props();
  render(<SessionList sessions={[s()]} {...p} />);
  await userEvent.click(screen.getByRole("button", { name: "Show on board" }));
  expect(p.onShowOnBoard).toHaveBeenCalledWith("abc");
});

test("send a prompt with Enter and report where it went", async () => {
  const p = props();
  render(<SessionList sessions={[s()]} {...p} />);
  const box = screen.getByPlaceholderText("Message Claude…");
  await userEvent.type(box, "now add tests{Enter}");
  expect(p.onSend).toHaveBeenCalledWith("claude:abc", "now add tests");
  expect(await screen.findByText("Typed into Terminal")).toBeInTheDocument();
  expect(box).toHaveValue("");
});

test("queued prompts explain when they will arrive", async () => {
  const p = { ...props(), onSend: vi.fn().mockResolvedValue("queued") };
  const { rerender } = render(<SessionList sessions={[s({ tty: null })]} {...p} />);
  await userEvent.type(screen.getByPlaceholderText(/Message Claude/), "hi{Enter}");
  expect(await screen.findByText(/sent when Claude finishes its current turn/)).toBeInTheDocument();
  rerender(<SessionList sessions={[s({ tty: null, queued: 2 })]} {...p} />);
  expect(screen.getByText("2 queued")).toBeInTheDocument();
});

test("send failures are shown and the text is kept", async () => {
  const p = { ...props(), onSend: vi.fn().mockRejectedValue("that session is no longer active") };
  render(<SessionList sessions={[s()]} {...p} />);
  await userEvent.type(screen.getByPlaceholderText("Message Claude…"), "hi{Enter}");
  expect(await screen.findByText("that session is no longer active")).toBeInTheDocument();
  expect(screen.getByPlaceholderText("Message Claude…")).toHaveValue("hi");
});

test("idle sessions in terminals Peekaboo can't type into say so up front", () => {
  render(<SessionList sessions={[s({ state: "idle", terminal: "Warp" })]} {...props()} />);
  expect(screen.getByText("Warp")).toBeInTheDocument();
  expect(screen.getByPlaceholderText(/Claude is idle in Warp/)).toBeDisabled();
});

test("busy sessions in other terminals can still queue", () => {
  render(<SessionList sessions={[s({ state: "working", terminal: "Warp" })]} {...props()} />);
  expect(screen.getByPlaceholderText(/sent when Claude finishes this turn/)).toBeEnabled();
});
