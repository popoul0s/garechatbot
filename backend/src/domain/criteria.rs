//! Critères de recherche structurés, extraits d'une question en langage naturel.
//!
//! Deux extracteurs produisent ce même format :
//! - le LLM (`llm::extract_criteria`), principal ;
//! - `extract_with_rules`, utilisé quand aucun LLM n'est configuré ou que sa réponse est invalide.

use deunicode::deunicode;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Vocabulaire de thèmes partagé avec l'ingestion (colonne `pois.tags`).
pub const THEMES: &[&str] = &[
    "nature", "randonnee", "montagne", "eau", "culture", "patrimoine", "musee", "famille",
    "loisirs", "panorama",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Criteria {
    /// Ville ou gare de départ, telle qu'exprimée par l'utilisateur.
    pub origin: Option<String>,
    pub max_travel_minutes: Option<i32>,
    #[serde(default)]
    pub themes: Vec<String>,
    /// "famille" quand la demande mentionne des enfants.
    pub audience: Option<String>,
    pub max_walk_minutes: Option<i32>,
    /// "facile" | "moyen" | "difficile"
    pub difficulty: Option<String>,
    /// Mots-clés libres utilisés en recherche plein texte (ex. "lac", "chateau").
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Gare sélectionnée sur la carte : la recherche se limite alors à ses alentours.
    pub around_station_id: Option<i64>,
}

impl Criteria {
    /// Nettoie des critères venant du LLM : thèmes hors vocabulaire, valeurs aberrantes.
    pub fn sanitized(mut self) -> Self {
        self.themes = normalize_themes(&self.themes);
        self.keywords = self
            .keywords
            .into_iter()
            .map(|k| k.trim().to_lowercase())
            .filter(|k| k.len() > 2)
            .take(6)
            .collect();
        self.max_travel_minutes = self.max_travel_minutes.filter(|m| (5..=600).contains(m));
        self.max_walk_minutes = self.max_walk_minutes.filter(|m| (1..=120).contains(m));
        self.audience = self.audience.filter(|a| a == "famille");
        self.difficulty = self
            .difficulty
            .map(|d| fold(&d))
            .filter(|d| ["facile", "moyen", "difficile"].contains(&d.as_str()));
        self.origin = self.origin.map(|o| o.trim().to_string()).filter(|o| !o.is_empty());
        self
    }

    /// Fusionne les critères d'un nouveau message avec ceux de la session.
    /// Une valeur nouvellement exprimée remplace l'ancienne ; sinon l'ancienne est conservée.
    /// Les thèmes sont remplacés en bloc ("et si plutôt culturel ?" => thèmes = [culture]).
    pub fn merged_onto(self, previous: &Criteria) -> Criteria {
        let themes_changed = !self.themes.is_empty();
        Criteria {
            origin: self.origin.or_else(|| previous.origin.clone()),
            max_travel_minutes: self.max_travel_minutes.or(previous.max_travel_minutes),
            themes: if themes_changed { self.themes } else { previous.themes.clone() },
            audience: self.audience.or_else(|| previous.audience.clone()),
            max_walk_minutes: self.max_walk_minutes.or(previous.max_walk_minutes),
            difficulty: self.difficulty.or_else(|| previous.difficulty.clone()),
            keywords: if !self.keywords.is_empty() {
                self.keywords
            } else if themes_changed {
                // un changement de thème rend les anciens mots-clés obsolètes
                Vec::new()
            } else {
                previous.keywords.clone()
            },
            around_station_id: self.around_station_id.or(previous.around_station_id),
        }
    }

    /// Thèmes effectivement recherchés (le public "famille" devient un thème à satisfaire).
    pub fn wanted_tags(&self) -> Vec<String> {
        let mut tags = self.themes.clone();
        if self.audience.as_deref() == Some("famille") && !tags.iter().any(|t| t == "famille") {
            tags.push("famille".into());
        }
        tags
    }
}

/// Minuscules + suppression des accents, pour comparer du texte libre.
pub fn fold(s: &str) -> String {
    deunicode(s).to_lowercase()
}

pub fn normalize_themes(themes: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in themes {
        let f = fold(t);
        let mapped = THEMES
            .iter()
            .find(|v| **v == f)
            .map(|v| v.to_string())
            .or_else(|| theme_from_word(&f).map(str::to_string));
        if let Some(m) = mapped {
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    out
}

/// Synonymes -> thème normalisé. Utilisé par l'extracteur à règles et pour nettoyer la sortie du LLM.
const THEME_SYNONYMS: &[(&str, &[&str])] = &[
    ("randonnee", &["randonnee", "rando", "balade", "promenade", "sentier", "marche en nature", "trek"]),
    ("montagne", &["montagne", "sommet", "alpage", "massif", "station de ski", "ski"]),
    ("nature", &["nature", "naturel", "foret", "bois", "parc naturel", "plein air", "campagne", "verdure"]),
    ("eau", &["lac", "riviere", "cascade", "baignade", "plage", "gorges", "eau"]),
    ("culture", &["culture", "culturel", "culturelle", "spectacle", "art", "exposition", "theatre"]),
    ("patrimoine", &["patrimoine", "historique", "histoire", "chateau", "eglise", "abbaye", "monument", "vieille ville", "medieval"]),
    ("musee", &["musee", "musees"]),
    ("loisirs", &["loisir", "loisirs", "activite ludique", "parc d'attraction", "accrobranche"]),
    ("panorama", &["panorama", "point de vue", "belvedere", "vue"]),
];

/// Texte réduit à des mots séparés par des espaces, encadré d'espaces, pour une recherche
/// "début de mot" : `has_word` trouve "enfant" dans "enfants" mais pas "eau" dans "chateau".
fn padded_words(folded: &str) -> String {
    let words: String = folded
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    format!(" {} ", words.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn has_word(padded: &str, word: &str) -> bool {
    padded.contains(&format!(" {word}"))
}

fn theme_from_word(folded: &str) -> Option<&'static str> {
    let padded = padded_words(folded);
    THEME_SYNONYMS
        .iter()
        .find(|(_, words)| words.iter().any(|w| has_word(&padded, w)))
        .map(|(theme, _)| *theme)
}

static RE_HM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{1,2})\s*h(?:eures?)?\s*(\d{1,2})?").unwrap());
static RE_MIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{1,3})\s*(?:min|minutes?|mn)\b").unwrap());
static RE_WORD_HOURS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(une|un|deux|trois|quatre)\s+heures?(\s+et\s+demie)?|\bdemi[- ]heure").unwrap()
});
static RE_ORIGIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\b(?:depuis|au départ de|partant de|de)\s+|\bd['’])([A-ZÀ-Ý][\p{L}'\-]+(?:[ -](?:[A-ZÀ-Ý][\p{L}'\-]+|sur|en|les|le|la))*)")
        .unwrap()
});

/// Extraction des critères par règles (sans LLM). Volontairement simple et explicable.
pub fn extract_with_rules(message: &str) -> Criteria {
    let text = fold(message);
    let padded = padded_words(&text);
    let mut c = Criteria::default();

    // Durées : on regarde le contexte autour du nombre pour savoir s'il s'agit de marche ou de train.
    let mut durations: Vec<(usize, usize, i32)> = Vec::new();
    for cap in RE_HM.captures_iter(&text) {
        let m = cap.get(0).unwrap();
        let h: i32 = cap[1].parse().unwrap_or(0);
        let min: i32 = cap.get(2).and_then(|x| x.as_str().parse().ok()).unwrap_or(0);
        durations.push((m.start(), m.end(), h * 60 + min));
    }
    for cap in RE_MIN.captures_iter(&text) {
        let m = cap.get(0).unwrap();
        if durations.iter().any(|(s, e, _)| m.start() < *e && m.end() > *s) {
            continue;
        }
        durations.push((m.start(), m.end(), cap[1].parse().unwrap_or(0)));
    }
    for cap in RE_WORD_HOURS.captures_iter(&text) {
        let m = cap.get(0).unwrap();
        let minutes = match cap.get(1).map(|x| x.as_str()) {
            None => 30,
            Some("un") | Some("une") => 60,
            Some("deux") => 120,
            Some("trois") => 180,
            Some(_) => 240,
        } + if cap.get(2).is_some() { 30 } else { 0 };
        durations.push((m.start(), m.end(), minutes));
    }
    for (start, end, minutes) in durations {
        let before = &text[text.floor_char_boundary(start.saturating_sub(30))..start];
        let after = &text[end..text.ceil_char_boundary((end + 20).min(text.len()))];
        let is_walk = ["march", "pied", "a pied"].iter().any(|w| before.contains(w) || after.contains(w));
        if is_walk {
            c.max_walk_minutes.get_or_insert(minutes);
        } else {
            c.max_travel_minutes.get_or_insert(minutes);
        }
    }

    for (theme, words) in THEME_SYNONYMS {
        if words.iter().any(|w| has_word(&padded, w)) {
            c.themes.push(theme.to_string());
        }
    }
    // "randonnée" implique un contexte nature
    if c.themes.iter().any(|t| t == "randonnee") && !c.themes.iter().any(|t| t == "nature") {
        c.themes.push("nature".into());
    }

    if ["enfant", "famille", "gamin", "petits", "bambin", "ados", "adolescent"].iter().any(|w| has_word(&padded, w)) {
        c.audience = Some("famille".into());
    }

    if ["facile", "debutant", "tranquille", "pas trop dur", "accessible a tous"].iter().any(|w| has_word(&padded, w)) {
        c.difficulty = Some("facile".into());
    } else if ["difficile", "sportif", "engage", "exigeant"].iter().any(|w| has_word(&padded, w)) {
        c.difficulty = Some("difficile".into());
    }

    // Origine : premier nom propre précédé de "de/depuis..." (le message original garde les majuscules).
    if let Some(cap) = RE_ORIGIN.captures(message) {
        c.origin = Some(cap[1].trim().to_string());
    }

    c.sanitized()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exemple_du_sujet() {
        let c = extract_with_rules(
            "Je cherche une sortie nature à moins de 1h30 de Grenoble, avec une randonnée facile et moins de 20 minutes de marche depuis la gare.",
        );
        assert_eq!(c.origin.as_deref(), Some("Grenoble"));
        assert_eq!(c.max_travel_minutes, Some(90));
        assert_eq!(c.max_walk_minutes, Some(20));
        assert_eq!(c.difficulty.as_deref(), Some("facile"));
        assert!(c.themes.contains(&"nature".to_string()));
        assert!(c.themes.contains(&"randonnee".to_string()));
    }

    #[test]
    fn enfants_et_heures_en_lettres() {
        let c = extract_with_rules(
            "Je voudrais passer une journée à la montagne sans voiture, à moins de deux heures de Grenoble, avec une activité adaptée à des enfants.",
        );
        assert_eq!(c.max_travel_minutes, Some(120));
        assert_eq!(c.audience.as_deref(), Some("famille"));
        assert!(c.themes.contains(&"montagne".to_string()));
        assert_eq!(c.origin.as_deref(), Some("Grenoble"));
    }

    #[test]
    fn marche_avant_le_nombre() {
        let c = extract_with_rules("Je ne veux pas marcher plus de 15 minutes après la gare.");
        assert_eq!(c.max_walk_minutes, Some(15));
        assert_eq!(c.max_travel_minutes, None);
    }

    #[test]
    fn fusion_de_session() {
        let first = extract_with_rules("Une sortie nature à moins d'une heure de Grenoble");
        assert_eq!(first.max_travel_minutes, Some(60));
        let second = extract_with_rules("Et si je préfère finalement quelque chose de culturel ?");
        let merged = second.merged_onto(&first);
        assert_eq!(merged.origin.as_deref(), Some("Grenoble"));
        assert_eq!(merged.max_travel_minutes, Some(60));
        assert_eq!(merged.themes, vec!["culture".to_string()]);
    }

    #[test]
    fn nettoyage_sortie_llm() {
        let c = Criteria {
            themes: vec!["Randonnée".into(), "inconnu".into(), "Château".into()],
            max_travel_minutes: Some(99999),
            difficulty: Some("Facile".into()),
            ..Default::default()
        }
        .sanitized();
        assert_eq!(c.themes, vec!["randonnee".to_string(), "patrimoine".to_string()]);
        assert_eq!(c.max_travel_minutes, None);
        assert_eq!(c.difficulty.as_deref(), Some("facile"));
    }

    #[test]
    fn pas_de_faux_positif_dans_les_mots() {
        // "chateau" contient "eau", "Grande" se termine par "de" : aucun des deux ne doit compter.
        let c = extract_with_rules("Visiter un château dans la Grande Chartreuse");
        assert_eq!(c.themes, vec!["patrimoine".to_string()]);
        assert_eq!(c.origin, None);
    }
}
