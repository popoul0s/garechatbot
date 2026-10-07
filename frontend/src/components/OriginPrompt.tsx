import { useEffect, useRef } from "react";
import { Station } from "../api";
import OriginPicker from "./OriginPicker";

interface Props {
  query: string | null;
  message: string | null;
  stations: Station[];
  /** Dernière gare utilisée, proposée en un clic (jamais choisie d'office). */
  last: string | null;
  onChoose: (name: string) => void;
}

/** La recherche attend une gare de départ : on la demande, puis la recherche repart toute seule. */
export default function OriginPrompt({ query, message, stations, last, onChoose }: Props) {
  const ref = useRef<HTMLElement>(null);
  useEffect(() => {
    // le champ de cette question prend le focus, pour taper la gare directement
    ref.current?.querySelector<HTMLInputElement>("input")?.focus();
  }, []);
  const lastKnown = last && stations.some((s) => s.name === last) ? last : null;

  return (
    <section className="origin-prompt" ref={ref} aria-live="polite" aria-labelledby="origin-prompt-title">
      <h2 id="origin-prompt-title">De quelle gare partez-vous ?</h2>
      <p>
        {query ? (
          <>
            Votre demande « {query} » est prête. Il me manque seulement votre gare de départ pour calculer les trajets.
          </>
        ) : (
          (message ?? "Indiquez votre gare de départ pour calculer les trajets.")
        )}
      </p>
      <OriginPicker value={null} stations={stations} onChange={onChoose} inputId="origin-prompt-input" label="Gare de départ" />
      {lastKnown && (
        <button className="origin-last" onClick={() => onChoose(lastKnown)}>
          Comme la dernière fois : <strong>{lastKnown}</strong>
        </button>
      )}
    </section>
  );
}
