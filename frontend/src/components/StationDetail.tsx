import { ChatCircleText, PersonSimpleWalk, Star, Train } from "@phosphor-icons/react";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  categoryOf,
  CATEGORIES,
  formatMinutes,
  Journey,
  PoiNearStation,
  PlaceArea,
  sourceLabel,
  Station,
  StationService,
  TAG_LABELS,
} from "../api";
import StationInfo from "./StationInfo";
import Journeys from "./Journeys";

interface Props {
  station: Station;
  pois: PoiNearStation[];
  services: StationService[];
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
  /** Destination sans gare : les lieux listés sont autour d'elle, à la distance indiquée depuis la gare. */
  around?: PlaceArea | null;
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

/** Distance à vol d'oiseau, affichée en km ou en m. */
function km(lon1: number, lat1: number, lon2: number, lat2: number): string {
  const r = (x: number) => (x * Math.PI) / 180;
  const a =
    Math.sin(r(lat2 - lat1) / 2) ** 2 + Math.cos(r(lat1)) * Math.cos(r(lat2)) * Math.sin(r(lon2 - lon1) / 2) ** 2;
  const d = 6_371_000 * 2 * Math.asin(Math.sqrt(a));
  return d < 1000 ? `${Math.round(d / 50) * 50} m` : `${(d / 1000).toFixed(1).replace(".", ",")} km`;
}

const nowHHMM = () => {
  const d = new Date();
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
};

const fold = (s: string) => s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();

/** Un lieu sans nom (aire de jeux, point de vue anonyme) n'est listé que s'il répond à une envie précise. */
const genericWanted = (p: PoiNearStation, wanted: string[]) => wanted.some((t) => t !== "nature" && p.tags.includes(t));

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
      Number(b.matches) - Number(a.matches) || b.relevance - a.relevance || a.poi.walk_minutes - b.poi.walk_minutes,
  );
}

