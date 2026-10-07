import { useEffect, useState } from "react";
import { Train } from "@phosphor-icons/react";
import { api, formatMinutes, Journey, JourneysResponse } from "../api";

interface Props {
  originId: number | null;
  originName: string | null;
  stationId: number;
  stationName: string;
  selected: Journey | null;
  onSelect: (j: Journey | null) => void;
}

const HOURS = Array.from({ length: 18 }, (_, i) => `${String(i + 5).padStart(2, "0")}:00`);

function trainLabel(l: Journey["legs"][number]): string {
  const parts = [l.route_name || "Train", l.number ? `n° ${l.number}` : null].filter(Boolean);
  return parts.join(" ");
}

/** Prochains trains (aller ou retour) entre la gare de départ et la gare choisie, avec le détail. */
export default function Journeys({ originId, originName, stationId, stationName, selected, onSelect }: Props) {
  const [dir, setDir] = useState<"aller" | "retour">("aller");
  const [after, setAfter] = useState("08:00");
  const [data, setData] = useState<JourneysResponse | null>(null);
  const [loading, setLoading] = useState(false);
  // aller : dernier retour possible le jour même (au moins 1h sur place après l'arrivée du 1er train)
  const [lastReturn, setLastReturn] = useState<{ after: string; departure: string | null } | null>(null);

  useEffect(() => {
    setAfter(dir === "aller" ? "08:00" : "16:00");
  }, [dir]);

  useEffect(() => {
    if (originId == null || originId === stationId) return;
    const [from, to] = dir === "aller" ? [originId, stationId] : [stationId, originId];
    setLoading(true);
    api
      .journeys(from, to, after, 4)
      .then((d) => {
        setData(d);
        onSelect(d.journeys[0] ?? null);
        setLastReturn(null);
        const first = d.journeys[0];
        if (dir === "aller" && first) {
          const [h, m] = first.arrival.split(":").map(Number);
          const t = Math.min(h * 60 + m + 60, 23 * 60 + 59);
          const back = `${String(Math.floor(t / 60)).padStart(2, "0")}:${String(t % 60).padStart(2, "0")}`;
          api
            .lastJourney(stationId, originId, back)
            .then((r) => setLastReturn({ after: back, departure: r.journeys[0]?.departure ?? null }))
            .catch(() => setLastReturn(null));
        }
      })
      .catch(() => setData(null))
      .finally(() => setLoading(false));
  }, [originId, stationId, dir, after]); // eslint-disable-line react-hooks/exhaustive-deps

  // dernier train de la journée (onglet Retour), calculé par le serveur
  const [lastOfDay, setLastOfDay] = useState<string | null>(null);
  useEffect(() => {
    setLastOfDay(null);
    if (dir !== "retour" || originId == null || originId === stationId) return;
    api
      .lastJourney(stationId, originId, "05:00")
      .then((r) => setLastOfDay(r.journeys[0]?.departure ?? null))
      .catch(() => setLastOfDay(null));
  }, [dir, originId, stationId]);

  if (originId == null || originId === stationId) return null;
  const [fromName, toName] = dir === "aller" ? [originName, stationName] : [stationName, originName];
  const journeys = data?.journeys ?? [];

  return (
    <section className="journeys" aria-label="Horaires des trains">
      <div className="journeys-head">
        <h3>Trains</h3>
        <div className="seg" role="tablist">
          {(["aller", "retour"] as const).map((d) => (
            <button key={d} role="tab" aria-selected={dir === d} className={dir === d ? "on" : ""} onClick={() => setDir(d)}>
              {d === "aller" ? "Aller" : "Retour"}
            </button>
          ))}
        </div>
      </div>
      <label className="after">
        {fromName} → {toName}, départ après
        <select value={after} onChange={(e) => setAfter(e.target.value)}>
          {HOURS.map((h) => (
            <option key={h} value={h}>
              {h.replace(":00", "h")}
            </option>
          ))}
        </select>
      </label>

      {loading && <p className="muted small">Recherche des trains…</p>}
      {!loading && data && journeys.length === 0 && (
        <p className="note">Aucun train après {after.replace(":00", "h")} dans les horaires de la journée type.</p>
      )}

      <ul className="journey-list">
        {journeys.map((j) => {
          const isSel = selected?.departure === j.departure && selected?.arrival === j.arrival;
          return (
            <li key={`${j.departure}-${j.arrival}`}>
              <button className={`journey ${isSel ? "active" : ""}`} onClick={() => onSelect(isSel ? null : j)} aria-expanded={isSel}>
                <span className="times">
                  <strong>{j.departure}</strong> → <strong>{j.arrival}</strong>
                </span>
                <span className="muted small">
                  {formatMinutes(j.duration_min)} · {j.changes === 0 ? "direct" : `${j.changes} correspondance${j.changes > 1 ? "s" : ""}`}
                </span>
              </button>
              {isSel && (
                <ol className="legs">
                  {j.legs.map((l, i) => (
                    <li key={i}>
                      {l.wait_before_min > 0 && (
                        <p className="transfer">
                          Correspondance à <strong>{l.from.name}</strong> : {l.wait_before_min} min d'attente
                        </p>
                      )}
                      <p className="leg-train">
                        <Train size={15} weight="bold" aria-hidden className="ico" /> <strong>{trainLabel(l)}</strong>
                        {l.headsign && <span className="muted"> direction {l.headsign}</span>}
                      </p>
                      <p className="leg-times">
                        <span>
                          {l.departure} <strong>{l.from.name}</strong>
                        </span>
                        <span className="muted small">
                          {formatMinutes(l.duration_min)}
                          {l.stops.length > 2 ? `, ${l.stops.length - 2} arrêt${l.stops.length > 3 ? "s" : ""}` : ", sans arrêt"}
                        </span>
                        <span>
                          {l.arrival} <strong>{l.to.name}</strong>
                        </span>
                      </p>
                    </li>
                  ))}
                </ol>
              )}
            </li>
          );
        })}
      </ul>
      {dir === "retour" && lastOfDay && (
        <p className="muted small">Dernier train retour de la journée : {lastOfDay}.</p>
      )}
      {dir === "aller" && lastReturn && (
        <p className={lastReturn.departure ? "return-ok" : "note"}>
          {lastReturn.departure
            ? `Retour vers ${originName} possible jusqu'à ${lastReturn.departure} (onglet Retour pour le détail).`
            : `Aucun train retour vers ${originName} après ${lastReturn.after} : pas d'aller-retour dans la journée avec ce départ.`}
        </p>
      )}
      {data && <p className="disclaimer">{data.note}</p>}
    </section>
  );
}
