use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::search::SearchIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Method,
    Type,
    Topic,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Method => "method",
            Kind::Type => "type",
            Kind::Topic => "topic",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Field {
    pub name: String,
    pub type_display: String,
    pub required: bool,
    pub description_md: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Method {
    pub id: String,
    pub description_md: String,
    pub fields: Vec<Field>,
    pub returns: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BotType {
    pub id: String,
    pub description_md: String,
    pub fields: Vec<Field>,
    pub variants: Vec<String>,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Topic {
    pub id: String,
    pub title: String,
    pub content_md: String,
    pub methods: Vec<String>,
    pub types: Vec<String>,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Entity {
    Method(Method),
    Type(BotType),
    Topic(Topic),
}

#[derive(Debug, Deserialize)]
pub struct Dataset {
    pub entities: Vec<Entity>,
}

/// Runtime dataset: parsed records plus derived indices (type_refs, used_by, search).
pub struct LoadedData {
    pub methods: Vec<Method>,
    pub types: Vec<BotType>,
    pub topics: Vec<Topic>,
    /// Derived type_refs per field, parallel to `methods[i].fields[j]`.
    pub method_field_refs: Vec<Vec<Vec<String>>>,
    /// Derived type_refs per field, parallel to `types[i].fields[j]`.
    pub type_field_refs: Vec<Vec<Vec<String>>>,
    /// Reverse index: type id -> `"owner_id.field_name"` entries, in document order.
    pub used_by: HashMap<String, Vec<String>>,
    pub search: SearchIndex,
    method_by_id: HashMap<String, usize>,
    type_by_id: HashMap<String, usize>,
    topic_by_id: HashMap<String, usize>,
}

impl LoadedData {
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read dataset file '{path}'"))?;
        let dataset: Dataset = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse dataset file '{path}'"))?;
        Self::from_dataset(dataset)
    }

    pub(crate) fn from_dataset(dataset: Dataset) -> Result<Self> {
        let mut methods = Vec::new();
        let mut types = Vec::new();
        let mut topics = Vec::new();
        let mut method_order = Vec::new();
        let mut type_order = Vec::new();
        let mut topic_order = Vec::new();
        let mut seen: HashMap<String, &'static str> = HashMap::new();

        for (doc_order, entity) in dataset.entities.into_iter().enumerate() {
            match entity {
                Entity::Method(m) => {
                    if let Some(prev) = seen.insert(m.id.to_ascii_lowercase(), "method") {
                        anyhow::bail!("duplicate id '{}' (already a {prev})", m.id);
                    }
                    methods.push(m);
                    method_order.push(doc_order);
                }
                Entity::Type(t) => {
                    if let Some(prev) = seen.insert(t.id.to_ascii_lowercase(), "type") {
                        anyhow::bail!("duplicate id '{}' (already a {prev})", t.id);
                    }
                    types.push(t);
                    type_order.push(doc_order);
                }
                Entity::Topic(t) => {
                    if let Some(prev) = seen.insert(t.id.to_ascii_lowercase(), "topic") {
                        anyhow::bail!("duplicate id '{}' (already a {prev})", t.id);
                    }
                    topics.push(t);
                    topic_order.push(doc_order);
                }
            }
        }

        let type_ids: HashSet<String> = types.iter().map(|t| t.id.clone()).collect();

        let mut used_by: HashMap<String, Vec<String>> = HashMap::new();
        let mut method_field_refs = Vec::with_capacity(methods.len());
        for m in &methods {
            let mut refs_per_field = Vec::with_capacity(m.fields.len());
            for f in &m.fields {
                let refs = derive_type_refs(&f.type_display, &type_ids);
                for r in &refs {
                    used_by
                        .entry(r.clone())
                        .or_default()
                        .push(format!("{}.{}", m.id, f.name));
                }
                refs_per_field.push(refs);
            }
            method_field_refs.push(refs_per_field);
        }
        let mut type_field_refs = Vec::with_capacity(types.len());
        for t in &types {
            let mut refs_per_field = Vec::with_capacity(t.fields.len());
            for f in &t.fields {
                let refs = derive_type_refs(&f.type_display, &type_ids);
                for r in &refs {
                    used_by
                        .entry(r.clone())
                        .or_default()
                        .push(format!("{}.{}", t.id, f.name));
                }
                refs_per_field.push(refs);
            }
            type_field_refs.push(refs_per_field);
        }
        for t in &types {
            used_by.entry(t.id.clone()).or_default();
        }

        let method_by_id = methods
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id.to_ascii_lowercase(), i))
            .collect();
        let type_by_id = types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.to_ascii_lowercase(), i))
            .collect();
        let topic_by_id = topics
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.to_ascii_lowercase(), i))
            .collect();

        for t in &topics {
            for ref_id in t.methods.iter().chain(t.types.iter()) {
                if !seen.contains_key(&ref_id.to_ascii_lowercase()) {
                    tracing::warn!(topic = %t.id, missing = %ref_id, "topic references unknown entity id");
                }
            }
        }

        let search = SearchIndex::build(
            &methods,
            &types,
            &topics,
            &method_order,
            &type_order,
            &topic_order,
        );

        Ok(LoadedData {
            methods,
            types,
            topics,
            method_field_refs,
            type_field_refs,
            used_by,
            search,
            method_by_id,
            type_by_id,
            topic_by_id,
        })
    }

    pub fn method(&self, id: &str) -> Option<(usize, &Method)> {
        self.method_by_id
            .get(&id.to_ascii_lowercase())
            .map(|&i| (i, &self.methods[i]))
    }

    pub fn bot_type(&self, id: &str) -> Option<(usize, &BotType)> {
        self.type_by_id
            .get(&id.to_ascii_lowercase())
            .map(|&i| (i, &self.types[i]))
    }

    pub fn topic(&self, id: &str) -> Option<&Topic> {
        self.topic_by_id
            .get(&id.to_ascii_lowercase())
            .map(|&i| &self.topics[i])
    }

    /// Exact case-insensitive match of any entity kind, for wrong-kind cross-suggestions.
    pub fn kind_of(&self, id: &str) -> Option<(Kind, &str)> {
        let id = id.to_ascii_lowercase();
        if let Some(&i) = self.method_by_id.get(&id) {
            Some((Kind::Method, &self.methods[i].id))
        } else if let Some(&i) = self.type_by_id.get(&id) {
            Some((Kind::Type, &self.types[i].id))
        } else if let Some(&i) = self.topic_by_id.get(&id) {
            Some((Kind::Topic, &self.topics[i].id))
        } else {
            None
        }
    }
}

