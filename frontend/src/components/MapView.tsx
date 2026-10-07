import { useEffect, useRef, useState } from "react";
import * as maplibregl from "maplibre-gl";
import type { GeoJSONSource, Map as MlMap } from "maplibre-gl";
import { categoryOf, CATEGORIES, sourceLabel, formatMinutes, Journey, PoiNearStation, Recommendation, Station } from "../api";

/**
 * La carte n'affiche que ce qui sert à l'étape en cours :
 * - overview : gares accessibles depuis le départ, colorées par temps de train ;
 * - results  : le départ + les destinations proposées, numérotées et nommées ;
 * - detail   : la gare choisie + ses lieux, numérotés comme dans la liste.
 */
export type MapMode = "overview" | "results" | "detail";

interface Props {
  mode: MapMode;
  stations: GeoJSON.FeatureCollection | null;
  lines: GeoJSON.FeatureCollection | null;
  originName: string | null;
  results: Recommendation[];
  detailStation: Station | null;
  detailPois: PoiNearStation[];
  focusedPoiId: number | null;
  /** Trajet en train sélectionné dans la fiche gare : tracé sur la carte. */
  journey: Journey | null;
  onSelectStation: (id: number) => void;
  onSelectPoi: (id: number) => void;
  visible: boolean;
  /** Destination survolée dans la liste : son marqueur est mis en avant (et inversement). */
  highlightId: number | null;
  onHoverStation: (id: number | null) => void;
}

maplibregl.setWorkerUrl("/maplibre/maplibre-gl-worker.mjs");

const EMPTY: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };

const STYLE: maplibregl.StyleSpecification = {
  version: 8,
  sources: {
    osm: {
      type: "raster",
      tiles: ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"],
      tileSize: 256,
      attribution: "© contributeurs OpenStreetMap",
      maxzoom: 19,
    },
  },
  layers: [{ id: "osm", type: "raster", source: "osm", paint: { "raster-saturation": -0.5 } }],
};

const TIME_STEPS = [
  { max: 30, color: "#16a34a", label: "30 min" },
  { max: 60, color: "#84cc16", label: "1h" },
  { max: 90, color: "#f59e0b", label: "1h30" },
  { max: 120, color: "#f97316", label: "2h" },
  { max: Infinity, color: "#dc2626", label: "plus" },
];

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

function el(className: string, html: string, title?: string): HTMLDivElement {
  const d = document.createElement("div");
  d.className = className;
  d.innerHTML = html;
  if (title) d.title = title;
  return d;
}

function poiPopupHtml(p: PoiNearStation): string {
  const cat = categoryOf(p.tags);
  return (
    `<strong>${escapeHtml(p.name)}</strong><br/>` +
    `<span style="color:${cat.color}">${cat.label}</span> · ${p.walk_minutes} min à pied de la gare` +
    (p.description ? `<p>${escapeHtml(p.description.slice(0, 200))}</p>` : "") +
    (p.url ? `<a href="${escapeHtml(p.url)}" target="_blank" rel="noreferrer">Site web</a><br/>` : "") +
    `<span class="source">Source : ${sourceLabel(p.source)}</span>`
  );
}

function fit(m: MlMap, pts: [number, number][], maxZoom: number) {
  if (pts.length === 0) return;
  const lons = pts.map((p) => p[0]);
  const lats = pts.map((p) => p[1]);
  m.fitBounds(
    [
      [Math.min(...lons), Math.min(...lats)],
      [Math.max(...lons), Math.max(...lats)],
    ],
    // marge basse plus grande : la légende occupe le bas de la carte
    { padding: { top: 60, right: 60, bottom: 150, left: 60 }, maxZoom, duration: 700 },
  );
}

