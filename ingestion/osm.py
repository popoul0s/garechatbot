"""OpenStreetMap (API Overpass) -> points d'intérêt autour des gares.

Les tags OSM sont traduits vers le vocabulaire de l'application (voir `map_tags`).
"""

from __future__ import annotations

import math
import time

import requests

from common import connect, require_stations

# Serveurs Overpass publics, essayés dans l'ordre en cas d'erreur
OVERPASS_URLS = [
    "https://overpass-api.de/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
]
# Les serveurs publics refusent (406/429) les requêtes anonymes : on s'identifie.
HEADERS = {
    "User-Agent": "Aiguillage/0.1 (projet etudiant tourisme ferroviaire AURA)",
    "Accept": "application/json",
}
RADIUS_M = 3000
BATCH = 5
# Une boîte englobante par gare (±3 km) : bien plus rapide pour Overpass qu'un filtre "around".
# Les POI hors du rayon réel sont écartés ensuite par link.py (ST_DWithin 3000 m).
DLAT = RADIUS_M / 111_000
DLON = RADIUS_M / 78_000  # ~1° de longitude à 45° de latitude

FILTERS = [
    '["tourism"~"^(attraction|museum|viewpoint|zoo|theme_park|picnic_site|gallery)$"]',
    '["historic"~"^(castle|monument|ruins|archaeological_site|fort|abbey|city_gate|manor)$"]',
    '["leisure"~"^(park|nature_reserve|playground|water_park|swimming_area|garden)$"]',
    '["natural"~"^(peak|waterfall|cave_entrance|beach|gorge)$"]',
    '["leisure"="fishing"]',
]
# Les lacs sont interrogés à part, avec leur contour : le centre d'un grand lac (lac du Bourget,
# lac d'Annecy) est souvent à plus de 3 km de la gare alors que la rive est à 10 min à pied.
LAKE_FILTER = '["water"="lake"]["name"]'

# Les relations route=hiking ne sont pas interrogées : calculer leur centre est très coûteux pour
# Overpass. Les itinéraires de randonnée viennent de DATAtourisme ; OSM apporte sommets, cascades, lacs...


def map_tags(t: dict[str, str]) -> list[str]:
    """Tags OSM -> thèmes de l'application."""
    out: set[str] = set()
    tourism, historic, leisure, natural = t.get("tourism"), t.get("historic"), t.get("leisure"), t.get("natural")
    if tourism == "museum":
        out |= {"musee", "culture"}
    if tourism == "gallery":
        out |= {"culture"}
    if tourism == "attraction":
        out |= {"loisirs"}
    if tourism == "viewpoint":
        out |= {"panorama", "nature"}
    if tourism in ("zoo", "theme_park"):
        out |= {"loisirs", "famille"}
    if tourism == "picnic_site":
        out |= {"nature", "famille"}
    if historic:
        out |= {"patrimoine", "culture"}
    if leisure in ("park", "garden"):
        out |= {"nature", "famille"}
    if leisure == "nature_reserve":
        out |= {"nature"}
    if leisure == "playground":
        out |= {"famille", "loisirs"}
    if leisure in ("water_park", "swimming_area"):
        out |= {"eau", "loisirs", "famille"}
    if natural == "peak":
        out |= {"montagne", "panorama", "randonnee", "nature"}
    if natural in ("waterfall", "gorge", "cave_entrance"):
        out |= {"nature", "eau"} if natural != "cave_entrance" else {"nature"}
    if leisure == "fishing":
        out |= {"eau", "nature"}
    if natural == "beach" or t.get("water") == "lake":
        out |= {"eau", "nature"}
    if t.get("route") == "hiking":
        out |= {"randonnee", "nature"}
    try:
        if float(t.get("ele", "0").replace(",", ".")) >= 1000:
            out.add("montagne")
    except ValueError:
        pass
    return sorted(out)


def default_name(t: dict[str, str]) -> str | None:
    if t.get("name"):
        return t["name"]
    if t.get("leisure") == "playground":
        return "Aire de jeux"
    if t.get("tourism") == "picnic_site":
        return "Aire de pique-nique"
    if t.get("tourism") == "viewpoint":
        return "Point de vue"
    if t.get("leisure") == "fishing":
        return "Coin de pêche"
    return None


def description(t: dict[str, str]) -> str | None:
    parts = [t.get("description:fr") or t.get("description")]
    if t.get("leisure") == "fishing":
        parts.append("Lieu de pêche")
    if t.get("ele"):
        parts.append(f"Altitude {t['ele']} m")
    if t.get("route") == "hiking" and t.get("distance"):
        parts.append(f"Itinéraire de {t['distance']} km")
    parts = [p for p in parts if p]
    return ". ".join(parts) or None


def bbox(lon: float, lat: float) -> str:
    return f"({lat - DLAT:.5f},{lon - DLON:.5f},{lat + DLAT:.5f},{lon + DLON:.5f})"


