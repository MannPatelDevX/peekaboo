import { useState } from "react";
import type { Decision, PendingView } from "../types";

type QuestionReq = Extract<PendingView, { kind: "question" }>;

export function QuestionCard({ req, onDecide }: { req: QuestionReq; onDecide: (d: Decision) => void }) {
  const [picked, setPicked] = useState<string[][]>(req.questions.map(() => []));
  const [other, setOther] = useState<string[]>(req.questions.map(() => ""));

  const toggle = (qi: number, label: string, multi: boolean) =>
    setPicked((p) =>
      p.map((sel, i) =>
        i !== qi ? sel : multi ? (sel.includes(label) ? sel.filter((l) => l !== label) : [...sel, label]) : [label],
      ),
    );

  // A typed answer wins over picked options. Multi-select labels are comma-joined, as Claude Code expects.
  const answers = req.questions.map((q, i) => ({ question: q.question, answer: other[i].trim() || picked[i].join(", ") }));
  const complete = answers.every((a) => a.answer.length > 0);

  return (
    <div className="card question">
      <div className="agent">{req.agent} · {req.project}</div>
      {req.questions.map((q, qi) => (
        <fieldset key={qi}>
          <legend>{q.question}</legend>
          <div className="options">
            {q.options.map((o) => (
              <button
                key={o.label}
                title={o.description ?? undefined}
                aria-pressed={picked[qi].includes(o.label)}
                className={picked[qi].includes(o.label) ? "chip on" : "chip"}
                onClick={() => toggle(qi, o.label, q.multiSelect)}
              >
                {o.label}
              </button>
            ))}
          </div>
          <input
            placeholder="Other…"
            value={other[qi]}
            onChange={(e) => setOther((v) => v.map((x, i) => (i === qi ? e.target.value : x)))}
          />
        </fieldset>
      ))}
      <div className="actions">
        <button className="primary" disabled={!complete} onClick={() => onDecide({ type: "answer", answers })}>Send</button>
        <button className="ghost-btn" onClick={() => onDecide({ type: "passthrough" })}>Answer in terminal</button>
      </div>
    </div>
  );
}
