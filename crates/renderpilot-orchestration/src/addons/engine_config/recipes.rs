use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use serde::{Deserialize, Serialize};

use super::EngineIniError;
use super::document::digest;

/// INI encoding accepted by the shared editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IniEncoding {
    /// UTF-8 without a BOM, including ASCII-compatible byte content.
    Utf8,
    /// UTF-8 with a BOM.
    Utf8Bom,
    /// UTF-16 little endian with a BOM.
    Utf16Le,
    /// UTF-16 big endian with a BOM.
    Utf16Be,
}

/// A scalar section/key/value instruction authorized by a tool catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineIniEntry {
    /// INI section without brackets.
    pub section: String,
    /// INI key.
    pub key: String,
    /// Desired scalar value.
    pub value: String,
}

/// A versioned typed recipe.  `presentation_code` is intentionally absent:
/// display code is an adapter concern and can never become write authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineIniRecipe {
    /// Recipe wire format version.
    pub schema_version: u32,
    /// Semantic revision used in the deterministic recipe fingerprint.
    pub revision: u32,
    /// Stable catalog identity.
    pub id: String,
    /// Typed assignments.
    pub entries: Vec<EngineIniEntry>,
}

impl EngineIniRecipe {
    /// Creates a recipe after validating its closed scalar vocabulary.
    pub fn new(
        id: impl Into<String>,
        revision: u32,
        entries: Vec<EngineIniEntry>,
    ) -> Result<Self, EngineIniError> {
        let recipe = Self {
            schema_version: 1,
            revision,
            id: id.into(),
            entries,
        };
        recipe.validate()?;
        Ok(recipe)
    }

    /// Validates the closed recipe contract.
    pub fn validate(&self) -> Result<(), EngineIniError> {
        if self.schema_version != 1 {
            return Err(EngineIniError::InvalidRecipe(format!(
                "unsupported schema version {}",
                self.schema_version
            )));
        }
        if self.revision == 0 || self.id.trim().is_empty() {
            return Err(EngineIniError::InvalidRecipe(
                "recipe id and positive revision are required".to_owned(),
            ));
        }
        if self.entries.is_empty() {
            return Err(EngineIniError::InvalidRecipe(
                "recipe must contain at least one entry".to_owned(),
            ));
        }
        let mut seen = BTreeSet::new();
        for entry in &self.entries {
            if entry.section.trim().is_empty()
                || entry.key.trim().is_empty()
                || entry.key.contains(['=', '\r', '\n'])
                || entry.section.contains(['[', ']', '\r', '\n'])
                || entry.value.contains(['\r', '\n'])
            {
                return Err(EngineIniError::InvalidRecipe(format!(
                    "invalid scalar entry {} / {}",
                    entry.section, entry.key
                )));
            }
            let identity = (ascii_key(&entry.section), ascii_key(&entry.key));
            if !seen.insert(identity) {
                return Err(EngineIniError::InvalidRecipe(format!(
                    "duplicate target {} / {}",
                    entry.section, entry.key
                )));
            }
        }
        Ok(())
    }
}

/// A deduplicated set of typed recipes ready for application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineIniRecipeSet {
    entries: Vec<EngineIniEntry>,
    /// Stable identity/revision pairs included in this set.
    pub sources: Vec<EngineIniSource>,
    fingerprint: String,
}

/// Catalog provenance included in a recipe set fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct EngineIniSource {
    /// Stable catalog recipe id.
    pub id: String,
    /// Recipe semantic revision.
    pub revision: u32,
}

impl EngineIniRecipeSet {
    /// Materializes recipes, deduplicating equal values and rejecting semantic
    /// conflicts before any filesystem operation is possible.
    pub fn from_recipes<'a>(
        recipes: impl IntoIterator<Item = &'a EngineIniRecipe>,
    ) -> Result<Self, EngineIniError> {
        let mut values: BTreeMap<(String, String), EngineIniEntry> = BTreeMap::new();
        let mut sources = BTreeSet::new();
        for recipe in recipes {
            recipe.validate()?;
            sources.insert(EngineIniSource {
                id: recipe.id.clone(),
                revision: recipe.revision,
            });
            for entry in &recipe.entries {
                let identity = (ascii_key(&entry.section), ascii_key(&entry.key));
                match values.entry(identity) {
                    std::collections::btree_map::Entry::Occupied(existing) => {
                        if existing.get().value != entry.value {
                            return Err(EngineIniError::RecipeConflict {
                                section: entry.section.clone(),
                                key: entry.key.clone(),
                            });
                        }
                    }
                    std::collections::btree_map::Entry::Vacant(vacant) => {
                        vacant.insert(entry.clone());
                    }
                }
            }
        }
        let entries = values.into_values().collect::<Vec<_>>();
        let sources = sources.into_iter().collect::<Vec<_>>();
        let mut fingerprint_input = String::new();
        for source in &sources {
            fingerprint_input.push_str(&source.id);
            fingerprint_input.push('#');
            let _ = write!(fingerprint_input, "{}", source.revision);
            fingerprint_input.push('\n');
        }
        for entry in &entries {
            fingerprint_input.push_str(&ascii_key(&entry.section));
            fingerprint_input.push('\0');
            fingerprint_input.push_str(&ascii_key(&entry.key));
            fingerprint_input.push('\0');
            fingerprint_input.push_str(&entry.value);
            fingerprint_input.push('\n');
        }
        Ok(Self {
            entries,
            sources,
            fingerprint: digest(fingerprint_input.as_bytes()),
        })
    }

    /// Returns the deterministic set fingerprint.
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// Returns materialized entries in deterministic order.
    #[must_use]
    pub fn entries(&self) -> &[EngineIniEntry] {
        &self.entries
    }
}

pub(crate) fn ascii_key(value: &str) -> String {
    value
        .bytes()
        .map(|byte| byte.to_ascii_lowercase() as char)
        .collect()
}
