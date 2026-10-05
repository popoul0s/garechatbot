import { useCallback, useEffect, useState } from "react";
import { api, ChatResponse, Criteria, PoiNearStation, Recommendation, Station } from "./api";
import ChatPanel, { Message } from "./components/ChatPanel";
import ExplorerPanel from "./components/ExplorerPanel";
import MapView from "./components/MapView";

type Tab = "assistant" | "explorer" | "map";

export default function App() {
  const [tab, setTab] = useState<Tab>("assistant");
  const [stations, setStations] = useState<GeoJSON.FeatureCollection | null>(null);
  const [lines, setLines] = useState<GeoJSON.FeatureCollection | null>(null);
  const [origin, setOrigin] = useState("Grenoble");

  const [messages, setMessages] = useState<Message[]>([]);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [criteria, setCriteria] = useState<Criteria | null>(null);
  const [loading, setLoading] = useState(false);
  const [results, setResults] = useState<Recommendation[]>([]);
  const [focused, setFocused] = useState<number | null>(null);

  const [selected, setSelected] = useState<{ station: Station; pois: PoiNearStation[] } | null>(null);

  useEffect(() => {
    api.mapLines().then(setLines).catch(console.error);
  }, []);
  useEffect(() => {
    api.mapStations(origin).then(setStations).catch(console.error);
  }, [origin]);

  // Carte -> contexte : sélectionner une gare charge ses POI et en fait le contexte de l'assistant.
  const selectStation = useCallback((id: number) => {
    api
      .station(id)
      .then((d) => {
        setSelected(d);
        setFocused(id);
      })
      .catch(console.error);
  }, []);

  const send = async (text: string) => {
    setMessages((m) => [...m, { role: "user", text }]);
    setLoading(true);
    if (window.matchMedia("(max-width: 800px)").matches) setTab("assistant");
    try {
      const d: ChatResponse = await api.chat(text, sessionId, selected?.station.id ?? null);
      setSessionId(d.session_id);
      setCriteria(d.criteria);
      setResults(d.recommendations); // IA -> carte
      if (d.origin) setOrigin(d.origin.name);
      setMessages((m) => [...m, { role: "assistant", data: d }]);
    } catch (e) {
      setMessages((m) => [...m, { role: "error", text: `Erreur : ${(e as Error).message}` }]);
    } finally {
      setLoading(false);
    }
  };

  const reset = () => {
    if (sessionId) api.resetChat(sessionId).catch(console.error);
    setSessionId(null);
    setCriteria(null);
    setMessages([]);
    setResults([]);
  };

  const focus = (id: number) => {
    setFocused(id);
    if (window.matchMedia("(max-width: 800px)").matches) setTab("map");
  };

  return (
    <div className={`app tab-${tab}`}>
      <header>
        <h1>🚆 GareChatBot</h1>
        <span className="muted">Découvrir Auvergne-Rhône-Alpes en train</span>
      </header>
      <main>
        <section className="panel">
          <nav className="tabs">
            <button className={tab === "assistant" ? "active" : ""} onClick={() => setTab("assistant")}>
              Assistant
            </button>
            <button className={tab === "explorer" ? "active" : ""} onClick={() => setTab("explorer")}>
              Explorer
            </button>
          </nav>
          <div className="panel-body">
            {tab !== "explorer" ? (
              <ChatPanel
                messages={messages}
                loading={loading}
                criteria={criteria}
                selectedStation={selected?.station ?? null}
                onSend={send}
                onReset={reset}
                onClearSelection={() => setSelected(null)}
                onFocus={focus}
              />
            ) : (
              <ExplorerPanel selected={selected} onSelectStation={selectStation} onAsk={() => setTab("assistant")} />
            )}
          </div>
        </section>
        <section className="map-wrap">
          <MapView
            stations={stations}
            lines={lines}
            results={results}
            focusedStationId={focused}
            selectedStationId={selected?.station.id ?? null}
            stationPois={selected?.pois ?? []}
            onSelectStation={selectStation}
            visible={tab === "map" || !window.matchMedia("(max-width: 800px)").matches}
          />
          <div className="legend">
            Trajet depuis {origin} : <i style={{ background: "#16a34a" }} />≤30 min <i style={{ background: "#84cc16" }} />
            ≤1h <i style={{ background: "#f59e0b" }} />≤1h30 <i style={{ background: "#f97316" }} />≤2h{" "}
            <i style={{ background: "#dc2626" }} />+2h
          </div>
        </section>
      </main>
      <nav className="mobile-tabs">
        <button className={tab === "assistant" ? "active" : ""} onClick={() => setTab("assistant")}>
          💬 Assistant
        </button>
        <button className={tab === "explorer" ? "active" : ""} onClick={() => setTab("explorer")}>
          🔎 Explorer
        </button>
        <button className={tab === "map" ? "active" : ""} onClick={() => setTab("map")}>
          🗺️ Carte{results.length ? ` (${results.length})` : ""}
        </button>
      </nav>
    </div>
  );
}
