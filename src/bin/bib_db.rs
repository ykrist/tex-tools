use anyhow::Context;
use jsonschema::JSONSchema;
use lazy_static::lazy_static;
use posix_cli_utils::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::PathBuf;
use tex_tools::*;

type CslEntry = serde_json::Map<String, JsonValue>;

trait JsonExt {
    fn unwrap_array(self) -> Vec<JsonValue>;
    fn unwrap_object(self) -> serde_json::Map<String, JsonValue>;

    fn unwrap_string(self) -> String;
    fn unwrap_str(&self) -> &str;
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

mod fetch {
    use std::time::Duration;
    use std::{num::NonZeroU32, path::Path};

    use super::*;
    use futures::future;
    use governor as gv;
    use reqwest::{header, Client, Response};

    type RateLimiter = gv::RateLimiter<
        gv::state::NotKeyed,
        gv::state::InMemoryState,
        gv::clock::DefaultClock,
        gv::middleware::NoOpMiddleware,
    >;

    fn pop_print_isbn(entry: &mut CslEntry) -> Option<JsonValue> {
        let kinds = match entry.remove("isbn-type")? {
            JsonValue::Array(a) => a,
            _ => return None,
        };

        for val in kinds {
            let mut val = match val {
                JsonValue::Object(m) => m,
                _ => continue,
            };

            if val.get("type").map(JsonValue::as_str).flatten() == Some("print") {
                return val.remove("value");
            }
        }
        None
    }

    fn clean_json(entry: &mut JsonValue) {
        let entry = match entry.as_object_mut() {
            Some(e) => e,
            None => return,
        };

        if let Some(isbn) = pop_print_isbn(entry) {
            entry.insert("ISBN".into(), isbn);
        }

        for drop_field in [
            "abstract",
            "alternative-id",
            "article-number",
            "assertion",
            "content-domain",
            "copywrite",
            "copyright",
            "created",
            "deposited",
            "funder",
            "indexed",
            "is-referenced-by-count",
            "ISSN",
            "journal-issue",
            "license",
            "link",
            "member",
            "prefix",
            "published-online",
            "published-print",
            "published",
            "publisher-location",
            "reference-count",
            "reference",
            "references-count",
            "relation",
            "resource",
            "score",
            "short-title",
            "subject",
            "subtitle",
            "update-policy",
        ] {
            entry.remove(drop_field);
        }

        for drop_if_empty in ["original-title"] {
            let v = match entry.get(drop_if_empty) {
                Some(v) => v,
                None => continue,
            };

            let drop = match v {
                JsonValue::Array(arr) => arr.is_empty(),
                JsonValue::Object(map) => map.is_empty(),
                JsonValue::Null => true,
                _ => false,
            };

            if drop {
                entry.remove(drop_if_empty);
            }
        }

        let ty = entry.remove_entry("type");
        if let Some((ty_field, ty)) = ty {
            let mut new_ty = None;

            if let Some(ty) = ty.as_str() {
                match ty {
                    "book-chapter" => {
                        new_ty = Some("book");
                    }
                    "journal-article" => {
                        new_ty = Some("article-journal");
                    }
                    "proceedings-article" => {
                        new_ty = Some("paper-conference");
                    }
                    _ => {}
                }
            }

            if let Some(new_ty) = new_ty {
                entry.insert(ty_field, new_ty.into());
            } else {
                entry.insert(ty_field, ty);
            }
        };

        if entry.get("type").map(JsonValue::as_str).flatten() == Some("book-chapter") {
            entry.insert("type".into(), "book".into());
        }

        if let Some(authors) = entry.get_mut("author").map(|a| a.as_array_mut()).flatten() {
            for author in authors {
                clean_name_fields(author);
            }
        }

        // if let Some(vals @ JsonValue::Array(_)) = entry.get_mut("ISSN") {
        //     if vals.as_array().unwrap().len() == 1 {
        //         let value = vals.take().unwrap_array().into_iter().next().unwrap();
        //         *vals = value;
        //     }
        // }
    }

    fn clean_name_fields(author: &mut JsonValue) {
        if let Some(fields) = author.as_object_mut() {
            for drop_field in ["ORCID", "authenticated-orcid", "sequence", "affiliation"] {
                fields.remove(drop_field);
            }
        }
    }

