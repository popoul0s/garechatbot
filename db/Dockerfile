# Image officielle PostgreSQL (dépôt apt.postgresql.org déjà configuré, Debian récente)
# + extensions PostGIS et pgvector.
FROM postgres:16

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        postgresql-16-postgis-3 \
        postgresql-16-pgvector \
    && rm -rf /var/lib/apt/lists/*
