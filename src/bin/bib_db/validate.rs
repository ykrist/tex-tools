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

pub fn load_and_validate_db(path: impl AsRef<Path>, ignore_errors: bool) -> Result<Vec<JsonValue>> {
    let mut db = read_json::<JsonValue, _>(path)?
        .expect_array()
        .context("top-level JSON value must be an array.")?;

    let mut all_valid = true;
    let mut entry_index = 0;

    db.retain(|entry| {
        let _s = error_span!("validate_db", entry_index).entered();
        let valid = validate_entry(entry, ignore_missing_type);
        all_valid &= valid;
        entry_index += 1;
        valid
    });

    if !all_valid && !ignore_errors {
        bail!("{}", EXIT_ERROR_MSG)
    }
    Ok(db)
}

pub fn main(args: ClArgs) -> Result<()> {
    load_and_validate_db(args.input, false)?;
    Ok(())
}

#[derive(Args)]
pub struct ValidateCacheOptions {
    /// Save the invalid entries to a JSON file.
    #[clap(short)]
    output: Option<PathBuf>,
}

#[cfg(debug_assertions)]
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
