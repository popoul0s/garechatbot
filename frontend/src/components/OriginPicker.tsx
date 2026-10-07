import { Crosshair } from "@phosphor-icons/react";
import { useMemo, useRef, useState } from "react";
import { Station } from "../api";

interface Props {
  value: string | null;
  stations: Station[];
  onChange: (name: string) => void;
}

const fold = (s: string) =>
  s
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .trim();

const MAX = 8;

/** Choix de la gare de départ : saisie avec suggestions, ou gare la plus proche de l'utilisateur. */
export default function OriginPicker({ value, stations, onChange }: Props) {
  const [text, setText] = useState<string | null>(null); // null = pas en cours de saisie
  const [active, setActive] = useState(0);
  const [locating, setLocating] = useState(false);
  const [geoError, setGeoError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const suggestions = useMemo(() => {
    if (text === null) return [];
    const q = fold(text);
    if (!q) return stations.slice(0, MAX);
    const starts: Station[] = [];
    const contains: Station[] = [];
    for (const s of stations) {
      const n = fold(s.name);
      if (n.startsWith(q) || fold(s.city ?? "").startsWith(q)) starts.push(s);
      else if (n.includes(q)) contains.push(s);
    }
    return [...starts, ...contains].slice(0, MAX);
  }, [text, stations]);

  const pick = (s: Station) => {
    setText(null);
    setGeoError(null);
    inputRef.current?.blur();
    if (s.name !== value) onChange(s.name);
  };

  const nearest = () => {
    if (!navigator.geolocation) {
      setGeoError("Géolocalisation indisponible dans ce navigateur.");
      return;
    }
    setLocating(true);
    setGeoError(null);
    navigator.geolocation.getCurrentPosition(
      ({ coords }) => {
        setLocating(false);
        const k = Math.cos((coords.latitude * Math.PI) / 180);
        const best = stations.reduce<Station | null>((b, s) => {
          const d = (s.lon - coords.longitude) ** 2 * k * k + (s.lat - coords.latitude) ** 2;
          const db = b ? (b.lon - coords.longitude) ** 2 * k * k + (b.lat - coords.latitude) ** 2 : Infinity;
          return d < db ? s : b;
        }, null);
        if (best) pick(best);
      },
      () => {
        setLocating(false);
        setGeoError("Position non autorisée : tapez le nom de votre gare.");
      },
      { timeout: 10000 },
    );
  };

  const open = text !== null && suggestions.length > 0;

  return (
    <div className="origin">
      <label htmlFor="origin-input">Je pars de</label>
      <div className="origin-row">
        <div className="combo">
          <input
            id="origin-input"
            ref={inputRef}
            role="combobox"
            aria-expanded={open}
            aria-controls="origin-list"
            aria-autocomplete="list"
            autoComplete="off"
            placeholder="Nom de votre gare…"
            value={text ?? value ?? ""}
            onFocus={(e) => {
              setText("");
              setActive(0);
              e.target.select();
            }}
            onChange={(e) => {
              setText(e.target.value);
              setActive(0);
            }}
            onBlur={() => setTimeout(() => setText(null), 150)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setActive((a) => Math.min(a + 1, suggestions.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActive((a) => Math.max(a - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                if (suggestions[active]) pick(suggestions[active]);
              } else if (e.key === "Escape") {
                setText(null);
                inputRef.current?.blur();
              }
            }}
          />
          {open && (
            <ul id="origin-list" role="listbox" className="combo-list">
              {suggestions.map((s, i) => (
                <li
                  key={s.id}
                  role="option"
                  aria-selected={i === active}
                  className={i === active ? "active" : undefined}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    pick(s);
                  }}
                  onMouseEnter={() => setActive(i)}
                >
                  {s.name}
                  {s.city && !fold(s.name).includes(fold(s.city)) && <span className="muted"> · {s.city}</span>}
                </li>
              ))}
            </ul>
          )}
          {text !== null && text.trim() !== "" && suggestions.length === 0 && (
            <p className="combo-empty">Aucune gare desservie par le train ne correspond.</p>
          )}
        </div>
        <button
          type="button"
          className="locate"
          onClick={nearest}
          disabled={locating || stations.length === 0}
          title="Partir de la gare la plus proche de moi"
          aria-label="Partir de la gare la plus proche de moi"
        >
          {locating ? "…" : <Crosshair size={20} aria-hidden />}
        </button>
      </div>
      {geoError && <p className="note small">{geoError}</p>}
    </div>
  );
}
