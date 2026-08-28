use std::collections::HashMap;

use crate::data::{BotType, Kind, Method, Topic};

const NAME_EXACT: u32 = 100;
const FIELD_EXACT: u32 = 90;
const NAME_PREFIX: u32 = 80;
const FIELD_PREFIX: u32 = 75;
const NAME_SUBSTRING: u32 = 70;
const FIELD_SUBSTRING: u32 = 65;
const NAME_FUZZY: u32 = 60;
const FIELD_FUZZY: u32 = 55;
const PROSE_PER_TOKEN: u32 = 40;

const MAX_RESULTS: usize = 20;
const SNIPPET_LEN: usize = 200;

/// Hand-built synonyms: query phrases mapped to doc-vocabulary expansions.
/// Keep small and maintain by hand; this is not an ontology.
const SYNONYMS: &[(&str, &[&str])] = &[
    ("dm", &["direct messages"]),
    ("authorize", &["authorizing"]),
    ("auth", &["authorizing"]),
    ("kick", &["ban"]),
    ("inline keyboard", &["inlinekeyboardmarkup", "reply markup"]),
    ("reply markup", &["inlinekeyboardmarkup", "reply keyboard"]),
    ("send a photo", &["sendphoto", "photo"]),
    ("send photo", &["sendphoto", "photo"]),
    ("media group", &["sendmediagroup"]),
    ("webhook", &["setwebhook"]),
    ("poll", &["poll", "quiz"]),
];

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub id: String,
    pub kind: &'static str,
    pub title: String,
    pub snippet: String,
}

struct Entry {
    id: String,
    id_lower: String,
    kind: Kind,
    title: String,
    title_lower: String,
    text_lower: String,
    paragraphs: Vec<String>,
    doc_order: usize,
}

struct FieldEntry {
    field: String,
    field_lower: String,
    owner_id: String,
    owner_kind: Kind,
    owner_desc: String,
    doc_order: usize,
}

struct Candidate {
    score: u32,
    doc_order: usize,
    title: String,
    snippet: String,
}

pub struct SearchIndex {
    entries: Vec<Entry>,
    fields: Vec<FieldEntry>,
}

impl SearchIndex {
    pub fn build(
        methods: &[Method],
        types: &[BotType],
        topics: &[Topic],
        method_order: &[usize],
        type_order: &[usize],
        topic_order: &[usize],
    ) -> Self {
        let mut entries = Vec::new();
        let mut fields = Vec::new();

        for (m, &order) in methods.iter().zip(method_order) {
            entries.push(Entry {
                id: m.id.clone(),
                id_lower: m.id.to_ascii_lowercase(),
                kind: Kind::Method,
                title: m.id.clone(),
                title_lower: m.id.to_ascii_lowercase(),
                text_lower: m.description_md.to_ascii_lowercase(),
                paragraphs: split_paragraphs(&m.description_md),
                doc_order: order,
            });
            for f in &m.fields {
                fields.push(FieldEntry {
                    field: f.name.clone(),
                    field_lower: f.name.to_ascii_lowercase(),
                    owner_id: m.id.clone(),
                    owner_kind: Kind::Method,
                    owner_desc: m.description_md.clone(),
                    doc_order: order,
                });
            }
        }
        for (t, &order) in types.iter().zip(type_order) {
            entries.push(Entry {
                id: t.id.clone(),
                id_lower: t.id.to_ascii_lowercase(),
                kind: Kind::Type,
                title: t.id.clone(),
                title_lower: t.id.to_ascii_lowercase(),
                text_lower: t.description_md.to_ascii_lowercase(),
                paragraphs: split_paragraphs(&t.description_md),
                doc_order: order,
            });
            for f in &t.fields {
                fields.push(FieldEntry {
                    field: f.name.clone(),
                    field_lower: f.name.to_ascii_lowercase(),
                    owner_id: t.id.clone(),
                    owner_kind: Kind::Type,
                    owner_desc: t.description_md.clone(),
                    doc_order: order,
                });
            }
        }
        for (t, &order) in topics.iter().zip(topic_order) {
            entries.push(Entry {
                id: t.id.clone(),
                id_lower: t.id.to_ascii_lowercase(),
                kind: Kind::Topic,
                title: t.title.clone(),
                title_lower: t.title.to_ascii_lowercase(),
                text_lower: t.content_md.to_ascii_lowercase(),
                paragraphs: split_paragraphs(&t.content_md),
                doc_order: order,
            });
        }

        SearchIndex { entries, fields }
    }

    /// Ranked mix of methods, types, and topics for a free-text query.
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let q = normalize(query);
        let tokens = expand_synonyms(&q);

