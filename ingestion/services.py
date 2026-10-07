"""Infos pratiques des gares : services SNCF Open Data + équipements OpenStreetMap autour de la gare.

SNCF Open Data publie un jeu par type d'info (toilettes, horaires d'ouverture, wifi, fréquentation...).
Leurs identifiants changent parfois : chaque catégorie essaie des identifiants connus, puis cherche
dans le catalogue par mots-clés. Les champs sont lus de façon tolérante (noms partiels).
`python run_all.py services --inspect` montre les jeux trouvés et leurs champs.
"""

from __future__ import annotations

import re
import time
from collections import defaultdict
from typing import Callable

import requests

from common import connect, require_stations, uic_from
from osm import HEADERS, OVERPASS_URLS

SNCF = "https://ressources.data.sncf.com/api/explore/v2.1"
SNCF_SOURCE = "SNCF Open Data"
OSM_SOURCE = "OpenStreetMap"
OSM_RADIUS_M = 200
OSM_BATCH = 10

CREATE = """CREATE TABLE IF NOT EXISTS station_services (
    station_id BIGINT NOT NULL REFERENCES stations(id) ON DELETE CASCADE,
    category TEXT NOT NULL, label TEXT NOT NULL, detail TEXT, source TEXT NOT NULL,
    PRIMARY KEY (station_id, category, source))"""


# ---------- lecture tolérante des enregistrements ----------

def field(rec: dict, *parts: str) -> str | None:
    """Première valeur non vide dont le nom de champ contient l'un des fragments."""
    for part in parts:
        for k, v in rec.items():
            if part in k.lower() and v not in (None, "", []):
                return str(v).strip()
    return None


def record_uic(rec: dict) -> str | None:
    for k, v in rec.items():
        if "uic" in k.lower():
            uic = uic_from(v)
            if uic:
                return uic
    return None


def yes(v: str | None) -> bool | None:
    if v is None:
        return None
    s = v.strip().lower()
    if s in ("oui", "o", "true", "1", "yes", "y", "vrai"):
        return True
    if s in ("non", "n", "false", "0", "no", "faux"):
        return False
    return None


def join(*parts: str | None) -> str | None:
    out = ", ".join(p for p in parts if p)
    return out[:200] or None


# ---------- résumés par catégorie (liste d'enregistrements d'une gare -> détail) ----------

def toilettes(recs: list[dict]) -> str | None:
    r = recs[0]
    paying = yes(field(r, "payant"))
    free = yes(field(r, "gratuit"))
    price = field(r, "tarif", "prix")
    if paying is True or free is False:
        cost = f"payantes ({price})" if price and not yes(price) else "payantes"
    elif paying is False or free is True:
        cost = "gratuites"
    else:
        cost = None
    pmr = "accessibles PMR" if yes(field(r, "pmr", "accessib", "handicap")) else None
    where = field(r, "localisation", "emplacement", "situation")
    hours = field(r, "horaire")
    return join(cost, pmr, where, hours)


DAYS = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"]


def opening(recs: list[dict]) -> str | None:
    """Horaires d'ouverture : un enregistrement par jour, regroupés quand ils sont identiques."""
    by_day: dict[str, str] = {}
    for r in recs:
        day = (field(r, "jour") or "").lower()
        hours = field(r, "horaire_normal", "horaire", "heure")
        if hours:
            by_day[next((d for d in DAYS if day.startswith(d[:3])), day or "?")] = hours
    if not by_day:
        return None
    groups: list[tuple[list[str], str]] = []
    for d in DAYS + sorted(k for k in by_day if k not in DAYS):
        if d not in by_day:
            continue
        if groups and groups[-1][1] == by_day[d]:
            groups[-1][0].append(d)
        else:
            groups.append(([d], by_day[d]))
    fmt = lambda ds: ds[0][:3].capitalize() + (f"-{ds[-1][:3].capitalize()}" if len(ds) > 1 else "")  # noqa: E731
    return " · ".join(f"{fmt(ds)} {h}" for ds, h in groups)[:200]


def frequentation(recs: list[dict]) -> str | None:
    best = None
    for k, v in recs[0].items():
        m = re.search(r"(?:voyageurs|total)\D*(20\d\d)$", k.lower())
        if "non" in k.lower():  # "voyageurs + non voyageurs" : on garde les voyageurs seuls
            continue
        if m and v not in (None, ""):
            try:
                n = int(float(v))
            except ValueError:
                continue
            if n > 0 and (best is None or m[1] > best[0]):
                best = (m[1], n)
    return f"{best[1]:,} voyageurs en {best[0]}".replace(",", " ") if best else None


