import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QuestionCard } from "./QuestionCard";

const base = { id: "q", agent: "claude", sessionKey: "k", project: "shop", createdAt: 0, kind: "question" as const };
const db = { question: "Which DB?", options: [{ label: "Postgres" }, { label: "Mongo" }], multiSelect: false };
const extras = { question: "Extras?", options: [{ label: "Auth" }, { label: "Cache" }], multiSelect: true };

test("collects single and multi answers", async () => {
  const onDecide = vi.fn();
  render(<QuestionCard req={{ ...base, questions: [db, extras] }} onDecide={onDecide} />);
  expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  await userEvent.click(screen.getByRole("button", { name: "Postgres" }));
  await userEvent.click(screen.getByRole("button", { name: "Auth" }));
  await userEvent.click(screen.getByRole("button", { name: "Cache" }));
  await userEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(onDecide).toHaveBeenCalledWith({ type: "answer", answers: [
    { question: "Which DB?", answer: "Postgres" }, { question: "Extras?", answer: "Auth, Cache" },
  ] });
});

test("free-text answer and terminal fallback", async () => {
  const onDecide = vi.fn();
  render(<QuestionCard req={{ ...base, questions: [db] }} onDecide={onDecide} />);
  await userEvent.type(screen.getByPlaceholderText("Other…"), "SQLite");
  await userEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(onDecide).toHaveBeenLastCalledWith({ type: "answer", answers: [{ question: "Which DB?", answer: "SQLite" }] });
  await userEvent.click(screen.getByRole("button", { name: "Answer in terminal" }));
  expect(onDecide).toHaveBeenLastCalledWith({ type: "passthrough" });
});
