use anyhow::Context;
use jsonschema::JSONSchema;
use lazy_static::lazy_static;
use posix_cli_utils::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::PathBuf;
use tex_tools::*;

pub mod convert;
pub mod csl_fields;

type CslEntry = serde_json::Map<String, JsonValue>;

fn json_type_name(s: &JsonValue) -> &'static str {
    match s {
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
        JsonValue::Null => "null",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Bool(_) => "bool",
    }
}

trait JsonExt {
    fn unwrap_array(self) -> Vec<JsonValue>;
    fn unwrap_object(self) -> serde_json::Map<String, JsonValue>;
    fn unwrap_string(self) -> String;
    fn unwrap_str(&self) -> &str;

    fn expect_string(self) -> Result<String>;
    fn expect_object(self) -> Result<serde_json::Map<String, JsonValue>>;
    fn expect_array(self) -> Result<Vec<JsonValue>>;
    fn expect_number(self) -> Result<serde_json::Number>;
    fn expect_int(self) -> Result<i64>;
    fn expect_uint(self) -> Result<u64>;
}

impl JsonExt for JsonValue {
    #[track_caller]
    #[inline]
    fn unwrap_array(self) -> Vec<JsonValue> {
        match self {
            JsonValue::Array(arr) => arr,
            other => panic!("expected JSON array: {}", other),
        }
    }

    #[track_caller]
    #[inline]
    fn unwrap_object(self) -> serde_json::Map<String, JsonValue> {
        match self {
            JsonValue::Object(v) => v,
            other => panic!("expected JSON object: {}", other),
        }
    }

    #[track_caller]
    #[inline]
    fn unwrap_string(self) -> String {
        match self {
            JsonValue::String(v) => v,
            other => panic!("expected string: {}", other),
        }
    }

    #[track_caller]
    #[inline]
    fn unwrap_str(&self) -> &str {
        match self {
            JsonValue::String(v) => v.as_str(),
            other => panic!("expected string: {}", other),
        }
    }

    fn expect_string(self) -> Result<String> {
        match self {
            JsonValue::String(s) => Ok(s),
            other => bail!("expected JSON string, not {}", json_type_name(&other)),
        }
    }

    fn expect_object(self) -> Result<serde_json::Map<String, JsonValue>> {
        match self {
            JsonValue::Object(v) => Ok(v),
            other => bail!("expected JSON object, not {}", json_type_name(&other)),
        }
    }

    fn expect_array(self) -> Result<Vec<JsonValue>> {
        match self {
            JsonValue::Array(v) => Ok(v),
            other => bail!("expected JSON array, not {}", json_type_name(&other)),
        }
    }

    fn expect_number(self) -> Result<serde_json::Number> {
        match self {
            JsonValue::Number(v) => Ok(v),
            other => bail!("expected JSON number, not {}", json_type_name(&other)),
        }
    }

    fn expect_int(self) -> Result<i64> {
        let n = self.expect_number()?;
        n.as_i64()
            .ok_or_else(|| anyhow!("cannot convert to integer: {}", n))
    }

    fn expect_uint(self) -> Result<u64> {
        let n = self.expect_number()?;
        n.as_u64()
            .ok_or_else(|| anyhow!("cannot convert to unsigned integer: {}", n))
    }
}

lazy_static! {
    static ref CSL_ENTRY_SCHEMA: JSONSchema = compile_schema(include_str!("csl-entry-schema.json"));
    static ref BIB_DB_SCHEMA: JSONSchema = compile_schema(include_str!("bib_db-schema.json"));
}

fn compile_schema(s: &str) -> JSONSchema {
    let json: JsonValue = serde_json::from_str(s).expect("schema is not valid JSON");
    jsonschema::JSONSchema::options()
        .with_draft(jsonschema::Draft::Draft7)
        .compile(&json)
        .unwrap()
}

mod cache {
    use std::collections::HashMap;

    use super::*;

    fn cache_dir() -> Result<PathBuf> {
        let mut path =
            dirs::cache_dir().ok_or_else(|| anyhow!("failed to find user cache directory"))?;
        path.push("tex-tools");
        if !path.exists() {
            std::fs::create_dir(&path).context_create_dir(&path)?;
        }
        Ok(path)
    }

    fn fetch_cache() -> Result<PathBuf> {
        let mut path = cache_dir()?;
        path.push("fetch.json");
        Ok(path)
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct FetchCache(HashMap<String, CslEntry>);

    impl FetchCache {
        pub fn clear() -> Result<()> {
            let path = fetch_cache()?;
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            Ok(())
        }

        pub fn load() -> Result<Self> {
            // FIXME: could add compression here
            let path = fetch_cache()?;
            if path.exists() {
                std::fs::File::open(&path)
                    .context_read(&path)
                    .and_then(|f| serde_json::from_reader(f).context("corrupt JSON data"))
            } else {
                Ok(FetchCache::empty())
            }
        }

        pub fn save(&self) -> Result<()> {
            let path = fetch_cache()?;
            let f = std::fs::File::create(&path).context_write(&path)?;
            serde_json::to_writer(f, &self)?;
            Ok(())
        }

        pub fn empty() -> Self {
            FetchCache(Default::default())
        }

        pub fn get(&self, doi: &str) -> Option<&CslEntry> {
            self.0.get(doi)
        }

        pub fn contains(&self, doi: &str) -> bool {
            self.0.contains_key(doi)
        }

        pub fn insert(&mut self, doi: String, value: CslEntry) {
            self.0.insert(doi, value);
        }

        pub fn into_inner(self) -> HashMap<String, CslEntry> {
            self.0
        }
    }
}

mod example;
mod fetch;
mod validate;
mod output {}

#[derive(Parser)]
enum Cmd {
    /// Fetch missing information from doi.org
    Fetch(fetch::ClArgs),

    /// Validate the input database against the CSL schema
    Validate(validate::ClArgs),

    /// Validate the cache against the CSL schema
    ValidateCache(validate::ValidateCacheOptions),

    /// Empty the request cache
    ClearCache,

    /// Print an example database entry
    Example(example::ClArgs),
}

fn main() -> Result<()> {
    logging_init();

    match Cmd::parse() {
        Cmd::Validate(args) => validate::main(args),
        Cmd::ValidateCache(args) => validate::validate_cache(args),
        Cmd::Fetch(args) => fetch::main(args),
        Cmd::ClearCache => cache::FetchCache::clear(),
        Cmd::Example(args) => example::main(args),
    }
}
