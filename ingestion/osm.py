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
    '["water"="lake"]["name"]',
    '["leisure"="fishing"]',
]
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


def query(stations: list[tuple[float, float]]) -> list[dict]:
    body = "".join(
        f"nwr{f}({lat - DLAT:.5f},{lon - DLON:.5f},{lat + DLAT:.5f},{lon + DLON:.5f});"
        for lon, lat in stations
        for f in FILTERS
    )
    q = f"[out:json][timeout:90];({body});out center tags;"
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


def run(limit_stations: int | None = None, restart: bool = False) -> None:
    print("OSM : récupération des POI autour des gares (Overpass)")
    with connect() as conn, conn.cursor() as cur:
        require_stations(cur)
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
        # les gares les plus proches des origines d'abord : --osm-limit garde les plus utiles
        cur.execute(
            """SELECT s.id, s.lon, s.lat, d.station_id IS NOT NULL FROM stations s
               LEFT JOIN (SELECT station_id, min(minutes) AS m FROM travel_times GROUP BY station_id
                          UNION ALL SELECT DISTINCT origin_id, 0 FROM travel_times) t
                 ON t.station_id = s.id
               LEFT JOIN osm_done d ON d.station_id = s.id
               ORDER BY t.m NULLS LAST, s.id"""
        )
        selected = cur.fetchall()[:limit_stations]
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
            cur.executemany(
                "INSERT INTO osm_done (station_id) VALUES (%s) ON CONFLICT DO NOTHING", [(sid,) for sid, _, _ in batch]
            )
            conn.commit()
            print(f"    {len(elements)} éléments reçus ({time.monotonic() - started:.0f} s écoulées)", flush=True)
            time.sleep(2)  # politesse envers l'API publique
        print(f"  {total} POI OSM enregistrés")
        if failed:
            print(f"  ! {len(failed)} gares non traitées (serveurs saturés) : relancez la même commande plus tard")
