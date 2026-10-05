"""OpenStreetMap (API Overpass) -> points d'intérêt autour des gares.

Les tags OSM sont traduits vers le vocabulaire de l'application (voir `map_tags`).
"""

from __future__ import annotations

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
    "User-Agent": "GareChatBot/0.1 (projet etudiant tourisme ferroviaire AURA)",
    "Accept": "application/json",
}
RADIUS_M = 3000
BATCH = 8

FILTERS = [
    '["tourism"~"^(attraction|museum|viewpoint|zoo|theme_park|picnic_site|gallery)$"]',
    '["historic"~"^(castle|monument|ruins|archaeological_site|fort|abbey|city_gate|manor)$"]',
    '["leisure"~"^(park|nature_reserve|playground|water_park|swimming_area|garden)$"]',
    '["natural"~"^(peak|waterfall|cave_entrance|beach|gorge)$"]',
    '["water"="lake"]["name"]',
    '["route"="hiking"]["name"]',
]


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
    return None


def description(t: dict[str, str]) -> str | None:
    parts = [t.get("description:fr") or t.get("description")]
    if t.get("ele"):
        parts.append(f"Altitude {t['ele']} m")
    if t.get("route") == "hiking" and t.get("distance"):
        parts.append(f"Itinéraire de {t['distance']} km")
    parts = [p for p in parts if p]
    return ". ".join(parts) or None


def query(stations: list[tuple[float, float]]) -> list[dict]:
    body = "".join(
        f"nwr(around:{RADIUS_M},{lat},{lon}){f};" for lon, lat in stations for f in FILTERS
    )
    q = f"[out:json][timeout:180];({body});out center tags;"
    errors = []
    for attempt in range(6):
        url = OVERPASS_URLS[attempt % len(OVERPASS_URLS)]
        try:
            r = requests.post(url, data={"data": q}, headers=HEADERS, timeout=240)
        except requests.RequestException as e:
            errors.append(f"{url}: {e}")
            continue
        if r.ok:
            return r.json().get("elements", [])
        errors.append(f"{url}: HTTP {r.status_code}")
        time.sleep(5 * (attempt + 1) if r.status_code in (429, 504) else 1)
    raise RuntimeError("Overpass indisponible :\n  " + "\n  ".join(errors))


def run(limit_stations: int | None = None) -> None:
    print("OSM : récupération des POI autour des gares (Overpass)")
    with connect() as conn, conn.cursor() as cur:
        require_stations(cur)
        # les gares les plus proches des origines d'abord : --osm-limit garde les plus utiles
        cur.execute(
            """SELECT s.lon, s.lat FROM stations s
               LEFT JOIN (SELECT station_id, min(minutes) AS m FROM travel_times GROUP BY station_id) t
                 ON t.station_id = s.id
               ORDER BY t.m NULLS LAST, s.id"""
        )
        stations = cur.fetchall()[:limit_stations]
        total = 0
        for i in range(0, len(stations), BATCH):
            elements = query(stations[i : i + BATCH])
            for e in elements:
                tags = e.get("tags", {})
                name = default_name(tags)
                lon = e.get("lon") or e.get("center", {}).get("lon")
                lat = e.get("lat") or e.get("center", {}).get("lat")
                app_tags = map_tags(tags)
                if not name or lon is None or not app_tags:
                    continue
                raw = [f"{k}={tags[k]}" for k in ("tourism", "historic", "leisure", "natural", "water", "route") if k in tags]
                cur.execute(
                    """INSERT INTO pois (source, source_id, name, description, tags, raw_types, url, lon, lat, geom)
                       VALUES ('osm', %s, %s, %s, %s, %s, %s, %s, %s, ST_SetSRID(ST_MakePoint(%s, %s), 4326)::geography)
                       ON CONFLICT (source, source_id) DO UPDATE SET name = EXCLUDED.name,
                           description = EXCLUDED.description, tags = EXCLUDED.tags, raw_types = EXCLUDED.raw_types""",
                    (f"{e['type']}/{e['id']}", name, description(tags), app_tags, raw,
                     tags.get("website"), lon, lat, lon, lat),
                )
                total += 1
            conn.commit()
            print(f"  gares {i + 1}-{min(i + BATCH, len(stations))}/{len(stations)} : {len(elements)} éléments")
            time.sleep(2)  # politesse envers l'API publique
        print(f"  {total} POI OSM enregistrés")
