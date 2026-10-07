//! Sélection et classement des destinations. Entièrement déterministe : le LLM n'intervient pas ici.
//!
//! score = (0.35 × envies + 0.25 × trajet + 0.15 × marche + 0.15 × richesse + 0.10 × accessibilité)
//!         × 0.85 par envie précise demandée mais absente (ex. « lac » sans aucun lac près de la gare)
//!
//! envies = part pondérée des envies couvertes par les lieux de la gare (voir `theme_weight`).

use std::collections::HashMap;

use super::criteria::Criteria;
use super::models::{CandidateRow, PoiHit, Recommendation, ScoreBreakdown, StationSummary};

pub const W_THEME: f64 = 0.35;
pub const W_TRAVEL: f64 = 0.25;
pub const W_WALK: f64 = 0.15;
pub const W_RICHNESS: f64 = 0.15;
pub const W_ACCESS: f64 = 0.10;

pub const DEFAULT_MAX_TRAVEL: i32 = 120;
pub const DEFAULT_MAX_WALK: i32 = 30;
const POIS_PER_RECOMMENDATION: usize = 5;
/// Pénalité par envie précise (poids 1 : lac, musée, patrimoine, montagne, famille) non couverte.
pub const MISSING_PRECISE_PENALTY: f64 = 0.85;
/// Note « trajet » de la gare de départ elle-même : neutre (ni bonus d'un trajet nul, ni pénalité).
pub const ON_SITE_TRAVEL_SCORE: f64 = 0.5;
/// Nombre de POI pertinents à partir duquel la richesse est maximale.
const RICHNESS_SATURATION: f64 = 5.0;

/// Poids d'un thème dans la note « correspond à vos envies » : une envie précise (lac, musée...)
/// compte plus qu'une envie vague (« nature » est porté par presque tous les lieux de plein air).
pub fn theme_weight(theme: &str) -> f64 {
    match theme {
        "nature" => 0.4,
        "randonnee" | "loisirs" => 0.7,
        "panorama" | "culture" => 0.8,
        _ => 1.0, // eau, montagne, patrimoine, musee, famille
    }
}

/// Part pondérée des envies couvertes par l'ensemble des lieux d'une gare (0..1).
/// Chaque envie est évaluée avec le lieu le plus intéressant qui la porte.
fn coverage(hits: &[PoiHit], wanted: &[String]) -> (f64, Vec<String>) {
    let total: f64 = wanted.iter().map(|t| theme_weight(t)).sum();
    let mut got = 0.0;
    let mut missing = Vec::new();
    for t in wanted {
        let best_interest = hits
            .iter()
            .filter(|h| h.poi.tags.contains(t))
            .map(|h| h.poi.interest)
            .fold(None, |acc: Option<f64>, i| Some(acc.map_or(i, |a| a.max(i))));
        match best_interest {
            Some(i) => got += theme_weight(t) * (0.4 + 0.6 * i),
            None => missing.push(t.clone()),
        }
    }
    (if total > 0.0 { got / total } else { 0.0 }, missing)
}

/// Lieux à mettre en avant : le meilleur lieu de chaque envie demandée d'abord (pour montrer
/// que la destination couvre bien « balade » ET « lac »), puis les autres par pertinence.
fn showcase(mut hits: Vec<PoiHit>, wanted: &[String], limit: usize) -> Vec<PoiHit> {
    let mut order: Vec<&String> = wanted.iter().collect();
    order.sort_by(|a, b| theme_weight(b).total_cmp(&theme_weight(a)));
    let mut out = Vec::new();
    for t in order {
        if out.iter().any(|h: &PoiHit| h.poi.tags.contains(t)) {
            continue;
        }
        if let Some(i) = hits.iter().position(|h| h.poi.tags.contains(t)) {
            out.push(hits.remove(i));
        }
    }
    out.extend(hits);
    out.truncate(limit);
    out
}

