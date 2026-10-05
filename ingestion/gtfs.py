"""GTFS (transport.data.gouv.fr / SNCF) -> gares, lignes et temps de trajet.

Étapes :
1. gares : arrêts ferroviaires du GTFS, regroupés par code UIC, limités à l'Auvergne-Rhône-Alpes ;
2. lignes : pour chaque route, tracé simplifié reliant les gares du trajet le plus long ;
3. temps de trajet : algorithme CSA (Connection Scan Algorithm) sur les horaires théoriques d'une
   journée type, pour chaque départ de l'origine entre 6h et 20h ; on garde le trajet le plus court.
"""

from __future__ import annotations

import io
import zipfile
from bisect import bisect_left
from collections import defaultdict
from pathlib import Path

import pandas as pd

from common import connect, download, fold, in_aura, uic_from

MIN_TRANSFER_MIN = 5
MAX_TRAVEL_MIN = 240
FIRST_DEPARTURE = 6 * 60
LAST_DEPARTURE = 20 * 60
# Types de route GTFS ferroviaires (2 = train, 100-117 = types étendus "rail")
RAIL_ROUTE_TYPES = {2} | set(range(100, 118))


def _read(z: zipfile.ZipFile, name: str, **kw) -> pd.DataFrame | None:
    if name not in z.namelist():
        return None
    return pd.read_csv(io.BytesIO(z.read(name)), dtype=str, keep_default_na=False, **kw)


def _to_min(hms: str) -> int:
    h, m, *_ = hms.split(":")
    return int(h) * 60 + int(m)


def load(source: str) -> dict[str, pd.DataFrame]:
    path = Path(source)
    if not path.exists():
        path = download(source, "gtfs.zip")
    with zipfile.ZipFile(path) as z:
        tables = {n: _read(z, f"{n}.txt") for n in ["stops", "routes", "trips", "stop_times", "calendar", "calendar_dates"]}
    for required in ["stops", "routes", "trips", "stop_times"]:
        if tables[required] is None:
            raise SystemExit(f"GTFS invalide : {required}.txt manquant")
    return tables


def pick_service_date(t: dict[str, pd.DataFrame]) -> tuple[str, set[str]]:
    """Choisit le jour (hors week-end) ayant le plus de services actifs, et renvoie ces services."""
    active: dict[str, set[str]] = defaultdict(set)
    cal = t["calendar"]
    if cal is not None and len(cal):
        days = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"]
        for _, row in cal.iterrows():
            for d in pd.date_range(row["start_date"], row["end_date"]):
                if row[days[d.weekday()]] == "1":
                    active[d.strftime("%Y%m%d")].add(row["service_id"])
    cd = t["calendar_dates"]
    if cd is not None and len(cd):
        for _, row in cd.iterrows():
            if row["exception_type"] == "1":
                active[row["date"]].add(row["service_id"])
            else:
                active[row["date"]].discard(row["service_id"])
    weekdays = {d: s for d, s in active.items() if pd.Timestamp(d).weekday() < 5}
    candidates = weekdays or active
    if not candidates:
        raise SystemExit("Impossible de déterminer une date de service dans le GTFS")
    date = max(candidates, key=lambda d: len(candidates[d]))
    return date, candidates[date]


def build_stations(t: dict[str, pd.DataFrame], rail_stop_ids: set[str]) -> pd.DataFrame:
    stops = t["stops"].copy()
    stops["uic"] = stops["stop_id"].map(uic_from)
    stops = stops[stops["uic"].notna()]
    stops["lon"] = stops["stop_lon"].astype(float)
    stops["lat"] = stops["stop_lat"].astype(float)
    rail_uics = set(stops[stops["stop_id"].isin(rail_stop_ids)]["uic"])
    stops = stops[stops["uic"].isin(rail_uics)]
    # une ligne par UIC : on privilégie la zone d'arrêt (location_type=1) pour le nom
    if "location_type" in stops:
        stops = stops.sort_values("location_type", ascending=False)
    stations = stops.drop_duplicates("uic")[["uic", "stop_name", "lon", "lat"]]
    stations = stations[[in_aura(lo, la) for lo, la in zip(stations["lon"], stations["lat"])]]
    return stations.rename(columns={"stop_name": "name"})