def overpass(q: str) -> list[dict]:
    errors = []
    for attempt in range(len(OVERPASS_URLS) * 3):
        url = OVERPASS_URLS[attempt % len(OVERPASS_URLS)]
        try:
            r = requests.post(url, data={"data": q}, headers=HEADERS, timeout=120)
        except requests.RequestException as e:
            errors.append(f"{url}: {type(e).__name__}")
            continue
        if r.ok:
            return r.json().get("elements", [])
        errors.append(f"{url}: HTTP {r.status_code}")
        if r.status_code == 429:
            # quota dépassé : on laisse le serveur souffler avant de réessayer
            print("    serveur saturé (429), pause de 60 s…", flush=True)
            time.sleep(60)
        else:
            time.sleep(10)
    raise RuntimeError("Overpass indisponible :\n  " + "\n  ".join(errors[-3:]))


def query(stations: list[tuple[float, float]], lakes_only: bool = False) -> list[dict]:
    """POI autour des gares : centre des objets, plus le contour des lacs (limité à la zone de chaque gare)."""
    parts = []
    if not lakes_only:
        body = "".join(f"nwr{f}{bbox(lon, lat)};" for lon, lat in stations for f in FILTERS)
        parts.append(f"({body});out center tags;")
    parts += [f"nwr{LAKE_FILTER}{bbox(lon, lat)};out geom{bbox(lon, lat)};" for lon, lat in stations]
    return overpass(f"[out:json][timeout:90];{''.join(parts)}")


def outline(e: dict) -> list[tuple[float, float]]:
    """Points (lon, lat) du contour d'un chemin ou d'une relation renvoyés par `out geom`."""
    geoms = [e.get("geometry")] + [m.get("geometry") for m in e.get("members", [])]
    return [(pt["lon"], pt["lat"]) for g in geoms if g for pt in g if pt]


def meters(a: tuple[float, float], b: tuple[float, float]) -> float:
    dx = (a[0] - b[0]) * 111_000 * math.cos(math.radians(a[1]))
    dy = (a[1] - b[1]) * 111_000
    return math.hypot(dx, dy)


def lake_shores(elements: list[dict], stations: list[tuple[int, float, float]]) -> list[tuple[str, dict, float, float]]:
    """Pour chaque lac et chaque gare proche : le point de rive le plus proche de la gare.

    Le lac est ainsi rattaché aux gares qui le bordent, avec un temps de marche réaliste."""
    lakes: dict[str, dict] = {}
    for e in elements:
        if e.get("tags", {}).get("water") != "lake" or "center" in e or "lat" in e:
            continue
        key = f"{e['type']}/{e['id']}"
        lake = lakes.setdefault(key, {"tags": e.get("tags", {}), "points": []})
        lake["points"] += outline(e)
    out = []
    for key, lake in lakes.items():
        if not lake["points"]:
            continue
        for sid, lon, lat in stations:
            shore = min(lake["points"], key=lambda p: meters(p, (lon, lat)))
            if meters(shore, (lon, lat)) <= RADIUS_M:
                out.append((f"{key}@{sid}", lake["tags"], shore[0], shore[1]))
    return out


def osm_ele(tags: dict) -> float | None:
    """Altitude donnée par OSM (sommets, cols) : « 742 », « 742 m »…"""
    try:
        return float(tags.get("ele", "").replace(",", ".").replace("m", "").strip())
    except ValueError:
        return None


def save(cur, source_id: str, tags: dict, lon: float, lat: float) -> bool:
    name = default_name(tags)
    app_tags = map_tags(tags)
    if not name or not app_tags:
        return False
    raw = [f"{k}={tags[k]}" for k in ("tourism", "historic", "leisure", "natural", "water", "route") if k in tags]
    cur.execute(
        """INSERT INTO pois (source, source_id, name, description, tags, raw_types, url, lon, lat, geom, ele)
           VALUES ('osm', %s, %s, %s, %s, %s, %s, %s, %s, ST_SetSRID(ST_MakePoint(%s, %s), 4326)::geography, %s)
           ON CONFLICT (source, source_id) DO UPDATE SET name = EXCLUDED.name, description = EXCLUDED.description,
               tags = EXCLUDED.tags, raw_types = EXCLUDED.raw_types, lon = EXCLUDED.lon, lat = EXCLUDED.lat,
               geom = EXCLUDED.geom, ele = COALESCE(EXCLUDED.ele, pois.ele)""",
        (source_id, name, description(tags), app_tags, raw, tags.get("website"), lon, lat, lon, lat, osm_ele(tags)),
    )
    return True


def save_elements(cur, elements: list[dict], stations: list[tuple[int, float, float]]) -> int:
    n = 0
    for e in elements:
        lon = e.get("lon") or e.get("center", {}).get("lon")
        lat = e.get("lat") or e.get("center", {}).get("lat")
        if lon is not None and lat is not None:
            n += save(cur, f"{e['type']}/{e['id']}", e.get("tags", {}), lon, lat)
    for source_id, tags, lon, lat in lake_shores(elements, stations):
        n += save(cur, source_id, tags, lon, lat)
    return n