        let mut best: HashMap<(Kind, String), Candidate> = HashMap::new();

        for e in &self.entries {
            let mut score: u32 = 0;
            let mut snippet: Option<String> = None;

            let matched = matched_token_count(&tokens, &e.text_lower);
            if matched > 0 {
                score = PROSE_PER_TOKEN * matched as u32;
                snippet = Some(matching_paragraph(&e.paragraphs, &tokens));
            }

            if e.id_lower == q || e.title_lower == q {
                score = score.max(NAME_EXACT);
            } else if e.id_lower.starts_with(&q) || e.title_lower.starts_with(&q) {
                score = score.max(NAME_PREFIX);
            } else if e.id_lower.contains(&q) || e.title_lower.contains(&q) {
                score = score.max(NAME_SUBSTRING);
            } else if fuzzy_matchable(q.len(), e.id_lower.len())
                && lev_distance(&e.id_lower, &q) <= fuzzy_threshold(q.len(), e.id_lower.len())
            {
                score = score.max(NAME_FUZZY);
            }

            if score > 0 {
                let fallback = truncate(
                    &e.paragraphs.first().cloned().unwrap_or_default(),
                    SNIPPET_LEN,
                );
                insert(
                    &mut best,
                    e.kind,
                    &e.id,
                    Candidate {
                        score,
                        doc_order: e.doc_order,
                        title: e.title.clone(),
                        snippet: snippet.unwrap_or(fallback),
                    },
                );
            }
        }

        for f in &self.fields {
            let score = if f.field_lower == q {
                FIELD_EXACT
            } else if f.field_lower.starts_with(&q) {
                FIELD_PREFIX
            } else if f.field_lower.contains(&q) {
                FIELD_SUBSTRING
            } else if fuzzy_matchable(q.len(), f.field_lower.len())
                && lev_distance(&f.field_lower, &q) <= fuzzy_threshold(q.len(), f.field_lower.len())
            {
                FIELD_FUZZY
            } else {
                continue;
            };
            let snippet = truncate(
                &format!(
                    "field {} of {}: {}",
                    f.field,
                    f.owner_id,
                    first_sentence(&f.owner_desc)
                ),
                SNIPPET_LEN,
            );
            insert(
                &mut best,
                f.owner_kind,
                &f.owner_id,
                Candidate {
                    score,
                    doc_order: f.doc_order,
                    title: f.owner_id.clone(),
                    snippet,
                },
            );
        }

        let mut ranked: Vec<((Kind, String), Candidate)> = best.into_iter().collect();
        ranked.sort_by(|a, b| {
            b.1.score
                .cmp(&a.1.score)
                .then_with(|| a.1.doc_order.cmp(&b.1.doc_order))
        });
        ranked
            .into_iter()
            .take(MAX_RESULTS)
            .map(|((kind, id), c)| SearchResult {
                id,
                kind: kind.as_str(),
                title: c.title,
                snippet: c.snippet,
            })
            .collect()
    }
}

fn insert(best: &mut HashMap<(Kind, String), Candidate>, kind: Kind, id: &str, cand: Candidate) {
    let key = (kind, id.to_string());
    let better = match best.get(&key) {
        Some(existing) => {
            cand.score > existing.score
                || (cand.score == existing.score && cand.doc_order < existing.doc_order)
        }
        None => true,
    };
    if better {
        best.insert(key, cand);
    }
}

/// Fuzzy id suggestions for "did you mean" errors on unknown ids.
pub fn fuzzy_candidates<'a, I>(ids: I, input: &str, max: usize) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let q = normalize(input);
    if q.is_empty() {
        return Vec::new();
    }
    let mut cands: Vec<(usize, String)> = ids
        .into_iter()
        .map(|id| {
            let lower = id.to_ascii_lowercase();
            (lev_distance(&lower, &q), id.to_string())
        })
        .filter(|(dist, id)| *dist <= fuzzy_threshold(q.len(), id.len()))
        .collect();
    cands.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    cands.into_iter().take(max).map(|(_, id)| id).collect()
}

fn normalize(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

fn expand_synonyms(q: &str) -> Vec<String> {
    let tokens = tokenize(q);
    let mut out = tokens.clone();
    for (key, expansions) in SYNONYMS {
        if q.contains(key) || tokens.iter().any(|t| t == key) {
            for e in *expansions {
                for t in tokenize(e) {
                    if !out.contains(&t) {
                        out.push(t);
                    }
                }
            }
        }
    }
    out
}

fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

fn matched_token_count(tokens: &[String], text_lower: &str) -> usize {
    tokens
        .iter()
        .filter(|t| text_contains_token(text_lower, t))
        .count()
}

fn text_contains_token(text_lower: &str, token: &str) -> bool {
    text_lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| token_matches_word(w, token))
}