def connections_for(t: dict[str, pd.DataFrame], services: set[str]) -> tuple[list[tuple], set[str], dict[str, str]]:
    routes = t["routes"]
    rail_routes = set(routes[routes["route_type"].astype(int).isin(RAIL_ROUTE_TYPES)]["route_id"])
    trips = t["trips"]
    trips = trips[trips["route_id"].isin(rail_routes) & trips["service_id"].isin(services)]
    trip_ids = set(trips["trip_id"])

    st = t["stop_times"]
    st = st[st["trip_id"].isin(trip_ids)].copy()
    st["seq"] = st["stop_sequence"].astype(int)
    st["uic"] = st["stop_id"].map(uic_from)
    st = st[st["uic"].notna() & (st["arrival_time"] != "") & (st["departure_time"] != "")]
    st = st.sort_values(["trip_id", "seq"])

    conns: list[tuple] = []  # (dep_min, arr_min, from_uic, to_uic, trip_id)
    for trip_id, g in st.groupby("trip_id", sort=False):
        rows = list(zip(g["uic"], g["arrival_time"], g["departure_time"]))
        for (u, _, dep), (v, arr, _) in zip(rows, rows[1:]):
            if u != v:
                conns.append((_to_min(dep), _to_min(arr), u, v, trip_id))
    conns.sort()
    return conns, set(t["stop_times"][t["stop_times"]["trip_id"].isin(trip_ids)]["stop_id"]), dict(
        zip(trips["trip_id"], trips["route_id"])
    )


def earliest_arrivals(conns: list[tuple], origin: str, start: int) -> dict[str, tuple[int, int]]:
    """CSA : heure d'arrivée au plus tôt et nombre de trains utilisés, en partant de `origin` à `start`."""
    arrival: dict[str, int] = {origin: start}
    legs: dict[str, int] = {origin: 0}
    trip_legs: dict[str, int] = {}
    limit = start + MAX_TRAVEL_MIN
    first = bisect_left(conns, (start,))
    for dep, arr, u, v, trip in conns[first:]:
        if dep > limit:
            break
        if trip in trip_legs:
            n = trip_legs[trip]
        elif u in arrival and arrival[u] + (0 if u == origin else MIN_TRANSFER_MIN) <= dep:
            n = legs[u] + 1
            trip_legs[trip] = n
        else:
            continue
        if arr < arrival.get(v, 10**9) or (arr == arrival[v] and n < legs[v]):
            arrival[v] = arr
            legs[v] = n
    return {s: (a, legs[s]) for s, a in arrival.items()}


def travel_times(conns: list[tuple], origin: str) -> dict[str, tuple[int, int, str]]:
    """Pour chaque gare : (durée minimale, nb de correspondances, heure de départ correspondante)."""
    departures = sorted({dep for dep, _, u, _, _ in conns if u == origin and FIRST_DEPARTURE <= dep <= LAST_DEPARTURE})
    best: dict[str, tuple[int, int, str]] = {}
    for start in departures:
        for station, (arr, n) in earliest_arrivals(conns, origin, start).items():
            if station == origin:
                continue
            duration = arr - start
            if station not in best or (duration, n - 1) < best[station][:2]:
                best[station] = (duration, max(n - 1, 0), f"{start // 60 % 24:02d}:{start % 60:02d}")
    return best


