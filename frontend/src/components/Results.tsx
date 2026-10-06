import { Answer, Criteria, formatMinutes, Recommendation, SearchOutcome, TAG_LABELS } from "../api";

interface Props {
  query: string | null;
  criteria: Criteria;
  outcome: SearchOutcome;
  answer: Answer | null;
  engine: { label: string; ms: number } | null;
  onOpen: (stationId: number) => void;
}

const EXAMPLES_IF_EMPTY = ["Élargissez le temps de train", "Retirez une envie", "Augmentez la marche autorisée"];

function summary(c: Criteria, o: SearchOutcome): string {
  const parts = [`depuis ${o.origin?.name ?? c.origin ?? "votre gare"}`];
  if (c.place) parts.push(`vers ${c.place}`);
  parts.push(`moins de ${formatMinutes(o.applied_max_travel_minutes)} de train`);
  parts.push(`moins de ${o.applied_max_walk_minutes} min à pied`);
  return parts.join(", ");
}

function Bar({ label, value }: { label: string; value: number }) {
  return (
    <div className="bar">
      <span>{label}</span>
      <div className="track">
        <div className="fill" style={{ width: `${Math.round(value * 100)}%` }} />
      </div>
    </div>
  );
}

function Card({ r, rank, explanation, wanted, onOpen }: {
  r: Recommendation;
  rank: number;
  explanation: string | null;
  wanted: string[];
  onOpen: () => void;
}) {
  const nearest = r.pois.reduce((m, p) => Math.min(m, p.walk_minutes), Infinity);
  const matched = [...new Set(r.pois.flatMap((p) => p.tags))].filter((t) => wanted.includes(t));
  return (
    <li className="card">
      <button className="card-main" onClick={onOpen}>
        <span className="rank" aria-hidden>
          {rank}
        </span>
        <span className="card-body">
          <span className="card-title">{r.station.name}</span>
          <span className="facts">
            {r.travel_minutes !== null && r.travel_minutes > 0 && (
              <span>
                🚆 {formatMinutes(r.travel_minutes)}
                {r.nb_changes ? `, ${r.nb_changes} correspondance${r.nb_changes > 1 ? "s" : ""}` : ", direct"}
              </span>
            )}
            {Number.isFinite(nearest) && <span>🚶 {nearest} min jusqu'au premier lieu</span>}
          </span>
          {explanation && <span className="explanation">{explanation}</span>}
          <span className="places">
            {r.pois.slice(0, 3).map((p) => p.name).join(" · ")}
          </span>
          {(matched.length > 0 || r.missing_themes.length > 0) && (
            <span className="tags">
              {matched.map((t) => (
                <span key={t} className="tag">
                  ✓ {TAG_LABELS[t] ?? t}
                </span>
              ))}
              {r.missing_themes.map((t) => (
                <span key={t} className="tag missing" title="Aucun lieu de ce type à proximité de la gare">
                  ✗ {TAG_LABELS[t] ?? t}
                </span>
              ))}
            </span>
          )}
        </span>
        <span className="chevron" aria-hidden>
          ›
        </span>
      </button>
      <details className="why">
        <summary>Pourquoi ce classement ?</summary>
        <Bar label="Correspond à vos envies" value={r.breakdown.theme} />
        <Bar label="Trajet court" value={r.breakdown.travel} />
        <Bar label="Peu de marche" value={r.breakdown.walk} />
        <Bar label="Nombre de lieux" value={r.breakdown.richness} />
        <Bar label="Accessibilité" value={r.breakdown.accessibility} />
      </details>
    </li>
  );
}

/** Étape 3 : les destinations proposées. */
export default function Results({ query, criteria, outcome, answer, engine, onOpen }: Props) {
  const explanations = new Map(answer?.items.map((i) => [i.station_id, i.explanation]) ?? []);
  const wanted = [...criteria.themes, ...(criteria.audience === "famille" ? ["famille"] : [])];
  const recos = outcome.recommendations;
  const notes = outcome.notes.filter((n) => n !== answer?.intro);

  return (
    <section className="results" aria-live="polite">
      {query && <p className="query">« {query} »</p>}
      <h2>
        {recos.length === 0
          ? "Aucune destination trouvée"
          : `${recos.length} destination${recos.length > 1 ? "s" : ""} pour vous`}
      </h2>
      {outcome.origin && <p className="muted">{summary(criteria, outcome)}</p>}
      {answer && answer.intro && recos.length > 0 && <p className="intro">{answer.intro}</p>}
      {notes.map((n) => (
        <p key={n} className="note">
          {n}
        </p>
      ))}
      {recos.length === 0 && (
        <ul className="hints">
          {EXAMPLES_IF_EMPTY.map((h) => (
            <li key={h}>{h}</li>
          ))}
        </ul>
      )}
      <ol className="cards">
        {recos.map((r, i) => (
          <Card
            key={r.station.id}
            r={r}
            rank={i + 1}
            explanation={explanations.get(r.station.id) ?? null}
            wanted={wanted}
            onOpen={() => onOpen(r.station.id)}
          />
        ))}
      </ol>
      {engine && (
        <details className="tech">
          <summary>Détails techniques</summary>
          <p>
            {engine.label} · {engine.ms} ms
          </p>
        </details>
      )}
    </section>
  );
}
