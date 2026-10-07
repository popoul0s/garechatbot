# Plan pour valider le module : tourisme ferroviaire en Auvergne-Rhône-Alpes

## 0. Les infos faciles à rater (le « relisez le sujet »)

1. **Les groupes avec 3 AL sont notés plus sévèrement.** Si c'est votre cas, il faut une app plus aboutie (responsive soigné, tests, synchro carte ↔ IA dans les deux sens).
2. **Le LLM ne doit pas choisir les destinations.** C'est votre code (SQL + scoring) qui sélectionne et classe. Le LLM sert seulement à comprendre la demande et à rédiger l'explication.
3. **Pas de dépendance à une clé payante.** Apidae est facultatif. Une API LLM cloud reste acceptable pour le prototype, mais l'appli doit savoir fonctionner sans elle (par exemple en se rabattant sur un modèle local).
4. **Il faut comparer cloud et auto-hébergé avec des chiffres** : 3 scénarios de charge, coûts mensuels et annuels, TCO, et des hypothèses écrites noir sur blanc.
5. **Les critères restent en mémoire pendant la session.** Exemple : « et si je préfère quelque chose de culturel ? » doit réutiliser l'origine et la durée déjà données.
6. **La synchro va dans les deux sens** : un clic sur une gare de la carte devient le contexte du chat.
7. **Le comportement sans résultat est évalué.** L'appli doit le dire franchement et proposer d'assouplir un critère.
8. **Mardi matin, on note du travail réel**, pas des intentions. Démo de 10 minutes maximum.
9. **Le nombre de slides est contradictoire** : le sujet parle d'une « slide technique unique », puis de « 2 seules slides ». Prévoyez 2 slides et faites confirmer par l'enseignante.
10. **Chaque membre doit pouvoir expliquer tout le système**, y compris le code généré par IA.
11. **Le tableur des équipes** se remplit avec les noms exactement comme sur MyGes, en précisant l'option (AL ou IABD).
12. **Hors périmètre** : comptes, authentification, réservation, back-office, etc. N'y passez aucune minute.
13. **La vidéo Loom** : à regarder, elle contient sans doute des précisions.

---

## 1. Choix techniques recommandés (à justifier à l'oral)

| Brique | Choix | Justification |
|---|---|---|
| Format | **Option A : web responsive** | Une seule base de code ; carte sur grand écran, chat sur mobile |
| Frontend | React + Vite + **MapLibre GL** (ou Leaflet) | Gratuit, fonds OSM, couches GeoJSON |
| Backend | **Rust (axum + sqlx)** | Performant et sobre en ressources, typage strict des critères et réponses ; l'ingestion reste en Python |
| Base de données | **PostgreSQL + PostGIS + pgvector** | Géospatial (`ST_DWithin`), SQL et vecteurs dans un seul outil |
| Ingestion | Scripts Python (pandas) | Pipeline reproductible, découplé de l'API par la base |
| LLM cloud | Claude Haiku, GPT-4o-mini ou Mistral API | Pour l'extraction de critères et la rédaction |
| LLM local | **Ollama** + Mistral 7B ou Llama 3.1 8B | Sert à la comparaison et de solution de repli |
| Embeddings | Modèle multilingue open source (ex. `multilingual-e5-small`) | Gratuit, tourne sur CPU |
| Déploiement | docker-compose (db, api, front, ollama) | Démo reproductible |

## 2. Périmètre réduit

- **Origine principale : Grenoble.** Ajoutez Lyon si vous avez le temps.
- **Destinations** : les gares TER d'AURA à moins de 2h de Grenoble, soit environ 30 à 60 gares.
- **Points d'intérêt** : ceux situés à moins de 3 km de ces gares.

## 3. Pipeline de données (rôle principal : IABD, mais tout le monde y touche)

| Source | Ce qu'on récupère | Traitement |
|---|---|---|
| **SNCF Open Data** (liste des gares, équipements, accessibilité PMR) | Gares, coordonnées, services | Nettoyage, code UIC comme identifiant |
| **transport.data.gouv.fr : GTFS TER AURA** | Lignes, arrêts, horaires | Calcul du **temps de trajet minimal depuis Grenoble** (direct + 1 correspondance), stocké dans une table `travel_time(origin, station, minutes, nb_changes)` |
| **DATAtourisme** (flux AURA) | POI, patrimoine, activités, itinéraires | Normalisation des catégories (nature, culture, famille, rando…) |
| **OpenStreetMap** (Overpass) | Sentiers, aires de jeux, parcs, sommets, tracés des lignes | Complète DATAtourisme, fournit le tracé des voies |

> ⚠️ **À faire en tout premier lundi : créer le compte DATAtourisme et le flux.** La génération peut prendre du temps. Si elle n'arrive pas à temps, OSM sert de solution de repli.

Étapes à montrer :

