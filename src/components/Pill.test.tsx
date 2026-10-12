import { render, screen } from "@testing-library/react";
import { Pill } from "./Pill";

test("shows the active account's weekly usage with a warning level", () => {
  const { rerender } = render(<Pill active={1} waiting={0} weekPct={87} onOpen={vi.fn()} />);
  expect(screen.getByText("87%")).toHaveClass("usage", "danger");
  rerender(<Pill active={1} waiting={0} weekPct={55} onOpen={vi.fn()} />);
  expect(screen.getByText("55%")).toHaveClass("warn");
  rerender(<Pill active={1} waiting={0} weekPct={null} onOpen={vi.fn()} />);
  expect(screen.queryByText(/%/)).not.toBeInTheDocument();
});