def count(word: str) -> Callable[[list[dict]], str | None]:
    return lambda recs: f"{len(recs)} {word}{'s' if len(recs) > 1 else ''}" if len(recs) > 1 else None


def located(recs: list[dict]) -> str | None:
    return field(recs[0], "localisation", "emplacement", "situation", "description")


def equipments(recs: list[dict]) -> str | None:
    names = sorted({v for r in recs if (v := field(r, "equipement", "libelle", "type"))})
    return ", ".join(names)[:200] or None


# catégorie, libellé, identifiants connus, recherche catalogue, mots attendus dans le titre, résumé
CATEGORIES: list[tuple[str, str, list[str], str, tuple[str, ...], Callable[[list[dict]], str | None]]] = [
    ("toilettes", "Toilettes", ["sanitaires-en-gare", "toilettes-en-gare"], "toilettes", ("toilette", "sanitaire"), toilettes),
    ("horaires", "Horaires d'ouverture de la gare", ["horaires-des-gares1", "horaires-des-gares"], "horaires des gares",
     ("horaire",), opening),
    ("wifi", "Wi-Fi gratuit", ["gares-equipees-du-wifi", "wifi-en-gare"], "wifi", ("wifi", "wi-fi"), lambda r: None),
    ("frequentation", "Fréquentation", ["frequentation-gares"], "fréquentation gares", ("fréquentation", "frequentation"),
     frequentation),
    ("acces_plus", "Assistance Accès Plus (voyageurs handicapés)", ["liste-des-gares-acces-plus", "gares-acces-plus"],
     "accès plus", ("accès plus", "acces plus"), lambda r: None),
    ("ascenseurs", "Ascenseurs", ["ascenseurs", "etat-des-ascenseurs"], "ascenseurs", ("ascenseur",), count("ascenseur")),
    ("escalators", "Escaliers mécaniques", ["escaliers-mecaniques", "escalators"], "escaliers mécaniques",
     ("escalier", "escalator"), count("escalier")),
    ("piano", "Piano en libre accès", ["gares-pianos"], "piano", ("piano",), lambda r: None),
    ("defibrillateur", "Défibrillateur", ["defibrillateurs", "defibrillateurs-en-gare"], "défibrillateur",
     ("défibrillateur", "defibrillateur"), located),
    ("consigne", "Consigne à bagages", ["consignes-a-bagages", "consignes"], "consigne bagages", ("consigne",), located),
    ("objets_trouves", "Service objets trouvés", ["bureaux-objets-trouves"], "bureaux objets trouvés",
     ("objets trouvés", "objets-trouves"), located),
    ("velo", "Stationnement vélo en gare", ["stationnement-velo-en-gare", "parkings-velos"], "stationnement vélo",
     ("vélo", "velo"), count("place")),
    ("accessibilite", "Équipements d'accessibilité", ["equipements-accessibilite-en-gares"], "équipements accessibilité",
     ("accessibilit",), equipments),
]


def fetch(dataset: str, limit: int = -1) -> list[dict]:
    r = requests.get(f"{SNCF}/catalog/datasets/{dataset}/exports/json", params={"limit": limit}, timeout=180)
    r.raise_for_status()
    return r.json()


def find_dataset(ids: list[str], search: str, title_words: tuple[str, ...]) -> str | None:
    """Identifiant connu qui répond, sinon premier jeu du catalogue dont le titre correspond et qui a un code UIC."""
    for ds in ids:
        try:
            sample = fetch(ds, 1)
            if sample and record_uic(sample[0]):
                return ds
        except requests.RequestException:
            pass
    try:
        r = requests.get(f"{SNCF}/catalog/datasets", params={"where": f'"{search}"', "limit": 20}, timeout=60)
        r.raise_for_status()
        results = r.json().get("results", [])
    except requests.RequestException:
        return None
    for d in results:
        title = str(d.get("metas", {}).get("default", {}).get("title", "")).lower()
        if any(w in title for w in title_words):
            try:
                sample = fetch(d["dataset_id"], 1)
            except requests.RequestException:
                continue
            if sample and record_uic(sample[0]):
                return d["dataset_id"]
    return None


