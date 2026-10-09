import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApprovalCard } from "./ApprovalCard";

const req = { id: "1", agent: "claude", sessionKey: "k", project: "shop", createdAt: 0,
  kind: "permission" as const, tool: "Bash", input: { command: "rm -rf build" } };

test("shows tool and command and sends decisions", async () => {
  const onDecide = vi.fn();
  render(<ApprovalCard req={req} onDecide={onDecide} />);
  expect(screen.getByText("Bash")).toBeInTheDocument();
  expect(screen.getByText("rm -rf build")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Allow" }));
  await userEvent.click(screen.getByRole("button", { name: "Always" }));
  await userEvent.click(screen.getByRole("button", { name: "Deny" }));
  await userEvent.click(screen.getByRole("button", { name: "Terminal" }));
  expect(onDecide.mock.calls.map((c) => c[0])).toEqual([
    { type: "allow", always: false }, { type: "allow", always: true }, { type: "deny" }, { type: "passthrough" },
  ]);
});
