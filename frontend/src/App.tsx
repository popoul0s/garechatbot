import { useCallback, useEffect, useRef, useState } from "react";
import { go } from "./route";
import {
  Answer,
  api,
  Journey,
  Criteria,
  EMPTY_CRITERIA,
  PoiNearStation,
  SearchOutcome,
  Station,
  StationDetailData,
  StationSummary,
} from "./api";
import MapView, { MapMode } from "./components/MapView";
import Results from "./components/Results";
import OriginPrompt from "./components/OriginPrompt";
import SearchPanel from "./components/SearchPanel";
import StationDetail from "./components/StationDetail";

type MobileTab = "search" | "map";
const ORIGIN_KEY = "garechatbot.origin";

/** Dernière gare de départ utilisée : proposée en un clic, jamais choisie d'office. */
function savedOrigin(): string | null {
  try {
    return localStorage.getItem(ORIGIN_KEY);
  } catch {
    return null;
  }
}

const STEPS = [
  ["Choisissez votre gare de départ", "Les temps de trajet sont calculés depuis les horaires SNCF."],
  ["Décrivez votre envie, ou cochez des filtres", "Nature, culture, avec des enfants, temps de train, marche…"],
  ["Comparez les destinations", "Sur la liste et sur la carte. Cliquez une destination pour voir ce qu'il y a autour."],
];

const SUGGESTIONS = [
  "Une balade nature facile à moins d'1h30, peu de marche",
  "Une journée à la montagne avec des enfants",
  "Du patrimoine et une randonnée",
];

const isMobile = () => window.matchMedia("(max-width: 800px)").matches;

