import { render, screen } from "@testing-library/react";
import { Pill } from "./Pill";

test("shows the active account's session and weekly usage with warning levels", () => {
  const { rerender } = render(<Pill active={1} waiting={0} sessionPct={32} weekPct={87} onOpen={vi.fn()} />);
  expect(screen.getByText("87%")).toHaveClass("usage", "danger");
  expect(screen.getByText("32%")).toHaveClass("usage", "ok");
  expect(screen.getByTitle(/5-hour session limit: 32%/)).toBeInTheDocument();
  expect(screen.getByTitle(/Weekly limit: 87%/)).toBeInTheDocument();
  rerender(<Pill active={1} waiting={0} sessionPct={55} weekPct={null} onOpen={vi.fn()} />);
  expect(screen.getByText("55%")).toHaveClass("warn");
  expect(screen.queryByTitle(/Weekly/)).not.toBeInTheDocument();
  rerender(<Pill active={1} waiting={0} sessionPct={null} weekPct={null} onOpen={vi.fn()} />);
  expect(screen.queryByText(/%/)).not.toBeInTheDocument();
});

test("a waiting request replaces the usage", () => {
  render(<Pill active={1} waiting={2} sessionPct={32} weekPct={87} onOpen={vi.fn()} />);
  expect(screen.getByText("2 waiting")).toBeInTheDocument();
  expect(screen.queryByText("87%")).not.toBeInTheDocument();
});