/// Pertinence d'un POI pour les critères : part des thèmes couverts, complétée par le plein texte.
fn poi_match(row: &CandidateRow, wanted: &[String], has_keywords: bool) -> f64 {
    let tag_part = if wanted.is_empty() {
        1.0
    } else {
        let covered = wanted.iter().filter(|t| row.poi.tags.contains(t)).count();
        covered as f64 / wanted.len() as f64
    };
    if !has_keywords {
        return tag_part;
    }
    // ts_rank est petit (~0.01–0.1) : on le ramène sur 0..1 avant de le combiner.
    let text_part = (row.text_rank as f64 * 10.0).min(1.0);
    (0.7 * tag_part + 0.3 * text_part).max(text_part.min(0.6))
}

/// Un lieu sans nom n'est retenu que s'il répond à une envie précise : une aire de jeux pour
/// une sortie en famille, un point de vue pour un panorama. "nature" seul ne suffit pas.
fn generic_wanted(tags: &[String], wanted: &[String]) -> bool {
    wanted.iter().any(|t| t != "nature" && tags.contains(t))
}

pub fn rank(rows: Vec<CandidateRow>, criteria: &Criteria, limit: usize) -> Vec<Recommendation> {
    let wanted = criteria.wanted_tags();
    let has_keywords = !criteria.keywords.is_empty();
    let max_travel = criteria.max_travel_minutes.unwrap_or(DEFAULT_MAX_TRAVEL).max(1) as f64;
    let max_walk = criteria.max_walk_minutes.unwrap_or(DEFAULT_MAX_WALK).max(1) as f64;

    let mut by_station: HashMap<i64, Vec<CandidateRow>> = HashMap::new();
    for row in rows {
        by_station.entry(row.station_id).or_default().push(row);
    }

    let mut recos: Vec<Recommendation> = by_station
        .into_values()
        .filter_map(|rows| {
            let first = rows.first()?.clone();
            let mut hits: Vec<PoiHit> = rows
                .into_iter()
                .filter(|r| !r.poi.generic || generic_wanted(&r.poi.tags, &wanted))
                .map(|r| {
                    let m = poi_match(&r, &wanted, has_keywords);
                    // un lac qui correspond vaut plus qu'un square qui correspond
                    let relevance = m * (0.4 + 0.6 * r.poi.interest);
                    PoiHit {
                        poi: r.poi,
                        walk_minutes: r.walk_minutes,
                        distance_m: r.distance_m,
                        climb_m: r.climb_m,
                        match_score: m,
                        relevance,
                    }
                })
                .filter(|h| h.match_score > 0.0)
                .collect();
            if hits.is_empty() {
                return None;
            }
            // un même nom ("Aire de jeux") ne compte qu'une fois : on garde le plus proche
            hits.sort_by_key(|h| h.walk_minutes);
            let mut seen = std::collections::HashSet::new();
            hits.retain(|h| seen.insert(h.poi.name.to_lowercase()));
            // lieux les plus pertinents d'abord, puis les plus proches
            hits.sort_by(|a, b| b.relevance.total_cmp(&a.relevance).then(a.walk_minutes.cmp(&b.walk_minutes)));

            let best = hits[0].relevance;
            let (covered, missing_themes) = coverage(&hits, &wanted);
            let strong: Vec<&PoiHit> = hits.iter().filter(|h| h.relevance >= best * 0.99).collect();
            let nearest_strong_walk = strong.iter().map(|h| h.walk_minutes).min().unwrap_or(0) as f64;
            // richesse pondérée par l'intérêt : 10 squares ne valent pas 10 sites majeurs
            let relevant: f64 = hits.iter().filter(|h| h.match_score >= 0.5).map(|h| h.poi.interest).sum();

            let travel_minutes = first.travel_minutes;
            let breakdown = ScoreBreakdown {
                // sans envie précise (mots-clés seuls), on garde la pertinence du meilleur lieu
                theme: if wanted.is_empty() { best } else { covered },
                // sur place (gare de départ) : note neutre, pour ne pas écraser les vraies sorties en train
                travel: match travel_minutes {
                    Some(0) => ON_SITE_TRAVEL_SCORE,
                    Some(m) => (1.0 - m as f64 / max_travel).clamp(0.0, 1.0),
                    None => 1.0,
                },
                walk: (1.0 - nearest_strong_walk / max_walk).clamp(0.0, 1.0),
                richness: (relevant / RICHNESS_SATURATION).min(1.0),
                accessibility: 0.5 * f64::from(u8::from(first.nb_changes.unwrap_or(0) == 0))
                    + 0.5 * f64::from(u8::from(first.station_pmr == Some(true))),
            };
            let missing_precise = missing_themes.iter().filter(|t| theme_weight(t) >= 1.0).count();
            let score = (W_THEME * breakdown.theme
                + W_TRAVEL * breakdown.travel
                + W_WALK * breakdown.walk
                + W_RICHNESS * breakdown.richness
                + W_ACCESS * breakdown.accessibility)
                * MISSING_PRECISE_PENALTY.powi(missing_precise as i32);

            let hits = showcase(hits, &wanted, POIS_PER_RECOMMENDATION);
            let station = StationSummary {
                id: first.station_id,
                name: first.station_name.clone(),
                city: first.station_city.clone(),
                lon: first.station_lon,
                lat: first.station_lat,
                pmr: first.station_pmr,
            };
            let facts = build_facts(&first, &hits);
            Some(Recommendation {
                station,
                travel_minutes,
                nb_changes: first.nb_changes,
                example_departure: first.example_departure.clone(),
                score: (score * 1000.0).round() / 1000.0,
                breakdown,
                pois: hits,
                facts,
                missing_themes,
            })
        })
        .collect();

    // Si des gares couvrent au moins une envie précise (lac, rando, musée...), on écarte celles
    // qui ne couvrent que l'envie vague « nature » : un square n'est pas une réponse à « balade lac ».
    let specific: Vec<&String> = wanted.iter().filter(|t| theme_weight(t) > theme_weight("nature")).collect();
    if !specific.is_empty() {
        let covers_specific = |r: &Recommendation| specific.iter().any(|t| !r.missing_themes.contains(t));
        if recos.iter().any(covers_specific) {
            recos.retain(covers_specific);
        }
    }
    recos.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.station.name.cmp(&b.station.name)));
    recos.truncate(limit);
    recos
}

