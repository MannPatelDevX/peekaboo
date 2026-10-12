import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SettingsPanel } from "./SettingsPanel";

test("install, uninstall and remove rule", async () => {
  const p = { onInstall: vi.fn(), onUninstall: vi.fn(), onRemoveRule: vi.fn() };
  const { rerender } = render(<SettingsPanel hooks={[{ agent: "claude", installed: false, stale: false, configPath: "~/.claude/settings.json", error: null }]} rules={[]} {...p} />);
  await userEvent.click(screen.getByRole("button", { name: "Install" }));
  expect(p.onInstall).toHaveBeenCalledWith("claude");
  rerender(<SettingsPanel hooks={[{ agent: "claude", installed: true, stale: false, configPath: "x", error: null }]} rules={[{ agent: "claude", tool: "Read", cwd: "/w/shop", command: null }]} {...p} />);
  await userEvent.click(screen.getByRole("button", { name: "Uninstall" }));
  expect(p.onUninstall).toHaveBeenCalledWith("claude");
  await userEvent.click(screen.getByRole("button", { name: "Remove Read" }));
  expect(p.onRemoveRule).toHaveBeenCalledWith({ agent: "claude", tool: "Read", cwd: "/w/shop", command: null });
});

test("stale hooks offer repair and rules show their scope", async () => {
  const p = { onInstall: vi.fn(), onUninstall: vi.fn(), onRemoveRule: vi.fn() };
  render(<SettingsPanel hooks={[{ agent: "claude", installed: true, stale: true, configPath: "x", error: null }]}
    rules={[{ agent: "claude", tool: "Bash", cwd: "/w/shop", command: "npm test" }]} {...p} />);
  await userEvent.click(screen.getByRole("button", { name: "Repair" }));
  expect(p.onInstall).toHaveBeenCalledWith("claude");
  expect(screen.getByText(/npm test/)).toBeInTheDocument();
  expect(screen.getByText(/shop/)).toBeInTheDocument();
});

const terminals = [
  { id: "terminal" as const, name: "Terminal", realPrompts: true, runsCommand: true },
  { id: "warp" as const, name: "Warp", realPrompts: false, runsCommand: false },
];

test("lists the terminal apps found on this Mac and what each can do", async () => {
  const p = { onInstall: vi.fn(), onUninstall: vi.fn(), onRemoveRule: vi.fn(), onOpenIn: vi.fn() };
  render(<SettingsPanel hooks={[]} rules={[]} openIn="terminal" terminals={terminals} {...p} />);
  const options = screen.getAllByRole("option").map((o) => o.textContent);
  expect(options).toEqual(["Terminal — real prompts", "Warp — messages as notes, paste to resume"]);
  await userEvent.selectOptions(screen.getByLabelText("Open Claude sessions in"), "warp");
  expect(p.onOpenIn).toHaveBeenCalledWith("warp");
});

test("explains the chosen app's limits", () => {
  const p = { onInstall: vi.fn(), onUninstall: vi.fn(), onRemoveRule: vi.fn(), onOpenIn: vi.fn() };
  render(<SettingsPanel hooks={[]} rules={[]} openIn="warp" terminals={terminals} {...p} />);
  expect(screen.getByText(/Warp can't be typed into/)).toBeInTheDocument();
});
