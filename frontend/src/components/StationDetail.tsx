import { useEffect, useMemo, useRef, useState } from "react";
import { categoryOf, CATEGORIES, formatMinutes, Journey, PoiNearStation, Station, TAG_LABELS } from "../api";
import Journeys from "./Journeys";

interface Props {
  station: Station;
  pois: PoiNearStation[];
  travel: { minutes: number | null; nb_changes: number | null } | null;
  originName: string | null;
  originId: number | null;
  journey: Journey | null;
  onSelectJourney: (j: Journey | null) => void;
  /** Thèmes et mots-clés de la recherche en cours : les lieux correspondants passent en premier. */
  wanted: string[];
  keywords: string[];
  canGoBack: boolean;
  focusedPoiId: number | null;
  onBack: () => void;
  onAsk: () => void;
  onPoiClick: (id: number) => void;
  /** Lieux actuellement listés : la carte affiche exactement les mêmes, avec les mêmes numéros. */
  onVisibleChange: (pois: PoiNearStation[]) => void;
}

/** Un lieu, ou plusieurs lieux de même nom regroupés ("Aire de jeux" x 8). */
interface Entry {
  poi: PoiNearStation; // le plus proche du groupe
  count: number;
  matches: boolean;
  /** Pertinence = correspondance à la recherche x intérêt touristique (même logique que le serveur). */
  relevance: number;
}

const PAGE = 12;

const fold = (s: string) =>
  s
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase();

function buildEntries(pois: PoiNearStation[], wanted: string[], keywords: string[]): Entry[] {
  const kws = keywords.map(fold);
  const groups = new Map<string, Entry>();
  for (const p of [...pois].sort((a, b) => a.walk_minutes - b.walk_minutes)) {
    const key = fold(p.name);
    const text = fold(`${p.name} ${p.description ?? ""}`);
    const tagRatio = wanted.length ? wanted.filter((t) => p.tags.includes(t)).length / wanted.length : 0;
    const kwHit = kws.some((k) => text.includes(k)) ? 1 : 0;
    const matches = tagRatio > 0 || kwHit > 0;
    const interest = p.interest ?? 0.35;
    // hors recherche, seul l'intérêt touristique compte
    const relevance = (matches ? Math.min(1, tagRatio + kwHit) : 0.2) * (0.4 + 0.6 * interest);
    const g = groups.get(key);
    if (g) {
      g.count += 1;
      g.matches ||= matches;
    } else {
      groups.set(key, { poi: p, count: 1, matches, relevance });
    }
  }
  // correspondances d'abord, puis les plus intéressantes, puis les plus proches
  return [...groups.values()].sort(
    (a, b) =>
      Number(b.matches) - Number(a.matches) ||
      b.relevance - a.relevance ||
      a.poi.walk_minutes - b.poi.walk_minutes,
  );
}

/** Étape 4 : ce qu'il y a autour d'une gare. */
export default function StationDetail(props: Props) {
  const { station, pois, travel, originName, focusedPoiId, wanted, keywords } = props;
  const [cat, setCat] = useState<string | null>(null);
  const [limit, setLimit] = useState(PAGE);
  const itemRefs = useRef(new Map<number, HTMLLIElement>());

  const entries = useMemo(() => buildEntries(pois, wanted, keywords), [pois, wanted.join(), keywords.join()]); // eslint-disable-line react-hooks/exhaustive-deps
  const counts = useMemo(() => {
    const m = new Map<string, number>();
    entries.forEach((e) => m.set(categoryOf(e.poi.tags).key, (m.get(categoryOf(e.poi.tags).key) ?? 0) + 1));
    return m;
  }, [entries]);
  const filtered = entries.filter((e) => !cat || categoryOf(e.poi.tags).key === cat);
  const shown = filtered.slice(0, limit);
  const hasSearch = wanted.length > 0 || keywords.length > 0;
  const nbMatches = entries.filter((e) => e.matches).length;

  useEffect(() => {
    setCat(null);
    setLimit(PAGE);
  }, [station.id]);
  useEffect(() => {
    props.onVisibleChange(shown.map((e) => e.poi));
  }, [shown.length, cat, station.id, entries]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (focusedPoiId != null) itemRefs.current.get(focusedPoiId)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [focusedPoiId]);

  const searchLabel = [...wanted.map((t) => TAG_LABELS[t] ?? t), ...keywords].join(", ");

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

      {hasSearch && (
        <p className={nbMatches > 0 ? "match-summary" : "note"}>
          {nbMatches > 0
            ? `${nbMatches} lieu${nbMatches > 1 ? "x" : ""} correspond${nbMatches > 1 ? "ent" : ""} à votre recherche (${searchLabel}).`
            : `Aucun lieu référencé ici ne correspond à « ${searchLabel} ». Voici ce qu'il y a autour de la gare.`}
        </p>
      )}

      <Journeys
        originId={props.originId}
        originName={originName}
        stationId={station.id}
        stationName={station.name}
        selected={props.journey}
        onSelect={props.onSelectJourney}
      />

      <button className="primary wide" onClick={props.onAsk}>
        Poser une question sur cette gare
      </button>

      <h3>À moins de 30 min à pied</h3>
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
        {shown.map((e, i) => {
          const p = e.poi;
          const c = categoryOf(p.tags);
          const firstOther = hasSearch && !e.matches && (i === 0 || shown[i - 1].matches);
          return (
            <li
              key={p.id}
              className={firstOther && i > 0 ? "separator" : undefined}
              ref={(node) => {
                if (node) itemRefs.current.set(p.id, node);
                else itemRefs.current.delete(p.id);
              }}
            >
              {firstOther && i > 0 && <span className="list-label">Autres lieux autour de la gare</span>}
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
                    <strong>
                      {p.name}
                      {e.count > 1 && <span className="muted"> · {e.count} sur place</span>}
                    </strong>
                    <span className="walk">🚶 {p.walk_minutes} min</span>
                  </span>
                  <span className="muted small">
                    {p.interest >= 1 && <span className="star">★ Site remarquable · </span>}
                    {c.label}
                    {hasSearch && e.matches && <span className="match"> · correspond à votre recherche</span>}
                  </span>
                  {focusedPoiId === p.id && p.description && <span className="small">{p.description.slice(0, 220)}</span>}
                </span>
              </button>
            </li>
          );
        })}
        {pois.length === 0 && <li className="muted">Aucun lieu touristique référencé près de cette gare.</li>}
      </ol>
      {filtered.length > shown.length && (
        <button className="more" onClick={() => setLimit((l) => l + PAGE)}>
          Voir {Math.min(PAGE, filtered.length - shown.length)} lieux de plus
        </button>
      )}
    </section>
  );
}
