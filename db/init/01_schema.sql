-- Schéma de la base GareChatBot (PostgreSQL + PostGIS + pgvector).
-- Exécuté automatiquement au premier démarrage du conteneur `db`.

CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS unaccent;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Gares (source principale : GTFS TER, enrichies par SNCF Open Data)
CREATE TABLE IF NOT EXISTS stations (
    id          BIGSERIAL PRIMARY KEY,
    uic         TEXT UNIQUE NOT NULL,          -- code UIC 8 chiffres, clé de rapprochement entre sources
    name        TEXT NOT NULL,
    city        TEXT,
    lon         DOUBLE PRECISION NOT NULL,
    lat         DOUBLE PRECISION NOT NULL,
    geom        GEOGRAPHY(Point, 4326) NOT NULL,
    pmr         BOOLEAN,                       -- accessibilité PMR (SNCF), NULL = inconnu
    equipments  TEXT[] NOT NULL DEFAULT '{}',
    sources     TEXT[] NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS stations_geom_idx ON stations USING GIST (geom);
CREATE INDEX IF NOT EXISTS stations_name_trgm_idx ON stations USING GIN (name gin_trgm_ops);

-- Services et infos pratiques des gares (toilettes, wifi, horaires d'ouverture, parking vélos...)
-- Une ligne par gare, catégorie et source. Rempli par ingestion/services.py.
CREATE TABLE IF NOT EXISTS station_services (
    station_id  BIGINT NOT NULL REFERENCES stations(id) ON DELETE CASCADE,
    category    TEXT NOT NULL,                 -- clé stable : toilettes, wifi, horaires, velo...
    label       TEXT NOT NULL,                 -- libellé affiché
    detail      TEXT,                          -- précision : "gratuites", "Lun-Ven 05:30-21:00"...
    source      TEXT NOT NULL,                 -- "SNCF Open Data" ou "OpenStreetMap"
    PRIMARY KEY (station_id, category, source)
);

-- Lignes ferroviaires (tracé simplifié issu du GTFS)
CREATE TABLE IF NOT EXISTS lines (
    id          SERIAL PRIMARY KEY,
    route_id    TEXT UNIQUE NOT NULL,
    name        TEXT NOT NULL,
    color       TEXT,
    geom        GEOMETRY(LineString, 4326) NOT NULL
);

-- Temps de trajet minimal origine -> gare, pré-calculé depuis les horaires GTFS
CREATE TABLE IF NOT EXISTS travel_times (
    origin_id       BIGINT NOT NULL REFERENCES stations(id) ON DELETE CASCADE,
    station_id      BIGINT NOT NULL REFERENCES stations(id) ON DELETE CASCADE,
    minutes         INTEGER NOT NULL,
    nb_changes      INTEGER NOT NULL DEFAULT 0,
    example_departure TEXT,                    -- ex. "08:12", horaire théorique ayant donné ce temps
    PRIMARY KEY (origin_id, station_id)
);

-- Points d'intérêt touristiques (DATAtourisme, OpenStreetMap)
-- `tags` utilise le vocabulaire normalisé de l'application :
--   nature, randonnee, montagne, eau, culture, patrimoine, musee, famille, loisirs, panorama
CREATE TABLE IF NOT EXISTS pois (
    id          BIGSERIAL PRIMARY KEY,
    source      TEXT NOT NULL,                 -- 'datatourisme' | 'osm'
    source_id   TEXT NOT NULL,
    name        TEXT NOT NULL,
    description TEXT,
    tags        TEXT[] NOT NULL DEFAULT '{}',
    raw_types   TEXT[] NOT NULL DEFAULT '{}', -- catégories d'origine, pour traçabilité
    url         TEXT,
    lon         DOUBLE PRECISION NOT NULL,
    lat         DOUBLE PRECISION NOT NULL,
    geom        GEOGRAPHY(Point, 4326) NOT NULL,
    embedding   VECTOR(384),                   -- rempli par ingestion/embed.py (optionnel)
    tsv         TSVECTOR GENERATED ALWAYS AS (
                    to_tsvector('french', coalesce(name, '') || ' ' || coalesce(description, ''))
                ) STORED,
    UNIQUE (source, source_id)
);
CREATE INDEX IF NOT EXISTS pois_geom_idx ON pois USING GIST (geom);
CREATE INDEX IF NOT EXISTS pois_tags_idx ON pois USING GIN (tags);
CREATE INDEX IF NOT EXISTS pois_tsv_idx ON pois USING GIN (tsv);

-- Rapprochement gare <-> POI (distance et temps de marche estimé)
CREATE TABLE IF NOT EXISTS station_poi (
    station_id    BIGINT NOT NULL REFERENCES stations(id) ON DELETE CASCADE,
    poi_id        BIGINT NOT NULL REFERENCES pois(id) ON DELETE CASCADE,
    distance_m    INTEGER NOT NULL,
    walk_minutes  INTEGER NOT NULL,
    PRIMARY KEY (station_id, poi_id)
);
CREATE INDEX IF NOT EXISTS station_poi_poi_idx ON station_poi (poi_id);

-- Horaires théoriques de la journée type (GTFS), pour les itinéraires détaillés calculés par l'API
CREATE TABLE IF NOT EXISTS gtfs_trips (
    trip_id     TEXT PRIMARY KEY,
    route_name  TEXT,
    headsign    TEXT,                          -- direction affichée
    number      TEXT                           -- numéro de train
);
CREATE TABLE IF NOT EXISTS connections (
    dep_min      INTEGER NOT NULL,             -- minutes depuis minuit (peut dépasser 1440)
    arr_min      INTEGER NOT NULL,
    from_station BIGINT NOT NULL REFERENCES stations(id) ON DELETE CASCADE,
    to_station   BIGINT NOT NULL REFERENCES stations(id) ON DELETE CASCADE,
    trip_id      TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS gtfs_meta (key TEXT PRIMARY KEY, value TEXT);
