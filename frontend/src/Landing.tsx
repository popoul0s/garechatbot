import { FormEvent, useEffect, useMemo, useState } from "react";
import { ArrowRight, MapTrifold, MagnifyingGlass } from "@phosphor-icons/react";
import { api } from "./api";
import { go } from "./route";

const EXAMPLES = [
  "Une balade au bord d'un lac, moins d'1h de train",
  "Un château à visiter avec des enfants",
  "Un point de vue en montagne depuis Chambéry",
];

/** Pondération réelle du classement (backend/src/domain/scoring.rs). */
const WEIGHTS = [
  { label: "Vos envies", value: 35, hint: "Les lieux autour de la gare couvrent-ils ce que vous demandez ?" },
  { label: "Durée du trajet", value: 25, hint: "Moins de train, plus de temps sur place." },
  { label: "Marche depuis la gare", value: 15, hint: "Les sites les plus proches du quai comptent plus." },
  { label: "Richesse du lieu", value: 15, hint: "Un lac ou un château pèse plus qu'un square." },
  { label: "Accessibilité", value: 10, hint: "Gare accessible, trajet direct." },
];

const SOURCES = [
  { name: "Horaires TER SNCF", what: "Chaque train de la journée type, les correspondances, le dernier retour.", via: "GTFS, transport.data.gouv.fr" },
  { name: "OpenStreetMap", what: "Lacs, sommets, châteaux, cascades, musées à moins de 30 min à pied des gares.", via: "API Overpass" },
  { name: "SNCF Open Data", what: "Services en gare : toilettes, horaires d'ouverture, accessibilité.", via: "ressources.data.sncf.com" },
  { name: "Réseau ferré national", what: "Le tracé réel des voies, pour suivre votre trajet sur la carte.", via: "SNCF Réseau" },
];

type Line = { d: string; len: number };

/** Projette les lignes GeoJSON du réseau dans un repère SVG (équirectangulaire corrigé de la latitude). */
function project(fc: GeoJSON.FeatureCollection): { lines: Line[]; w: number; h: number } | null {
  const parts: number[][][] = [];
  for (const f of fc.features) {
    const g = f.geometry;
    if (g?.type === "LineString") parts.push(g.coordinates);
    else if (g?.type === "MultiLineString") parts.push(...g.coordinates);
  }
  const pts = parts.flat();
  if (pts.length < 2) return null;
  const lons = pts.map((p) => p[0]);
  const lats = pts.map((p) => p[1]);
  const [minLon, maxLon, minLat, maxLat] = [Math.min(...lons), Math.max(...lons), Math.min(...lats), Math.max(...lats)];
  const k = Math.cos((((minLat + maxLat) / 2) * Math.PI) / 180);
  const w = 1000;
  const scale = w / ((maxLon - minLon) * k || 1);
  const h = (maxLat - minLat) * scale;
  // au plus ~15 000 points au total : le dessin reste fluide
  const step = Math.max(1, Math.ceil(pts.length / 15000));
  const lines = parts
    .filter((c) => c.length > 1)
    .map((coords) => {
      const kept = coords.filter((_, i) => i % step === 0 || i === coords.length - 1);
      let len = 0;
      const xy = kept.map(([lon, lat], i) => {
        const x = (lon - minLon) * k * scale;
        const y = (maxLat - lat) * scale;
        if (i > 0) {
          const [px, py] = [(kept[i - 1][0] - minLon) * k * scale, (maxLat - kept[i - 1][1]) * scale];
          len += Math.hypot(x - px, y - py);
        }
        return `${x.toFixed(1)} ${y.toFixed(1)}`;
      });
      return { d: `M${xy.join("L")}`, len };
    });
  return { lines, w, h };
}

function Network() {
  const [fc, setFc] = useState<GeoJSON.FeatureCollection | null>(null);
  useEffect(() => {
    api.mapLines().then(setFc).catch(() => setFc(null));
  }, []);
  const net = useMemo(() => (fc ? project(fc) : null), [fc]);
  if (!net) return <div className="lp-network lp-network-empty" aria-hidden />;
  // les plus longues lignes portent les « trains » lumineux
  const longest = [...net.lines.keys()].sort((a, b) => net.lines[b].len - net.lines[a].len).slice(0, 5);
  return (
    <svg className="lp-network" viewBox={`-20 -20 ${net.w + 40} ${net.h + 40}`} role="img" aria-label="Le réseau TER d'Auvergne-Rhône-Alpes">
      <g className="lp-tracks">
        {net.lines.map((l, i) => (
          <path key={i} id={`lp-line-${i}`} d={l.d} pathLength={1} style={{ animationDelay: `${(i % 24) * 45}ms` }} />
        ))}
      </g>
      <g className="lp-trains">
        {longest.map((i, n) => (
          <circle key={i} r={5}>
            <animateMotion dur={`${9 + n * 2.5}s`} begin={`${1.6 + n * 0.7}s`} repeatCount="indefinite" rotate="auto">
              <mpath href={`#lp-line-${i}`} />
            </animateMotion>
          </circle>
        ))}
      </g>
    </svg>
  );
}

