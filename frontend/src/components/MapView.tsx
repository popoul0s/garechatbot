import { useEffect, useRef, useState } from "react";
import * as maplibregl from "maplibre-gl";
import type { GeoJSONSource, LngLatBoundsLike, Map as MlMap } from "maplibre-gl";
import { formatMinutes, PoiNearStation, Recommendation, TAG_LABELS } from "../api";

interface Props {
  stations: GeoJSON.FeatureCollection | null;
  lines: GeoJSON.FeatureCollection | null;
  results: Recommendation[];
  focusedStationId: number | null;
  selectedStationId: number | null;
  stationPois: PoiNearStation[];
  onSelectStation: (id: number) => void;
  visible: boolean;
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
  layers: [{ id: "osm", type: "raster", source: "osm", paint: { "raster-saturation": -0.4 } }],
};

const TAG_COLORS: Record<string, string> = {
  randonnee: "#15803d",
  nature: "#22c55e",
  montagne: "#78716c",
  eau: "#0ea5e9",
  musee: "#7c3aed",
  culture: "#a855f7",
  patrimoine: "#b45309",
  famille: "#ec4899",
  loisirs: "#f97316",
  panorama: "#0d9488",
};

function poiFeatures(pois: PoiNearStation[], result: boolean): GeoJSON.Feature[] {
  return pois.map((p) => ({
    type: "Feature",
    geometry: { type: "Point", coordinates: [p.lon, p.lat] },
    properties: {
      id: p.id,
      name: p.name,
      tags: p.tags.join(","),
      color: TAG_COLORS[p.tags[0]] ?? "#64748b",
      description: p.description ?? "",
      url: p.url ?? "",
      walk: p.walk_minutes,
      source: p.source,
      result,
    },
  }));
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

export default function MapView(props: Props) {
  const container = useRef<HTMLDivElement>(null);
  const map = useRef<MlMap | null>(null);
  const ready = useRef(false);
  const pendingApply = useRef<(() => void) | null>(null);
  const markers = useRef<maplibregl.Marker[]>([]);
  const onSelect = useRef(props.onSelectStation);
  onSelect.current = props.onSelectStation;

  // Création de la carte et des couches
  const [mapError, setMapError] = useState<string | null>(null);

  useEffect(() => {
    let m: MlMap;
    try {
      m = new maplibregl.Map({
        container: container.current!,
        style: STYLE,
        center: [5.72, 45.19],
        zoom: 8,
      });
    } catch (e) {
      // Navigateur sans WebGL (ex. aperçu intégré de VS Code) : on garde le reste de l'application utilisable.
      console.error(e);
      setMapError((e as Error).message);
      return;
    }
    map.current = m;
    m.addControl(new maplibregl.NavigationControl(), "top-right");

    m.on("load", () => {
      m.addSource("lines", { type: "geojson", data: EMPTY });
      m.addSource("stations", { type: "geojson", data: EMPTY });
      m.addSource("pois", { type: "geojson", data: EMPTY });

      m.addLayer({
        id: "lines",
        type: "line",
        source: "lines",
        paint: { "line-color": ["coalesce", ["get", "color"], "#475569"], "line-width": 2, "line-opacity": 0.6 },
      });
      m.addLayer({
        id: "stations",
        type: "circle",
        source: "stations",
        paint: {
          "circle-radius": ["case", ["get", "is_origin"], 9, ["get", "selected"], 9, 5],
          "circle-color": [
            "case",
            ["==", ["get", "minutes"], null], "#94a3b8",
            ["<=", ["get", "minutes"], 30], "#16a34a",
            ["<=", ["get", "minutes"], 60], "#84cc16",
            ["<=", ["get", "minutes"], 90], "#f59e0b",
            ["<=", ["get", "minutes"], 120], "#f97316",
            "#dc2626",
          ],
          "circle-stroke-width": ["case", ["get", "selected"], 4, ["get", "is_origin"], 3, 1],
          "circle-stroke-color": ["case", ["get", "selected"], "#1d4ed8", "#ffffff"],
        },
      });
      m.addLayer({
        id: "pois",
        type: "circle",
        source: "pois",
        paint: {
          "circle-radius": ["case", ["get", "result"], 7, 5],
          "circle-color": ["get", "color"],
          "circle-stroke-width": 1.5,
          "circle-stroke-color": "#ffffff",
        },
      });

      m.on("click", "stations", (e) => {
        const f = e.features?.[0];
        if (!f) return;
        const p = f.properties as Record<string, unknown>;
        const minutes = p.minutes === null || p.minutes === undefined || p.minutes === "null" ? null : Number(p.minutes);
        const changes = Number(p.nb_changes ?? 0);
        new maplibregl.Popup({ offset: 8 })
          .setLngLat(e.lngLat)
          .setHTML(
            `<strong>${escapeHtml(String(p.name))}</strong><br/>` +
              (minutes !== null
                ? `${formatMinutes(minutes)} depuis l'origine${changes > 0 ? ` (${changes} corresp.)` : " (direct)"}<br/>`
                : "Temps de trajet inconnu<br/>") +
              `${p.poi_count} points d'intérêt à proximité` +
              (p.pmr === true ? "<br/>♿ Accessible PMR" : ""),
          )
          .addTo(m);
        onSelect.current(Number(p.id));
      });
      m.on("click", "pois", (e) => {
        const p = e.features?.[0]?.properties as Record<string, string> | undefined;
        if (!p) return;
        const tags = p.tags
          .split(",")
          .filter(Boolean)
          .map((t) => TAG_LABELS[t] ?? t)
          .join(" · ");
        new maplibregl.Popup({ offset: 8 })
          .setLngLat(e.lngLat)
          .setHTML(
            `<strong>${escapeHtml(p.name)}</strong><br/><em>${escapeHtml(tags)}</em><br/>` +
              `🚶 ${p.walk} min depuis la gare<br/>` +
              (p.description ? `<p>${escapeHtml(p.description.slice(0, 220))}</p>` : "") +
              (p.url ? `<a href="${escapeHtml(p.url)}" target="_blank" rel="noreferrer">Site web</a><br/>` : "") +
              `<small>Source : ${escapeHtml(p.source)}</small>`,
          )
          .addTo(m);
      });
      for (const layer of ["stations", "pois"]) {
        m.on("mouseenter", layer, () => (m.getCanvas().style.cursor = "pointer"));
        m.on("mouseleave", layer, () => (m.getCanvas().style.cursor = ""));
      }
      ready.current = true;
      pendingApply.current?.();
    });
    return () => m.remove();
  }, []);

  // Mise à jour des données (attend que la carte soit chargée)
  useEffect(() => {
    const m = map.current;
    if (!m) return;
    const apply = () => {
      const stations = props.stations ?? EMPTY;
      (m.getSource("stations") as GeoJSONSource).setData({
        ...stations,
        features: stations.features.map((f) => ({
          ...f,
          properties: { ...f.properties, selected: f.properties?.id === props.selectedStationId },
        })),
      });
      (m.getSource("lines") as GeoJSONSource).setData(props.lines ?? EMPTY);
      const resultPois = props.results.flatMap((r) => poiFeatures(r.pois, true));
      const resultIds = new Set(resultPois.map((f) => f.properties!.id));
      const explorerPois = poiFeatures(props.stationPois, false).filter((f) => !resultIds.has(f.properties!.id));
      (m.getSource("pois") as GeoJSONSource).setData({
        type: "FeatureCollection",
        features: [...explorerPois, ...resultPois],
      });
    };
    if (ready.current) apply();
    else pendingApply.current = apply;
  }, [props.stations, props.lines, props.results, props.stationPois, props.selectedStationId]);

  // Marqueurs numérotés des recommandations + cadrage
  useEffect(() => {
    const m = map.current;
    if (!m) return;
    markers.current.forEach((mk) => mk.remove());
    markers.current = props.results.map((r, i) => {
      const el = document.createElement("div");
      el.className = "result-marker";
      el.textContent = String(i + 1);
      el.title = r.station.name;
      el.addEventListener("click", () => onSelect.current(r.station.id));
      return new maplibregl.Marker({ element: el }).setLngLat([r.station.lon, r.station.lat]).addTo(m);
    });
    if (props.results.length > 0) {
      const pts = props.results.flatMap((r) => [[r.station.lon, r.station.lat], ...r.pois.map((p) => [p.lon, p.lat])]);
      const lons = pts.map((p) => p[0]);
      const lats = pts.map((p) => p[1]);
      const bounds: LngLatBoundsLike = [
        [Math.min(...lons), Math.min(...lats)],
        [Math.max(...lons), Math.max(...lats)],
      ];
      m.fitBounds(bounds, { padding: 60, maxZoom: 13, duration: 800 });
    }
  }, [props.results]);

  // Centrage sur une recommandation ou une gare choisie
  useEffect(() => {
    const m = map.current;
    if (!m || props.focusedStationId === null) return;
    const r = props.results.find((x) => x.station.id === props.focusedStationId);
    const f = props.stations?.features.find((x) => x.properties?.id === props.focusedStationId);
    const coords = r
      ? [r.station.lon, r.station.lat]
      : ((f?.geometry as GeoJSON.Point | undefined)?.coordinates ?? null);
    if (coords) m.flyTo({ center: coords as [number, number], zoom: 13, duration: 800 });
  }, [props.focusedStationId]); // eslint-disable-line react-hooks/exhaustive-deps

  // Le conteneur est masqué sur mobile quand un autre onglet est actif : on recalcule la taille.
  useEffect(() => {
    if (props.visible) setTimeout(() => map.current?.resize(), 50);
  }, [props.visible]);

  return (
    <>
      <div ref={container} className="map" />
      {mapError && (
        <div className="map-error">
          <strong>Carte indisponible dans ce navigateur</strong>
          <p>
            La carte nécessite WebGL. Ouvrez l'application dans Chrome, Edge ou Firefox (http://localhost:5173).
            L'assistant reste utilisable.
          </p>
        </div>
      )}
    </>
  );
}
