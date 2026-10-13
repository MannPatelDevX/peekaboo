import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AppState } from "./types";

const empty: AppState = { sessions: [], pending: [], rules: [], hooks: [] };
let current: AppState = empty;
let emitStateChanged: () => void = () => {};
let cursorHere = true;

vi.mock("./api", () => ({
  getState: () => Promise.resolve(current),
  getNotch: () => Promise.resolve({ height: 0, width: 0 }),
  setSize: () => Promise.resolve(),
  getGroups: () => Promise.resolve({ groups: [], assignments: {} }),
  onStateChanged: (cb: () => void) => { emitStateChanged = cb; return Promise.resolve(() => {}); },
  onToggle: () => Promise.resolve(() => {}),
  listTranscripts: () => Promise.resolve([]),
  claudexSnapshot: () => Promise.resolve(null),
  decide: () => Promise.resolve(),
  cursorHere: () => Promise.resolve(cursorHere),
  onDisplayChanged: () => Promise.resolve(() => {}),
}));

import App from "./App";

test("a new request brings the Agents tab to the front", async () => {
  render(<App />);
  await userEvent.click(await screen.findByText("Peekaboo"));
  await userEvent.click(screen.getByRole("tab", { name: /Sessions/ }));
  expect(screen.getByRole("tab", { name: /Sessions/ })).toHaveAttribute("aria-selected", "true");

  current = { ...empty, pending: [{ id: "q1", agent: "claude", sessionKey: "k", project: "shop", createdAt: 1,
    kind: "question", questions: [{ question: "Which DB?", options: [{ label: "Postgres" }], multiSelect: false }] }] };
  await act(async () => emitStateChanged());

  expect(await screen.findByText("Which DB?")).toBeInTheDocument();
  expect(screen.getByRole("tab", { name: /Agents/ })).toHaveAttribute("aria-selected", "true");
});

test("a request only opens the pill on the screen the mouse is on", async () => {
  current = empty;
  cursorHere = false;
  render(<App />);
  await screen.findByText("Peekaboo");
  current = { ...empty, pending: [{ id: "p2", agent: "claude", sessionKey: "k", project: "shop", createdAt: 1,
    kind: "permission", tool: "Bash", input: { command: "ls" } }] };
  await act(async () => emitStateChanged());
  expect(await screen.findByText("1 waiting")).toBeInTheDocument();
  expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
  cursorHere = true;
});
