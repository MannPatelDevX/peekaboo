import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SettingsPanel } from "./SettingsPanel";

test("install, uninstall and remove rule", async () => {
  const p = { onInstall: vi.fn(), onUninstall: vi.fn(), onRemoveRule: vi.fn() };
  const { rerender } = render(<SettingsPanel hooks={[{ agent: "claude", installed: false, configPath: "~/.claude/settings.json", error: null }]} rules={[]} {...p} />);
  await userEvent.click(screen.getByRole("button", { name: "Install" }));
  expect(p.onInstall).toHaveBeenCalledWith("claude");
  rerender(<SettingsPanel hooks={[{ agent: "claude", installed: true, configPath: "x", error: null }]} rules={[{ agent: "claude", tool: "Read" }]} {...p} />);
  await userEvent.click(screen.getByRole("button", { name: "Uninstall" }));
  expect(p.onUninstall).toHaveBeenCalledWith("claude");
  await userEvent.click(screen.getByRole("button", { name: "Remove Read" }));
  expect(p.onRemoveRule).toHaveBeenCalledWith({ agent: "claude", tool: "Read" });
});
