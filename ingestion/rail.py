"""Tracé réel des voies entre deux gares desservies successivement.

Le GTFS SNCF ne contient pas la forme des lignes (pas de shapes.txt) : sans cette étape, la carte relie
les gares en ligne droite. On construit ici un graphe du réseau ferré à partir de sa géométrie
(SNCF Open Data « Formes des lignes du RFN », ou OpenStreetMap en secours), on rattache chaque gare
au point du réseau le plus proche, puis on calcule le plus court chemin le long des voies (A*)
pour chaque paire de gares consécutives présente dans les horaires.

Résultat : table `rail_segments` (utilisée par l'API pour tracer les trajets) et table `lines`
reconstruite à partir de ces tronçons (vue d'ensemble de la carte).
"""

from __future__ import annotations

import heapq
import json
import math
import time
from collections import defaultdict
from pathlib import Path

import requests

from common import AURA_BBOX, connect, resolve_source

SNCF_RFN_URL = (
    "https://ressources.data.sncf.com/api/explore/v2.1/catalog/datasets/formes-des-lignes-du-rfn/exports/geojson"
)
OVERPASS_URL = "https://overpass-api.de/api/interpreter"
HEADERS = {"User-Agent": "Aiguillage/0.1 (projet etudiant tourisme ferroviaire AURA)"}

SNAP_MAX_M = 1500  # distance max gare -> voie
JOIN_MAX_M = 40  # raccord entre deux tronçons de voie qui ne partagent pas exactement un sommet
DETOUR_MAX = 3.0  # au-delà de 3x la distance à vol d'oiseau, on garde la ligne droite (chemin aberrant)
GRID = 0.01  # taille de cellule de l'index spatial (degrés)

Node = tuple[float, float]


def haversine(a: Node, b: Node) -> float:
    lon1, lat1, lon2, lat2 = map(math.radians, (a[0], a[1], b[0], b[1]))
    h = math.sin((lat2 - lat1) / 2) ** 2 + math.cos(lat1) * math.cos(lat2) * math.sin((lon2 - lon1) / 2) ** 2
    return 2 * 6_371_000 * math.asin(math.sqrt(h))


def in_zone(p: Node, margin: float = 0.3) -> bool:
    lon_min, lat_min, lon_max, lat_max = AURA_BBOX
    return lon_min - margin <= p[0] <= lon_max + margin and lat_min - margin <= p[1] <= lat_max + margin


# ---------- Chargement de la géométrie ----------

def _lines_from_geojson(data: dict) -> list[list[Node]]:
    out = []
    for f in data.get("features", []):
        g = f.get("geometry") or {}
        parts = [g["coordinates"]] if g.get("type") == "LineString" else g.get("coordinates", []) if g.get("type") == "MultiLineString" else []
        for part in parts:
            pts = [(round(c[0], 6), round(c[1], 6)) for c in part]
            if len(pts) >= 2 and any(in_zone(p) for p in pts):
                out.append(pts)
    return out


def load_sncf(source: str | None) -> list[list[Node]]:
    path = resolve_source(source or SNCF_RFN_URL, "rfn.geojson")
    return _lines_from_geojson(json.loads(Path(path).read_text(encoding="utf-8")))


def load_osm() -> list[list[Node]]:
    """Secours : voies OSM (railway=rail) par tuiles de 1° sur la région."""
    lon_min, lat_min, lon_max, lat_max = AURA_BBOX
    lines: list[list[Node]] = []
    lat = lat_min
    while lat < lat_max:
        lon = lon_min
        while lon < lon_max:
            q = (f'[out:json][timeout:180];way["railway"="rail"]["service"!~"."]'
                 f"({lat},{lon},{min(lat + 1, lat_max)},{min(lon + 1, lon_max)});out geom;")
            for attempt in range(3):
                r = requests.post(OVERPASS_URL, data={"data": q}, headers=HEADERS, timeout=240)
                if r.ok:
                    for w in r.json().get("elements", []):
                        pts = [(round(p["lon"], 6), round(p["lat"], 6)) for p in w.get("geometry", [])]
                        if len(pts) >= 2:
                            lines.append(pts)
                    break
                time.sleep(30 * (attempt + 1))
            print(f"  tuile {lat:.0f}/{lon:.0f} : {len(lines)} voies cumulées", flush=True)
            lon += 1
        lat += 1
    return lines


# ---------- Graphe ----------

