import { useEffect, useMemo, useRef, useState } from "react";
import { categoryOf, CATEGORIES, formatMinutes, PoiNearStation, Station } from "../api";

interface Props {
  station: Station;
  pois: PoiNearStation[];
  travel: { minutes: number | null; nb_changes: number | null } | null;
  originName: string | null;
  canGoBack: boolean;
  focusedPoiId: number | null;
  onBack: () => void;
  onAsk: () => void;
  onPoiClick: (id: number) => void;
  /** Lieux actuellement listés : la carte affiche exactement les mêmes, avec les mêmes numéros. */
  onVisibleChange: (pois: PoiNearStation[]) => void;
}

const PAGE = 15;

/** Étape 4 : ce qu'il y a autour d'une gare. */
export default function StationDetail(props: Props) {
  const { station, pois, travel, originName, focusedPoiId } = props;
  const [cat, setCat] = useState<string | null>(null);
  const [limit, setLimit] = useState(PAGE);
  const itemRefs = useRef(new Map<number, HTMLLIElement>());

  const counts = useMemo(() => {
    const m = new Map<string, number>();
    pois.forEach((p) => m.set(categoryOf(p.tags).key, (m.get(categoryOf(p.tags).key) ?? 0) + 1));
    return m;
  }, [pois]);
  const filtered = useMemo(() => pois.filter((p) => !cat || categoryOf(p.tags).key === cat), [pois, cat]);
  const shown = filtered.slice(0, limit);

  useEffect(() => {
    setCat(null);
    setLimit(PAGE);
  }, [station.id]);
  useEffect(() => {
    props.onVisibleChange(shown);
  }, [shown.length, cat, station.id]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (focusedPoiId != null) itemRefs.current.get(focusedPoiId)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [focusedPoiId]);

  return (
    <section className="detail">
      <button className="back" onClick={props.onBack}>
        ‹ {props.canGoBack ? "Retour aux destinations" : "Retour à la recherche"}
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

      <button className="primary wide" onClick={props.onAsk}>
        Poser une question sur cette gare
      </button>

      <h3>
        {pois.length === 0
          ? "Aucun lieu référencé à moins de 30 min à pied"
          : `${pois.length} lieu${pois.length > 1 ? "x" : ""} à moins de 30 min à pied`}
      </h3>
      {counts.size > 1 && (
        <div className="options" role="group" aria-label="Filtrer par type de lieu">
          <button className={`option ${cat === null ? "on" : ""}`} aria-pressed={cat === null} onClick={() => setCat(null)}>
            Tout
          </button>
          {CATEGORIES.filter((c) => counts.has(c.key)).map((c) => (
            <button
              key={c.key}
              className={`option ${cat === c.key ? "on" : ""}`}
              aria-pressed={cat === c.key}
              onClick={() => {
                setCat(cat === c.key ? null : c.key);
                setLimit(PAGE);
              }}
            >
              <i className="dot" style={{ background: c.color }} />
              {c.label} ({counts.get(c.key)})
            </button>
          ))}
        </div>
      )}
      <ol className="places-list">
        {shown.map((p, i) => {
          const c = categoryOf(p.tags);
          return (
            <li
              key={p.id}
              ref={(node) => {
                if (node) itemRefs.current.set(p.id, node);
                else itemRefs.current.delete(p.id);
              }}
            >
              <button
                className={`place ${focusedPoiId === p.id ? "active" : ""}`}
                onClick={() => props.onPoiClick(p.id)}
                title="Voir sur la carte"
              >
                <span className="poi-n" style={{ background: c.color }}>
                  {i + 1}
                </span>
                <span className="place-body">
                  <span className="place-head">
                    <strong>{p.name}</strong>
                    <span className="walk">🚶 {p.walk_minutes} min</span>
                  </span>
                  <span className="muted small">{c.label}</span>
                  {focusedPoiId === p.id && p.description && <span className="small">{p.description.slice(0, 220)}</span>}
                </span>
              </button>
            </li>
          );
        })}
      </ol>
      {filtered.length > shown.length && (
        <button className="more" onClick={() => setLimit((l) => l + PAGE)}>
          Voir {Math.min(PAGE, filtered.length - shown.length)} lieux de plus
        </button>
      )}
    </section>
  );
}
