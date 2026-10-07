import { Check, MapPin, PersonSimpleWalk, Train, X } from "@phosphor-icons/react";
import { Answer, Criteria, formatMinutes, Recommendation, SearchOutcome, TAG_LABELS } from "../api";

interface Props {
  criteria: Criteria;
  outcome: SearchOutcome;
  answer: Answer | null;
  /** Mode jury : moteur utilisé et temps de réponse. */
  engine: { label: string; ms: number } | null;
  hoveredId: number | null;
  onHover: (id: number | null) => void;
  onOpen: (stationId: number) => void;
  /** Aucun résultat : relance avec un critère assoupli. */
  onRelax: (next: Criteria) => void;
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

function Card({
  r,
  rank,
  explanation,
  wanted,
  hot,
  onHover,
  onOpen,
}: {
  r: Recommendation;
  rank: number;
  explanation: string | null;
  wanted: string[];
  hot: boolean;
  onHover: (id: number | null) => void;
  onOpen: () => void;
}) {
  const best = r.pois[0];
  const others = r.pois.slice(1, 5);
  const matched = [...new Set(r.pois.flatMap((p) => p.tags))].filter((t) => wanted.includes(t));
  return (
    <li
      className={`card ${hot ? "hot" : ""}`}
      onMouseEnter={() => onHover(r.station.id)}
      onMouseLeave={() => onHover(null)}
    >
      <button className="card-main" onClick={onOpen} onFocus={() => onHover(r.station.id)} onBlur={() => onHover(null)}>
        <span className="rank" aria-hidden>
          {rank}
        </span>
        <span className="card-body">
          <span className="card-title">{r.station.name}</span>
          <span className="card-trip">
            {r.travel_minutes === 0 ? (
              <>
                <MapPin size={15} aria-hidden className="ico" /> Sur place, sans train
              </>
            ) : r.travel_minutes != null ? (
              <>
                <Train size={15} aria-hidden className="ico" /> {formatMinutes(r.travel_minutes)} de train
                {r.nb_changes ? `, ${r.nb_changes} correspondance${r.nb_changes > 1 ? "s" : ""}` : ", direct"}
              </>
            ) : null}
          </span>
          {best && (
            <span className="card-highlight">
              <PersonSimpleWalk size={15} aria-hidden className="ico" />
              <span>
                <strong>{best.name}</strong> à {best.walk_minutes} min à pied
              </span>
            </span>
          )}
          {explanation && <span className="explanation">{explanation}</span>}
        </span>
        <span className="chevron" aria-hidden>
          ›
        </span>
      </button>
      <details className="card-more">
        <summary>Détails et classement</summary>
        {others.length > 0 && (
          <p className="small">
            <span className="muted">Aussi à pied : </span>
            {others.map((p) => `${p.name} (${p.walk_minutes} min)`).join(", ")}
          </p>
        )}
        {(matched.length > 0 || r.missing_themes.length > 0) && (
          <span className="tags">
            {matched.map((t) => (
              <span key={t} className="tag">
                <Check size={12} weight="bold" aria-hidden /> {TAG_LABELS[t] ?? t}
              </span>
            ))}
            {r.missing_themes.map((t) => (
              <span key={t} className="tag missing" title="Aucun lieu de ce type à proximité de la gare">
                <X size={12} weight="bold" aria-hidden /> {TAG_LABELS[t] ?? t}
              </span>
            ))}
          </span>
        )}
        <div className="why">
          <Bar label="Correspond à vos envies" value={r.breakdown.theme} />
          <Bar label="Trajet court" value={r.breakdown.travel} />
          <Bar label="Peu de marche" value={r.breakdown.walk} />
          <Bar label="Nombre de lieux" value={r.breakdown.richness} />
          <Bar label="Accessibilité" value={r.breakdown.accessibility} />
        </div>
      </details>
    </li>
  );
}

/** Critères assouplis proposés en un clic quand rien ne correspond. */
function relaxations(c: Criteria, o: SearchOutcome): { label: string; next: Criteria }[] {
  const out: { label: string; next: Criteria }[] = [];
  const base = { ...c, keywords: [] };
  if (c.place) out.push({ label: `Chercher partout, pas seulement vers ${c.place}`, next: { ...base, place: null } });
  if (!c.place && o.applied_max_travel_minutes < 240) {
    const t = Math.min(o.applied_max_travel_minutes + 60, 240);
    out.push({ label: `Élargir à ${formatMinutes(t)} de train`, next: { ...base, max_travel_minutes: t } });
  }
  if (o.applied_max_walk_minutes < 45) {
    const w = o.applied_max_walk_minutes < 30 ? 30 : 45;
    out.push({ label: `Autoriser ${w} min à pied`, next: { ...base, max_walk_minutes: w } });
  }
  c.themes.forEach((t) =>
    out.push({
      label: `Retirer « ${TAG_LABELS[t] ?? t} »`,
      next: { ...base, themes: c.themes.filter((x) => x !== t) },
    }),
  );
  if (c.audience) out.push({ label: "Retirer « avec des enfants »", next: { ...base, audience: null } });
  return out;
}

/** Les destinations proposées. */
export default function Results({ criteria, outcome, answer, engine, hoveredId, onHover, onOpen, onRelax }: Props) {
  const explanations = new Map(answer?.items.map((i) => [i.station_id, i.explanation]) ?? []);
  const wanted = [...criteria.themes, ...(criteria.audience === "famille" ? ["famille"] : [])];
  const recos = outcome.recommendations;
  const notes = outcome.notes.filter((n) => n !== answer?.intro);

  return (
    <section className="results" aria-live="polite">
      <h2>
        {recos.length === 0
          ? "Aucune destination trouvée"
          : `${recos.length} destination${recos.length > 1 ? "s" : ""} pour vous`}
      </h2>
      {answer && answer.intro && recos.length > 0 && <p className="intro">{answer.intro}</p>}
      {notes.map((n) => (
        <p key={n} className="note">
          {n}
        </p>
      ))}
      {recos.length === 0 && (
        <div className="relax">
          <p>Essayez en un clic :</p>
          <div className="relax-actions">
            {relaxations(criteria, outcome).map((r) => (
              <button key={r.label} className="option" onClick={() => onRelax(r.next)}>
                {r.label}
              </button>
            ))}
          </div>
        </div>
      )}
      <ol className="cards">
        {recos.map((r, i) => (
          <Card
            key={r.station.id}
            r={r}
            rank={i + 1}
            explanation={explanations.get(r.station.id) ?? null}
            wanted={wanted}
            hot={hoveredId === r.station.id}
            onHover={onHover}
            onOpen={() => onOpen(r.station.id)}
          />
        ))}
      </ol>
      {engine && (
        <p className="jury-info">
          Mode jury · {engine.label} · {engine.ms} ms
        </p>
      )}
    </section>
  );
}
