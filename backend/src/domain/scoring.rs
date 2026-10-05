//! Sélection et classement des destinations. Entièrement déterministe : le LLM n'intervient pas ici.
//!
//! score = 0.35 × thème + 0.25 × trajet + 0.15 × marche + 0.15 × richesse + 0.10 × accessibilité

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
/// Nombre de POI pertinents à partir duquel la richesse est maximale.
const RICHNESS_SATURATION: f64 = 5.0;

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
                .map(|r| {
                    let m = poi_match(&r, &wanted, has_keywords);
                    PoiHit { poi: r.poi, walk_minutes: r.walk_minutes, distance_m: r.distance_m, match_score: m }
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
            // meilleurs POI d'abord, puis les plus proches
            hits.sort_by(|a, b| {
                b.match_score
                    .total_cmp(&a.match_score)
                    .then(a.walk_minutes.cmp(&b.walk_minutes))
            });

            let best_match = hits[0].match_score;
            let strong: Vec<&PoiHit> = hits.iter().filter(|h| h.match_score >= best_match * 0.99).collect();
            let nearest_strong_walk = strong.iter().map(|h| h.walk_minutes).min().unwrap_or(0) as f64;
            let relevant = hits.iter().filter(|h| h.match_score >= 0.5).count() as f64;

            let travel_minutes = first.travel_minutes;
            let breakdown = ScoreBreakdown {
                theme: best_match,
                travel: travel_minutes
                    .map(|m| (1.0 - m as f64 / max_travel).clamp(0.0, 1.0))
                    .unwrap_or(1.0),
                walk: (1.0 - nearest_strong_walk / max_walk).clamp(0.0, 1.0),
                richness: (relevant / RICHNESS_SATURATION).min(1.0),
                accessibility: 0.5 * f64::from(u8::from(first.nb_changes.unwrap_or(0) == 0))
                    + 0.5 * f64::from(u8::from(first.station_pmr == Some(true))),
            };
            let score = W_THEME * breakdown.theme
                + W_TRAVEL * breakdown.travel
                + W_WALK * breakdown.walk
                + W_RICHNESS * breakdown.richness
                + W_ACCESS * breakdown.accessibility;

            hits.truncate(POIS_PER_RECOMMENDATION);
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
            })
        })
        .collect();

    recos.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.station.name.cmp(&b.station.name)));
    recos.truncate(limit);
    recos
}

/// Phrases factuelles, issues uniquement des données. Servent au gabarit de réponse
/// et sont transmises au LLM comme seule matière autorisée.
fn build_facts(row: &CandidateRow, hits: &[PoiHit]) -> Vec<String> {
    let mut facts = Vec::new();
    if let Some(m) = row.travel_minutes {
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
        facts.push(format!("{} à {} min à pied de la gare{tags}", h.poi.name, h.walk_minutes));
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
            },
            walk_minutes: walk,
            distance_m: walk * 70,
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
        assert_eq!(recos[1].breakdown.theme, 0.5);
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
    fn faits_generes_par_le_code() {
        let recos = rank(vec![row(1, "Gare", 75, 10, &["nature"], 12)], &Criteria::default(), 1);
        assert!(recos[0].facts[0].contains("1h15"));
        assert!(recos[0].facts.iter().any(|f| f.contains("POI 10 à 12 min à pied")));
    }
}