/// Phrases factuelles, issues uniquement des données. Servent au gabarit de réponse
/// et sont transmises au LLM comme seule matière autorisée.
fn build_facts(row: &CandidateRow, hits: &[PoiHit]) -> Vec<String> {
    let mut facts = Vec::new();
    if row.travel_minutes == Some(0) {
        facts.push("Sur place : pas de train à prendre depuis votre gare de départ".into());
    } else if let Some(m) = row.travel_minutes {
        let changes = match row.nb_changes.unwrap_or(0) {
            0 => "direct".to_string(),
            1 => "1 correspondance".to_string(),
            n => format!("{n} correspondances"),
        };
        let dep = row
            .example_departure
            .as_deref()
            .map(|d| format!(", ex. départ théorique à {d}"))
            .unwrap_or_default();
        facts.push(format!("Trajet en train : {} ({changes}{dep})", format_minutes(m)));
    }
    if row.station_pmr == Some(true) {
        facts.push("Gare accessible PMR".into());
    }
    for h in hits.iter().take(3) {
        let tags = if h.poi.tags.is_empty() { String::new() } else { format!(" [{}]", h.poi.tags.join(", ")) };
        let climb = match h.climb_m {
            Some(c) if c >= 50 => format!(" (dont {c} m de montée)"),
            _ => String::new(),
        };
        facts.push(format!("{} à {} min à pied de la gare{climb}{tags}", h.poi.name, h.walk_minutes));
    }
    facts
}