def build_lines(t: dict[str, pd.DataFrame], trip_route: dict[str, str], coords: dict[str, tuple[float, float]]):
    st = t["stop_times"]
    st = st[st["trip_id"].isin(trip_route.keys())].copy()
    st["seq"] = st["stop_sequence"].astype(int)
    st["uic"] = st["stop_id"].map(uic_from)
    lines = {}
    for trip_id, g in st.sort_values(["trip_id", "seq"]).groupby("trip_id", sort=False):
        route = trip_route[trip_id]
        seq = [u for u in g["uic"] if u in coords]
        seq = [u for i, u in enumerate(seq) if i == 0 or seq[i - 1] != u]
        if len(seq) >= 2 and len(seq) > len(lines.get(route, [])):
            lines[route] = seq
    routes = t["routes"].set_index("route_id")
    out = []
    for route_id, seq in lines.items():
        r = routes.loc[route_id]
        name = (r.get("route_long_name") or r.get("route_short_name") or route_id).strip()
        color = r.get("route_color") or None
        out.append((route_id, name, f"#{color}" if color else None, [coords[u] for u in seq]))
    return out


def run(source: str, origins: list[str]) -> None:
    print("GTFS : chargement")
    t = load(source)
    date, services = pick_service_date(t)
    print(f"  journée type retenue : {date} ({len(services)} services actifs)")

    conns, rail_stop_ids, trip_route = connections_for(t, services)
    print(f"  {len(conns)} connexions ferroviaires ce jour-là")
    stations = build_stations(t, rail_stop_ids)
    print(f"  {len(stations)} gares en Auvergne-Rhône-Alpes")

    with connect() as conn, conn.cursor() as cur:
        for s in stations.itertuples():
            cur.execute(
                """INSERT INTO stations (uic, name, lon, lat, geom, sources)
                   VALUES (%s, %s, %s, %s, ST_SetSRID(ST_MakePoint(%s, %s), 4326)::geography, ARRAY['gtfs'])
                   ON CONFLICT (uic) DO UPDATE SET name = EXCLUDED.name, lon = EXCLUDED.lon, lat = EXCLUDED.lat,
                       geom = EXCLUDED.geom,
                       sources = ARRAY(SELECT DISTINCT unnest(stations.sources || EXCLUDED.sources))""",
                (s.uic, s.name, s.lon, s.lat, s.lon, s.lat),
            )
        cur.execute("SELECT uic, id, name FROM stations")
        rows = cur.fetchall()
        ids = {uic: sid for uic, sid, _ in rows}

        coords = {s.uic: (s.lon, s.lat) for s in stations.itertuples()}
        lines = build_lines(t, trip_route, coords)
        cur.execute("DELETE FROM lines")
        for route_id, name, color, pts in lines:
            wkt = "LINESTRING(" + ", ".join(f"{lo} {la}" for lo, la in pts) + ")"
            cur.execute(
                "INSERT INTO lines (route_id, name, color, geom) VALUES (%s, %s, %s, ST_GeomFromText(%s, 4326))",
                (route_id, name, color, wkt),
            )
        print(f"  {len(lines)} lignes")

        for origin_name in origins:
            target = fold(origin_name)
            matches = [(uic, name) for uic, _, name in rows if fold(name) == target] or [
                (uic, name) for uic, _, name in rows if fold(name).startswith(target)
            ]
            if not matches:
                print(f"  ! origine « {origin_name} » introuvable parmi les gares, ignorée")
                continue
            uic, name = min(matches, key=lambda m: len(m[1]))
            best = travel_times(conns, uic)
            cur.execute("DELETE FROM travel_times WHERE origin_id = %s", (ids[uic],))
            n = 0
            for station_uic, (minutes, changes, dep) in best.items():
                if station_uic in ids:
                    cur.execute(
                        """INSERT INTO travel_times (origin_id, station_id, minutes, nb_changes, example_departure)
                           VALUES (%s, %s, %s, %s, %s)""",
                        (ids[uic], ids[station_uic], minutes, changes, dep),
                    )
                    n += 1
            print(f"  temps de trajet depuis {name} : {n} gares atteignables en moins de {MAX_TRAVEL_MIN} min")
