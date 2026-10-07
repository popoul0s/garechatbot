import { useState } from "react";
import { Plus, X } from "@phosphor-icons/react";
import { Criteria, formatMinutes, SearchOutcome, TAG_LABELS } from "../api";

const DURATIONS = [30, 60, 90, 120, 180];
const WALKS = [5, 10, 20, 30];
const THEMES = ["nature", "randonnee", "montagne", "eau", "culture", "patrimoine", "musee", "loisirs", "panorama"];

type Editor = "origin" | "travel" | "walk" | "themes" | null;

interface Props {
  criteria: Criteria;
  outcome: SearchOutcome;
  /** Nouveaux critères : la recherche est relancée (sans IA). */
  onChange: (next: Criteria) => void;
  /** Changer de gare de départ ouvre le sélecteur de gare. */
  onEditOrigin: () => void;
}

/** « Ce que j'ai compris » : chaque critère retenu, modifiable ou retirable d'un clic. */
export default function CriteriaChips({ criteria: c, outcome, onChange, onEditOrigin }: Props) {
  const [open, setOpen] = useState<Editor>(null);
  const set = (patch: Partial<Criteria>) => {
    setOpen(null);
    onChange({ ...c, ...patch, keywords: [] });
  };
  const toggle = (e: Editor) => setOpen((o) => (o === e ? null : e));
  const travelDefault = c.max_travel_minutes == null;
  const walkDefault = c.max_walk_minutes == null;
  // destination imposée : le temps de train n'est pas un filtre
  const showTravel = !c.place && !outcome.around_station;

  return (
    <section className="understood" aria-label="Ce que j'ai compris">
      <h2 className="understood-title">Ce que j'ai compris</h2>
      <div className="chips">
        <button className="chip chip-strong" onClick={onEditOrigin} title="Changer de gare de départ">
          De {outcome.origin?.name ?? c.origin}
        </button>
        {c.place && (
          <span className="chip">
            {outcome.place_area ? `Autour de ${outcome.place_area.name} (sans gare)` : `Vers ${c.place}`}
            <button className="chip-x" aria-label={`Retirer ${c.place}`} onClick={() => set({ place: null })}>
              <X size={12} weight="bold" />
            </button>
          </span>
        )}
        {outcome.around_station && <span className="chip">Autour de {outcome.around_station.name}</span>}
        {c.themes.map((t) => (
          <span key={t} className="chip">
            {TAG_LABELS[t] ?? t}
            <button
              className="chip-x"
              aria-label={`Retirer ${TAG_LABELS[t] ?? t}`}
              onClick={() => set({ themes: c.themes.filter((x) => x !== t) })}
            >
              <X size={12} weight="bold" />
            </button>
          </span>
        ))}
        {c.audience === "famille" && (
          <span className="chip">
            Avec des enfants
            <button className="chip-x" aria-label="Retirer avec des enfants" onClick={() => set({ audience: null })}>
              <X size={12} weight="bold" />
            </button>
          </span>
        )}
        {showTravel && (
          <button
            className={`chip ${travelDefault ? "chip-default" : ""}`}
            aria-expanded={open === "travel"}
            onClick={() => toggle("travel")}
            title="Changer le temps de train maximum"
          >
            {formatMinutes(outcome.applied_max_travel_minutes)} de train max
          </button>
        )}
        {!outcome.place_area && (
          <button
            className={`chip ${walkDefault ? "chip-default" : ""}`}
            aria-expanded={open === "walk"}
            onClick={() => toggle("walk")}
            title="Changer la marche maximum depuis la gare"
          >
            {outcome.applied_max_walk_minutes} min à pied max
          </button>
        )}
        <button className="chip chip-add" aria-expanded={open === "themes"} onClick={() => toggle("themes")}>
          <Plus size={12} weight="bold" aria-hidden /> Envie
        </button>
      </div>

      {open === "travel" && (
        <div className="chip-editor" role="group" aria-label="Temps de train maximum">
          {DURATIONS.map((m) => (
            <button
              key={m}
              className={`option ${outcome.applied_max_travel_minutes === m ? "on" : ""}`}
              onClick={() => set({ max_travel_minutes: m })}
            >
              {formatMinutes(m)}
            </button>
          ))}
        </div>
      )}
      {open === "walk" && (
        <div className="chip-editor" role="group" aria-label="Marche maximum depuis la gare">
          {WALKS.map((m) => (
            <button
              key={m}
              className={`option ${outcome.applied_max_walk_minutes === m ? "on" : ""}`}
              onClick={() => set({ max_walk_minutes: m })}
            >
              {m} min
            </button>
          ))}
        </div>
      )}
      {open === "themes" && (
        <div className="chip-editor" role="group" aria-label="Ajouter une envie">
          {THEMES.filter((t) => !c.themes.includes(t)).map((t) => (
            <button key={t} className="option" onClick={() => set({ themes: [...c.themes, t] })}>
              {TAG_LABELS[t] ?? t}
            </button>
          ))}
          {c.audience !== "famille" && (
            <button className="option" onClick={() => set({ audience: "famille" })}>
              Avec des enfants
            </button>
          )}
        </div>
      )}
      {((showTravel && travelDefault) || (!outcome.place_area && walkDefault)) && (
        <p className="understood-hint">En gris : valeurs par défaut, cliquez pour les changer.</p>
      )}
    </section>
  );
}
