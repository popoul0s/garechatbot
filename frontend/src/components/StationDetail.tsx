import { useState } from "react";
import { formatMinutes, PoiNearStation, Station, TAG_LABELS } from "../api";

interface Props {
  station: Station;
  pois: PoiNearStation[];
  travel: { minutes: number | null; nb_changes: number | null } | null;
  originName: string | null;
  canGoBack: boolean;
  onBack: () => void;
  onAsk: () => void;
}

/** Étape 4 : ce qu'il y a autour d'une gare. */
export default function StationDetail({ station, pois, travel, originName, canGoBack, onBack, onAsk }: Props) {
  const [tag, setTag] = useState<string | null>(null);
  const tags = [...new Set(pois.flatMap((p) => p.tags))].sort();
  const shown = pois.filter((p) => !tag || p.tags.includes(tag));

  return (
    <section className="detail">
      <button className="back" onClick={onBack}>
        ‹ {canGoBack ? "Retour aux résultats" : "Retour à la recherche"}
      </button>
      <h2>{station.name}</h2>
      <p className="muted">
        {travel?.minutes != null && originName
          ? `🚆 ${formatMinutes(travel.minutes)} depuis ${originName}${
              travel.nb_changes ? `, ${travel.nb_changes} correspondance${travel.nb_changes > 1 ? "s" : ""}` : ", direct"
            }`
          : "Temps de trajet non calculé depuis votre gare de départ"}
        {station.pmr === true && " · Gare accessible en fauteuil"}
      </p>

      <button className="primary wide" onClick={onAsk}>
        Poser une question sur cette gare
      </button>

      <h3>
        {pois.length} lieu{pois.length > 1 ? "x" : ""} à moins de 30 min à pied
      </h3>
      {tags.length > 1 && (
        <div className="options" role="group" aria-label="Filtrer par type de lieu">
          <button className={`option ${tag === null ? "on" : ""}`} aria-pressed={tag === null} onClick={() => setTag(null)}>
            Tout
          </button>
          {tags.map((t) => (
            <button
              key={t}
              className={`option ${tag === t ? "on" : ""}`}
              aria-pressed={tag === t}
              onClick={() => setTag(tag === t ? null : t)}
            >
              {TAG_LABELS[t] ?? t}
            </button>
          ))}
        </div>
      )}
      <ul className="places-list">
        {shown.map((p) => (
          <li key={p.id}>
            <div className="place-head">
              <strong>{p.name}</strong>
              <span className="walk">🚶 {p.walk_minutes} min</span>
            </div>
            <div className="muted small">{p.tags.map((t) => TAG_LABELS[t] ?? t).join(" · ")}</div>
            {p.description && <p className="small">{p.description.slice(0, 160)}</p>}
          </li>
        ))}
        {pois.length === 0 && <li className="muted">Aucun lieu touristique référencé près de cette gare.</li>}
      </ul>
    </section>
  );
}
