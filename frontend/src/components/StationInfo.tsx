import { useState } from "react";
import { StationService } from "../api";

const ICONS: Record<string, string> = {
  toilettes: "🚻",
  toilettes_osm: "🚻",
  horaires: "🕒",
  wifi: "📶",
  frequentation: "👥",
  acces_plus: "🧑‍🦽",
  accessibilite: "♿",
  pmr: "♿",
  ascenseurs: "🛗",
  escalators: "🪜",
  piano: "🎹",
  defibrillateur: "❤️‍🩹",
  consigne: "🧳",
  consigne_osm: "🧳",
  objets_trouves: "🔎",
  velo: "🚲",
  velo_parking: "🚲",
  velo_location: "🚴",
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

/** Groupes affichés en lignes de pastilles, chacun avec sa couleur. */
const GROUPS: { key: string; title: string; keys: string[] }[] = [
  {
    key: "gare",
    title: "En gare",
    keys: ["horaires", "toilettes", "wifi", "consigne", "objets_trouves", "billets", "piano", "defibrillateur", "frequentation"],
  },
  { key: "access", title: "Accessibilité", keys: ["pmr", "acces_plus", "accessibilite", "ascenseurs", "escalators"] },
  {
    key: "trajet",
    title: "Pour repartir",
    keys: ["bus", "tram", "taxi", "velo", "velo_parking", "velo_location", "autopartage", "parking"],
  },
  {
    key: "autour",
    title: "Juste autour",
    keys: ["toilettes_osm", "consigne_osm", "restauration", "commerces", "distributeur", "eau", "pharmacie", "info_touristique"],
  },
];

interface Props {
  pmr: boolean | null;
  equipments: string[];
  services: StationService[];
}

type Align = "left" | "center" | "right";

function Tile({ s, open, align, onShow, onHide }: {
  s: StationService;
  open: boolean;
  align: Align;
  onShow: (el: HTMLElement) => void;
  onHide: () => void;
}) {
  const id = `tip-${s.category}-${s.source.replace(/\W/g, "")}`;
  return (
    <li className="tile-wrap">
      <button
        type="button"
        className="tile"
        aria-label={s.detail ? `${s.label} : ${s.detail}` : s.label}
        aria-describedby={open ? id : undefined}
        onMouseEnter={(e) => onShow(e.currentTarget)}
        onMouseLeave={onHide}
        onFocus={(e) => onShow(e.currentTarget)}
        onBlur={onHide}
        onClick={(e) => (open ? onHide() : onShow(e.currentTarget))}
      >
        <span aria-hidden>{ICONS[s.category] ?? "•"}</span>
      </button>
      {open && (
        <span role="tooltip" id={id} className={`tip tip-${align}`}>
          <strong>{s.label}</strong>
          {s.detail && <span>{s.detail}</span>}
          <span className="tip-source">Source : {s.source}</span>
        </span>
      )}
    </li>
  );
}

/** Infos pratiques d'une gare : services en pastilles (SNCF Open Data, OpenStreetMap). */
export default function StationInfo({ pmr, equipments, services }: Props) {
  const [tip, setTip] = useState<{ key: string; align: Align } | null>(null);

  // l'accessibilité PMR de la gare devient une pastille comme les autres
  const all: StationService[] = [
    ...(pmr === true
      ? [{ category: "pmr", label: "Gare accessible PMR", detail: null, source: "SNCF Open Data" }]
      : []),
    ...(equipments.length > 0
      ? [{ category: "accessibilite", label: "Équipements d'accessibilité", detail: equipments.join(", "), source: "SNCF Open Data" }]
      : []),
    ...services,
  ];
  // une même catégorie venant de deux sources n'est montrée qu'une fois
  const seen = new Set<string>();
  const unique = all.filter((s) => !seen.has(s.category) && !!seen.add(s.category));
  const known = new Set(GROUPS.flatMap((g) => g.keys));
  const rows = [
    ...GROUPS.map((g) => ({
      ...g,
      items: unique.filter((s) => g.keys.includes(s.category)).sort((a, b) => g.keys.indexOf(a.category) - g.keys.indexOf(b.category)),
    })),
    { key: "autres", title: "Autres", keys: [], items: unique.filter((s) => !known.has(s.category)) },
  ].filter((r) => r.items.length > 0);
  const sources = [...new Set(services.map((s) => s.source))];

  const show = (key: string) => (el: HTMLElement) => {
    // bulle alignée pour ne pas sortir du panneau
    const row = el.closest(".tiles")?.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    const x = row ? (r.left + r.width / 2 - row.left) / row.width : 0.5;
    setTip({ key, align: x < 0.3 ? "left" : x > 0.7 ? "right" : "center" });
  };

  return (
    <section className="station-info" aria-label="Infos pratiques de la gare">
      <h3>Infos pratiques</h3>
      {rows.length > 0 && (
        <div className="tile-rows">
          {rows.map((r) => (
            <div key={r.key} className={`tile-row tiles-${r.key}`}>
              <span className="tile-count">
                {r.title}
                <small>
                  {r.items.length} service{r.items.length > 1 ? "s" : ""}
                </small>
              </span>
              <ul className="tiles">
                {r.items.map((s) => {
                  const key = `${s.category}|${s.source}`;
                  return (
                    <Tile
                      key={key}
                      s={s}
                      open={tip?.key === key}
                      align={tip?.align ?? "center"}
                      onShow={show(key)}
                      onHide={() => setTip((t) => (t?.key === key ? null : t))}
                    />
                  );
                })}
              </ul>
            </div>
          ))}
        </div>
      )}
      {rows.length === 0 && (
        <p className="muted small">
          Pas encore d'infos pour cette gare (lancer <code>python run_all.py services</code>).
        </p>
      )}
      {sources.length > 0 && (
        <p className="source">
          Survolez une pastille pour le détail · Sources :{" "}
          {sources.join(" · ")}
        </p>
      )}
    </section>
  );
}