**Ingestion → nettoyage → normalisation des catégories → rapprochement gare ↔ POI (distance PostGIS, temps de marche ≈ distance × 1,3 / 5 km/h) → stockage → API.**

Tables minimales :

- `stations`
- `lines`
- `travel_times`
- `pois` (catégorie, tags famille, difficulté, geom, description, embedding)
- `station_poi` (distance, minutes de marche)

## 4. API métier, indépendante du LLM (AL)

- `GET /stations?q=` : recherche d'une gare
- `GET /stations/{id}` : détail de la gare et de ses POI proches
- `GET /stations/reachable?from=grenoble&max_minutes=90` : gares accessibles depuis une origine
- `GET /pois?near_station=&max_walk=&category=` : POI filtrés
- `GET /map/layers` : GeoJSON des gares, lignes et POI
- `POST /search` : recherche structurée (critères JSON → résultats classés et scorés)
- `POST /chat` : l'assistant (il appelle `/search` en interne)

## 5. Chaîne IA (IABD en pilote, avec un AL en binôme)

```
Question (+ contexte de session : critères précédents, gare sélectionnée)
  ↓ 1. LLM : extraction des critères → JSON validé par Pydantic
       {origin, max_minutes, themes[], activity, difficulty, audience, max_walk_min}
  ↓ 2. Fusion avec les critères de session (mémoire de conversation)
  ↓ 3. Recherche structurée SQL/PostGIS (filtres durs : temps, marche)
     + recherche vectorielle pgvector (similarité thématique sur les descriptions de POI)
  ↓ 4. Scoring déterministe (Python) → top 3 à 5 destinations
  ↓ 5. LLM : rédaction, avec UNIQUEMENT les résultats fournis (id, nom, temps, POI)
  ↓ 6. Post-validation : chaque id ou nom cité doit exister dans le contexte, sinon on retire
  ↓ Réponse texte + liste d'ids → la carte zoome et met en surbrillance
```

### Exemple de formule de scoring (à présenter)

```
score = 0.35 × compatibilité_thème (tags + similarité vectorielle)
      + 0.25 × (1 − temps_trajet / temps_max)
      + 0.15 × (1 − marche / marche_max)
      + 0.15 × richesse_touristique (nb POI pertinents, normalisé)
      + 0.10 × accessibilité (direct sans correspondance, PMR)
```

Chaque résultat garde le détail de son score, ce qui rend la recommandation explicable.

### Limiter les hallucinations

- Le prompt système impose de n'utiliser que les données fournies.
- Les horaires et durées viennent de la base, jamais du LLM.
- Une validation vérifie les ids cités.
- Avec 0 résultat, l'appli répond « Aucune destination ne respecte X » et propose d'assouplir un critère.
- Si la gare de départ n'est pas citée (ni choisie), l'appli la demande toujours (« De quelle gare partez-vous ? ») puis relance la recherche : aucune gare n'est choisie d'office.

## 6. Frontend (AL)

- **Bureau** : carte en plein écran à gauche, chat ou recherche à droite.
- **Mobile** : chat par défaut, la carte s'ouvre dans un onglet ou un panneau.
- **Couches de la carte** : gares (couleur selon le temps de trajet), lignes, POI par catégorie, résultats mis en avant.
- **Popups** avec les infos principales, et un bouton « Utiliser comme contexte » qui envoie la gare au chat.
- **Panneau d'exploration** : gare → destination → liste de POI.
- **Cartes de recommandation** : nom, temps de trajet, minutes de marche, POI clés, explication, badges du score.
- **Chips des critères actifs** (« Grenoble · ≤ 90 min · nature · enfants »), modifiables, pour rendre la mémoire de session visible.

## 7. Jeu de tests IA (IABD + 1 AL)

Préparez au moins **10 requêtes**, dont les 6 du sujet, plus :

- un cas à 0 résultat ;
- un cas ambigu ;
- une relance conversationnelle (« et si plutôt culturel ? ») ;
- un cas avec une gare sélectionnée sur la carte.

Pour chaque requête, testée **avec le modèle cloud et le modèle local**, mesurez :

- la justesse des critères extraits (comparés à des valeurs attendues) ;
- le respect des contraintes ;
- la pertinence (note de 1 à 5) ;
- les hallucinations (oui/non) ;
- la latence.

Ces résultats nourrissent directement la comparaison cloud vs local. Un script `eval.py` qui sort un tableau CSV est un gros plus.

## 8. Analyse cloud vs auto-hébergé (IABD, relue par toute l'équipe)

**Hypothèses** : environ 2 appels LLM par requête (extraction + rédaction), soit environ 2 500 tokens en entrée et 400 en sortie au total. Mesurez les vrais chiffres dans vos logs.

**Coût cloud** = requêtes × (tokens entrée × prix entrée + tokens sortie × prix sortie), à calculer pour 5 000, 50 000 et 500 000 requêtes par mois. Prenez les prix sur les pages tarifs officielles du jour, avec la date et la source.

