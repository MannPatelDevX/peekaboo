import { render, screen } from "@testing-library/react";
import { SessionList } from "./SessionList";

test("renders sessions and empty state", () => {
  const { rerender } = render(<SessionList sessions={[]} />);
  expect(screen.getByText(/No active agents/)).toBeInTheDocument();
  rerender(<SessionList sessions={[{ key: "k", agent: "claude", cwd: "/w/shop", project: "shop", state: "waitingApproval", lastAction: "Needs approval: Bash", updatedAt: 1 }]} />);
  expect(screen.getByText("shop")).toBeInTheDocument();
  expect(screen.getByText("Needs approval: Bash")).toBeInTheDocument();
  expect(screen.getByText("Waiting for you")).toBeInTheDocument();
});
