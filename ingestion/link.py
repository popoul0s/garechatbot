"""Rapprochement des données :
1. dédoublonnage OSM / DATAtourisme (même lieu présent dans les deux sources) ;
2. association gare <-> POI avec distance et temps de marche estimé.
"""

from common import CLIMB_MIN_PER_M, DETOUR_FACTOR, WALK_METERS_PER_MIN, connect

MAX_DISTANCE_M = 3000


def run() -> None:
    print("Rapprochement gares / POI")
    with connect() as conn, conn.cursor() as cur:
        # Un POI OSM à moins de 200 m d'un POI DATAtourisme au nom proche est un doublon :
        # on fusionne ses thèmes dans la fiche DATAtourisme (plus riche) puis on le supprime.
        cur.execute(
            """
            WITH dup AS (
                SELECT o.id AS osm_id, d.id AS dt_id, o.tags AS osm_tags
                FROM pois o JOIN pois d
                  ON d.source = 'datatourisme' AND o.source = 'osm'
                 AND ST_DWithin(o.geom, d.geom, 200)
                 AND similarity(unaccent(lower(o.name)), unaccent(lower(d.name))) > 0.5
            ), merged AS (
                UPDATE pois p SET tags = ARRAY(SELECT DISTINCT unnest(p.tags || dup.osm_tags))
                FROM dup WHERE p.id = dup.dt_id
            )
            DELETE FROM pois WHERE id IN (SELECT osm_id FROM dup)
            """
        )
        print(f"  {cur.rowcount} doublons OSM fusionnés dans DATAtourisme")

        # altitude (étape elevation) : 10 min par 100 m de montée, règle de randonnée classique
        cur.execute("ALTER TABLE stations ADD COLUMN IF NOT EXISTS ele REAL")
        cur.execute("ALTER TABLE pois ADD COLUMN IF NOT EXISTS ele REAL")
        cur.execute("ALTER TABLE station_poi ADD COLUMN IF NOT EXISTS climb_m INTEGER")
        cur.execute("TRUNCATE station_poi")
        cur.execute(
            """
            INSERT INTO station_poi (station_id, poi_id, distance_m, walk_minutes, climb_m)
            SELECT s.id, p.id,
                   round(ST_Distance(s.geom, p.geom))::int,
                   greatest(1, ceil(ST_Distance(s.geom, p.geom) * %s / %s
                                    + greatest(0, coalesce(p.ele - s.ele, 0)) * %s))::int,
                   CASE WHEN p.ele IS NOT NULL AND s.ele IS NOT NULL
                        THEN greatest(0, round(p.ele - s.ele))::int END
            FROM stations s JOIN pois p ON ST_DWithin(s.geom, p.geom, %s)
            """,
            (DETOUR_FACTOR, WALK_METERS_PER_MIN, CLIMB_MIN_PER_M, MAX_DISTANCE_M),
        )
        print(f"  {cur.rowcount} associations gare-POI (rayon {MAX_DISTANCE_M} m)")