fn singular(w: &str) -> Option<String> {
    if let Some(rest) = w.strip_suffix("ies") {
        Some(format!("{rest}y"))
    } else if let Some(rest) = w.strip_suffix('s') {
        Some(rest.to_string())
    } else {
        None
    }
}

fn token_matches_word(word: &str, token: &str) -> bool {
    if word == token {
        return true;
    }
    singular(word).as_deref() == Some(token) || singular(token).as_deref() == Some(word)
}

fn split_paragraphs(text: &str) -> Vec<String> {
    let parts: Vec<String> = text
        .split("\n\n")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        vec![text.trim().to_string()]
    } else {
        parts
    }
}

fn matching_paragraph(paragraphs: &[String], tokens: &[String]) -> String {
    let hit = paragraphs.iter().find(|p| {
        let lower = p.to_ascii_lowercase();
        tokens.iter().any(|t| text_contains_token(&lower, t))
    });
    truncate(hit.unwrap_or(&paragraphs[0]), SNIPPET_LEN)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

fn first_sentence(s: &str) -> String {
    let cut = s.find(". ").map(|i| i + 2).unwrap_or(s.len());
    s[..cut.min(s.len())].to_string()
}

fn fuzzy_matchable(q_len: usize, id_len: usize) -> bool {
    let min = q_len.min(id_len);
    min >= 3
}

fn fuzzy_threshold(q_len: usize, id_len: usize) -> usize {
    if q_len <= 4 || id_len <= 4 { 1 } else { 2 }
}

/// Classic iterative Levenshtein distance.
fn lev_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::LoadedData;

    fn index() -> SearchIndex {
        let loaded = LoadedData::from_dataset(crate::data::fixture_dataset()).unwrap();
        let orders: Vec<usize> = (0..loaded.methods.len()).collect();
        let ty_orders: Vec<usize> = (0..loaded.types.len()).collect();
        let tp_orders: Vec<usize> = (0..loaded.topics.len()).collect();
        SearchIndex::build(
            &loaded.methods,
            &loaded.types,
            &loaded.topics,
            &orders,
            &ty_orders,
            &tp_orders,
        )
    }

    #[test]
    fn exact_id_wins() {
        let results = index().search("sendMessage");
        assert_eq!(results[0].id, "sendMessage");
        assert_eq!(results[0].kind, "method");
    }

    #[test]
    fn case_insensitive_exact() {
        let results = index().search("SENDMESSAGE");
        assert_eq!(results[0].id, "sendMessage");
    }

    #[test]
    fn fuzzy_id() {
        let results = index().search("sendmesage");
        assert_eq!(results[0].id, "sendMessage");
    }

    #[test]
    fn field_name_reverse_lookup() {
        let results = index().search("reply_markup");
        assert_eq!(results[0].id, "sendMessage");
        assert_eq!(results[0].kind, "method");
        assert!(results[0].snippet.starts_with("field"));
    }

    #[test]
    fn prose_query() {
        let results = index().search("send a photo");
        let top = &results[0];
        assert_eq!(top.id, "sendPhoto");
        assert_eq!(top.kind, "method");
    }

    #[test]
    fn synonym_inline_keyboard() {
        let results = index().search("inline keyboard");
        assert!(results.iter().any(|r| r.id == "InlineKeyboardMarkup"));
    }

    #[test]
    fn topic_title_or_content_hit() {
        let results = index().search("updates");
        assert!(
            results
                .iter()
                .any(|r| r.id == "getUpdates" || r.id == "getting-updates")
        );
    }

    #[test]
    fn kind_mixing() {
        let results = index().search("message");
        let kinds: std::collections::HashSet<&str> = results.iter().map(|r| r.kind).collect();
        assert!(kinds.contains("type")); // Message type
        assert!(kinds.contains("method")); // sendMessage / sendPhoto mention it
    }

    #[test]
    fn no_match_is_empty() {
        assert!(index().search("zzzqqqnnn").is_empty());
    }

    #[test]
    fn fuzzy_candidates_offer() {
        let ids = ["sendMessage", "sendPhoto", "getUpdates"];
        assert_eq!(
            fuzzy_candidates(ids.iter().copied(), "sendmesage", 3),
            vec!["sendMessage"]
        );
        assert_eq!(
            fuzzy_candidates(ids.iter().copied(), "sendpotos", 3),
            vec!["sendPhoto"]
        );
    }
}
