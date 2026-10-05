# GareChatBot : découvrir Auvergne-Rhône-Alpes en train

Prototype de plateforme de tourisme ferroviaire. Deux usages :

- **Explorer** : carte → gare → points d'intérêt ;
- **Demander** : une question en langage naturel donne des recommandations argumentées, affichées sur la carte.

## Architecture

```
 Sources                     Ingestion (Python)            Stockage                  API (Rust / axum)              Client (React + MapLibre)
 ─────────────────────       ───────────────────────       ────────────────────      ─────────────────────────      ─────────────────────────
 GTFS TER (transport.  ──▶   gtfs.py : gares, lignes,                                /api/stations, /api/pois       Carte : gares (couleur =
 data.gouv.fr)               temps de trajet (CSA)    ──▶  PostgreSQL               /api/map/*  (GeoJSON)          temps de trajet), lignes,
 SNCF Open Data        ──▶   sncf.py : commune, PMR   ──▶  + PostGIS (géo)    ──▶   /api/search (sans IA)    ──▶   POI, résultats numérotés
 OpenStreetMap         ──▶   osm.py : POI (Overpass)  ──▶  + pgvector               /api/chat   (IA)               Assistant (chat + critères)
 DATAtourisme          ──▶   datatourisme.py : POI    ──▶  + plein texte FR                │                       Explorer (gare → POI)
                             link.py : dédoublonnage,                                       ▼
                             gare ↔ POI (distance,                                  LLM (API compatible OpenAI) :
                             temps de marche)                                        Ollama (local) ou cloud
```

### Chaîne IA de `/api/chat`

```
Question (+ critères de la session + gare sélectionnée sur la carte)
  ↓ 1. Compréhension : le LLM extrait un JSON de critères (repli : extracteur à règles en Rust)
  ↓ 2. Validation (vocabulaire fermé, bornes) puis fusion avec les critères de la session
  ↓ 3. Recherche SQL/PostGIS : contraintes dures (temps de train, temps de marche) + plein texte
  ↓ 4. Scoring déterministe en Rust → top 5 (le LLM ne choisit jamais les destinations)
  ↓ 5. Rédaction : le LLM reçoit uniquement les faits calculés par le code
  ↓ 6. Garde-fou : on retire les destinations inventées, et une explication contenant un chiffre absent des faits est remplacée par le gabarit
  ↓ Réponse JSON : texte + recommandations (coordonnées) → la carte se met à jour
```

**Scoring** (`backend/src/domain/scoring.rs`), chaque composante est entre 0 et 1 :

```
score = 0.35 × thème        (part des thèmes demandés couverts par le meilleur POI, + plein texte)
      + 0.25 × trajet       (1 − durée / durée max)
      + 0.15 × marche       (1 − marche jusqu'au meilleur POI / marche max)
      + 0.15 × richesse     (nombre de POI pertinents, saturé à 5)
      + 0.10 × accessibilité (trajet direct, gare PMR)
```

Le détail du score est renvoyé par l'API et affiché sous chaque recommandation.

**Sans LLM** (`LLM_BASE_URL` vide), l'application fonctionne entièrement : l'extraction se fait par règles et la réponse est construite à partir des faits. Aucune clé payante n'est donc indispensable.

**Si les données ne suffisent pas** : l'application assouplit une fois les contraintes (+30 min de train, +15 min de marche) et le signale. Sinon, elle répond qu'aucune destination des données ne correspond. Une ville d'origine inconnue ou sans temps de trajet calculés est signalée explicitement.

### Pourquoi ces choix

| Brique | Choix | Raison |
|---|---|---|
| API | **Rust + axum + sqlx** | Performances et faible empreinte mémoire (hébergement peu coûteux), typage strict des critères et des réponses, erreurs détectées à la compilation |
| Base | **PostgreSQL + PostGIS + pgvector** | Requêtes géospatiales (`ST_DWithin`, distances), plein texte français, vecteurs : un seul moteur |
| Ingestion | **Python (pandas)** | Écosystème data (GTFS, JSON-LD), scripts rapides à écrire et à relancer ; découplée de l'API par la base |
| Carte | **MapLibre GL + tuiles OSM** | Open source, sans clé, rendu GeoJSON performant |
| Client | **React + Vite** (web responsive) | Carte mise en avant sur grand écran, assistant en premier sur mobile (onglets) |
| LLM | **Toute API compatible OpenAI** | Le même code bascule entre Ollama (auto-hébergé) et une API cloud : la comparaison ne demande qu'un changement de configuration |

## Démarrage

Prérequis : Docker, Rust (stable), Node 20+, Python 3.11+.

