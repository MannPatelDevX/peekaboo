import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AccountsPanel } from "./AccountsPanel";
import type { ClaudexSnapshot } from "../types";

const g = (pct: number | null, at: string | null = null) => ({ pct, at });
const snap = (over: Partial<ClaudexSnapshot> = {}): ClaudexSnapshot => ({
  installed: true, installUrl: "https://example.com/install",
  usage: { raw: "", error: null, data: [
    { account: "alice", active: false, tier: "Max 5x", session: g(9), week: g(9, "Aug 23"), updated: "live" },
    { account: "brian", active: true, tier: "Max 5x", session: g(32), week: g(87, "Aug 20"), updated: "live" },
  ] },
  accounts: { raw: "", error: null, data: [] },
  current: { raw: "", error: null, data: { account: "brian", email: "b@x.com", org: "acme", plan: "team" } },
  pool: { raw: "", error: null, data: { consuming: { on: false, detail: "" }, sharing: { on: true, detail: "" } } },
  autoswitch: false,
  ...over,
});

test("shows gauges and switches only after confirming", async () => {
  const onAct = vi.fn();
  render(<AccountsPanel snap={snap()} loading={false} onRefresh={vi.fn()} onAct={onAct} />);
  expect(screen.getByText("87%")).toBeInTheDocument();
  expect(screen.getByText("Active")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Switch" }));
  expect(onAct).not.toHaveBeenCalled();
  expect(screen.getByText(/Switch this machine to alice/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(onAct).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "Switch" }));
  await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
  expect(onAct).toHaveBeenCalledWith({ type: "switch", account: "alice" });
});

test("pool and autoswitch toggles show the action that would change something", async () => {
  const onAct = vi.fn();
  render(<AccountsPanel snap={snap({ autoswitch: true })} loading={false} onRefresh={vi.fn()} onAct={onAct} />);
  await userEvent.click(screen.getByRole("button", { name: "Start pool" }));
  await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
  expect(onAct).toHaveBeenLastCalledWith({ type: "pool", on: true });
  await userEvent.click(screen.getByRole("button", { name: "Disable autoswitch" }));
  await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
  expect(onAct).toHaveBeenLastCalledWith({ type: "autoswitch", on: false });
});

test("not installed shows install link and no actions", () => {
  render(<AccountsPanel snap={snap({ installed: false })} loading={false} onRefresh={vi.fn()} onAct={vi.fn()} />);
  expect(screen.getByText(/claudex is not installed/)).toBeInTheDocument();
  expect(screen.getByRole("link", { name: /How to install/ })).toHaveAttribute("href", "https://example.com/install");
  expect(screen.queryByRole("button", { name: "Switch" })).not.toBeInTheDocument();
});

test("unrecognised output falls back to raw text", () => {
  render(<AccountsPanel snap={snap({ usage: { data: null, raw: "NEW FORMAT TABLE", error: null } })} loading={false} onRefresh={vi.fn()} onAct={vi.fn()} />);
  expect(screen.getByText("NEW FORMAT TABLE")).toBeInTheDocument();
});

test("nothing can be submitted again while claudex is busy", async () => {
  const onAct = vi.fn();
  const { rerender } = render(<AccountsPanel snap={snap()} loading={false} onRefresh={vi.fn()} onAct={onAct} />);
  await userEvent.click(screen.getByRole("button", { name: "Switch" }));
  rerender(<AccountsPanel snap={snap()} loading={true} onRefresh={vi.fn()} onAct={onAct} />);
  for (const name of ["Confirm", "Switch", "Start pool", "Enable autoswitch"]) {
    expect(screen.getByRole("button", { name })).toBeDisabled();
  }
  await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
  expect(onAct).not.toHaveBeenCalled();
});
