import { useEffect, useState } from "react";
import { api, PoiNearStation, Station, TAG_LABELS } from "../api";

interface Props {
  selected: { station: Station; pois: PoiNearStation[] } | null;
  onSelectStation: (id: number) => void;
  onAsk: () => void;
}

/** Parcours « Explorer » : gare -> points d'intérêt, filtrables par thème. */
export default function ExplorerPanel({ selected, onSelectStation, onAsk }: Props) {
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<Station[]>([]);
  const [tag, setTag] = useState<string | null>(null);

  useEffect(() => {
    if (query.trim().length < 2) {
      setFound([]);
      return;
    }
    const t = setTimeout(() => api.searchStations(query).then(setFound).catch(() => setFound([])), 250);
    return () => clearTimeout(t);
  }, [query]);

  useEffect(() => setTag(null), [selected?.station.id]);

  const tags = selected ? [...new Set(selected.pois.flatMap((p) => p.tags))].sort() : [];
  const pois = selected ? selected.pois.filter((p) => !tag || p.tags.includes(tag)) : [];

  return (
    <div className="explorer">
      <input
        className="search"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder="Rechercher une gare…"
      />
      {found.length > 0 && (
        <ul className="station-results">
          {found.map((s) => (
            <li key={s.id}>
              <button
                onClick={() => {
                  onSelectStation(s.id);
                  setQuery("");
                }}
              >
                {s.name} {s.city && <span className="muted">· {s.city}</span>}
              </button>
            </li>
          ))}
        </ul>
      )}

      {!selected && <p className="muted">Cliquez sur une gare de la carte ou recherchez-la.</p>}

      {selected && (
        <div className="station-detail">
          <h2>{selected.station.name}</h2>
          <p className="muted">
            {selected.station.city ?? ""} {selected.station.pmr === true && "· ♿ accessible PMR"}
          </p>
          <button className="primary" onClick={onAsk}>
            Demander à l'assistant autour de cette gare
          </button>
          <div className="chips">
            <button className={`chip ${tag === null ? "active" : ""}`} onClick={() => setTag(null)}>
              Tout ({selected.pois.length})
            </button>
            {tags.map((t) => (
              <button key={t} className={`chip ${tag === t ? "active" : ""}`} onClick={() => setTag(t)}>
                {TAG_LABELS[t] ?? t}
              </button>
            ))}
          </div>
          <ul className="poi-list">
            {pois.map((p) => (
              <li key={p.id}>
                <strong>{p.name}</strong> <span className="muted">· 🚶 {p.walk_minutes} min</span>
                <div className="muted small">{p.tags.map((t) => TAG_LABELS[t] ?? t).join(" · ")}</div>
              </li>
            ))}
            {pois.length === 0 && <li className="muted">Aucun point d'intérêt à moins de 30 min à pied.</li>}
          </ul>
        </div>
      )}
    </div>
  );
}
