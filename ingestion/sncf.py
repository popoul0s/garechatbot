"""SNCF Open Data -> enrichissement des gares (commune, accessibilité PMR, équipements).

Les gares sont créées à partir du GTFS ; ce script les complète en rapprochant les jeux SNCF
par code UIC. Les noms de champs des jeux SNCF changent parfois : lancer d'abord
`python run_all.py sncf --inspect` pour afficher les champs disponibles, puis ajuster FIELDS.
"""

from __future__ import annotations

import requests

from common import connect, require_stations, uic_from

API = "https://ressources.data.sncf.com/api/explore/v2.1/catalog/datasets/{}/exports/json"

DATASETS = {
    # jeu principal : liste des gares de voyageurs
    "gares": "gares-de-voyageurs",
    # accessibilité / équipements (optionnel, à vérifier sur ressources.data.sncf.com)
    "equipements": "equipements-accessibilite-en-gares",
}

# Champs candidats, dans l'ordre de préférence
FIELDS = {
    "city": ["commune", "commune_libellemin", "nom_commune", "libellecommune"],
    "pmr": ["accessibilite_pmr", "pmr", "accessible_pmr", "prm_accessibility"],
    "equipment": ["equipement", "libelle_equipement", "nom_equipement", "type_equipement"],
}


def fetch(dataset: str, aura_only: bool = True) -> list[dict]:
    params = {"limit": -1}
    r = requests.get(API.format(dataset), params=params, timeout=120)
    r.raise_for_status()
    return r.json()


def record_uic(rec: dict) -> str | None:
    for key, value in rec.items():
        if "uic" in key.lower():
            uic = uic_from(value)
            if uic:
                return uic
    return None


def pick(rec: dict, names: list[str]):
    for n in names:
        if rec.get(n) not in (None, ""):
            return rec[n]
    return None


def as_bool(value) -> bool | None:
    if value is None:
        return None
    s = str(value).strip().lower()
    if s in ("true", "1", "oui", "o", "yes", "accessible", "total"):
        return True
    if s in ("false", "0", "non", "n", "no", "non accessible"):
        return False
    return None


def inspect() -> None:
    for label, ds in DATASETS.items():
        try:
            recs = requests.get(API.format(ds), params={"limit": 1}, timeout=60).json()
            print(f"{label} ({ds}) : {sorted(recs[0].keys()) if recs else 'vide'}")
        except Exception as e:  # noqa: BLE001 - outil de diagnostic
            print(f"{label} ({ds}) : erreur {e}")


def run() -> None:
    print("SNCF Open Data : enrichissement des gares")
    with connect() as conn, conn.cursor() as cur:
        require_stations(cur)
        cur.execute("SELECT uic FROM stations")
        known = {r[0] for r in cur.fetchall()}

        updated = 0
        for rec in fetch(DATASETS["gares"]):
            uic = record_uic(rec)
            if uic not in known:
                continue
            city = pick(rec, FIELDS["city"])
            cur.execute(
                """UPDATE stations SET city = COALESCE(%s, city),
                       sources = ARRAY(SELECT DISTINCT unnest(sources || ARRAY['sncf']))
                   WHERE uic = %s""",
                (city, uic),
            )
            updated += 1
        print(f"  {updated} gares rapprochées avec « {DATASETS['gares']} »")

        try:
            records = fetch(DATASETS["equipements"])
        except requests.HTTPError as e:
            print(f"  jeu équipements indisponible ({e}) : à vérifier, étape ignorée")
            return
        per_station: dict[str, dict] = {}
        for rec in records:
            uic = record_uic(rec)
            if uic not in known:
                continue
            entry = per_station.setdefault(uic, {"pmr": None, "equipments": set()})
            pmr = as_bool(pick(rec, FIELDS["pmr"]))
            if pmr is not None:
                entry["pmr"] = pmr if entry["pmr"] is None else entry["pmr"] or pmr
            eq = pick(rec, FIELDS["equipment"])
            if eq:
                entry["equipments"].add(str(eq))
        for uic, e in per_station.items():
            cur.execute(
                "UPDATE stations SET pmr = COALESCE(%s, pmr), equipments = %s WHERE uic = %s",
                (e["pmr"], sorted(e["equipments"]), uic),
            )
        print(f"  accessibilité / équipements renseignés pour {len(per_station)} gares")
