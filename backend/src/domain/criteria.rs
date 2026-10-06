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
    /// Lieu de destination nommé par l'utilisateur ("pêcher à Herbeys" -> "Herbeys").
    /// La recherche se limite alors aux gares proches de ce lieu.
    #[serde(default)]
    pub place: Option<String>,
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
        self.place = self.place.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        // NB : un lieu identique à l'origine reste valable (« que faire autour de la gare de Grenoble »)
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
            place: self.place.or_else(|| previous.place.clone()),
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
    ("eau", &["lac", "riviere", "cascade", "baignade", "plage", "gorges", "eau", "peche", "pecher", "pecheur"]),
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
/// Lieu de destination : "à Herbeys", "vers Annecy", "autour de Vienne", "près d'Aix-les-Bains".
static RE_PLACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\b(?:à|au|aux|vers|autour de|près de|du côté de|dans)\s+|\b(?:autour|près|côté) d['’])([A-ZÀ-Ý][\p{L}'\-]+(?:[ -](?:[A-ZÀ-Ý][\p{L}'\-]+|sur|en|les|le|la|de|du|d'))*)")
        .unwrap()
});

static RE_ORIGIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\b(?:depuis|au départ de|partant de|de)\s+|\bd['’])([A-ZÀ-Ý][\p{L}'\-]+(?:[ -](?:[A-ZÀ-Ý][\p{L}'\-]+|sur|en|les|le|la))*)")
        .unwrap()
});

static RE_GARE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgare (?:de |d'|du |des )?([a-z0-9'\- ]+)").unwrap());

/// Mots qui terminent un nom de gare dans une phrase ("gare de grenoble avec des enfants").
const NAME_STOP: &[&str] = &[
    "avec", "pour", "et", "en", "a", "au", "aux", "depuis", "ou", "qui", "que", "dans", "un", "une", "des", "je",
    "on", "nous", "moins", "plus", "sans", "si", "ce", "cette", "entre", "vers", "apres", "avant", "pres", "autour",
    "pas", "mais", "car", "puis", "svp", "stp",
];

