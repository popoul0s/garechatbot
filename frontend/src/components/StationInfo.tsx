import { StationService, StationTraffic } from "../api";

const ICONS: Record<string, string> = {
  toilettes: "🚻",
  toilettes_osm: "🚻",
  horaires: "🕒",
  wifi: "📶",
  frequentation: "👥",
  acces_plus: "♿",
  accessibilite: "♿",
  ascenseurs: "🛗",
  escalators: "↗️",
  piano: "🎹",
  defibrillateur: "❤️",
  consigne: "🧳",
  consigne_osm: "🧳",
  objets_trouves: "🔎",
  velo: "🚲",
  velo_parking: "🚲",
  velo_location: "🚲",
  taxi: "🚕",
  parking: "🅿️",
  autopartage: "🚗",
  bus: "🚌",
  tram: "🚊",
  restauration: "☕",
  commerces: "🥐",
  distributeur: "🏧",
  eau: "🚰",
  billets: "🎫",
  pharmacie: "💊",
  info_touristique: "ℹ️",
};

/** Ordre d'affichage : en gare d'abord, puis pour repartir, puis autour. */
const GROUPS: { title: string; keys: string[] }[] = [
  {
    title: "En gare",
    keys: ["horaires", "toilettes", "wifi", "consigne", "objets_trouves", "piano", "defibrillateur", "billets", "frequentation"],
  },
  { title: "Accessibilité", keys: ["acces_plus", "accessibilite", "ascenseurs", "escalators"] },
  {
    title: "Pour continuer le trajet",
    keys: ["bus", "tram", "taxi", "velo", "velo_parking", "velo_location", "autopartage", "parking"],
  },
  {
    title: "Juste autour",
    keys: ["toilettes_osm", "consigne_osm", "restauration", "commerces", "distributeur", "eau", "pharmacie", "info_touristique"],
  },
];

interface Props {
  pmr: boolean | null;
  equipments: string[];
  services: StationService[];
  traffic: StationTraffic | null;
}

/** Infos pratiques d'une gare : trafic (horaires SNCF) et services (SNCF Open Data, OpenStreetMap). */
export default function StationInfo({ pmr, equipments, services, traffic }: Props) {
  const known = new Set(GROUPS.flatMap((g) => g.keys));
  const groups = [
    ...GROUPS.map((g) => ({
      title: g.title,
      items: services
        .filter((s) => g.keys.includes(s.category))
        .sort((a, b) => g.keys.indexOf(a.category) - g.keys.indexOf(b.category)),
    })),
    { title: "Autres", items: services.filter((s) => !known.has(s.category)) },
  ].filter((g) => g.items.length > 0);
  const sources = [...new Set(services.map((s) => s.source))];
  const empty = !traffic && services.length === 0 && pmr === null && equipments.length === 0;

  return (
    <details className="station-info" open>
      <summary>Infos pratiques de la gare</summary>
      {empty && (
        <p className="muted small">
          Pas encore d'infos pour cette gare (lancer <code>python run_all.py services</code>).
        </p>
      )}
      {traffic && (
        <div className="traffic">
          <div className="traffic-stats">
            <span>
              <strong>{traffic.departures}</strong> trains / jour
            </span>
            {traffic.first_departure && (
              <span>
                1<sup>er</sup> départ <strong>{traffic.first_departure}</strong>
              </span>
            )}
            {traffic.last_departure && (
              <span>
                dernier <strong>{traffic.last_departure}</strong>
              </span>
            )}
            <span>
              <strong>{traffic.direct_destinations}</strong> gares en direct
            </span>
          </div>
          {traffic.directions.length > 0 && (
            <p className="small">
              <span className="muted">Directions : </span>
              {traffic.directions.join(", ")}
            </p>
          )}
          {traffic.lines.length > 0 && (
            <p className="small">
              <span className="muted">Lignes : </span>
              {traffic.lines.join(", ")}
            </p>
          )}
        </div>
      )}
      {pmr !== null && (
        <p className="small">♿ {pmr ? "Gare accessible aux personnes à mobilité réduite" : "Gare non accessible PMR"}</p>
      )}
      {groups.map((g) => (
        <div key={g.title} className="service-group">
          <h4>{g.title}</h4>
          <ul className="services">
            {g.items.map((s) => (
              <li key={`${s.category}-${s.source}`} title={`Source : ${s.source}`}>
                <span className="service-icon" aria-hidden>
                  {ICONS[s.category] ?? "•"}
                </span>
                <span>
                  {s.label}
                  {s.detail && <span className="muted"> · {s.detail}</span>}
                </span>
              </li>
            ))}
          </ul>
        </div>
      ))}
      {equipments.length > 0 && (
        <p className="small">
          <span className="muted">Équipements : </span>
          {equipments.join(", ")}
        </p>
      )}
      <p className="source">
        Sources : {[traffic ? "horaires SNCF (GTFS)" : null, ...sources].filter(Boolean).join(" · ")}
      </p>
    </details>
  );
}
