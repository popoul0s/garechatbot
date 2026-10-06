"""Point d'entrée du pipeline d'ingestion.

Exemples :
    python run_all.py all --gtfs <url-ou-fichier.zip> --datatourisme <url-ou-archive-ou-dossier>
    python run_all.py gtfs --gtfs data/gtfs.zip --origins "Grenoble,Lyon Part-Dieu,Chambéry - Challes-les-Eaux"
    python run_all.py osm
    python run_all.py sncf --inspect
    python run_all.py link
    python run_all.py stats
"""

from __future__ import annotations

import argparse

import datatourisme
import gtfs
import link
import osm
import rail
import sncf
from common import connect

# À vérifier sur transport.data.gouv.fr (jeu « Horaires des TER », ressource GTFS) :
DEFAULT_GTFS = "https://eu.ftp.opendatasoft.com/sncf/gtfs/export-ter-gtfs-last.zip"
DEFAULT_ORIGINS = "Grenoble"


def stats() -> None:
    with connect() as conn, conn.cursor() as cur:
        for table in ["stations", "lines", "travel_times", "pois", "station_poi"]:
            cur.execute(f"SELECT count(*) FROM {table}")
            print(f"  {table:<13} {cur.fetchone()[0]:>7}")
        cur.execute("SELECT source, count(*) FROM pois GROUP BY source ORDER BY source")
        for source, n in cur.fetchall():
            print(f"  pois[{source}] {n:>7}")
        cur.execute("SELECT unnest(tags) t, count(*) FROM pois GROUP BY t ORDER BY 2 DESC")
        print("  thèmes :", ", ".join(f"{t}={n}" for t, n in cur.fetchall()))


def main() -> None:
    p = argparse.ArgumentParser(description="Ingestion GareChatBot")
    p.add_argument("step", choices=["all", "gtfs", "rail", "sncf", "osm", "datatourisme", "link", "stats"])
    p.add_argument("--gtfs", default=DEFAULT_GTFS, help="URL ou chemin du GTFS ferroviaire")
    p.add_argument("--origins", default=DEFAULT_ORIGINS, help="gares d'origine, séparées par des virgules")
    p.add_argument("--datatourisme", help="URL, archive .zip ou dossier du flux DATAtourisme")
    p.add_argument("--osm-limit", type=int, help="ne traiter que les N gares les plus proches des origines")
    p.add_argument("--osm-restart", action="store_true", help="osm : retraiter aussi les gares déjà importées")
    p.add_argument("--rail", help="rail : fichier GeoJSON du réseau ferré (par défaut, téléchargé depuis SNCF Open Data)")
    p.add_argument("--rail-osm", action="store_true", help="rail : utiliser OpenStreetMap au lieu de SNCF Open Data")
    p.add_argument("--inspect", action="store_true", help="sncf : afficher les champs des jeux SNCF")
    a = p.parse_args()

    if a.step in ("all", "gtfs"):
        gtfs.run(a.gtfs, [o.strip() for o in a.origins.split(",") if o.strip()])
    if a.step in ("all", "rail"):
        try:
            rail.run(a.rail, a.rail_osm)
        except Exception as e:  # noqa: BLE001 - sans tracé, la carte retombe sur des lignes droites
            print(f"  ! tracé des voies en échec : {e}")
            if a.step == "rail":
                raise
    if a.step in ("all", "sncf"):
        if a.inspect:
            sncf.inspect()
        else:
            try:
                sncf.run()
            except Exception as e:  # noqa: BLE001 - l'enrichissement SNCF ne doit pas bloquer le pipeline
                print(f"  ! enrichissement SNCF en échec : {e}")
    if a.step in ("all", "osm"):
        osm.run(a.osm_limit, a.osm_restart)
    if a.step in ("all", "datatourisme"):
        if a.datatourisme:
            datatourisme.run(a.datatourisme)
        elif a.step == "datatourisme":
            p.error("--datatourisme est requis")
        else:
            print("DATAtourisme : aucun flux fourni (--datatourisme), étape ignorée")
    if a.step in ("all", "datatourisme", "osm", "link"):
        link.run()
    if a.step in ("all", "stats"):
        stats()


if __name__ == "__main__":
    main()