def run_sncf(cur, uic_to_id: dict[str, int], inspect: bool = False) -> None:
    for key, label, ids, search, words, summarize in CATEGORIES:
        ds = find_dataset(ids, search, words)
        if not ds:
            print(f"  {label:<45} jeu introuvable, ignoré")
            continue
        if inspect:
            print(f"  {label:<45} {ds} : {sorted(fetch(ds, 1)[0].keys())}")
            continue
        try:
            recs = fetch(ds)
        except requests.RequestException as e:
            print(f"  {label:<45} {ds} en échec ({e}), ignoré")
            continue
        per_station: dict[int, list[dict]] = defaultdict(list)
        for rec in recs:
            sid = uic_to_id.get(record_uic(rec) or "")
            if sid:
                per_station[sid].append(rec)
        # wifi, piano... : un champ oui/non peut dire que le service est absent
        n = 0
        cur.execute("DELETE FROM station_services WHERE category = %s AND source = %s", (key, SNCF_SOURCE))
        for sid, rs in per_station.items():
            flag = yes(field(rs[0], "wifi", "disponible", "presence", "équipée", "equipee"))
            if flag is False:
                continue
            cur.execute(
                """INSERT INTO station_services (station_id, category, label, detail, source)
                   VALUES (%s, %s, %s, %s, %s) ON CONFLICT DO NOTHING""",
                (sid, key, label, summarize(rs), SNCF_SOURCE),
            )
            n += 1
        print(f"  {label:<45} {n:>4} gares ({ds})")


# ---------- OpenStreetMap : ce qu'il y a autour de la gare ----------

# (catégorie, libellé, filtre Overpass, compter les éléments ?)
OSM_SERVICES = [
    ("toilettes_osm", "Toilettes publiques à proximité", '["amenity"="toilets"]', False),
    ("velo_parking", "Parking vélos", '["amenity"="bicycle_parking"]', True),
    ("velo_location", "Vélos en location / libre-service", '["amenity"="bicycle_rental"]', False),
    ("taxi", "Station de taxis", '["amenity"="taxi"]', False),
    ("parking", "Parking voitures", '["amenity"="parking"]', True),
    ("autopartage", "Autopartage / location de voitures", '["amenity"~"^(car_sharing|car_rental)$"]', False),
    ("bus", "Arrêts de bus / car", '["highway"="bus_stop"]', True),
    ("tram", "Arrêt de tramway", '["railway"="tram_stop"]', False),
    ("restauration", "Cafés et restauration", '["amenity"~"^(cafe|restaurant|fast_food|bar)$"]', True),
    ("commerces", "Commerces (boulangerie, presse, supérette)", '["shop"~"^(bakery|newsagent|convenience|supermarket)$"]',
     True),
    ("distributeur", "Distributeur de billets", '["amenity"="atm"]', False),
    ("eau", "Point d'eau potable", '["amenity"="drinking_water"]', False),
    ("consigne_osm", "Consigne / casiers", '["amenity"="luggage_locker"]', False),
    ("billets", "Distributeur de billets de train", '["vending"~"public_transport_tickets"]', False),
    ("pharmacie", "Pharmacie", '["amenity"="pharmacy"]', False),
    ("info_touristique", "Office de tourisme / point info", '["tourism"="information"]["information"="office"]', False),
]


def osm_query(stations: list[tuple[int, float, float]]) -> list[dict]:
    body = "".join(
        f"nwr{flt}(around:{OSM_RADIUS_M},{lat:.6f},{lon:.6f});" for _, lon, lat in stations for _, _, flt, _ in OSM_SERVICES
    )
    q = f"[out:json][timeout:90];({body});out center tags;"
    for attempt in range(len(OVERPASS_URLS) * 2):
        url = OVERPASS_URLS[attempt % len(OVERPASS_URLS)]
        try:
            r = requests.post(url, data={"data": q}, headers=HEADERS, timeout=120)
            if r.ok:
                return r.json().get("elements", [])
            time.sleep(60 if r.status_code == 429 else 10)
        except requests.RequestException:
            time.sleep(5)
    raise RuntimeError("Overpass indisponible")