/// Nom de gare cité après « gare de », sur le texte normalisé (minuscules sans accents), remis en forme.
fn station_mention(folded: &str) -> Option<String> {
    let cap = RE_GARE.captures(folded)?;
    let words: Vec<&str> = cap[1]
        .split_whitespace()
        .enumerate()
        .take_while(|(i, w)| *i == 0 && ["la", "le", "les"].contains(w) || !NAME_STOP.contains(w))
        .map(|(_, w)| w)
        .take(4)
        .collect();
    if words.is_empty() || words.iter().all(|w| ["la", "le", "les"].contains(w)) {
        return None;
    }
    // "saint-marcellin" -> "Saint-Marcellin" (l'affichage ; la recherche ignore casse et accents)
    let title = |w: &str| {
        w.split('-')
            .map(|p| {
                let mut c = p.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join("-")
    };
    Some(words.iter().map(|w| title(w)).collect::<Vec<_>>().join(" "))
}

/// Retire les petits mots de liaison capturés en fin de nom ("Aix-les-Bains de" -> "Aix-les-Bains").
fn clean_place_name(raw: &str) -> String {
    let mut words: Vec<&str> = raw.split_whitespace().collect();
    while words.last().is_some_and(|w| w.chars().next().is_some_and(char::is_lowercase)) {
        words.pop();
    }
    words.join(" ")
}

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
        // Durée de marche si : la même proposition commence par "marcher..." / "à pied..."
        // ("je ne veux pas marcher plus de 15 min"), ou la durée est immédiatement suivie de
        // "de marche" / "à pied" ("20 minutes de marche"). Une virgule sépare deux propositions :
        // dans "à moins d'1h30, peu de marche", 1h30 reste une durée de train.
        let before = &text[text.floor_char_boundary(start.saturating_sub(30))..start];
        let clause = before.rsplit([',', '.', ';']).next().unwrap_or(before);
        let after = text[end..].trim_start();
        let is_walk = ["march", "pied"].iter().any(|w| clause.contains(w))
            || ["de march", "a pied", "de pied", "de la gare", "de gare", "depuis la gare"]
                .iter()
                .any(|w| after.starts_with(w));
        if is_walk {
            c.max_walk_minutes.get_or_insert(minutes);
        } else {
            c.max_travel_minutes.get_or_insert(minutes);
        }
    }

    if c.max_walk_minutes.is_none()
        && ["peu de marche", "pas trop marcher", "pas beaucoup marcher", "sans marcher"]
            .iter()
            .any(|w| text.contains(w))
    {
        c.max_walk_minutes = Some(15);
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
    // Destination d'abord ; l'origine ne peut pas être le même nom ("près d'Aix" n'est pas un départ).
    let place = RE_PLACE.captures(message).and_then(|cap| cap.get(1));
    if let Some(m) = place {
        c.place = Some(clean_place_name(m.as_str()));
    }
    // « la gare de grenoble » : désigne explicitement une gare, même écrite en minuscules
    if let Some(name) = station_mention(&text) {
        c.place = Some(name);
    }
    let origin = RE_ORIGIN
        .captures_iter(message)
        .filter_map(|cap| cap.get(1))
        .find(|m| place.is_none_or(|p| m.start() != p.start()));
    if let Some(m) = origin {
        c.origin = Some(clean_place_name(m.as_str()));
    }
    // activités sans lieu touristique dédié dans nos thèmes : on les garde comme mots-clés
    if ["peche", "pecher", "pecheur"].iter().any(|w| has_word(&padded, w)) {
        c.keywords.push("pêche".into());
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
    fn la_marche_ne_deborde_pas_sur_la_duree_de_train() {
        let c = extract_with_rules("Une balade nature facile à moins d'1h30, peu de marche");
        assert_eq!(c.max_travel_minutes, Some(90));
        assert_eq!(c.max_walk_minutes, Some(15));

        let c = extract_with_rules("Moins de 20 min de marche, à moins de 1h de Grenoble");
        assert_eq!(c.max_walk_minutes, Some(20));
        assert_eq!(c.max_travel_minutes, Some(60));
    }

    #[test]
    fn destination_et_peche() {
        let c = extract_with_rules("Je veux aller pêcher à Herbeys");
        assert_eq!(c.place.as_deref(), Some("Herbeys"));
        assert_eq!(c.origin, None);
        assert!(c.themes.contains(&"eau".to_string()));
        assert_eq!(c.keywords, vec!["pêche".to_string()]);

        let c = extract_with_rules("Une rando près d'Aix-les-Bains depuis Lyon Part-Dieu");
        assert_eq!(c.place.as_deref(), Some("Aix-les-Bains"));
        assert_eq!(c.origin.as_deref(), Some("Lyon Part-Dieu"));

        // "moins de 1h30 de Grenoble" : Grenoble est l'origine, pas une destination
        let c = extract_with_rules("Une sortie nature à moins de 1h30 de Grenoble");
        assert_eq!(c.place, None);
    }

    #[test]
    fn minutes_de_la_gare_est_de_la_marche() {
        let c = extract_with_rules("je cherche activité nature a 5 minutes de la gare de grenoble");
        assert_eq!(c.max_walk_minutes, Some(5));
        assert_eq!(c.max_travel_minutes, None);
        assert_eq!(c.place.as_deref(), Some("Grenoble"));
        assert!(c.themes.contains(&"nature".to_string()));

        let c = extract_with_rules("Que faire près de la gare de saint-marcellin avec des enfants ?");
        assert_eq!(c.place.as_deref(), Some("Saint-Marcellin"));

        // origine et lieu identiques : on cherche autour de la gare de départ
        let c = extract_with_rules("Une balade autour de la gare de Grenoble depuis Grenoble");
        assert_eq!(c.place.as_deref(), Some("Grenoble"));
        assert_eq!(c.origin.as_deref(), Some("Grenoble"));
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
