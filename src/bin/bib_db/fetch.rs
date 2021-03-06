use std::time::Duration;
use std::{num::NonZeroU32, path::Path};

use super::csl_fields as csl;
use super::*;

use futures::future;
use governor as gv;
use reqwest::{header, Client, Response};
use tex_tools::biblatex::ToBiblatex;

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

fn field_value_hacks(entry: &mut serde_json::Map<String, JsonValue>) {
    for field in [csl::CONTAINER_TITLE, csl::CONTAINER_TITLE_SHORT] {
        if let Some(JsonValue::String(s)) = entry.get_mut(field) {
            if s.contains("&amp;") {
                let new = s.replace("&amp;", "&");
                *s = new;
            }
        }
    }
}

#[instrument(level = "error", name = "clean", skip(entry))]
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
                    // You'd think this would be a book, but for some reason Crosscite and gang
                    // like listing conference papers as book-chapters.
                    new_ty = Some("paper-conference");
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
            warn!(invalid=%ty, inferred=%new_ty, "converting out-of-spec type, inferred type may be wrong");
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

    if entry.get(csl::PUBLISHER).map(JsonValue::as_str).flatten() == Some("arXiv") {
        if !entry.contains_key(csl::GENRE) {
            entry.insert("genre".into(), "arxiv".into());
        }
    }

    field_value_hacks(entry);
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
        async move { (doi, fetch_one(&client, &rl, doi).await) }
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

        let _s = error_span!("validate", doi).entered();
        clean_json(&mut json);

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
        for (doi, json) in fetch_and_validate(options, to_fetch, options.dump_raw())? {
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
    Biblatex,
}

impl OutputFormat {
    pub fn suffix(&self) -> &'static str {
        match self {
            OutputFormat::Json => "json",
            OutputFormat::Biblatex => "bib",
        }
    }
}

#[derive(Args)]
pub struct ClArgs {
    /// Input file (CSL JSON format)
    input: PathBuf,

    /// Maximum number of API requests allowed per second.
    #[clap(short = 'r', default_value_t = 20)]
    max_requests_per_sec: u32,

    /// Dump the raw JSON retrieved, prior to cleaning.
    #[cfg(debug_assertions)]
    #[clap(long, value_name = "PATH")]
    dump_raw: Option<PathBuf>,

    /// Output format
    #[clap(arg_enum, short='f', default_value_t=OutputFormat::Biblatex)]
    format: OutputFormat,

    /// Output path. Default is same name as input file with "-filled" appended to the filename stem. Use "-"
    /// for STDOUT.
    #[clap(short = 'o')]
    output: Option<String>,

    /// Output a single entry only, useful for debugging.
    #[clap(short = 'e')]
    entry: Option<String>,

    /// Ignore and skip over entries with errors
    #[clap(short = 'c')]
    ignore_errors: bool,
}

impl ClArgs {
    fn dump_raw(&self) -> Option<&PathBuf> {
        #[cfg(debug_assertions)]
        {
            return self.dump_raw.as_ref();
        }
        #[cfg(not(debug_assertions))]
        {
            return None;
        }
    }
}

fn output_json(db: Vec<CslEntry>, path: Option<impl AsRef<Path>>) -> Result<()> {
    if let Some(path) = path {
        write_json_pretty(path, &db)
    } else {
        let out = std::io::stdout();
        serde_json::to_writer_pretty(out.lock(), &db)?;
        Ok(())
    }
}

fn output_biblatex(
    db: Vec<CslEntry>,
    path: Option<impl AsRef<Path>>,
    ignore_errors: bool,
) -> Result<()> {
    use std::io::Write;

    fn write<W: Write, I: IntoIterator<Item = Result<biblatex::entry::Entry>>>(
        db: I,
        mut w: W,
        ignore_errors: bool,
    ) -> Result<()> {
        for e in db {
            match e {
                Ok(e) => write!(w, "{}\n", e.biblatex())?,
                Err(e) => {
                    if !ignore_errors {
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    let db = db.into_iter().map(convert::csl_to_biblatex);

    if let Some(path) = path {
        let path = path.as_ref();
        let file = std::fs::File::create(path)
            .context_write(path)
            .map(std::io::BufWriter::new)?;
        write(db, file, ignore_errors)?;
    } else {
        let out = std::io::stdout();
        write(db, out.lock(), ignore_errors)?;
    }
    Ok(())
}

pub fn main(mut args: ClArgs) -> Result<()> {
    args.max_requests_per_sec = args.max_requests_per_sec.max(1);
    let mut db: Vec<_> = validate::load_and_validate_db(&args.input, args.ignore_errors)?
        .into_iter()
        .map(JsonExt::unwrap_object)
        .collect();
    info!(n_entries = db.len(), "DB read successfully");

    fetch_and_merge(&args, &mut db)?;

    let output_file = match args.output.take() {
        None => {
            let mut n = args.input.file_stem().expect("no file name").to_os_string();
            n.push("-filled.");
            n.push(args.format.suffix());
            Some(args.input.with_file_name(n))
        }
        Some(s) if s == "-" => None,
        Some(p) => Some(PathBuf::from(p)),
    };

    if let Some(id) = args.entry.as_ref() {
        db.retain(|e| e["id"].as_str() == Some(id))
    }

    match args.format {
        OutputFormat::Json => output_json(db, output_file.as_ref())?,
        OutputFormat::Biblatex => output_biblatex(db, output_file.as_ref(), args.ignore_errors)?,
    }

    if let Some(p) = &output_file {
        info!(path=%p.display(), "wrote output file");
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