export default function App({ initialQuery = null }: { initialQuery?: string | null }) {
  const [mobileTab, setMobileTab] = useState<MobileTab>("search");
  const [origins, setOrigins] = useState<Station[]>([]);
  const [stationsGeo, setStationsGeo] = useState<GeoJSON.FeatureCollection | null>(null);
  const [linesGeo, setLinesGeo] = useState<GeoJSON.FeatureCollection | null>(null);

  // pas de gare de départ par défaut : elle est demandée à la première recherche qui n'en cite pas
  const [criteria, setCriteria] = useState<Criteria>({ ...EMPTY_CRITERIA });
  // recherche en attente de la gare de départ, relancée dès qu'elle est choisie
  const [pending, setPending] = useState<{ kind: "ask"; text: string } | { kind: "filters"; next: Criteria } | null>(
    null,
  );
  const [sessionId, setSessionId] = useState<string | null>(null);
  // demande venue de l'accueil : affichée dès le premier rendu (la transition montre le champ déjà rempli)
  const [query, setQuery] = useState<string | null>(initialQuery);
  const [resetKey, setResetKey] = useState(0);
  const [outcome, setOutcome] = useState<SearchOutcome | null>(null);
  const [answer, setAnswer] = useState<Answer | null>(null);
  const [engine, setEngine] = useState<{ label: string; ms: number } | null>(null);
  const [loading, setLoading] = useState(!!initialQuery);
  const [error, setError] = useState<string | null>(null);

  const [detail, setDetail] = useState<StationDetailData | null>(null);
  const [askAround, setAskAround] = useState<StationSummary | null>(null);
  const [visiblePois, setVisiblePois] = useState<PoiNearStation[]>([]);
  const [focusedPoi, setFocusedPoi] = useState<number | null>(null);
  const [journey, setJourney] = useState<Journey | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    api.mapLines().then(setLinesGeo).catch(console.error);
    api
      .origins()
      .then(setOrigins)
      .catch(console.error);
  }, []);
  useEffect(() => {
    api.mapStations(criteria.origin).then(setStationsGeo).catch(console.error);
    if (!criteria.origin) return;
    try {
      localStorage.setItem(ORIGIN_KEY, criteria.origin);
    } catch {
      /* stockage indisponible : sans conséquence */
    }
  }, [criteria.origin]);

  const afterResults = (o: SearchOutcome) => {
    setOutcome(o);
    setDetail(null);
  };

  // gare choisie (en haut ou dans la question « De quelle gare partez-vous ? ») : la recherche en
  // attente repart avec elle ; sinon, les résultats affichés sont recalculés depuis cette gare
  const chooseOrigin = (name: string) => {
    const p = pending;
    setPending(null);
    if (p?.kind === "ask") ask(p.text, name);
    else if (p?.kind === "filters") runFilters({ ...p.next, origin: name });
    else if (outcome) runFilters({ ...criteria, origin: name, keywords: [] });
    else setCriteria((c) => ({ ...c, origin: name }));
  };

  // Recherche par filtres : aucune IA, directement la recherche + classement du backend.
  const runFilters = async (next: Criteria) => {
    setCriteria(next);
    setQuery(null);
    setLoading(true);
    setError(null);
    const t = performance.now();
    try {
      const o = await api.search({ ...next, around_station_id: askAround?.id ?? null });
      setPending(o.needs_origin ? { kind: "filters", next } : null);
      afterResults(o);
      setAnswer(null);
      setEngine({ label: "Recherche par filtres (sans IA)", ms: Math.round(performance.now() - t) });
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  };

  // Recherche en langage naturel : l'IA comprend la demande, le backend cherche et classe.
  const ask = async (text: string, origin?: string) => {
    setQuery(text);
    setLoading(true);
    setError(null);
    try {
      const context = origin ? { ...criteria, origin } : criteria;
      const d = await api.chat(text, sessionId, askAround?.id ?? null, context);
      setSessionId(d.session_id);
      setPending(d.needs_origin ? { kind: "ask", text } : null);
      // gare de départ : celle réellement retenue par la recherche (« en partant de gieres » -> Gières)
      setCriteria({ ...d.criteria, origin: d.origin?.name ?? (d.needs_origin ? null : d.criteria.origin), around_station_id: null });
      afterResults(d);
      // Sans IA, le texte généré répète les infos déjà affichées sur chaque carte : on ne le montre pas.
      setAnswer(d.engine.generation === "llm" ? d.answer : null);
      setEngine({
        label: `Compréhension : ${d.engine.extraction === "llm" ? "IA" : "règles"}, rédaction : ${
          d.engine.generation === "llm" ? "IA" : "modèle de texte"
        }${d.engine.model ? ` (${d.engine.model})` : ""}`,
        ms: d.timings.total_ms,
      });
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  };

  // Carte ou liste -> fiche de la gare
  const openStation = useCallback((id: number) => {
    setFocusedPoi(null);
    setJourney(null);
    api
      .station(id)
      .then((d) => {
        setDetail(d);
        if (isMobile()) setMobileTab("search");
      })
      .catch(console.error);
  }, []);

  const askAboutStation = () => {
    if (!detail) return;
    setAskAround({ ...detail.station });
    setDetail(null);
    setTimeout(() => inputRef.current?.focus(), 0);
  };

  // demande saisie sur la page d'accueil : lancée une seule fois à l'ouverture
  const started = useRef(false);
  useEffect(() => {
    document.title = "Aiguillage";
    if (initialQuery && !started.current) {
      started.current = true;
      ask(initialQuery);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const reset = () => {
    if (sessionId) api.resetChat(sessionId).catch(console.error);
    setSessionId(null);
    setCriteria({ ...EMPTY_CRITERIA, origin: criteria.origin });
    setPending(null);
    setResetKey((k) => k + 1);
    setOutcome(null);
    setAnswer(null);
    setQuery(null);
    setDetail(null);
    setAskAround(null);
    setError(null);
  };

  const travelFor = (id: number) => {
    const f = stationsGeo?.features.find((x) => x.properties?.id === id);
    return f ? { minutes: f.properties?.minutes ?? null, nb_changes: f.properties?.nb_changes ?? null } : null;
  };

  const results = outcome?.recommendations ?? [];
  const originId =
    outcome?.origin?.id ??
    (stationsGeo?.features.find((f) => f.properties?.is_origin)?.properties?.id as number | undefined) ??
    null;
  const mapMode: MapMode = detail ? "detail" : results.length > 0 ? "results" : "overview";

  return (
    <div className={`app tab-${mobileTab}`}>
      <header>
        <a
          className="brand"
          href="#/"
          title="Retour à l'accueil"
          onClick={(e) => {
            e.preventDefault();
            go("home");
          }}
        >
          Aiguillage
        </a>
        <span className="tagline">Sorties en train en Auvergne-Rhône-Alpes</span>
        {(outcome || detail) && (
          <button className="new-search" onClick={reset}>
            Nouvelle recherche
          </button>
        )}
      </header>

      <main>
        <aside className="panel">
          {detail ? (
            <StationDetail
              station={detail.station}
              pois={detail.pois}
              services={detail.services ?? []}
              travel={travelFor(detail.station.id)}
              originName={criteria.origin}
              originId={originId}
              journey={journey}
              onSelectJourney={setJourney}
              wanted={outcome ? [...criteria.themes, ...(criteria.audience === "famille" ? ["famille"] : [])] : []}
              keywords={outcome ? criteria.keywords : []}
              canGoBack={!!outcome}
              focusedPoiId={focusedPoi}
              onBack={() => {
                setDetail(null);
                setJourney(null);
              }}
              onAsk={askAboutStation}
              onPoiClick={(id) => {
                setFocusedPoi(id);
                if (isMobile()) setMobileTab("map");
              }}
              onVisibleChange={setVisiblePois}
            />
          ) : (
            <>
              <SearchPanel
                origins={origins}
                criteria={criteria}
                loading={loading}
                askAround={askAround}
                onAsk={(text) => ask(text)}
                onFilters={runFilters}
                onOrigin={chooseOrigin}
                onClearAskAround={() => setAskAround(null)}
                inputRef={inputRef}
                query={query}
                resetKey={resetKey}
              />

              {error && (
                <p className="error" role="alert">
                  Le serveur ne répond pas correctement. Vérifiez que l'API est lancée. ({error.slice(0, 120)})
                </p>
              )}
              {loading && (
                <div className="loading" aria-live="polite">
                  <span className="spinner" aria-hidden /> Recherche des destinations…
                </div>
              )}

              {!loading && outcome?.needs_origin && (
                <OriginPrompt
                  query={pending?.kind === "ask" ? pending.text : null}
                  message={outcome.notes[0] ?? null}
                  stations={origins}
                  last={savedOrigin()}
                  onChoose={chooseOrigin}
                />
              )}

              {!loading && outcome && !outcome.needs_origin && (
                <Results
                  query={query}
                  criteria={criteria}
                  outcome={outcome}
                  answer={answer}
                  engine={engine}
                  onOpen={openStation}
                />
              )}

              {!loading && !outcome && (
                <section className="welcome">
                  <ol className="steps">
                    {STEPS.map(([title, text], i) => (
                      <li key={title}>
                        <span className="step-n">{i + 1}</span>
                        <div>
                          <strong>{title}</strong>
                          <p>{text}</p>
                        </div>
                      </li>
                    ))}
                  </ol>
                  <p className="muted">Pour essayer :</p>
                  <div className="suggestions">
                    {SUGGESTIONS.map((s) => (
                      <button key={s} className="suggestion" onClick={() => ask(s)}>
                        {s}
                      </button>
                    ))}
                  </div>
                </section>
              )}
            </>
          )}
        </aside>

        <section className="map-wrap">
          <MapView
            mode={mapMode}
            stations={stationsGeo}
            lines={linesGeo}
            originName={criteria.origin}
            results={results}
            detailStation={detail?.station ?? null}
            detailPois={detail ? visiblePois : []}
            focusedPoiId={focusedPoi}
            journey={detail ? journey : null}
            onSelectStation={openStation}
            onSelectPoi={setFocusedPoi}
            visible={mobileTab === "map" || !isMobile()}
          />
        </section>
      </main>

      <nav className="mobile-tabs">
        <button className={mobileTab === "search" ? "active" : ""} onClick={() => setMobileTab("search")}>
          Recherche
        </button>
        <button className={mobileTab === "map" ? "active" : ""} onClick={() => setMobileTab("map")}>
          Carte{results.length ? ` (${results.length})` : ""}
        </button>
      </nav>
    </div>
  );
}