class Graph:
    def __init__(self, lines: list[list[Node]]):
        self.adj: dict[Node, list[tuple[Node, float]]] = defaultdict(list)
        self.grid: dict[tuple[int, int], list[Node]] = defaultdict(list)
        for pts in lines:
            for a, b in zip(pts, pts[1:]):
                if a != b:
                    self._edge(a, b, haversine(a, b))
        for n in self.adj:
            self.grid[self._cell(n)].append(n)
        # raccorde les extrémités de tronçons proches (jeux de données sans sommet commun aux jonctions)
        joined = 0
        for pts in lines:
            for end in (pts[0], pts[-1]):
                for other, d in self.near(end, JOIN_MAX_M):
                    if other != end and other not in {x for x, _ in self.adj[end]}:
                        self._edge(end, other, d)
                        joined += 1
        print(f"  graphe ferroviaire : {len(self.adj)} sommets, {joined} raccords")

    def _edge(self, a: Node, b: Node, d: float) -> None:
        self.adj[a].append((b, d))
        self.adj[b].append((a, d))

    @staticmethod
    def _cell(p: Node) -> tuple[int, int]:
        return int(p[0] // GRID), int(p[1] // GRID)

    def near(self, p: Node, max_m: float) -> list[tuple[Node, float]]:
        cx, cy = self._cell(p)
        r = max(1, math.ceil(max_m / 111_000 / GRID) + 1)
        found = []
        for dx in range(-r, r + 1):
            for dy in range(-r, r + 1):
                for n in self.grid.get((cx + dx, cy + dy), ()):
                    d = haversine(p, n)
                    if d <= max_m:
                        found.append((n, d))
        return sorted(found, key=lambda x: x[1])

    def path(self, a: Node, b: Node, max_m: float) -> list[Node] | None:
        """A* : plus court chemin le long des voies, abandonné au-delà de `max_m`."""
        dist = {a: 0.0}
        prev: dict[Node, Node] = {}
        heap = [(haversine(a, b), 0.0, a)]
        while heap:
            _, d, n = heapq.heappop(heap)
            if n == b:
                out = [b]
                while out[-1] != a:
                    out.append(prev[out[-1]])
                return out[::-1]
            if d > dist.get(n, math.inf) or d > max_m:
                continue
            for m, w in self.adj[n]:
                nd = d + w
                if nd < dist.get(m, math.inf):
                    dist[m] = nd
                    prev[m] = n
                    heapq.heappush(heap, (nd + haversine(m, b), nd, m))
        return None


def simplify(pts: list[Node], tol_m: float = 15) -> list[Node]:
    """Allège le tracé (on garde un point s'il s'écarte de plus de `tol_m` du précédent conservé)."""
    if len(pts) <= 2:
        return pts
    out = [pts[0]]
    for p in pts[1:-1]:
        if haversine(out[-1], p) >= tol_m:
            out.append(p)
    out.append(pts[-1])
    return out


# ---------- Étape d'ingestion ----------

def run(source: str | None = None, use_osm: bool = False) -> None:
    print("Tracé des voies : chargement de la géométrie du réseau ferré")
    lines = load_osm() if use_osm else load_sncf(source)
    print(f"  {len(lines)} tronçons de voie dans la zone")
    graph = Graph(lines)

    with connect() as conn, conn.cursor() as cur:
        cur.execute("SELECT to_regclass('connections') IS NOT NULL")
        if not cur.fetchone()[0]:
            raise SystemExit("Horaires absents : lancez d'abord l'étape gtfs.")
        cur.execute(
            """CREATE TABLE IF NOT EXISTS rail_segments (
                   from_station BIGINT NOT NULL REFERENCES stations(id) ON DELETE CASCADE,
                   to_station BIGINT NOT NULL REFERENCES stations(id) ON DELETE CASCADE,
                   geom GEOMETRY(LineString, 4326) NOT NULL,
                   PRIMARY KEY (from_station, to_station));
               TRUNCATE rail_segments;"""
        )
        cur.execute("SELECT id, lon, lat FROM stations")
        stations = {sid: (lon, lat) for sid, lon, lat in cur.fetchall()}
        # paires non orientées : le tracé A->B sert aussi pour B->A
        cur.execute(
            "SELECT DISTINCT LEAST(from_station, to_station), GREATEST(from_station, to_station) FROM connections"
        )
        pairs = cur.fetchall()

        snapped: dict[int, Node] = {}
        for sid, p in stations.items():
            near = graph.near(p, SNAP_MAX_M)
            if near:
                snapped[sid] = near[0][0]

        ok, straight = 0, 0
        for i, (a, b) in enumerate(pairs, 1):
            pa, pb = stations[a], stations[b]
            crow = haversine(pa, pb)
            path = None
            if a in snapped and b in snapped:
                path = graph.path(snapped[a], snapped[b], max_m=max(crow * DETOUR_MAX, 2000))
            if path:
                pts = [pa, *simplify(path), pb]
                pts = [q for i, q in enumerate(pts) if i == 0 or q != pts[i - 1]]
                ok += 1
            else:
                pts = [pa, pb]
                straight += 1
            wkt = "LINESTRING(" + ", ".join(f"{x} {y}" for x, y in pts) + ")"
            cur.execute(
                "INSERT INTO rail_segments VALUES (%s, %s, ST_GeomFromText(%s, 4326)) ON CONFLICT DO NOTHING",
                (a, b, wkt),
            )
            if i % 200 == 0:
                print(f"  {i}/{len(pairs)} tronçons calculés", flush=True)

        # vue d'ensemble : les lignes affichées suivent désormais les voies
        cur.execute("DELETE FROM lines")
        cur.execute(
            """INSERT INTO lines (route_id, name, color, geom)
               SELECT 'seg:' || from_station || '-' || to_station, '', NULL, geom FROM rail_segments"""
        )
        print(f"  {ok} tronçons suivent les voies, {straight} restent en ligne droite (gare ou voie introuvable)")