**Coût auto-hébergé** = instance GPU (ex. L4 ou A10 chez OVHcloud ou Scaleway, prix relevés), à multiplier pour la haute dispo et la montée en charge, + stockage + supervision + temps humain de maintenance (ex. 0,2 ETP). Le coût est presque fixe, quel que soit le volume.

Tableau à remplir pour chaque scénario :

| Élément | Cloud | Auto-hébergé |
|---|---|---|
| Modèle étudié | | |
| Volume étudié | | |
| Infrastructure | | |
| Coût mensuel estimé | | |
| Coût annuel estimé | | |
| TCO estimé (3 ans) | | |
| Solution retenue | | |

Tenez compte aussi des critères qualitatifs : qualité (issue de vos tests), latence, RGPD et souveraineté, maintenance, supervision.

**Recommandation probable** : une solution **hybride**. Cloud (ou Mistral API, hébergé en UE) au lancement, parce qu'il coûte quelques euros par mois à faible volume. Puis un modèle auto-hébergé ou un petit modèle spécialisé pour l'extraction quand le volume régional est atteint. Le seuil de rentabilité doit être chiffré.

## 9. Planning

### Lundi

- **Matin**
  - Remplir le tableur des équipes.
  - Fixer le stack et faire un schéma d'architecture.
  - Créer le compte et le flux DATAtourisme.
  - Mettre en place le dépôt, docker-compose et la base PostGIS.
  - Télécharger les données SNCF et GTFS.
- **Après-midi**
  - IABD : ingestion des gares, du GTFS et des POI, calcul des temps de trajet.
  - AL1 : API (stations, pois, reachable, layers).
  - AL2 : carte React avec gares et POI.
  - AL3 : endpoint `/chat` v0 (extraction des critères → `/search`).
- **Soir, objectif** : une chaîne complète même grossière, de la question jusqu'à des points sur la carte.

### Mardi matin : point d'avancement noté

- Démo du chemin données → carte → question → résultats.
- Montrer les tables remplies (nombre de lignes par source), le schéma d'architecture, le pipeline IA prévu et la formule de scoring.
- Chacun présente sa partie, et chacun sait expliquer le reste.

### Mardi après-midi

- Scoring complet, recherche vectorielle, mémoire de session, synchro carte → chat.
- Garde-fous anti-hallucinations, gestion du cas 0 résultat.
- Mise en place d'Ollama, lancement du jeu de tests sur les deux modèles.
- Responsive mobile.

### Mercredi matin

- Analyse des coûts finalisée.
- 2 slides (stack et sources réelles / solution IA et tableau comparatif chiffré).
- README, polish de l'interface.
- **Au moins 2 répétitions** de la démo de 15 minutes, avec un scénario figé et une vidéo de secours si le réseau ou l'API tombe.
- Questions blanches croisées : chaque membre interroge les autres sur la partie qu'il n'a pas codée.

### Mercredi après-midi

Soutenance.

## 10. Répartition des rôles

| Rôle | Responsable | Binôme de compréhension |
|---|---|---|
| Pipeline data + évaluation IA + coûts | IABD | AL3 |
| Backend, API, scoring, architecture | AL1 | IABD |
| Front, carte, responsive | AL2 | AL1 |
| Orchestration LLM, chat, session, Ollama | AL3 (si vous êtes 4) / AL1 (si vous êtes 3) | IABD |

## 11. Scénario de démo conseillé (15 minutes)

1. Pitch de 30 secondes.
2. **Explorer** : carte → ligne → gare → POI.
3. **Demander** : « Sortie nature à moins de 1h30 de Grenoble, randonnée facile, moins de 20 minutes de marche » → 3 recommandations argumentées, visibles sur la carte.
4. Relance : « Et si plutôt culturel ? » → la mémoire de session fonctionne.
5. Clic sur une gare → « Que faire ici avec des enfants ? ».
6. Cas impossible → réponse honnête.
7. Slides techniques et comparatif chiffré.
8. Questions.

## 12. Checklist de validation finale

- [ ] Données réelles issues d'au moins 3 sources, croisées entre elles
- [ ] Pipeline d'ingestion reproductible (un script, une commande)
- [ ] API métier utilisable sans LLM
- [ ] Carte avec gares, POI, lignes et résultats, plus popups
- [ ] Chat : extraction des critères → recherche → scoring → explication
- [ ] Classement fait par le code, pas par le LLM, avec un score explicable
- [ ] Mémoire de session et synchro carte ↔ chat
- [ ] Garde-fous anti-hallucinations et cas sans résultat
- [ ] Responsive bureau et mobile
- [ ] Jeu de tests sur les 2 modèles, avec résultats chiffrés
- [ ] Tableau des coûts sur 3 scénarios, avec hypothèses et recommandation
- [ ] 2 slides, démo répétée, chaque membre capable d'expliquer l'ensemble
