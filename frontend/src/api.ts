// Types et appels vers l'API Rust (voir backend/src/routes).

export interface Station {
  id: number;
  uic: string;
  name: string;
  city: string | null;
  lon: number;
  lat: number;
  pmr: boolean | null;
  equipments: string[];
}

export interface Poi {
  id: number;
  source: string;
  name: string;
  description: string | null;
  tags: string[];
  url: string | null;
  lon: number;
  lat: number;
  /** Intérêt touristique calculé par le serveur : 1 = site majeur, 0.35 = square, aire de jeux. */
  interest: number;
}

export interface PoiNearStation extends Poi {
  walk_minutes: number;
  distance_m: number;
}

export interface PoiHit extends PoiNearStation {
  match_score: number;
}

export interface StationSummary {
  id: number;
  name: string;
  city: string | null;
  lon: number;
  lat: number;
  pmr: boolean | null;
}

export interface Recommendation {
  station: StationSummary;
  travel_minutes: number | null;
  nb_changes: number | null;
  example_departure: string | null;
  score: number;
  breakdown: Record<"theme" | "travel" | "walk" | "richness" | "accessibility", number>;
  pois: PoiHit[];
  facts: string[];
}

export interface Criteria {
  origin: string | null;
  max_travel_minutes: number | null;
  themes: string[];
  audience: string | null;
  max_walk_minutes: number | null;
  difficulty: string | null;
  keywords: string[];
  around_station_id: number | null;
  place: string | null;
}

export interface SearchOutcome {
  origin: StationSummary | null;
  around_station: StationSummary | null;
  applied_max_travel_minutes: number;
  applied_max_walk_minutes: number;
  relaxed: boolean;
  recommendations: Recommendation[];
  notes: string[];
}

export interface Answer {
  intro: string;
  items: { station_id: number; explanation: string }[];
}

export interface ChatResponse {
  session_id: string;
  criteria: Criteria;
  answer: Answer;
  origin: StationSummary | null;
  around_station: StationSummary | null;
  applied_max_travel_minutes: number;
  applied_max_walk_minutes: number;
  relaxed: boolean;
  recommendations: Recommendation[];
  notes: string[];
  engine: {
    extraction: string;
    generation: string;
    model: string | null;
    usage: { prompt_tokens: number; completion_tokens: number };
  };
  timings: { extraction_ms: number; search_ms: number; generation_ms: number; total_ms: number };
}

async function get<T>(url: string): Promise<T> {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${r.status} ${await r.text()}`);
  return r.json();
}

async function post<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!r.ok) throw new Error(`${r.status} ${await r.text()}`);
  return r.json();
}

export const api = {
  origins: () => get<Station[]>("/api/origins"),
  search: (criteria: Criteria) => post<SearchOutcome>("/api/search", criteria),
  searchStations: (q: string) => get<Station[]>(`/api/stations?q=${encodeURIComponent(q)}`),
  station: (id: number, maxWalk = 30) =>
    get<{ station: Station; pois: PoiNearStation[] }>(`/api/stations/${id}?max_walk=${maxWalk}`),
  mapStations: (origin: string) =>
    get<GeoJSON.FeatureCollection>(`/api/map/stations?origin=${encodeURIComponent(origin)}`),
  mapLines: () => get<GeoJSON.FeatureCollection>("/api/map/lines"),
  chat: (message: string, sessionId: string | null, selectedStationId: number | null, context: Criteria) =>
    post<ChatResponse>("/api/chat", {
      message,
      session_id: sessionId,
      selected_station_id: selectedStationId,
      context,
    }),
  resetChat: (sessionId: string) => post("/api/chat/reset", { session_id: sessionId }),
};

export function formatMinutes(m: number): string {
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  const r = m % 60;
  return r === 0 ? `${h}h` : `${h}h${String(r).padStart(2, "0")}`;
}

export const EMPTY_CRITERIA: Criteria = {
  origin: null,
  max_travel_minutes: null,
  themes: [],
  audience: null,
  max_walk_minutes: null,
  difficulty: null,
  keywords: [],
  around_station_id: null,
  place: null,
};

export const TAG_LABELS: Record<string, string> = {
  nature: "Nature",
  randonnee: "Randonnée",
  montagne: "Montagne",
  eau: "Lac / rivière",
  culture: "Culture",
  patrimoine: "Patrimoine",
  musee: "Musée",
  famille: "Famille",
  loisirs: "Loisirs",
  panorama: "Panorama",
};

/** Catégories affichées sur la carte : peu nombreuses pour rester lisibles. */
export const CATEGORIES = [
  { key: "culture", label: "Patrimoine et culture", color: "#7c3aed", tags: ["musee", "patrimoine", "culture"] },
  { key: "rando", label: "Randonnée et montagne", color: "#b45309", tags: ["randonnee", "montagne", "panorama"] },
  { key: "eau", label: "Lacs et rivières", color: "#0284c7", tags: ["eau"] },
  { key: "loisirs", label: "Loisirs et famille", color: "#db2777", tags: ["loisirs", "famille"] },
  { key: "nature", label: "Nature et parcs", color: "#16a34a", tags: ["nature"] },
] as const;
export type Category = (typeof CATEGORIES)[number];

export function categoryOf(tags: string[]): Category {
  return CATEGORIES.find((c) => c.tags.some((t) => tags.includes(t))) ?? CATEGORIES[CATEGORIES.length - 1];
}
