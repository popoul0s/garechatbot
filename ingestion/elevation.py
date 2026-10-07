"""Altitude des gares et des lieux (API altimétrique de l'IGN, gratuite, sans clé).

Sert au temps de marche : 500 m de montée entre la gare et un sommet ajoutent près d'une heure,
ce qu'une simple distance ignore. Règle de randonnée utilisée ensuite par link.py :
10 minutes par 100 m de dénivelé positif.
"""

from __future__ import annotations

import time

import requests

from common import connect, require_stations

API = "https://data.geopf.fr/altimetrie/1.0/calcul/alti/rest/elevation.json"
BATCH = 100  # points par requête (longueur d'URL raisonnable)
NO_DATA = -1000  # l'API renvoie -99999 hors couverture


def fetch(points: list[tuple[float, float]]) -> list[float | None]:
    params = {
        "lon": "|".join(f"{lon:.6f}" for lon, _ in points),
        "lat": "|".join(f"{lat:.6f}" for _, lat in points),
        "resource": "ign_rgealti_world",
        "delimiter": "|",
        "zonly": "true",
        "indent": "false",
        "measures": "false",
    }
    for attempt in range(3):
        try:
            r = requests.get(API, params=params, timeout=60)
            r.raise_for_status()
            values = r.json().get("elevations", [])
            out = []
            for v in values:
                z = v.get("z") if isinstance(v, dict) else v
                out.append(float(z) if z is not None and float(z) > NO_DATA else None)
            return out
        except (requests.RequestException, ValueError) as e:
            if attempt == 2:
                raise RuntimeError(f"API altimétrique IGN indisponible : {e}") from e
            time.sleep(3)
    return []


def fill(cur, conn, table: str) -> int:
    cur.execute(f"SELECT id, lon, lat FROM {table} WHERE ele IS NULL ORDER BY id")
    rows = cur.fetchall()
    done = 0
    for i in range(0, len(rows), BATCH):
        batch = rows[i : i + BATCH]
        values = fetch([(lon, lat) for _, lon, lat in batch])
        cur.executemany(
            f"UPDATE {table} SET ele = %s WHERE id = %s",
            [(z, rid) for (rid, _, _), z in zip(batch, values) if z is not None],
        )
        conn.commit()
        done += sum(z is not None for z in values)
        print(f"  {table} : {min(i + BATCH, len(rows))}/{len(rows)}", flush=True)
        time.sleep(0.3)
    return done


def ensure_columns(cur) -> None:
    cur.execute("ALTER TABLE stations ADD COLUMN IF NOT EXISTS ele REAL")
    cur.execute("ALTER TABLE pois ADD COLUMN IF NOT EXISTS ele REAL")
    cur.execute("ALTER TABLE station_poi ADD COLUMN IF NOT EXISTS climb_m INTEGER")


def run() -> None:
    print("Altitudes (API IGN) : gares et lieux")
    with connect() as conn, conn.cursor() as cur:
        require_stations(cur)
        ensure_columns(cur)
        conn.commit()
        try:
            n_st = fill(cur, conn, "stations")
            n_poi = fill(cur, conn, "pois")
        except RuntimeError as e:
            print(f"  ! {e} : les temps de marche resteront sans dénivelé")
            return
        print(f"  altitudes ajoutées : {n_st} gares, {n_poi} lieux")