/// Extract the type ids referenced by a verbatim `type_display` string.
///
/// The docs' type vocabulary is a sequence of capitalized identifiers joined by
/// "Array of", "or", "and", and commas (e.g. "Array of MessageEntity",
/// "InlineKeyboardMarkup or ReplyKeyboardMarkup or ..."). We collect every
/// `[A-Z][A-Za-z0-9]*` token that names a known type record, deduplicated in
/// order; primitives ("Integer", "String", "Boolean", "Float", "True") are not
/// records and are dropped.
fn derive_type_refs(type_display: &str, type_ids: &HashSet<String>) -> Vec<String> {
    let mut refs = Vec::new();
    let mut seen = HashSet::new();
    for ident in type_identifiers(type_display) {
        if type_ids.contains(ident) && seen.insert(ident) {
            refs.push(ident.to_string());
        }
    }
    refs
}

fn type_identifiers(s: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, (byte_idx, c)) in chars.iter().enumerate() {
        let prev_is_alnum = i > 0 && chars[i - 1].1.is_ascii_alphanumeric();
        if c.is_ascii_uppercase() && !prev_is_alnum {
            start = Some(*byte_idx);
        } else if !c.is_ascii_alphanumeric() {
            if let Some(st) = start.take() {
                out.push(&s[st..*byte_idx]);
            }
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

#[cfg(test)]
pub(crate) fn fixture_dataset() -> Dataset {
    serde_json::from_value(serde_json::json!({
        "entities": [
            {
                "id": "getting-updates",
                "kind": "topic",
                "title": "Getting updates",
                "content_md": "There are two complementary ways to receive updates.",
                "methods": ["getUpdates"],
                "types": ["Update"],
                "url": "https://core.telegram.org/bots/api#getting-updates"
            },
            {
                "id": "getUpdates",
                "kind": "method",
                "description_md": "Use this method to receive incoming updates using long polling. Returns an Array of Update.",
                "fields": [
                    {
                        "name": "allowed_updates",
                        "type_display": "Array of String",
                        "required": false,
                        "description_md": "A JSON-serialized list of the update types you want your bot to receive."
                    },
                    {
                        "name": "polling",
                        "type_display": "Integer or String",
                        "required": true,
                        "description_md": "Dummy field."
                    }
                ],
                "returns": "Array of Update",
                "url": "https://core.telegram.org/bots/api#getupdates"
            },
            {
                "id": "Update",
                "kind": "type",
                "description_md": "This object represents an incoming update.",
                "fields": [],
                "variants": [],
                "url": "https://core.telegram.org/bots/api#update"
            },
            {
                "id": "InlineKeyboardMarkup",
                "kind": "type",
                "description_md": "This object represents an inline keyboard that appears right next to the message it belongs to.",
                "fields": [
                    {
                        "name": "inline_keyboard",
                        "type_display": "Array of Array of InlineKeyboardButton",
                        "required": false,
                        "description_md": "Array of button rows."
                    }
                ],
                "variants": [],
                "url": "https://core.telegram.org/bots/api#inlinekeyboardmarkup"
            },
            {
                "id": "InlineKeyboardButton",
                "kind": "type",
                "description_md": "This object represents one button of an inline keyboard.",
                "fields": [],
                "variants": [],
                "url": "https://core.telegram.org/bots/api#inlinekeyboardbutton"
            },
            {
                "id": "ReplyKeyboardMarkup",
                "kind": "type",
                "description_md": "This object represents a custom keyboard with reply options.",
                "fields": [],
                "variants": [],
                "url": "https://core.telegram.org/bots/api#replykeyboardmarkup"
            },
            {
                "id": "ReplyKeyboardRemove",
                "kind": "type",
                "description_md": "Upon receiving a message with this object, Telegram clients will remove the current custom keyboard.",
                "fields": [],
                "variants": [],
                "url": "https://core.telegram.org/bots/api#replykeyboardremove"
            },
            {
                "id": "ForceReply",
                "kind": "type",
                "description_md": "Upon receiving a message with this object, Telegram clients will display a reply interface to the user.",
                "fields": [],
                "variants": [],
                "url": "https://core.telegram.org/bots/api#forcereply"
            },
            {
                "id": "sendMessage",
                "kind": "method",
                "description_md": "Use this method to send text messages. On success, the sent Message is returned.",
                "fields": [
                    {
                        "name": "reply_markup",
                        "type_display": "InlineKeyboardMarkup or ReplyKeyboardMarkup or ReplyKeyboardRemove or ForceReply",
                        "required": false,
                        "description_md": "Additional interface options."
                    },
                    {
                        "name": "chat_id",
                        "type_display": "Integer or String",
                        "required": true,
                        "description_md": "Unique identifier for the target chat."
                    }
                ],
                "returns": "Message",
                "url": "https://core.telegram.org/bots/api#sendmessage"
            },
            {
                "id": "sendPhoto",
                "kind": "method",
                "description_md": "Use this method to send photos. On success, the sent Message is returned.",
                "fields": [
                    {
                        "name": "photo",
                        "type_display": "InputFile or String",
                        "required": true,
                        "description_md": "Photo to send."
                    }
                ],
                "returns": "Message",
                "url": "https://core.telegram.org/bots/api#sendphoto"
            },
            {
                "id": "Message",
                "kind": "type",
                "description_md": "This object represents a message.",
                "fields": [],
                "variants": [],
                "url": "https://core.telegram.org/bots/api#message"
            },
            {
                "id": "InputFile",
                "kind": "type",
                "description_md": "This object represents the contents of a file to be uploaded.",
                "fields": [],
                "variants": [],
                "url": "https://core.telegram.org/bots/api#inputfile"
            }
        ]
    }))
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_partitions() {
        let loaded = LoadedData::from_dataset(fixture_dataset()).unwrap();
        assert_eq!(loaded.methods.len(), 3);
        assert_eq!(loaded.types.len(), 8);
        assert_eq!(loaded.topics.len(), 1);
        assert_eq!(loaded.methods[0].id, "getUpdates");
        assert_eq!(loaded.types[0].id, "Update");
    }

    #[test]
    fn duplicate_ids_fail() {
        let mut d = fixture_dataset();
        if let Entity::Method(m) = &mut d.entities[1] {
            m.id = "Update".to_string();
        }
        assert!(LoadedData::from_dataset(d).is_err());
    }

    #[test]
    fn case_insensitive_lookup() {
        let loaded = LoadedData::from_dataset(fixture_dataset()).unwrap();
        let (_, m) = loaded.method("SENDMESSAGE").unwrap();
        assert_eq!(m.id, "sendMessage");
        let (_, t) = loaded.bot_type("update").unwrap();
        assert_eq!(t.id, "Update");
        assert!(loaded.topic("GETTING-UPDATES").is_some());
        assert!(loaded.method("nope").is_none());
        assert_eq!(
            loaded.kind_of("message").map(|(k, id)| (k, id.to_string())),
            Some((Kind::Type, "Message".to_string()))
        );
    }

    #[test]
    fn type_refs_derivation() {
        let loaded = LoadedData::from_dataset(fixture_dataset()).unwrap();
        let (_, get_updates) = loaded.method("getupdates").unwrap();
        assert_eq!(get_updates.id, "getUpdates");
        let refs = &loaded.method_field_refs[0];
        assert_eq!(refs[0], Vec::<String>::new()); // Array of String
        assert_eq!(refs[1], Vec::<String>::new()); // Integer or String

        let (_, send_message) = loaded.method("sendmessage").unwrap();
        let refs = &loaded.method_field_refs[1];
        assert_eq!(
            refs[0],
            vec![
                "InlineKeyboardMarkup".to_string(),
                "ReplyKeyboardMarkup".to_string(),
                "ReplyKeyboardRemove".to_string(),
                "ForceReply".to_string()
            ]
        );
        assert_eq!(send_message.fields[0].name, "reply_markup");

        let (_, ikm) = loaded.bot_type("inlinekeyboardmarkup").unwrap();
        assert_eq!(
            loaded.type_field_refs[1],
            vec![vec!["InlineKeyboardButton".to_string()]]
        );
        assert_eq!(ikm.id, "InlineKeyboardMarkup");

        let (_, send_photo) = loaded.method("sendphoto").unwrap();
        assert_eq!(
            loaded.method_field_refs[2][0],
            vec!["InputFile".to_string()]
        );
        assert_eq!(send_photo.id, "sendPhoto");
    }

    #[test]
    fn used_by_reverse_index() {
        let loaded = LoadedData::from_dataset(fixture_dataset()).unwrap();
        assert_eq!(
            loaded.used_by.get("InlineKeyboardMarkup").unwrap(),
            &vec!["sendMessage.reply_markup".to_string()]
        );
        assert_eq!(
            loaded.used_by.get("InputFile").unwrap(),
            &vec!["sendPhoto.photo".to_string()]
        );
        // types without referrers have an empty entry
        assert_eq!(loaded.used_by.get("Update").unwrap(), &Vec::<String>::new());
    }

    #[test]
    fn topic_unknown_reference_warns_not_fails() {
        let mut d = fixture_dataset();
        if let Entity::Topic(t) = &mut d.entities[0] {
            t.methods.push("notARealMethod".to_string());
        }
        let loaded = LoadedData::from_dataset(d).unwrap();
        assert_eq!(loaded.methods.len(), 3);
    }

    #[test]
    fn identifiers_extraction() {
        assert_eq!(
            type_identifiers("Array of Array of InlineKeyboardButton"),
            vec!["Array", "Array", "InlineKeyboardButton"]
        );
        assert_eq!(
            type_identifiers(
                "InlineKeyboardMarkup or ReplyKeyboardMarkup or ReplyKeyboardRemove or ForceReply"
            ),
            vec![
                "InlineKeyboardMarkup",
                "ReplyKeyboardMarkup",
                "ReplyKeyboardRemove",
                "ForceReply"
            ]
        );
        assert_eq!(
            type_identifiers("Integer or String"),
            vec!["Integer", "String"]
        );
        assert_eq!(
            type_identifiers(
                "Array of InputMediaAudio, InputMediaDocument, InputMediaLivePhoto, InputMediaPhoto and InputMediaVideo"
            ),
            vec![
                "Array",
                "InputMediaAudio",
                "InputMediaDocument",
                "InputMediaLivePhoto",
                "InputMediaPhoto",
                "InputMediaVideo"
            ]
        );
        assert_eq!(type_identifiers("True"), vec!["True"]);
    }
}
