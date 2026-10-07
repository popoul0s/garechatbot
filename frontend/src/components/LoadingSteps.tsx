import { useEffect, useState } from "react";
import { Check } from "@phosphor-icons/react";

const WITH_AI = [
  "Je comprends votre demande",
  "Je cherche parmi les gares et les horaires",
  "Je classe les destinations",
];
const FILTERS = ["Je cherche parmi les gares et les horaires", "Je classe les destinations"];

/** Chargement qui montre les étapes : l'IA comprend, puis le code cherche et classe. */
export default function LoadingSteps({ withAi }: { withAi: boolean }) {
  const steps = withAi ? WITH_AI : FILTERS;
  const [current, setCurrent] = useState(0);
  useEffect(() => {
    setCurrent(0);
    // progression indicative : la dernière étape reste active jusqu'à la réponse
    const id = window.setInterval(() => setCurrent((c) => Math.min(c + 1, steps.length - 1)), 900);
    return () => window.clearInterval(id);
  }, [withAi, steps.length]);

  return (
    <ol className="loading-steps" aria-live="polite" aria-busy="true">
      {steps.map((s, i) => (
        <li key={s} className={i < current ? "done" : i === current ? "active" : "todo"}>
          <span className="step-mark" aria-hidden>
            {i < current ? <Check size={12} weight="bold" /> : i === current ? <span className="spinner" /> : null}
          </span>
          {s}
          {i === current && "…"}
        </li>
      ))}
    </ol>
  );
}
