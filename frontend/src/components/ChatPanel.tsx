import { FormEvent, useEffect, useRef, useState } from "react";
import { ChatResponse, Criteria, formatMinutes, Recommendation, StationSummary, TAG_LABELS } from "../api";

export type Message = { role: "user"; text: string } | { role: "assistant"; data: ChatResponse } | { role: "error"; text: string };

interface Props {
  messages: Message[];
  loading: boolean;
  selectedStation: StationSummary | null;
  criteria: Criteria | null;
  onSend: (text: string) => void;
  onReset: () => void;
  onClearSelection: () => void;
  onFocus: (stationId: number) => void;
}

const EXAMPLES = [
  "Une sortie nature à moins de 1h30 de Grenoble, randonnée facile, moins de 20 min de marche",
  "Une journée à la montagne sans voiture, moins de 2h de Grenoble, avec des enfants",
  "Je cherche une destination avec randonnée et patrimoine",
];

function CriteriaChips({ c }: { c: Criteria }) {
  const chips: string[] = [];
  if (c.origin) chips.push(`📍 ${c.origin}`);
  if (c.max_travel_minutes) chips.push(`🚆 ≤ ${formatMinutes(c.max_travel_minutes)}`);
  if (c.max_walk_minutes) chips.push(`🚶 ≤ ${c.max_walk_minutes} min`);
  c.themes.forEach((t) => chips.push(TAG_LABELS[t] ?? t));
  if (c.audience === "famille") chips.push("👨‍👩‍👧 Famille");
  if (c.difficulty) chips.push(`Difficulté : ${c.difficulty}`);
  c.keywords.forEach((k) => chips.push(`« ${k} »`));
  if (chips.length === 0) return null;
  return (
    <div className="chips" title="Critères compris et conservés pendant la session">
      {chips.map((ch) => (
        <span key={ch} className="chip">
          {ch}
        </span>
      ))}
    </div>
  );
}

function RecommendationCard({
  r,
  rank,
  explanation,
  onFocus,
}: {
  r: Recommendation;
  rank: number;
  explanation: string;
  onFocus: () => void;
}) {
  return (
    <button className="reco" onClick={onFocus} title="Voir sur la carte">
      <div className="reco-head">
        <span className="rank">{rank}</span>
        <strong>{r.station.name}</strong>
        {r.travel_minutes !== null && (
          <span className="badge">
            🚆 {formatMinutes(r.travel_minutes)}
            {r.nb_changes ? ` · ${r.nb_changes} corresp.` : " · direct"}
          </span>
        )}
      </div>
      <p>{explanation}</p>
      <ul className="poi-list">
        {r.pois.slice(0, 3).map((p) => (
          <li key={p.id}>
            {p.name} <span className="muted">· 🚶 {p.walk_minutes} min</span>
          </li>
        ))}
      </ul>
      <div className="score" title="Composantes du score (0 à 1)">
        score {r.score.toFixed(2)} — thème {r.breakdown.theme.toFixed(2)} · trajet {r.breakdown.travel.toFixed(2)} ·
        marche {r.breakdown.walk.toFixed(2)} · richesse {r.breakdown.richness.toFixed(2)} · accès{" "}
        {r.breakdown.accessibility.toFixed(2)}
      </div>
    </button>
  );
}

function AssistantMessage({ d, onFocus }: { d: ChatResponse; onFocus: (id: number) => void }) {
  const explanations = new Map(d.answer.items.map((i) => [i.station_id, i.explanation]));
  const notes = d.notes.filter((n) => n !== d.answer.intro);
  return (
    <div className="msg assistant">
      <p>{d.answer.intro}</p>
      {notes.map((n) => (
        <p key={n} className="note">
          ℹ️ {n}
        </p>
      ))}
      {d.recommendations.map((r, i) => (
        <RecommendationCard
          key={r.station.id}
          r={r}
          rank={i + 1}
          explanation={explanations.get(r.station.id) ?? r.facts.join(". ")}
          onFocus={() => onFocus(r.station.id)}
        />
      ))}
      <div className="engine">
        compréhension : {d.engine.extraction} · rédaction : {d.engine.generation}
        {d.engine.model ? ` (${d.engine.model})` : ""} · {d.timings.total_ms} ms
      </div>
    </div>
  );
}

export default function ChatPanel(props: Props) {
  const [text, setText] = useState("");
  const bottom = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // pas de "return" implicite : scrollIntoView renvoie une Promise dans les navigateurs récents,
    // que React prendrait pour une fonction de nettoyage ("destroy is not a function").
    bottom.current?.scrollIntoView({ behavior: "smooth" });
  }, [props.messages, props.loading]);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!text.trim() || props.loading) return;
    props.onSend(text.trim());
    setText("");
  };

  return (
    <div className="chat">
      <div className="chat-context">
        {props.criteria && <CriteriaChips c={props.criteria} />}
        {props.selectedStation && (
          <div className="selection">
            Contexte : gare de <strong>{props.selectedStation.name}</strong>
            <button onClick={props.onClearSelection} aria-label="Retirer la gare du contexte">
              ✕
            </button>
          </div>
        )}
        {props.messages.length > 0 && (
          <button className="link" onClick={props.onReset}>
            Nouvelle recherche
          </button>
        )}
      </div>

      <div className="messages">
        {props.messages.length === 0 && (
          <div className="welcome">
            <p>Décrivez votre envie de sortie en train, je cherche dans les données disponibles.</p>
            {EXAMPLES.map((ex) => (
              <button key={ex} className="example" onClick={() => props.onSend(ex)}>
                {ex}
              </button>
            ))}
          </div>
        )}
        {props.messages.map((m, i) =>
          m.role === "user" ? (
            <div key={i} className="msg user">
              {m.text}
            </div>
          ) : m.role === "error" ? (
            <div key={i} className="msg error">
              {m.text}
            </div>
          ) : (
            <AssistantMessage key={i} d={m.data} onFocus={props.onFocus} />
          ),
        )}
        {props.loading && <div className="msg assistant loading">Recherche en cours…</div>}
        <div ref={bottom} />
      </div>

      <form className="composer" onSubmit={submit}>
        <input
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder={props.selectedStation ? `Que faire autour de ${props.selectedStation.name} ?` : "Votre demande…"}
        />
        <button type="submit" disabled={props.loading || !text.trim()}>
          Envoyer
        </button>
      </form>
    </div>
  );
}
