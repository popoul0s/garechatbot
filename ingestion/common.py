"""Outils partagés par les scripts d'ingestion."""

from __future__ import annotations

import os
import re
import unicodedata
from pathlib import Path

import psycopg
import requests

ROOT = Path(__file__).resolve().parent.parent
DATA_DIR = ROOT / "data"
DATA_DIR.mkdir(exist_ok=True)

# Emprise approximative de la région Auvergne-Rhône-Alpes (lon_min, lat_min, lon_max, lat_max)
AURA_BBOX = (2.06, 44.11, 7.19, 46.81)

# Vocabulaire de tags de l'application (identique à backend/src/domain/criteria.rs)
THEMES = {"nature", "randonnee", "montagne", "eau", "culture", "patrimoine", "musee", "famille", "loisirs", "panorama"}

# Vitesse de marche et facteur de détour pour estimer un temps de marche à partir d'une distance à vol d'oiseau
WALK_METERS_PER_MIN = 5000 / 60
DETOUR_FACTOR = 1.3

UIC_RE = re.compile(r"(87\d{6})")


def database_url() -> str:
    url = os.environ.get("DATABASE_URL")
    if url:
        return url
    env_file = ROOT / ".env"
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            if line.startswith("DATABASE_URL="):
                return line.split("=", 1)[1].strip()
    return "postgres://gare:gare@localhost:5432/garechatbot"


def connect() -> psycopg.Connection:
    return psycopg.connect(database_url())


def in_aura(lon: float, lat: float) -> bool:
    lon_min, lat_min, lon_max, lat_max = AURA_BBOX
    return lon_min <= lon <= lon_max and lat_min <= lat <= lat_max


def fold(text: str) -> str:
    """Minuscules sans accents, pour comparer des noms."""
    text = unicodedata.normalize("NFKD", text)
    return "".join(c for c in text if not unicodedata.combining(c)).lower().strip()


def uic_from(value: str) -> str | None:
    m = UIC_RE.findall(str(value))
    return m[-1] if m else None


def resolve_source(source: str, filename: str) -> Path:
    """Chemin local existant, ou URL http(s) téléchargée dans data/. Sinon : message explicite."""
    if source.startswith(("http://", "https://")):
        return download(source, filename)
    path = Path(source)
    if path.exists():
        return path
    available = sorted(p.name for p in DATA_DIR.iterdir()) if DATA_DIR.exists() else []
    raise SystemExit(
        f"Fichier introuvable : {source}\n"
        f"  Indiquez le chemin réel du fichier téléchargé, ou une URL http(s).\n"
        f"  Contenu de {DATA_DIR} : {', '.join(available) or '(vide)'}"
    )


def require_stations(cur) -> None:
    cur.execute("SELECT count(*) FROM stations")
    if cur.fetchone()[0] == 0:
        raise SystemExit("Aucune gare en base : lancez d'abord l'étape gtfs (python run_all.py gtfs --gtfs ...).")


def download(url: str, filename: str, force: bool = False) -> Path:
    """Télécharge une fois dans data/ (cache local)."""
    target = DATA_DIR / filename
    if target.exists() and not force:
        print(f"  (cache) {target}")
        return target
    print(f"  téléchargement {url}")
    with requests.get(url, stream=True, timeout=300) as r:
        r.raise_for_status()
        with open(target, "wb") as f:
            for chunk in r.iter_content(1 << 20):
                f.write(chunk)
    return target