pub fn format_minutes(m: i32) -> String {
    if m < 60 {
        format!("{m} min")
    } else if m % 60 == 0 {
        format!("{}h", m / 60)
    } else {
        format!("{}h{:02}", m / 60, m % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::Poi;

    fn row(station_id: i64, name: &str, travel: i32, poi_id: i64, tags: &[&str], walk: i32) -> CandidateRow {
        CandidateRow {
            station_id,
            station_name: name.into(),
            station_city: None,
            station_lon: 5.7,
            station_lat: 45.2,
            station_pmr: Some(true),
            travel_minutes: Some(travel),
            nb_changes: Some(0),
            example_departure: Some("08:00".into()),
            poi: Poi {
                id: poi_id,
                source: "osm".into(),
                name: format!("POI {poi_id}"),
                description: None,
                tags: tags.iter().map(|t| t.to_string()).collect(),
                url: None,
                lon: 5.7,
                lat: 45.2,
                interest: 1.0,
                generic: false,
            },
            walk_minutes: walk,
            distance_m: walk * 70,
            climb_m: None,
            text_rank: 0.0,
        }
    }

    #[test]
    fn la_destination_la_plus_proche_et_pertinente_gagne() {
        let criteria = Criteria {
            themes: vec!["nature".into(), "randonnee".into()],
            max_travel_minutes: Some(90),
            max_walk_minutes: Some(20),
            ..Default::default()
        };
        let rows = vec![
            row(1, "Proche", 30, 10, &["nature", "randonnee"], 5),
            row(2, "Lointaine", 85, 20, &["nature", "randonnee"], 5),
            row(3, "Hors theme", 20, 30, &["musee"], 2),
        ];
        let recos = rank(rows, &criteria, 3);
        assert_eq!(recos.len(), 2, "une gare sans POI pertinent est écartée");
        assert_eq!(recos[0].station.name, "Proche");
        assert!(recos[0].score > recos[1].score);
    }

    #[test]
    fn le_public_famille_compte_comme_un_theme() {
        let criteria = Criteria { audience: Some("famille".into()), themes: vec!["nature".into()], ..Default::default() };
        let rows = vec![
            row(1, "Sans famille", 30, 10, &["nature"], 5),
            row(2, "Avec famille", 30, 20, &["nature", "famille"], 5),
        ];
        let recos = rank(rows, &criteria, 3);
        assert_eq!(recos[0].station.name, "Avec famille");
        assert_eq!(recos[0].breakdown.theme, 1.0);
        // la gare sans rien pour les enfants ne couvre que « nature » (vague) : elle est écartée
        assert_eq!(recos.len(), 1);
    }

    #[test]
    fn une_balade_avec_lac_prefere_la_gare_qui_a_les_deux() {
        // « balader voir un lac » : randonnée + eau (+ nature)
        let criteria = Criteria { themes: vec!["randonnee".into(), "eau".into(), "nature".into()], ..Default::default() };
        let rows = vec![
            // gare A : sentiers et rochers, pas de lac, plus proche
            row(1, "Sans lac", 20, 10, &["randonnee", "nature"], 5),
            row(1, "Sans lac", 20, 11, &["montagne", "randonnee", "nature"], 8),
            // gare B : un lac et un sentier, un peu plus loin
            row(2, "Avec lac", 35, 20, &["eau", "nature"], 15),
            row(2, "Avec lac", 35, 21, &["randonnee", "nature"], 10),
            // gare C : un lac seul
            row(3, "Lac seul", 30, 30, &["eau", "nature"], 10),
        ];
        let recos = rank(rows, &criteria, 3);
        assert_eq!(recos[0].station.name, "Avec lac");
        assert!(recos.iter().position(|r| r.station.name == "Lac seul") < recos.iter().position(|r| r.station.name == "Sans lac"));
        let sans_lac = recos.iter().find(|r| r.station.name == "Sans lac").unwrap();
        assert_eq!(sans_lac.missing_themes, vec!["eau".to_string()]);
        // la carte de résultat montre d'abord le lac, puis le sentier
        assert!(recos[0].pois[0].poi.tags.contains(&"eau".to_string()));
    }

    #[test]
    fn les_doublons_de_nom_ne_gonflent_pas_la_richesse() {
        let mut rows: Vec<CandidateRow> = (0..8).map(|i| row(1, "Ville", 30, i, &["famille"], 5 + i as i32)).collect();
        for r in &mut rows {
            r.poi.name = "Aire de jeux".into();
        }
        let recos = rank(rows, &Criteria { audience: Some("famille".into()), ..Default::default() }, 1);
        assert_eq!(recos[0].pois.len(), 1);
        assert_eq!(recos[0].pois[0].walk_minutes, 5);
        assert!(recos[0].breakdown.richness <= 0.2);
    }

    #[test]
    fn un_site_majeur_passe_devant_un_square_plus_proche() {
        let criteria = Criteria { themes: vec!["nature".into()], ..Default::default() };
        let mut rows = vec![row(1, "Ville", 30, 10, &["nature", "famille"], 3), row(1, "Ville", 30, 11, &["nature", "eau"], 22)];
        rows[0].poi.name = "Square".into();
        rows[0].poi.interest = 0.35;
        rows[1].poi.name = "Lac".into();
        let recos = rank(rows, &criteria, 1);
        assert_eq!(recos[0].pois[0].poi.name, "Lac");
    }

    #[test]
    fn une_gare_qui_n_a_que_de_la_nature_est_ecartee() {
        let criteria = Criteria { themes: vec!["randonnee".into(), "eau".into()], ..Default::default() };
        let rows = vec![
            row(1, "Parc seulement", 5, 10, &["nature", "famille"], 2),
            row(2, "Lac", 40, 20, &["eau", "nature"], 10),
        ];
        // le parc ne couvre ni randonnée ni lac : il n'est pas proposé
        let recos = rank(rows, &criteria, 5);
        assert_eq!(recos.iter().map(|r| r.station.name.as_str()).collect::<Vec<_>>(), vec!["Lac"]);
    }

    #[test]
    fn la_gare_de_depart_est_proposee_sans_ecraser_les_sorties_en_train() {
        let criteria = Criteria { themes: vec!["eau".into()], ..Default::default() };
        let rows = vec![row(1, "Départ", 0, 10, &["eau"], 5), row(2, "En train", 30, 20, &["eau"], 5)];
        let recos = rank(rows, &criteria, 5);
        let depart = recos.iter().find(|r| r.station.name == "Départ").expect("la gare de départ est proposée");
        assert_eq!(depart.breakdown.travel, ON_SITE_TRAVEL_SCORE);
        assert!(depart.facts[0].starts_with("Sur place"));
    }

    #[test]
    fn un_lieu_sans_nom_n_apparait_que_s_il_est_demande() {
        let mut rows = vec![row(1, "Ville", 20, 10, &["panorama", "nature"], 5), row(2, "Lac", 30, 20, &["eau", "nature"], 5)];
        rows[0].poi.generic = true; // "Point de vue" sans nom en centre-ville
        let balade = Criteria { themes: vec!["nature".into()], ..Default::default() };
        let recos = rank(rows.clone(), &balade, 5);
        assert_eq!(recos.iter().map(|r| r.station.name.as_str()).collect::<Vec<_>>(), vec!["Lac"]);
        let panorama = Criteria { themes: vec!["panorama".into()], ..Default::default() };
        assert_eq!(rank(rows, &panorama, 5)[0].station.name, "Ville");
    }

    #[test]
    fn faits_generes_par_le_code() {
        let recos = rank(vec![row(1, "Gare", 75, 10, &["nature"], 12)], &Criteria::default(), 1);
        assert!(recos[0].facts[0].contains("1h15"));
        assert!(recos[0].facts.iter().any(|f| f.contains("POI 10 à 12 min à pied")));
    }
}