/** Étape 4 : ce qu'il y a autour d'une gare. */
export default function StationDetail(props: Props) {
  const { station, pois, travel, originName, focusedPoiId, wanted, keywords } = props;
  const [tab, setTab] = useState<"voir" | "trajet" | "gare">("voir");
  const [startAfter, setStartAfter] = useState<string | undefined>(undefined);
  const [nextTrain, setNextTrain] = useState<string | null>(null);
  const [cat, setCat] = useState<string | null>(null);
  const [limit, setLimit] = useState(PAGE);
  const itemRefs = useRef(new Map<number, HTMLLIElement>());

  const [showGeneric, setShowGeneric] = useState(false);
  const hiddenGeneric = useMemo(
    () => (showGeneric ? 0 : pois.filter((p) => p.generic && !genericWanted(p, wanted)).length),
    [pois, wanted.join(), showGeneric], // eslint-disable-line react-hooks/exhaustive-deps
  );
  const entries = useMemo(
    () =>
      buildEntries(
        pois.filter((p) => showGeneric || !p.generic || genericWanted(p, wanted)),
        wanted,
        keywords,
      ),
    [pois, wanted.join(), keywords.join(), showGeneric], // eslint-disable-line react-hooks/exhaustive-deps
  );
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
    setShowGeneric(false);
    setTab("voir");
    setStartAfter(undefined);
  }, [station.id]);

  // prochain train depuis la gare de départ, à partir de l'heure actuelle (horaires de la journée type)
  const canTravel = props.originId != null && props.originId !== station.id;
  useEffect(() => {
    setNextTrain(null);
    if (!canTravel) return;
    api
      .journeys(props.originId!, station.id, nowHHMM(), 1)
      .then((d) => setNextTrain(d.journeys[0]?.departure ?? null))
      .catch(() => setNextTrain(null));
  }, [props.originId, station.id, canTravel]);
  useEffect(() => {
    props.onVisibleChange(shown.map((e) => e.poi));
  }, [shown.length, cat, station.id, entries]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (focusedPoiId != null)
      itemRefs.current.get(focusedPoiId)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [focusedPoiId]);

  const searchLabel = [...wanted.map((t) => TAG_LABELS[t] ?? t), ...keywords].join(", ");

  return (
    <section className="detail">
      <button className="back" onClick={props.onBack}>
        ‹ {props.canGoBack ? "Retour aux destinations" : "Retour à la recherche"}
      </button>
      <div className="detail-head">
        <h2>{station.name}</h2>
        <p className="muted">
          {travel?.minutes === 0
            ? "Votre gare de départ : rien à prendre, tout est à pied"
            : travel?.minutes != null && originName
              ? `${formatMinutes(travel.minutes)} de train depuis ${originName}${
                  travel.nb_changes
                    ? `, ${travel.nb_changes} correspondance${travel.nb_changes > 1 ? "s" : ""}`
                    : ", direct"
                }`
              : "Temps de trajet non calculé depuis votre gare de départ"}
        </p>
        <div className="detail-actions">
          {canTravel && (
            <button
              className="primary"
              onClick={() => {
                setStartAfter(nowHHMM());
                setTab("trajet");
              }}
            >
              <Train size={16} weight="bold" aria-hidden className="ico" />
              {nextTrain ? ` Prochain train : ${nextTrain}` : " Voir les trains"}
            </button>
          )}
          <button className="secondary" onClick={props.onAsk}>
            <ChatCircleText size={16} aria-hidden className="ico" /> Poser une question
          </button>
        </div>
      </div>

      <div className="tabs" role="tablist" aria-label="Fiche de la gare">
        {(
          [
            ["voir", `À voir (${entries.length})`],
            ["trajet", "Y aller"],
            ["gare", "La gare"],
          ] as const
        ).map(([key, label]) => (
          <button
            key={key}
            role="tab"
            id={`tab-${key}`}
            aria-selected={tab === key}
            aria-controls={`panel-${key}`}
            className={tab === key ? "on" : ""}
            onClick={() => setTab(key)}
          >
            {label}
          </button>
        ))}
      </div>

      {tab === "trajet" && (
        <div role="tabpanel" id="panel-trajet" aria-labelledby="tab-trajet" className="tab-panel">
          {canTravel ? (
            <Journeys
              key={startAfter ?? "defaut"}
              originId={props.originId}
              originName={originName}
              stationId={station.id}
              stationName={station.name}
              selected={props.journey}
              onSelect={props.onSelectJourney}
              startAfter={startAfter}
            />
          ) : (
            <p className="muted">
              {props.originId == null
                ? "Choisissez votre gare de départ pour voir les trains."
                : "C'est votre gare de départ : pas de train à prendre."}
            </p>
          )}
        </div>
      )}

      {tab === "gare" && (
        <div role="tabpanel" id="panel-gare" aria-labelledby="tab-gare" className="tab-panel">
          <StationInfo pmr={station.pmr} equipments={station.equipments} services={props.services} />
        </div>
      )}

      {tab === "voir" && (
        <div role="tabpanel" id="panel-voir" aria-labelledby="tab-voir" className="tab-panel">
          {hasSearch && (
            <p className={nbMatches > 0 ? "match-summary" : "note"}>
              {nbMatches > 0
                ? `${nbMatches} lieu${nbMatches > 1 ? "x" : ""} correspond${nbMatches > 1 ? "ent" : ""} à votre recherche (${searchLabel}).`
                : `Aucun lieu référencé ici ne correspond à « ${searchLabel} ». Voici ce qu'il y a autour de la gare.`}
            </p>
          )}
          <p className="muted small">
            {props.around
              ? `Lieux autour de ${props.around.name} (sans gare, repéré par l'étiquette noire sur la carte), avec le temps à pied depuis cette gare et la distance au village.`
              : "Lieux à moins de 30 min à pied, numérotés comme sur la carte."}
          </p>
          {counts.size > 1 && (
            <div className="options" role="group" aria-label="Filtrer par type de lieu">
              <button
                className={`option ${cat === null ? "on" : ""}`}
                aria-pressed={cat === null}
                onClick={() => setCat(null)}
              >
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
                        <span className="walk">
                          <PersonSimpleWalk size={14} aria-hidden className="ico" /> {p.walk_minutes} min
                        </span>
                      </span>
                      <span className="muted small">
                        {p.interest >= 1 && (
                          <span className="star">
                            <Star size={12} weight="fill" aria-hidden /> Site remarquable ·{" "}
                          </span>
                        )}
                        {c.label}
                        {hasSearch && e.matches && <span className="match"> · correspond à votre recherche</span>}
                        <span className="source"> · {sourceLabel(p.source)}</span>
                    {props.around && (
                      <span className="from-place">
                        {" "}
                        · {km(p.lon, p.lat, props.around.lon, props.around.lat)} de {props.around.name}
                      </span>
                    )}
                      </span>
                      {focusedPoiId === p.id && p.description && (
                        <span className="small">{p.description.slice(0, 220)}</span>
                      )}
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
          {hiddenGeneric > 0 && (
            <button className="more" onClick={() => setShowGeneric(true)}>
              Afficher aussi {hiddenGeneric} lieu{hiddenGeneric > 1 ? "x" : ""} sans nom (aires de jeux, points de vue…)
            </button>
          )}
          <p className="muted small credits">
            Lieux : © contributeurs OpenStreetMap (ODbL)
            {pois.some((p) => p.source === "datatourisme") && " · DATAtourisme (offices de tourisme, Etalab 2.0)"}
          </p>
        </div>
      )}
    </section>
  );
}