def drop_lake_centers(cur) -> None:
    """Anciens imports : lacs placés à leur centre, remplacés par des points de rive."""
    cur.execute("DELETE FROM pois WHERE source = 'osm' AND 'water=lake' = ANY(raw_types) AND source_id NOT LIKE '%@%'")


def run_lakes(limit_stations: int | None = None) -> None:
    """Réimporte uniquement les lacs (contours), sans refaire tout l'import OSM."""
    print("OSM : lacs autour des gares (contours)")
    with connect() as conn, conn.cursor() as cur:
        require_stations(cur)
        cur.execute("ALTER TABLE pois ADD COLUMN IF NOT EXISTS ele REAL")
        cur.execute("CREATE TABLE IF NOT EXISTS osm_done (station_id BIGINT PRIMARY KEY REFERENCES stations(id) ON DELETE CASCADE)")
        drop_lake_centers(cur)
        stations = ordered_stations(cur)[:limit_stations]
        total = 0
        for i in range(0, len(stations), BATCH * 2):
            batch = [(sid, lon, lat) for sid, lon, lat, _ in stations[i : i + BATCH * 2]]
            print(f"  gares {i + 1}-{i + len(batch)}/{len(stations)}…", flush=True)
            try:
                elements = query([(lon, lat) for _, lon, lat in batch], lakes_only=True)
            except RuntimeError as e:
                print(f"    lot ignoré, {e}", flush=True)
                continue
            total += save_elements(cur, elements, batch)
            conn.commit()
            time.sleep(2)
        print(f"  {total} points de rive enregistrés")


def ordered_stations(cur) -> list[tuple[int, float, float, bool]]:
    """Gares les plus proches des origines d'abord : --osm-limit garde les plus utiles."""
    cur.execute(
        """SELECT s.id, s.lon, s.lat, d.station_id IS NOT NULL FROM stations s
           LEFT JOIN (SELECT station_id, min(minutes) AS m FROM travel_times GROUP BY station_id
                      UNION ALL SELECT DISTINCT origin_id, 0 FROM travel_times) t
             ON t.station_id = s.id
           LEFT JOIN osm_done d ON d.station_id = s.id
           ORDER BY t.m NULLS LAST, s.id"""
    )
    return cur.fetchall()


def run(limit_stations: int | None = None, restart: bool = False) -> None:
    print("OSM : récupération des POI autour des gares (Overpass)")
    with connect() as conn, conn.cursor() as cur:
        require_stations(cur)
        cur.execute("ALTER TABLE pois ADD COLUMN IF NOT EXISTS ele REAL")
        # Suivi des gares déjà traitées : une relance reprend là où l'import s'est arrêté.
        cur.execute("SELECT to_regclass('osm_done') IS NOT NULL")
        tracked = cur.fetchone()[0]
        cur.execute("CREATE TABLE IF NOT EXISTS osm_done (station_id BIGINT PRIMARY KEY REFERENCES stations(id) ON DELETE CASCADE)")
        if not tracked:
            # import fait avant l'existence du suivi : les gares ayant déjà des POI OSM proches sont considérées faites
            cur.execute(
                f"""INSERT INTO osm_done SELECT DISTINCT s.id FROM stations s JOIN pois p
                    ON p.source = 'osm' AND ST_DWithin(s.geom, p.geom, {RADIUS_M})"""
            )
        if restart:
            cur.execute("TRUNCATE osm_done")
        selected = ordered_stations(cur)[:limit_stations]
        stations = [(sid, lon, lat) for sid, lon, lat, done in selected if not done]
        if len(stations) < len(selected):
            print(f"  {len(selected) - len(stations)} gares déjà traitées (reprise) ; --osm-restart pour tout refaire")
        total = 0
        failed: list[int] = []
        started = time.monotonic()
        for i in range(0, len(stations), BATCH):
            batch = stations[i : i + BATCH]
            print(f"  gares {i + 1}-{i + len(batch)}/{len(stations)} : requête Overpass…", flush=True)
            try:
                elements = query([(lon, lat) for _, lon, lat in batch])
            except RuntimeError as e:
                # on n'interrompt pas tout l'import : ces gares seront retentées à la prochaine relance
                print(f"    lot ignoré, {e}", flush=True)
                failed += [sid for sid, _, _ in batch]
                continue
            total += save_elements(cur, elements, batch)
            cur.executemany(
                "INSERT INTO osm_done (station_id) VALUES (%s) ON CONFLICT DO NOTHING", [(sid,) for sid, _, _ in batch]
            )
            conn.commit()
            print(f"    {len(elements)} éléments reçus ({time.monotonic() - started:.0f} s écoulées)", flush=True)
            time.sleep(2)  # politesse envers l'API publique
        print(f"  {total} POI OSM enregistrés")
        if failed:
            print(f"  ! {len(failed)} gares non traitées (serveurs saturés) : relancez la même commande plus tard")