export default function Landing() {
  const [text, setText] = useState("");
  const [stations, setStations] = useState<number | null>(null);

  useEffect(() => {
    document.title = "Aiguillage : vos sorties en train en Auvergne-Rhône-Alpes";
    api
      .origins()
      .then((o) => setStations(o.length))
      .catch(() => setStations(null));
  }, []);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    go("app", text.trim() || undefined);
  };
  const scrollTo = (id: string) => document.getElementById(id)?.scrollIntoView({ behavior: "smooth" });

  return (
    <div className="lp">
      <nav className="lp-nav" aria-label="Navigation principale">
        <a className="lp-wordmark" href="#/" onClick={(e) => e.preventDefault()}>
          Aiguillage
        </a>
        <div className="lp-nav-links">
          <button onClick={() => scrollTo("lp-how")}>Comment ça marche</button>
          <button onClick={() => scrollTo("lp-sources")}>Les données</button>
          <button className="lp-nav-cta" onClick={() => go("app")}>
            Ouvrir l'appli
          </button>
        </div>
      </nav>

      <header className="lp-hero">
        <Network />
        <div className="lp-hero-copy">
          <h1>
            Où descendre
            <br />
            {"ce\u00a0week\u2011end\u00a0?"}
          </h1>
          <p className="lp-lead">
            Dites ce qui vous tente. Aiguillage parcourt les horaires TER et les sites autour des gares
            d'Auvergne-Rhône-Alpes, puis vous dit où descendre, quel train prendre et ce qu'il y a à voir à pied.
          </p>
          <form className="lp-search" onSubmit={submit} role="search">
            <label htmlFor="lp-q" className="lp-sr">
              Votre envie
            </label>
            <MagnifyingGlass size={22} aria-hidden className="lp-search-icon" />
            <input
              id="lp-q"
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder="Une balade au bord d'un lac, pas trop loin de Grenoble"
              autoComplete="off"
            />
            <button type="submit">
              Trouver <ArrowRight size={18} weight="bold" aria-hidden />
            </button>
          </form>
          <div className="lp-examples" aria-label="Exemples de demandes">
            {EXAMPLES.map((ex) => (
              <button key={ex} onClick={() => go("app", ex)}>
                {ex}
              </button>
            ))}
          </div>
          <button className="lp-map-link" onClick={() => go("app")}>
            <MapTrifold size={20} aria-hidden /> Ou partir de la carte
          </button>
        </div>
        <p className="lp-caption">
          En fond, le vrai réseau TER de la région
          {stations ? `, ${stations} gares desservies dans la journée type` : ""}.
        </p>
      </header>

      <section className="lp-how" id="lp-how" aria-labelledby="lp-how-title">
        <h2 id="lp-how-title">Trois arrêts, pas plus.</h2>
        <ol className="lp-line">
          <li>
            <h3>Dites ce qui vous tente</h3>
            <p>
              En une phrase, comme à un ami : « un lac à moins d'une heure, on a des enfants ». L'IA en tire vos
              critères, rien de plus.
            </p>
          </li>
          <li>
            <h3>Comparez les gares</h3>
            <p>
              Chaque destination arrive classée et justifiée : durée du trajet, correspondances, lieux à pied. La
              carte montre la même chose que la liste.
            </p>
          </li>
          <li>
            <h3>Prenez le bon train</h3>
            <p>
              Heure de départ, correspondances, dernier retour possible. Le tracé suit les voies jusqu'à la gare, puis
              le chemin à pied.
            </p>
          </li>
        </ol>
      </section>

      <section className="lp-why" aria-labelledby="lp-why-title">
        <div className="lp-why-copy">
          <h2 id="lp-why-title">Un classement qui se justifie.</h2>
          <p>
            L'IA ne choisit pas les destinations : elle comprend votre demande et rédige la réponse. Le classement,
            lui, est calculé par une formule publique, et chaque chiffre affiché vient des données. Un nom de gare ou
            une durée inventés sont retirés avant d'arriver jusqu'à vous.
          </p>
        </div>
        <figure className="lp-weights">
          <div className="lp-bar" aria-hidden>
            {WEIGHTS.map((w) => (
              <span key={w.label} style={{ flexGrow: w.value }} />
            ))}
          </div>
          <dl>
            {WEIGHTS.map((w) => (
              <div key={w.label}>
                <dt>
                  <strong>{w.value} %</strong> {w.label}
                </dt>
                <dd>{w.hint}</dd>
              </div>
            ))}
          </dl>
          <figcaption>Poids de chaque critère dans la note d'une destination.</figcaption>
        </figure>
      </section>

      <section className="lp-sources" id="lp-sources" aria-labelledby="lp-sources-title">
        <h2 id="lp-sources-title">Des données ouvertes, rien d'inventé.</h2>
        <ul>
          {SOURCES.map((s) => (
            <li key={s.name}>
              <h3>{s.name}</h3>
              <p>{s.what}</p>
              <span>{s.via}</span>
            </li>
          ))}
        </ul>
      </section>

      <section className="lp-end">
        <h2>Votre prochaine sortie commence par une phrase.</h2>
        <button className="lp-end-cta" onClick={() => go("app")}>
          Ouvrir la carte <ArrowRight size={20} weight="bold" aria-hidden />
        </button>
      </section>

      <footer className="lp-footer">
        <span>Aiguillage, projet étudiant du module IA</span>
        <span>Cartes et lieux © contributeurs OpenStreetMap. Horaires théoriques SNCF.</span>
      </footer>
    </div>
  );
}
