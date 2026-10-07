import { FormEvent, useRef, useState } from "react";
import { Criteria, formatMinutes, Station, StationSummary, TAG_LABELS } from "../api";
import OriginPicker from "./OriginPicker";

interface Props {
  origins: Station[];
  criteria: Criteria;
  loading: boolean;
  askAround: StationSummary | null;
  onAsk: (text: string) => void;
  onFilters: (next: Criteria) => void;
  onClearAskAround: () => void;
  inputRef: React.RefObject<HTMLInputElement | null>;
}

const DURATIONS = [30, 60, 90, 120, 180];
const WALKS = [5, 10, 20, 30];
const THEMES = ["nature", "randonnee", "montagne", "eau", "culture", "patrimoine", "musee", "loisirs"];

/** Étapes 1 et 2 : gare de départ, puis envie (texte libre ou filtres). */
export default function SearchPanel(props: Props) {
  const { criteria: c } = props;
  const [text, setText] = useState("");
  const [showFilters, setShowFilters] = useState(false);
  const formRef = useRef<HTMLFormElement>(null);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!text.trim() || props.loading) return;
    props.onAsk(text.trim());
    setText("");
  };

  const update = (patch: Partial<Criteria>) => props.onFilters({ ...c, ...patch, keywords: [] });
  const toggleTheme = (t: string) =>
    update({ themes: c.themes.includes(t) ? c.themes.filter((x) => x !== t) : [...c.themes, t] });

  const activeCount =
    (c.max_travel_minutes ? 1 : 0) + (c.max_walk_minutes ? 1 : 0) + c.themes.length + (c.audience ? 1 : 0);

  return (
    <div className="search">
      <OriginPicker value={c.origin} stations={props.origins} onChange={(origin) => update({ origin })} />

      <form ref={formRef} onSubmit={submit} className="ask">
        <label htmlFor="ask-input" className="ask-label">
          {props.askAround ? `Votre question sur ${props.askAround.name}` : "Qu'avez-vous envie de faire ?"}
        </label>
        {!props.askAround && c.place && (
          <div className="context-chip">
            <span>
              Destination : <strong>{c.place}</strong>
            </span>
            <button type="button" onClick={() => update({ place: null })} aria-label="Retirer la destination">
              ✕
            </button>
          </div>
        )}
        {props.askAround && (
          <div className="context-chip">
            <span>
              Autour de la gare de <strong>{props.askAround.name}</strong>
            </span>
            <button type="button" onClick={props.onClearAskAround} aria-label="Ne plus limiter à cette gare">
              ✕
            </button>
          </div>
        )}
        <div className="ask-row">
          <input
            id="ask-input"
            ref={props.inputRef}
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={
              props.askAround ? "Ex. que faire avec des enfants ?" : "Ex. une balade nature facile à moins d'1h"
            }
            autoComplete="off"
          />
          <button type="submit" className="primary" disabled={props.loading || !text.trim()}>
            Chercher
          </button>
        </div>
      </form>

      <button
        type="button"
        className="filters-toggle"
        aria-expanded={showFilters}
        onClick={() => setShowFilters((v) => !v)}
      >
        {showFilters ? "Masquer les filtres" : "Ou choisir avec des filtres"}
        {activeCount > 0 && <span className="count">{activeCount}</span>}
      </button>

      {showFilters && (
        <div className="filters">
          <fieldset>
            <legend>Temps de train maximum</legend>
            <div className="options">
              {DURATIONS.map((m) => (
                <button
                  key={m}
                  type="button"
                  className={`option ${c.max_travel_minutes === m ? "on" : ""}`}
                  aria-pressed={c.max_travel_minutes === m}
                  onClick={() => update({ max_travel_minutes: c.max_travel_minutes === m ? null : m })}
                >
                  {formatMinutes(m)}
                </button>
              ))}
            </div>
          </fieldset>
          <fieldset>
            <legend>Envies</legend>
            <div className="options">
              {THEMES.map((t) => (
                <button
                  key={t}
                  type="button"
                  className={`option ${c.themes.includes(t) ? "on" : ""}`}
                  aria-pressed={c.themes.includes(t)}
                  onClick={() => toggleTheme(t)}
                >
                  {TAG_LABELS[t]}
                </button>
              ))}
              <button
                type="button"
                className={`option ${c.audience === "famille" ? "on" : ""}`}
                aria-pressed={c.audience === "famille"}
                onClick={() => update({ audience: c.audience === "famille" ? null : "famille" })}
              >
                Avec des enfants
              </button>
            </div>
          </fieldset>
          <fieldset>
            <legend>Marche maximum depuis la gare</legend>
            <div className="options">
              {WALKS.map((m) => (
                <button
                  key={m}
                  type="button"
                  className={`option ${c.max_walk_minutes === m ? "on" : ""}`}
                  aria-pressed={c.max_walk_minutes === m}
                  onClick={() => update({ max_walk_minutes: c.max_walk_minutes === m ? null : m })}
                >
                  {m} min
                </button>
              ))}
            </div>
          </fieldset>
        </div>
      )}
    </div>
  );
}
