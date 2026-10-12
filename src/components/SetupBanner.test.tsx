import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SetupBanner } from "./SetupBanner";
import type { HookStatus } from "../types";

const hook = (over: Partial<HookStatus> = {}): HookStatus => ({ agent: "claude", installed: true, stale: false, configPath: "x", error: null, ...over });

test("not installed: one button sets up messaging for every session", async () => {
  const onSetup = vi.fn().mockResolvedValue(undefined);
  render(<SetupBanner hook={hook({ installed: false })} unreachable={0} onSetup={onSetup} />);
  expect(screen.getByText(/Messaging Claude from Peekaboo isn't set up/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Set up messaging" }));
  expect(onSetup).toHaveBeenCalled();
});

test("outdated hooks: same button, update wording", () => {
  render(<SetupBanner hook={hook({ stale: true })} unreachable={0} onSetup={vi.fn()} />);
  expect(screen.getByText(/needs an update/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Set up messaging" })).toBeInTheDocument();
});

test("set up, but some sessions started before it", () => {
  render(<SetupBanner hook={hook()} unreachable={2} onSetup={vi.fn()} />);
  expect(screen.getByText(/2 idle sessions started before setup/)).toBeInTheDocument();
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});

test("all good: nothing shown", () => {
  const { container } = render(<SetupBanner hook={hook()} unreachable={0} onSetup={vi.fn()} />);
  expect(container).toBeEmptyDOMElement();
});

test("config can't be read: shows the problem instead of a button", () => {
  render(<SetupBanner hook={hook({ installed: false, error: "config is not a valid JSON object" })} unreachable={0} onSetup={vi.fn()} />);
  expect(screen.getByText(/not a valid JSON object/)).toBeInTheDocument();
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});