def matches(tags: dict[str, str], flt: str) -> bool:
    for key, op, val in re.findall(r'\["([^"]+)"(=|~)"([^"]+)"\]', flt):
        v = tags.get(key)
        if v is None or (op == "=" and v != val) or (op == "~" and not re.search(val, v)):
            return False
    return True


def run_osm(cur, conn, limit: int | None, restart: bool) -> None:
    if restart:
        cur.execute("DELETE FROM station_services WHERE source = %s", (OSM_SOURCE,))
    # gares proches des origines d'abord, et seulement celles pas encore traitées (reprise)
    cur.execute(
        """SELECT s.id, s.lon, s.lat FROM stations s
           LEFT JOIN (SELECT station_id, min(minutes) m FROM travel_times GROUP BY station_id
                      UNION ALL SELECT DISTINCT origin_id, 0 FROM travel_times) t ON t.station_id = s.id
           GROUP BY s.id ORDER BY min(t.m) NULLS LAST, s.id"""
    )
    stations = cur.fetchall()[:limit]
    cur.execute("SELECT DISTINCT station_id FROM station_services WHERE source = %s", (OSM_SOURCE,))
    done = {r[0] for r in cur.fetchall()}
    todo = [s for s in stations if s[0] not in done]
    print(f"  OSM : {len(todo)} gares à traiter ({len(stations) - len(todo)} déjà faites)")
    for i in range(0, len(todo), OSM_BATCH):
        batch = todo[i : i + OSM_BATCH]
        print(f"  gares {i + 1}-{i + len(batch)}/{len(todo)}…", flush=True)
        try:
            elements = osm_query(batch)
        except RuntimeError as e:
            print(f"    lot ignoré ({e}) : relancez plus tard", flush=True)
            continue
        found: dict[tuple[int, str], int] = defaultdict(int)
        for e in elements:
            lon = e.get("lon") or e.get("center", {}).get("lon")
            lat = e.get("lat") or e.get("center", {}).get("lat")
            if lon is None:
                continue
            sid, slon, slat = min(batch, key=lambda s: (s[1] - lon) ** 2 + (s[2] - lat) ** 2)
            if ((slon - lon) * 78_000) ** 2 + ((slat - lat) * 111_000) ** 2 > (OSM_RADIUS_M * 1.2) ** 2:
                continue
            for key, _, flt, _ in OSM_SERVICES:
                if matches(e.get("tags", {}), flt):
                    found[(sid, key)] += 1
        labels = {k: (lbl, cnt) for k, lbl, _, cnt in OSM_SERVICES}
        for (sid, key), n in found.items():
            label, counted = labels[key]
            detail = f"{n} à moins de {OSM_RADIUS_M} m" if counted and n > 1 else f"à moins de {OSM_RADIUS_M} m"
            cur.execute(
                """INSERT INTO station_services (station_id, category, label, detail, source) VALUES (%s, %s, %s, %s, %s)
                   ON CONFLICT (station_id, category, source) DO UPDATE SET detail = EXCLUDED.detail""",
                (sid, key, label, detail, OSM_SOURCE),
            )
        # gare sans aucun service trouvé : marqueur pour ne pas la retraiter
        for sid, _, _ in batch:
            if not any(k[0] == sid for k in found):
                cur.execute(
                    """INSERT INTO station_services (station_id, category, label, detail, source)
                       VALUES (%s, '_aucun', '', NULL, %s) ON CONFLICT DO NOTHING""",
                    (sid, OSM_SOURCE),
                )
        conn.commit()
        time.sleep(2)


def run(limit: int | None = None, restart: bool = False, inspect: bool = False, skip_osm: bool = False) -> None:
    print("Services et infos pratiques des gares")
    with connect() as conn, conn.cursor() as cur:
        require_stations(cur)
        cur.execute(CREATE)
        cur.execute("SELECT uic, id FROM stations")
        uic_to_id = dict(cur.fetchall())
        try:
            run_sncf(cur, uic_to_id, inspect)
        except requests.RequestException as e:
            print(f"  ! SNCF Open Data injoignable ({e}) : services SNCF ignorés")
        conn.commit()
        if not inspect and not skip_osm:
            run_osm(cur, conn, limit, restart)
        cur.execute(
            "SELECT source, count(DISTINCT station_id) FROM station_services WHERE category <> '_aucun' GROUP BY source"
        )
        for source, n in cur.fetchall():
            print(f"  {source} : infos pour {n} gares")