    #[instrument(level = "error", name = "fetch", skip(client, rl), fields(doi=%doi, url))]
    async fn fetch_one<'a>(client: &Client, rl: &RateLimiter, doi: &'a str) -> Option<JsonValue> {
        let url = format!("https://doi.org/{}", urlencoding::Encoded(doi));
        tracing::span::Span::current().record("url", &tracing::field::display(&url));

        rl.until_ready_with_jitter(gv::Jitter::up_to(Duration::from_millis(200)))
            .await;
        info!("GET");
        let resp = match client
            .get(url)
            .send()
            .await
            .and_then(Response::error_for_status)
        {
            Ok(resp) => resp,
            Err(err) => {
                if let Some(status) = err.status() {
                    error!(%status, %err, "HTTP error")
                } else {
                    error!(%err, "Failed to send request")
                }
                return None;
            }
        };

        match resp.json().await {
            Ok(json) => Some(json),
            Err(err) => {
                error!(%err, "invalid JSON");
                None
            }
        }
    }

    pub fn fetch_and_validate<'a>(
        options: &ClArgs,
        dois: impl IntoIterator<Item = &'a str>,
        dump_raw: Option<impl AsRef<Path>>,
    ) -> Result<Vec<(&'a str, JsonValue)>> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            "application/vnd.citationstyles.csl+json".parse().unwrap(),
        );
        debug!(?headers);
        let client = Client::builder().default_headers(headers).build()?;

        let rl = RateLimiter::direct(gv::Quota::per_second(
            NonZeroU32::new(options.max_requests_per_sec).unwrap(),
        ));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .worker_threads(1)
            .enable_time()
            .enable_io()
            .build()?;

        let tasks = dois.into_iter().map(|doi| {
            let client = &client;
            let rl = &rl;
            async move { (doi, fetch_one(&client, &rl, &doi).await) }
        });

        let fetch_results = runtime.block_on(future::join_all(tasks));
        drop(runtime);

        let mut results = Vec::with_capacity(fetch_results.len());
        let count_total = fetch_results.len();

        let mut raw = if dump_raw.is_some() {
            Some(Vec::with_capacity(fetch_results.len()))
        } else {
            None
        };

        for (doi, json) in fetch_results {
            let mut json = match json {
                Some(v) => v,
                None => continue,
            };

            if let Some(raw) = &mut raw {
                raw.push(json.clone());
            }

            clean_json(&mut json);
            let _s = error_span!("validate", doi).entered();

            if !validate::validate_entry(&json, validate::ignore_missing_id) {
                continue;
            }

            results.push((doi, json))
        }

        if let Some(path) = dump_raw {
            write_json_pretty(path, raw.unwrap())?;
        }

        info!(
            count_total,
            count_successful = results.len(),
            "fetch complete"
        );
        Ok(results)
    }

    fn merge_one(entry: &mut CslEntry, new: &CslEntry) {
        for (field, val) in new {
            if !entry.contains_key(field) {
                entry.insert(field.clone(), val.clone());
            }
        }
    }

    pub fn fetch_and_merge(options: &ClArgs, db: &mut Vec<CslEntry>) -> Result<()> {
        let mut cache = cache::FetchCache::load()?;

        let to_fetch: Vec<_> = db
            .iter()
            .filter_map(|e| e.get("DOI").map(JsonExt::unwrap_str))
            .filter(|doi| !cache.contains(doi))
            .collect();
        let count = to_fetch.len();

        if count > 0 {
            info!(count, "retrieving entries");

            for (doi, json) in fetch_and_validate(options, to_fetch, options.dump_raw.as_ref())? {
                cache.insert(doi.to_string(), json.unwrap_object());
            }
            cache.save()?;
        } else {
            info!("all entries found in cache")
        }

        for e in db {
            if let Some(doi) = e.get("DOI") {
                if let Some(update) = cache.get(doi.unwrap_str()) {
                    merge_one(e, update);
                }
            }
        }

        Ok(())
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, ArgEnum)]
    pub enum OutputFormat {
        Json,
    }

    impl OutputFormat {
        pub fn suffix(&self) -> &'static str {
            match self {
                OutputFormat::Json => "json",
            }
        }
    }

    #[derive(Args)]
    pub struct ClArgs {
        /// Input file (CSL JSON format)
        input: PathBuf,

        /// Maximum number of requests allowed per second.
        #[clap(short = 'r', default_value_t = 20)]
        max_requests_per_sec: u32,

        /// Dump the raw JSON retrieved, prior to cleaning
        #[clap(long, value_name = "PATH")]
        dump_raw: Option<PathBuf>,

        /// Output format
        #[clap(arg_enum, short='f', default_value_t=OutputFormat::Json)]
        format: OutputFormat,

        /// Output path. Default is same name as input file with "-filled" appended to the filename stem.
        #[clap(short = 'o')]
        output: Option<PathBuf>,
    }

    fn output_json(db: &Vec<CslEntry>, path: Option<impl AsRef<Path>>) -> Result<()> {
        if let Some(path) = path {
            write_json_pretty(path, &db)
        } else {
            let out = std::io::stdout();
            serde_json::to_writer_pretty(out.lock(), &db)?;
            Ok(())
        }
    }

    pub fn main(mut args: ClArgs) -> Result<()> {
        args.max_requests_per_sec = args.max_requests_per_sec.max(1);
        let mut db: Vec<_> = validate::load_and_validate_db(&args.input)?
            .into_iter()
            .map(JsonExt::unwrap_object)
            .collect();
        info!(n_entries = db.len(), "DB read successfully");

        fetch_and_merge(&args, &mut db)?;

        let output_file = args.output.take().unwrap_or_else(|| {
            let mut n = args.input.file_stem().expect("no file name").to_os_string();
            n.push("-filled.");
            n.push(args.format.suffix());
            args.input.with_file_name(n)
        });

        match args.format {
            OutputFormat::Json => output_json(&db, Some(output_file))?,
        }

        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn clean() -> Result<()> {
            logging_init_test();
            let raw: Vec<JsonValue> = read_json("tests/raw-fetch.json")?;
            let mut ok = true;
            for mut entry in raw {
                clean_json(&mut entry);
                ok &= validate::validate_entry(&entry, validate::ignore_missing_id);
            }
            assert!(ok);
            Ok(())
        }
    }
}

