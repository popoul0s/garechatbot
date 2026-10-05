"""DATAtourisme -> points d'intérêt.

Prérequis : créer un compte diffuseur sur https://diffuseur.datatourisme.fr, configurer un flux
(région Auvergne-Rhône-Alpes, types : lieux / itinéraires) au format « JSON-LD, un fichier par objet »,
puis télécharger l'archive (ou passer l'URL de téléchargement du flux).

Le format JSON-LD de DATAtourisme est riche et varie selon les producteurs : la lecture est volontairement
tolérante (champs absents ignorés).
"""

from __future__ import annotations

import json
import zipfile
from typing import Any, Iterator

from common import connect, in_aura, resolve_source

# Types DATAtourisme ignorés (hors périmètre « découverte ») et correspondance mots-clés -> thèmes
SKIP_TYPES = ("accommodation", "foodestablishment", "restaurant", "entertainmentandevent", "event", "store", "service")
TYPE_RULES = [
    ("museum", {"musee", "culture"}),
    ("culturalsite", {"culture"}),
    ("theater", {"culture"}),
    ("religioussite", {"patrimoine", "culture"}),
    ("church", {"patrimoine", "culture"}),
    ("castle", {"patrimoine", "culture"}),
    ("remarkablebuilding", {"patrimoine", "culture"}),
    ("landmark", {"patrimoine", "culture"}),
    ("historic", {"patrimoine", "culture"}),
    ("naturalheritage", {"nature"}),
    ("parkandgarden", {"nature", "famille"}),
    ("park", {"nature"}),
    ("walkingtour", {"randonnee", "nature"}),
    ("hiking", {"randonnee", "nature"}),
    ("sportsandleisureplace", {"loisirs"}),
    ("leisure", {"loisirs"}),
    ("zoo", {"loisirs", "famille"}),
    ("amusementpark", {"loisirs", "famille"}),
    ("lake", {"eau", "nature"}),
    ("beach", {"eau", "nature"}),
    ("mountain", {"montagne", "nature"}),
]


def first_fr(value: Any) -> str | None:
    """Les libellés DATAtourisme sont de la forme {"fr": ["..."]} (parfois une chaîne)."""
    if isinstance(value, dict):
        v = value.get("fr") or value.get("@value") or next(iter(value.values()), None)
        return first_fr(v)
    if isinstance(value, list):
        return first_fr(value[0]) if value else None
    return str(value).strip() if value else None


def iter_objects(source: str) -> Iterator[dict]:
    path = resolve_source(source, "datatourisme.zip")
    if path.is_dir():
        for f in path.rglob("*.json"):
            if f.name != "index.json":
                yield json.loads(f.read_text(encoding="utf-8"))
    else:
        with zipfile.ZipFile(path) as z:
            for name in z.namelist():
                if name.endswith(".json") and not name.endswith("index.json"):
                    yield json.loads(z.read(name))


def tags_for(obj: dict) -> tuple[list[str], list[str]]:
    types = obj.get("@type", [])
    types = [types] if isinstance(types, str) else types
    raw = [str(t).split(":")[-1] for t in types]
    low = [t.lower() for t in raw]
    tags: set[str] = set()
    for keyword, themes in TYPE_RULES:
        if any(keyword in t for t in low):
            tags |= themes
    audience = json.dumps(obj.get("hasAudience", "")).lower()
    if "famil" in audience or "child" in audience or "enfant" in audience:
        tags.add("famille")
    return sorted(tags), raw


def run(source: str) -> None:
    print("DATAtourisme : import des POI")
    n, skipped = 0, 0
    with connect() as conn, conn.cursor() as cur:
        for obj in iter_objects(source):
            tags, raw = tags_for(obj)
            if not tags or any(s in t.lower() for t in raw for s in SKIP_TYPES):
                skipped += 1
                continue
            try:
                geo = obj["isLocatedAt"][0]["schema:geo"]
                lat = float(geo["schema:latitude"])
                lon = float(geo["schema:longitude"])
            except (KeyError, IndexError, TypeError, ValueError):
                skipped += 1
                continue
            name = first_fr(obj.get("rdfs:label"))
            if not name or not in_aura(lon, lat):
                skipped += 1
                continue
            desc = first_fr(obj.get("rdfs:comment"))
            if not desc:
                descs = obj.get("hasDescription") or [{}]
                desc = first_fr(descs[0].get("shortDescription") or descs[0].get("dc:description"))
            url = None
            contacts = obj.get("hasContact") or []
            if contacts and isinstance(contacts[0], dict):
                url = first_fr(contacts[0].get("foaf:homepage"))
            cur.execute(
                """INSERT INTO pois (source, source_id, name, description, tags, raw_types, url, lon, lat, geom)
                   VALUES ('datatourisme', %s, %s, %s, %s, %s, %s, %s, %s, ST_SetSRID(ST_MakePoint(%s, %s), 4326)::geography)
                   ON CONFLICT (source, source_id) DO UPDATE SET name = EXCLUDED.name, description = EXCLUDED.description,
                       tags = EXCLUDED.tags, raw_types = EXCLUDED.raw_types, url = EXCLUDED.url""",
                (obj.get("@id"), name, desc, tags, raw, url, lon, lat, lon, lat),
            )
            n += 1
    print(f"  {n} POI importés, {skipped} objets ignorés (hors périmètre, sans coordonnées ou hors région)")