```bash
cp .env.example .env

# 1. Base de données (le schéma db/init/01_schema.sql est appliqué au premier démarrage)
docker compose up -d db

# 2. Ingestion des données
cd ingestion
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
python run_all.py gtfs --gtfs <URL ou chemin du GTFS TER> --origins "Grenoble"
python run_all.py sncf
python run_all.py osm
python run_all.py datatourisme --datatourisme <archive ou dossier du flux>
python run_all.py stats
cd ..

# 3. API (port 8080)
cd backend && cargo run

# 4. Client (port 5173, relaie /api vers 8080)
cd frontend && npm install && npm run dev
```

### Données : où les trouver

| Source | Où | Remarques |
|---|---|---|
| GTFS TER | transport.data.gouv.fr → « Horaires des TER SNCF » → ressource GTFS | Télécharger le zip et passer son chemin (ou l'URL) à `--gtfs`. La journée type (jour de semaine le plus chargé) est choisie automatiquement. |
| SNCF Open Data | ressources.data.sncf.com | `python run_all.py sncf --inspect` affiche les champs disponibles. Ajuster `DATASETS` / `FIELDS` dans `sncf.py` si les noms ont changé. |
| OpenStreetMap | API Overpass publique | Environ 2 s de pause entre deux lots de gares. `--osm-limit 5` permet un test rapide. |
| API Géo (geo.api.gouv.fr) | Appelée par l'API au moment de la recherche | Localise une commune demandée comme destination (« pêcher à Herbeys »). Si elle n'a pas de gare, on cherche autour des gares à moins de 12 km. Gratuite, sans clé. |
| DATAtourisme | diffuseur.datatourisme.fr | Créer un compte, puis un flux « Auvergne-Rhône-Alpes », format JSON-LD (un fichier par objet). **Le faire tôt : la génération du flux prend du temps.** |

`--origins` accepte plusieurs gares, par exemple `"Grenoble,Lyon Part-Dieu"`. Les temps de trajet ne sont calculés que depuis ces gares.

### Brancher un LLM

```bash
# Auto-hébergé
docker compose --profile llm up -d ollama
docker compose exec ollama ollama pull mistral
# .env : LLM_BASE_URL=http://localhost:11434/v1  LLM_MODEL=mistral

# Cloud (exemple Mistral API ; tout fournisseur compatible OpenAI convient)
# .env : LLM_BASE_URL=https://api.mistral.ai/v1  LLM_MODEL=mistral-small-latest  LLM_API_KEY=...
```

## API

| Méthode | Route | Rôle |
|---|---|---|
| GET | `/api/health` | État et modèle LLM configuré |
| GET | `/api/stations?q=gren` | Recherche de gare |
| GET | `/api/stations/{id}?max_walk=30` | Gare + POI à proximité |
| GET | `/api/stations/reachable?from=Grenoble&max_minutes=90` | Gares accessibles depuis une origine |
| GET | `/api/pois?station_id=&max_walk=&tag=` | POI filtrés par catégorie et distance |
| GET | `/api/map/stations?origin=Grenoble` | GeoJSON des gares (avec temps de trajet) |
| GET | `/api/map/lines` | GeoJSON des lignes |
| GET | `/api/map/pois?bbox=&tag=` | GeoJSON des POI |
| POST | `/api/search` | Recherche à partir de critères structurés (sans IA) |
| POST | `/api/chat` | Assistant : `{message, session_id?, selected_station_id?}` |
| POST | `/api/chat/reset` | Oublie les critères d'une session |

## Tests et évaluation

```bash
cd backend && cargo test          # extraction de critères, scoring, garde-fous anti-hallucination
python eval/run_eval.py --label ollama-mistral   # API lancée avec la config à évaluer
```

`eval/run_eval.py` rejoue `eval/queries.json` (les requêtes du sujet, plus un cas sans résultat, une origine inconnue et une relance). Pour chaque requête, il mesure la compréhension des critères, le respect des contraintes, les hallucinations, les tokens consommés et la latence. Les résultats sont ajoutés à `eval/results.csv` pour comparer les configurations (règles / Ollama / cloud). Les tokens mesurés servent directement au calcul des coûts.

## Structure

```
backend/      API Rust
  src/domain/   critères (extraction par règles, fusion de session), scoring, modèles
  src/llm/      client compatible OpenAI, prompts, validation des réponses
  src/db.rs     requêtes SQL / PostGIS
  src/service.rs logique de recherche (origine, contraintes, assouplissement)
  src/routes/   endpoints HTTP
db/           Dockerfile (PostGIS + pgvector) et schéma
ingestion/    pipeline de données Python
frontend/     client React + MapLibre
eval/         jeu de requêtes et script d'évaluation
PLAN.md       plan de projet et checklist de validation
```
