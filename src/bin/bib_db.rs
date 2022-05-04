use anyhow::Context;
use tex_tools::*;
use std::{path::PathBuf};
use lazy_static::lazy_static;

lazy_static! {
    static ref CSL_SCHEMA : serde_json::Value = {
        serde_json::from_str(include_str!("csl-schema.json")).expect("schema is not valid JSON")
    };

    static ref BIB_DB_SCHEMA : serde_json::Value = {
        serde_json::from_str(include_str!("bib_db-schema.json")).expect("schema is not valid JSON")
    };
}

fn compile_schema() -> Result<jsonschema::JSONSchema> {
    jsonschema::JSONSchema::options()
        .with_draft(jsonschema::Draft::Draft7)
        .compile(&*CSL_SCHEMA)
        .context("schema did not fit JSONSchema schema")
}

mod validate {
    use jsonschema::{paths::JSONPointer, ValidationError, error::ValidationErrorKind};
    use serde_json::Value;
    use super::*;

    #[derive(Args)]
    pub struct ClArgs {
        /// Input file (CSL JSON format)
        input: PathBuf
    }

    fn try_find_id<'a>(root: &'a Value, error_path: &JSONPointer) -> Option<&'a Value> {
        let root = root.as_array()?;
        let entry = match error_path.iter().next()? {
            jsonschema::paths::PathChunk::Index(i) => root.get(*i)?,
            _ => return None
        };
        entry.as_object()?.get("id")
    }

    fn filter_validation_errors(e: &ValidationError) -> bool {
        match &e.kind {
            ValidationErrorKind::Required { property } => property.as_str() != Some("type"),
            _ => true,
        }
    }


    pub fn main(args: ClArgs) -> Result<()> {
        let schema = compile_schema()?;
        let db : serde_json::Value = read_json(&args.input)?;
        let mut exit_code = 0;
        
        let result = schema.validate(&db).map_err(
            |e| e.filter(filter_validation_errors)
        );
        if let Err(errors) = result {
            for error in errors.into_iter().filter(filter_validation_errors) {
                if let Some(id) = try_find_id(&db, &error.instance_path)  {
                    error!(json_path=%error.instance_path, %id, %error);
                } else {
                    error!(json_path=%error.instance_path, %error);
                }
            }
            exit_code = 1;
        }

        std::process::exit(exit_code)
    }
}

#[derive(Parser)]
enum Cmd {
    /// Validate the input database against the CSL schema
    Validate(validate::ClArgs),
}

fn main() -> Result<()> {
    logging_init();

    match Cmd::parse() {
        Cmd::Validate(args) => validate::main(args),
    }
}