export default function MapView(props: Props) {
  const container = useRef<HTMLDivElement>(null);
  const map = useRef<MlMap | null>(null);
  const [ready, setReady] = useState(false);
  const [mapError, setMapError] = useState<string | null>(null);
  const markers = useRef<maplibregl.Marker[]>([]);
  const poiMarkers = useRef(new Map<number, maplibregl.Marker>());
  const destEls = useRef(new Map<number, HTMLElement>());
  const popup = useRef<maplibregl.Popup | null>(null);
  const cb = useRef(props);
  cb.current = props;

  // Création de la carte (une seule fois)
  useEffect(() => {
    let m: MlMap;
    try {
      m = new maplibregl.Map({ container: container.current!, style: STYLE, center: [5.72, 45.19], zoom: 8 });
    } catch (e) {
      // Navigateur sans WebGL (ex. aperçu intégré de VS Code) : le reste de l'application reste utilisable.
      console.error(e);
      setMapError((e as Error).message);
      return;
    }
    map.current = m;
    m.addControl(new maplibregl.NavigationControl({ showCompass: false }), "top-right");
    m.on("load", () => {
      m.addSource("lines", { type: "geojson", data: EMPTY });
      m.addSource("stations", { type: "geojson", data: EMPTY });
      m.addSource("journey", { type: "geojson", data: EMPTY });
      m.addSource("walk", { type: "geojson", data: EMPTY });
      m.addLayer({
        id: "journey-casing",
        type: "line",
        source: "journey",
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": "#fafaf9", "line-width": 7 },
      });
      m.addLayer({
        id: "journey",
        type: "line",
        source: "journey",
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": "#0f766e", "line-width": 4 },
      });
      m.addLayer({
        id: "walk",
        type: "line",
        source: "walk",
        layout: { "line-cap": "round" },
        paint: { "line-color": "#1f2937", "line-width": 3, "line-dasharray": [1, 2] },
      });
      m.addLayer({
        id: "lines",
        type: "line",
        source: "lines",
        paint: { "line-color": "#64748b", "line-width": 1.5, "line-opacity": 0.35 },
      });
      m.addLayer({
        id: "stations",
        type: "circle",
        source: "stations",
        paint: {
          "circle-radius": ["interpolate", ["linear"], ["zoom"], 7, 4, 12, 8],
          "circle-color": [
            "case",
            ...TIME_STEPS.slice(0, -1).flatMap((s) => [["<=", ["get", "minutes"], s.max], s.color]),
            TIME_STEPS[TIME_STEPS.length - 1].color,
          ] as unknown as maplibregl.ExpressionSpecification,
          "circle-stroke-width": 1.5,
          "circle-stroke-color": "#fafaf9",
        },
      });
      m.on("click", "stations", (e) => {
        const p = e.features?.[0]?.properties as Record<string, unknown> | undefined;
        if (p) cb.current.onSelectStation(Number(p.id));
      });
      m.on("mousemove", "stations", (e) => {
        const p = e.features?.[0]?.properties as Record<string, unknown> | undefined;
        if (!p) return;
        m.getCanvas().style.cursor = "pointer";
        popup.current?.remove();
        popup.current = new maplibregl.Popup({ closeButton: false, offset: 10 })
          .setLngLat(e.lngLat)
          .setHTML(
            `<strong>${escapeHtml(String(p.name))}</strong><br/>${formatMinutes(Number(p.minutes))} de train` +
              `<br/><small>Cliquer pour voir ce qu'il y a autour</small>`,
          )
          .addTo(m);
      });
      m.on("mouseleave", "stations", () => {
        m.getCanvas().style.cursor = "";
        popup.current?.remove();
      });
      setReady(true);
    });
    return () => m.remove();
  }, []);

  // Contenu de la carte selon l'étape
  useEffect(() => {
    const m = map.current;
    if (!m || !ready) return;
    const { mode, stations, lines, results, detailStation, detailPois, originName } = props;

    markers.current.forEach((mk) => mk.remove());
    markers.current = [];
    poiMarkers.current.forEach((mk) => mk.remove());
    poiMarkers.current.clear();
    popup.current?.remove();

    const origin = stations?.features.find((f) => f.properties?.is_origin);
    const originLngLat = (origin?.geometry as GeoJSON.Point | undefined)?.coordinates as [number, number] | undefined;
    const addMarker = (div: HTMLElement, lngLat: [number, number], anchor: maplibregl.PositionAnchor = "center") =>
      markers.current.push(new maplibregl.Marker({ element: div, anchor }).setLngLat(lngLat).addTo(m));

    // gares : seulement en exploration, et seulement celles accessibles depuis le départ
    const reachable = (stations?.features ?? []).filter(
      (f) => f.properties?.minutes != null && !f.properties?.is_origin,
    );
    (m.getSource("stations") as GeoJSONSource).setData(
      mode === "overview" ? { type: "FeatureCollection", features: reachable } : EMPTY,
    );
    (m.getSource("lines") as GeoJSONSource).setData(mode === "overview" ? (lines ?? EMPTY) : EMPTY);

    // la gare de départ peut être elle-même une destination proposée (« sur place ») :
    // une seule étiquette, celle de la destination, qui le signale
    const originId = Number(origin?.properties?.id);
    const originInResults = mode === "results" && results.some((r) => r.station.id === originId);

    if (originLngLat && mode !== "detail" && !originInResults) {
      // la gare de départ est cliquable : ce qu'il y a à faire sur place, sans prendre le train
      const div = el("origin-marker clickable", `Départ · ${escapeHtml(originName ?? "")}`, "Voir ce qu'il y a autour");
      div.addEventListener("click", () => cb.current.onSelectStation(Number(origin?.properties?.id)));
      addMarker(div, originLngLat);
    }

    if (mode === "overview") {
      if (originLngLat) m.flyTo({ center: originLngLat, zoom: 8.5, duration: 700 });
    }

    destEls.current.clear();
    if (mode === "results") {
      results.forEach((r, i) => {
        const div = el(
          "dest-marker",
          `<span class="n">${i + 1}</span><span class="label">${escapeHtml(r.station.name)}${
            r.station.id === originId ? " · votre départ" : ""
          }</span>`,
          "Voir ce qu'il y a autour",
        );
        div.addEventListener("click", () => cb.current.onSelectStation(r.station.id));
        div.addEventListener("mouseenter", () => cb.current.onHoverStation(r.station.id));
        div.addEventListener("mouseleave", () => cb.current.onHoverStation(null));
        destEls.current.set(r.station.id, div);
        addMarker(div, [r.station.lon, r.station.lat], "left");
      });
      fit(
        m,
        [...(originLngLat ? [originLngLat] : []), ...results.map((r) => [r.station.lon, r.station.lat] as [number, number])],
        11,
      );
    }

    // trajet en train choisi : tracé gare par gare, correspondances signalées
    const j = mode === "detail" ? props.journey : null;
    (m.getSource("journey") as GeoJSONSource).setData(
      j
        ? {
            type: "FeatureCollection",
            features: j.legs.map((l) => ({
              type: "Feature",
              geometry: {
                type: "LineString",
                coordinates: l.path?.length > 1 ? l.path : l.stops.map((s) => [s.lon, s.lat]),
              },
              properties: {},
            })),
          }
        : EMPTY,
    );
    // Une seule étiquette par gare : la gare de la fiche porte l'heure de départ (retour) ou
    // d'arrivée (aller) ; l'autre extrémité du trajet reçoit sa propre étiquette.
    let stationTime = "";
    if (j && detailStation) {
      const first = j.legs[0].from;
      const last = j.legs[j.legs.length - 1].to;
      if (first.station_id === detailStation.id) stationTime = ` · départ ${j.departure}`;
      else addMarker(el("origin-marker", `Départ ${escapeHtml(j.departure)} · ${escapeHtml(first.name)}`), [first.lon, first.lat]);
      if (last.station_id === detailStation.id) stationTime = ` · arrivée ${j.arrival}`;
      else addMarker(el("origin-marker", `Arrivée ${escapeHtml(j.arrival)} · ${escapeHtml(last.name)}`), [last.lon, last.lat]);
      j.legs.slice(1).forEach((l) =>
        addMarker(
          el("transfer-marker", `Correspondance · ${escapeHtml(l.from.name)} (${l.wait_before_min} min)`),
          [l.from.lon, l.from.lat],
        ),
      );
    }

    if (mode === "detail" && detailStation) {
      // étiquette au-dessus de la gare (ancre en bas) pour ne pas masquer les lieux voisins
      addMarker(
        el("station-marker", `${escapeHtml(detailStation.name)}${escapeHtml(stationTime)}`),
        [detailStation.lon, detailStation.lat],
        "bottom",
      );
      detailPois.forEach((p, i) => {
        const cat = categoryOf(p.tags);
        const div = el("poi-marker", String(i + 1), p.name);
        div.style.background = cat.color;
        div.addEventListener("click", (ev) => {
          ev.stopPropagation();
          cb.current.onSelectPoi(p.id);
        });
        poiMarkers.current.set(
          p.id,
          new maplibregl.Marker({ element: div }).setLngLat([p.lon, p.lat]).addTo(m),
        );
      });
      const journeyPts = j
        ? j.legs.flatMap((l) => (l.path?.length > 1 ? l.path : l.stops.map((s) => [s.lon, s.lat] as [number, number])))
        : [];
      fit(
        m,
        [[detailStation.lon, detailStation.lat], ...detailPois.map((p) => [p.lon, p.lat] as [number, number]), ...journeyPts],
        15,
      );
    }
  }, [ready, props.mode, props.stations, props.lines, props.results, props.detailStation, props.detailPois, props.journey]); // eslint-disable-line react-hooks/exhaustive-deps

  // Lieu sélectionné (depuis la liste ou la carte) : on le montre et on affiche sa fiche
  useEffect(() => {
    const m = map.current;
    if (!m || !ready) return;
    poiMarkers.current.forEach((mk, id) => mk.getElement().classList.toggle("active", id === props.focusedPoiId));
    const p = props.detailPois.find((x) => x.id === props.focusedPoiId);
    const st = props.detailStation;
    // chemin à pied gare -> lieu (ligne droite indicative ; le temps affiché inclut un détour de 30 %)
    (m.getSource("walk") as GeoJSONSource).setData(
      p && st
        ? {
            type: "Feature",
            geometry: { type: "LineString", coordinates: [[st.lon, st.lat], [p.lon, p.lat]] },
            properties: {},
          }
        : EMPTY,
    );
    if (!p) return;
    popup.current?.remove();
    popup.current = new maplibregl.Popup({ offset: 16, maxWidth: "260px" })
      .setLngLat([p.lon, p.lat])
      .setHTML(poiPopupHtml(p))
      .addTo(m);
    m.easeTo({ center: [p.lon, p.lat], duration: 500 });
  }, [ready, props.focusedPoiId]); // eslint-disable-line react-hooks/exhaustive-deps

  // liste <-> carte : la destination survolée ressort, au-dessus des autres étiquettes
  useEffect(() => {
    destEls.current.forEach((div, id) => {
      const hot = id === props.highlightId;
      div.classList.toggle("hot", hot);
      const wrapper = div.closest(".maplibregl-marker") as HTMLElement | null;
      if (wrapper) wrapper.style.zIndex = hot ? "5" : "";
    });
  }, [props.highlightId, props.results, props.mode]);

  // Le conteneur est masqué sur mobile quand l'autre onglet est actif : on recalcule la taille.
  useEffect(() => {
    if (props.visible) setTimeout(() => map.current?.resize(), 50);
  }, [props.visible]);

  const presentCats = CATEGORIES.filter((c) => props.detailPois.some((p) => categoryOf(p.tags).key === c.key));

  return (
    <>
      <div ref={container} className="map" />
      {mapError ? (
        <div className="map-error">
          <strong>Carte indisponible dans ce navigateur</strong>
          <p>
            La carte nécessite WebGL. Ouvrez l'application dans Chrome, Edge ou Firefox (http://localhost:5173).
            La recherche reste utilisable.
          </p>
        </div>
      ) : (
        <div className="legend">
          {props.mode === "overview" && (
            <>
              <strong>Gares accessibles depuis {props.originName}</strong>
              {TIME_STEPS.map((s) => (
                <span key={s.label}>
                  <i style={{ background: s.color }} />
                  {s.label === "plus" ? "plus de 2h" : `≤ ${s.label}`}
                </span>
              ))}
              <small>Survolez une gare pour son temps de trajet, cliquez pour voir ce qu'il y a autour.</small>
            </>
          )}
          {props.mode === "results" && (
            <>
              <strong>Destinations proposées</strong>
              <small>Les numéros correspondent à la liste. Cliquez une destination pour voir ses lieux.</small>
            </>
          )}
          {props.mode === "detail" && (
            <>
              <strong>Lieux autour de {props.detailStation?.name}</strong>
              {presentCats.map((c) => (
                <span key={c.key}>
                  <i style={{ background: c.color }} />
                  {c.label}
                </span>
              ))}
              <small>Les numéros correspondent à la liste.</small>
            </>
          )}
        </div>
      )}
    </>
  );
}