mod validate {
    use std::path::Path;

    use super::*;
    use jsonschema::{error::ValidationErrorKind, ValidationError};
    use serde_json::Value;

    #[derive(Args)]
    pub struct ClArgs {
        /// Input file (CSL JSON format)
        input: PathBuf,
    }

    fn try_find_id<'a>(entry: &'a Value) -> Option<&'a Value> {
        entry.as_object()?.get("id")
    }

    pub fn ignore_missing_type(e: &ValidationError) -> bool {
        match &e.kind {
            ValidationErrorKind::Required { property } => property.as_str() == Some("type"),
            _ => false,
        }
    }

    pub fn ignore_missing_id(e: &ValidationError) -> bool {
        match &e.kind {
            ValidationErrorKind::Required { property } => property.as_str() == Some("id"),
            _ => false,
        }
    }

    pub fn validate_entry(entry: &JsonValue, ignore: impl Fn(&ValidationError) -> bool) -> bool {
        let errors = CSL_ENTRY_SCHEMA.validate(entry).err();

        let mut errs = false;
        if let Some(errors) = errors {
            let id = try_find_id(&entry);

            for error in errors.filter(|e| !ignore(e)) {
                let span = error_span!(
                    "validate_entry",
                    id = tracing::field::Empty,
                    json_path = tracing::field::Empty,
                );
                if let Some(id) = id {
                    span.record("id", &tracing::field::display(id));
                };
                if error.instance_path.iter().next().is_some() {
                    span.record("json_path", &tracing::field::display(&error.instance_path));
                }
                let _s = span.enter();
                error!("{}", error);
                errs = true;
            }
        }

        !errs
    }
    const EXIT_ERROR_MSG: &str = "Validation failed";

    pub fn load_and_validate_db(path: impl AsRef<Path>) -> Result<Vec<JsonValue>> {
        let db = match read_json::<JsonValue, _>(path)? {
            JsonValue::Array(db) => db,
            _ => {
                error!("top-level JSON value must be an array.");
                bail!("{}", EXIT_ERROR_MSG)
            }
        };

        let mut is_ok = true;
        for (entry_index, entry) in db.iter().enumerate() {
            let _s = error_span!("validate_db", entry_index).entered();
            is_ok &= validate_entry(&entry, ignore_missing_type);
        }
        if !is_ok {
            bail!("{}", EXIT_ERROR_MSG)
        }
        Ok(db)
    }

    pub fn main(args: ClArgs) -> Result<()> {
        load_and_validate_db(args.input)?;
        Ok(())
    }

    #[derive(Args)]
    pub struct ValidateCacheOptions {
        /// Save the invalid entries to a JSON file.
        #[clap(short)]
        output: Option<PathBuf>,
    }

    pub fn validate_cache(options: ValidateCacheOptions) -> Result<()> {
        let cache = cache::FetchCache::load()?.into_inner();
        let mut ok = true;

        let mut invalid = Vec::new();
        for (doi, entry) in cache {
            let _s = error_span!("validate_cache", doi = &*doi).entered();
            let entry = JsonValue::Object(entry);
            if !validate_entry(&entry, ignore_missing_id) {
                ok = false;
                if options.output.is_some() {
                    invalid.push(entry);
                }
            }
        }
        if let Some(path) = options.output {
            write_json(path, invalid)?;
        }

        if ok {
            Ok(())
        } else {
            bail!("{}", EXIT_ERROR_MSG)
        }
    }
}

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

mod example {
    use super::*;

    #[derive(Args)]
    pub struct ClArgs {}

    pub fn main(_args: ClArgs) -> Result<()> {
        todo!()
    }
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
