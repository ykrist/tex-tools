use super::csl_fields as csl;
use super::*;
use regex::Regex;
use tex_tools::biblatex::entry::{self, Entry};
use tex_tools::biblatex::types::{self, Date, Name};

trait ExpectField {
    fn try_field(&mut self, f: &str) -> Option<JsonValue>;

    fn try_field_then<F, T>(&mut self, f: &str, then: F) -> Result<Option<T>>
    where
        F: FnOnce(JsonValue) -> Result<T>,
    {
        match self.try_field(f) {
            None => Ok(None),
            Some(v) => then(v)
                .map(Some)
                .with_context(|| format!("error in field `{}`", f)),
        }
    }

    fn require_field(&mut self, f: &str) -> Result<JsonValue> {
        self.try_field(f)
            .ok_or_else(|| anyhow!("missing field `{}` from entry", f))
    }

    fn require_field_then<F, T>(&mut self, f: &str, then: F) -> Result<T>
    where
        F: FnOnce(JsonValue) -> Result<T>,
    {
        let v = self.require_field(f)?;
        then(v).with_context(|| format!("error in field `{}`", f))
    }
}

impl ExpectField for CslEntry {
    fn try_field(&mut self, f: &str) -> Option<JsonValue> {
        self.remove(f)
    }
}

#[instrument(level = "trace", skip_all)]
fn convert_name(name: JsonValue) -> Result<Name> {
    let mut name = name.expect_object()?;
    let given = name.require_field("given")?.expect_string()?;
    let family = name.require_field("family")?.expect_string()?;
    Ok(Name::new(given, family))
}

#[instrument(level = "trace", skip_all)]
fn convert_name_list(list: JsonValue) -> Result<types::List<Name>> {
    let names: Result<Vec<_>> = list.expect_array()?.into_iter().map(convert_name).collect();
    names.map(types::List)
}

#[instrument(level = "trace", skip_all)]
fn convert_date(date: JsonValue) -> Result<Date> {
    #[instrument(level = "trace", skip_all)]
    fn convert_date_parts(parts: JsonValue) -> Result<Date> {
        let mut parts = parts.expect_array()?;
        if parts.len() != 1 {
            bail!("expected a single date")
        }
        let mut parts = parts.pop().unwrap().expect_array()?;

        if parts.is_empty() || parts.len() > 3 {
            bail!(
                "date array should be between 1 and 3 elements long: {:?}",
                parts
            )
        }

        parts.reverse(); // ymd to dmy (or my or y)
        let year = parts.pop().unwrap().expect_int().map(|y| y as i32)?;
        let month = parts
            .pop()
            .map(|v| v.expect_uint().map(|y| y as i32))
            .transpose()?;
        let day = parts
            .pop()
            .map(|v| v.expect_uint().map(|y| y as i32))
            .transpose()?;

        Ok(Date { year, month, day })
    }

    #[instrument(level = "trace", skip_all)]
    fn convert_raw_date(raw: JsonValue) -> Result<Date> {
        lazy_static! {
            static ref YEAR: Regex = Regex::new(r"^(\d{4})$").unwrap();
            static ref YEAR_MONTH: Regex = Regex::new(r"^(\d{4})-(\d\d?)$").unwrap();
            static ref FULL: Regex = Regex::new(r"^(\d{4})-(\d\d?)-(\d\d?)$").unwrap();
        }

        let s = raw.expect_string()?;
        let parse_month = |s: &str| -> Result<i32> {
            let i: i32 = s.parse().unwrap();
            if (1..=12).contains(&i) {
                Ok(i)
            } else {
                bail!("months are from 1 to 12")
            }
        };
        let parse_date = |s: &str| -> Result<i32> {
            let i: i32 = s.parse().unwrap();
            if (1..=31).contains(&i) {
                Ok(i)
            } else {
                bail!("days are from 1 to 31")
            }
        };

        for re in [&*YEAR, &*YEAR_MONTH, &*FULL] {
            if let Some(c) = re.captures(&s) {
                return Ok(Date {
                    year: c
                        .get(1)
                        .unwrap()
                        .as_str()
                        .parse()
                        .context("failed to parse year")?,
                    month: c.get(2).map(|s| parse_month(s.as_str())).transpose()?,
                    day: c.get(3).map(|s| parse_date(s.as_str())).transpose()?,
                });
            }
        }

        bail!(
            "bad date format: `{}`. Acceptable formats are YYYY, YYYY-MM and YYYY-MM-DD.",
            &s
        )
    }

    let mut date = date.expect_object()?;
    if let Some(parts) = date.remove("date-parts") {
        return convert_date_parts(parts);
    }
    if let Some(raw) = date.remove("raw") {
        return convert_raw_date(raw);
    }

    bail!("date fields must have either a `date-parts` or `raw` property");
}

#[instrument(level = "trace", skip_all)]
fn convert_page_range(v: JsonValue) -> Result<types::Range> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"^(\d+)-(\d+)$").unwrap();
    }

    let range = v.expect_string()?;
    if let Ok(i) = range.parse() {
        return Ok(types::Range::Single(i));
    }
    let make_err_ctx = || format!("unable to parse `{}` as a range", &range);
    let make_err = || anyhow::Error::msg(make_err_ctx());

    let captures = RE.captures(&range).ok_or_else(make_err)?;

    let start: i32 = captures
        .get(1)
        .unwrap()
        .as_str()
        .parse()
        .with_context(make_err_ctx)?;

    let end: i32 = captures
        .get(2)
        .unwrap()
        .as_str()
        .parse()
        .with_context(make_err_ctx)?;

    Ok(types::Range::Multi {
        start,
        end: Some(end),
    })
}

fn take_optional_string_field<T: From<String>>(e: &mut CslEntry, f: &str) -> Result<Option<T>> {
    let v = e.try_field_then(f, JsonValue::expect_string)?;
    Ok(v.map(From::from))
}

fn take_string_field<T: From<String>>(e: &mut CslEntry, f: &str) -> Result<T> {
    let v = e.require_field_then(f, JsonValue::expect_string)?;
    Ok(v.into())
}

#[instrument(level = "info", skip(e))]
fn convert_article(id: String, mut e: CslEntry) -> Result<entry::Article> {
    let author = e.require_field_then(csl::AUTHOR, convert_name_list)?;
    let title = e.require_field_then(csl::TITLE, |t| t.expect_string().map(From::from))?;
    let journal_title =
        e.require_field_then(csl::CONTAINER_TITLE, |t| t.expect_string().map(From::from))?;
    let date = e.require_field_then(csl::ISSUED, convert_date)?;
    let year = date.year;

    let mut a = entry::Article::new(id, author, title, journal_title, year);

    a.month = date.month;
    a.doi = take_optional_string_field(&mut e, csl::DOI)?;
    a.number = take_optional_string_field(&mut e, csl::ISSUE)?;
    a.pages = e.try_field_then(csl::PAGE, convert_page_range)?;
    a.volume = take_optional_string_field(&mut e, csl::VOLUME)?;
    Ok(a)
}

#[instrument(level = "info", skip(e))]
fn convert_thesis(id: String, mut e: CslEntry) -> Result<entry::Thesis> {
    let author = e.require_field_then(csl::AUTHOR, convert_name_list)?;
    let title = take_string_field(&mut e, csl::TITLE)?;
    let date = e.require_field_then(csl::ISSUED, convert_date)?;
    let year = date.year;
    let institution = types::List(vec![take_string_field(&mut e, csl::PUBLISHER)?]);
    let kind = e.require_field_then(csl::GENRE, |v| v.expect_string().map(From::from))?;

    let mut t = entry::Thesis::new(id, author, title, kind, institution, year);
    t.month = date.month;
    Ok(t)
}

#[instrument(level = "info", skip(e))]
fn convert_conference_paper(id: String, mut e: CslEntry) -> Result<entry::InProceedings> {
    let author = e.require_field_then(csl::AUTHOR, convert_name_list)?;
    let title = take_string_field(&mut e, csl::TITLE)?;
    let date = e.require_field_then(csl::ISSUED, convert_date)?;
    let year = date.year;
    let book_title = take_string_field(&mut e, csl::CONTAINER_TITLE)?;

    let mut c = entry::InProceedings::new(id, author, title, book_title, year);
    c.month = date.month;
    c.doi = take_optional_string_field(&mut e, csl::DOI)?;
    c.publisher = take_optional_string_field(&mut e, csl::PUBLISHER)?.map(types::List::singleton);
    c.location =
        take_optional_string_field(&mut e, csl::PUBLISHER_PLACE)?.map(types::List::singleton);
    Ok(c)
}

#[instrument(level = "info", skip(e))]
fn convert_report(id: String, mut e: CslEntry) -> Result<entry::Report> {
    let author = e.require_field_then(csl::AUTHOR, convert_name_list)?;
    let title = take_string_field(&mut e, csl::TITLE)?;
    let date = e.require_field_then(csl::ISSUED, convert_date)?;
    let year = date.year;
    let institution = types::List(vec![take_string_field(&mut e, csl::PUBLISHER)?]);
    let kind = e.require_field_then(csl::GENRE, |v| v.expect_string().map(From::from))?;

    let mut r = entry::Report::new(id, author, title, kind, institution, year);
    r.month = date.month;
    Ok(r)
}

#[instrument(level = "info", skip(e))]
fn convert_working_paper(id: String, mut e: CslEntry) -> Result<entry::Report> {
    let author = e.require_field_then(csl::AUTHOR, convert_name_list)?;
    let title = take_string_field(&mut e, csl::TITLE)?;
    let date = e.require_field_then(csl::ISSUED, convert_date)?;
    let year = date.year;
    let institution = types::List(vec![take_string_field(&mut e, csl::PUBLISHER)?]);
    let kind = "Working paper".to_string().into();

    let mut r = entry::Report::new(id, author, title, kind, institution, year);
    r.month = date.month;
    r.number = take_optional_string_field(&mut e, csl::NUMBER)?;
    r.url = take_optional_string_field(&mut e, csl::URL)?;
    Ok(r)
}

#[instrument(level="info", skip(s), fields(s=?s.as_ref()))]
fn parse_arxiv_category(s: impl AsRef<str>) -> Result<String> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"\(([a-z]+\.[A-Z]+)\)").unwrap();
    }

    let s = s.as_ref();
    if let Some(c) = RE.captures(s) {
        return Ok(c.get(1).unwrap().as_str().to_string());
    }
    bail!("failed to parse arXiv category: {}", s);
}

#[instrument(level = "info", skip(e))]
fn convert_arxiv_paper(id: String, mut e: CslEntry) -> Result<entry::Misc> {
    let author = e.require_field_then(csl::AUTHOR, convert_name_list)?;
    let title = take_string_field(&mut e, csl::TITLE)?;
    let date = e.require_field_then(csl::ISSUED, convert_date)?;
    let mut b = entry::Misc::new(id, author, title, date.year);

    let url: String = take_string_field(&mut e, csl::URL)?;
    let arxiv_id = url
        .split("/")
        .last()
        .ok_or_else(|| anyhow!("failed to parse URL for arXiv ID: {}", &url))?;

    b.eprint = Some(arxiv_id.into());
    b.eprint_type = Some("arXiv".into());
    let main_category = e.require_field_then(csl::CATEGORIES, |v| {
        let v = v.expect_array()?;
        let c = v
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("At least one arXiv category must be given"))?
            .expect_string()?;
        parse_arxiv_category(c)
    })?;
    b.eprint_class = Some(main_category.into());
    b.version = take_optional_string_field(&mut e, csl::VERSION)?;
    Ok(b)
}

fn convert_book(id: String, mut e: CslEntry) -> Result<entry::Book> {
    let author = e.require_field_then(csl::AUTHOR, convert_name_list)?;
    let title = take_string_field(&mut e, csl::TITLE)?;
    let date = e.require_field_then(csl::ISSUED, convert_date)?;
    let mut b = entry::Book::new(id, author, title, date.year);

    b.chapter = take_optional_string_field(&mut e, csl::CHAPTER_NUMBER)?;
    b.doi = take_optional_string_field(&mut e, csl::DOI)?;
    b.isbn = take_optional_string_field(&mut e, csl::ISBN)?;
    b.publisher = take_optional_string_field(&mut e, csl::PUBLISHER)?.map(types::List::singleton);
    b.location =
        take_optional_string_field(&mut e, csl::PUBLISHER_PLACE)?.map(types::List::singleton);
    b.pages = e.try_field_then(csl::PAGE, convert_page_range)?;

    Ok(b)
}

#[instrument(level = "error", skip(e), fields(id))]
pub fn csl_to_biblatex(mut e: CslEntry) -> Result<Entry> {
    let id = e.require_field(csl::ID)?.expect_string()?;
    tracing::Span::current().record("id", &&*id);

    let err_context = format!("failed to convert entry `{}`", id);
    #[inline]
    fn match_type(id: String, mut e: CslEntry) -> Result<Entry> {
        match e.require_field(csl::TYPE)?.expect_string()?.as_str() {
            "article-journal" => convert_article(id, e).map(Entry::Article),
            "article" => {
                let mut ty = e.require_field(csl::GENRE)?.expect_string()?;
                ty.make_ascii_lowercase();
                match ty.trim() {
                    "working paper" => convert_working_paper(id, e).map(Entry::Report),
                    "arxiv" => convert_arxiv_paper(id, e).map(Entry::Misc),
                    unknown => bail!("unknown article sub-type `{}`", unknown),
                }
            }
            "thesis" => convert_thesis(id, e).map(Entry::Thesis),
            "paper-conference" => convert_conference_paper(id, e).map(Entry::InProceedings),
            "report" => convert_report(id, e).map(Entry::Report),
            "book" => convert_book(id, e).map(Entry::Book),
            ty => bail!("no BibLaTex entry type for CSL type {}", ty),
        }
    }

    match_type(id, e).context(err_context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use posix_cli_utils::IoContext;
    use pretty_assertions::assert_str_eq;
    use std::path::Path;
    use tex_tools::biblatex::ToBiblatex;

    fn check_output(name: &str) -> Result<()> {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/biblatex/"));
        let mut path = dir.join(name);
        path.set_extension("json");
        let input: CslEntry = read_json(&path)?;
        path.set_extension("bib");
        let expected = std::fs::read_to_string(&path).context_read(&path)?;
        let output = csl_to_biblatex(input)?;
        assert_str_eq!(expected, output.biblatex().to_string());
        Ok(())
    }

    #[test]
    fn parse_arxiv_category() -> Result<()> {
        use super::parse_arxiv_category as parse;
        assert_eq!(parse("Optimization and Control (math.OC)")?, "math.OC");
        assert_eq!(parse("Discrete Mathematics (cs.DM)")?, "cs.DM");
        Ok(())
    }

    #[test]
    fn convert_date() -> Result<()> {
        use super::convert_date as convert;
        use serde_json::json;
        assert_eq!(
            convert(json!({ "date-parts": [[2001, 1, 25]]}))?,
            Date::full(2001, 1, 25)
        );
        assert_eq!(
            convert(json!({ "date-parts": [[2001, 1]]}))?,
            Date::year_month(2001, 1)
        );
        assert_eq!(convert(json!({ "date-parts": [[2001]]}))?, Date::year(2001));
        assert_eq!(
            convert(json!({ "raw": "2001-01-25"}))?,
            Date::full(2001, 1, 25)
        );
        assert_eq!(
            convert(json!({ "raw": "2001-01"}))?,
            Date::year_month(2001, 1)
        );
        assert_eq!(
            convert(json!({ "raw": "2001-1"}))?,
            Date::year_month(2001, 1)
        );
        assert_eq!(convert(json!({ "raw": "2001"}))?, Date::year(2001));
        Ok(())
    }

    #[test]
    fn article() -> Result<()> {
        check_output("article")
    }

    #[test]
    fn phd_thesis() -> Result<()> {
        check_output("phd-thesis")
    }

    #[test]
    fn honours_thesis() -> Result<()> {
        check_output("honours-thesis")
    }

    #[test]
    fn conference_paper() -> Result<()> {
        check_output("conference-paper")
    }

    #[test]
    fn technical_report() -> Result<()> {
        check_output("tech-report")
    }

    #[test]
    fn working_paper() -> Result<()> {
        check_output("working-paper")
    }

    #[test]
    fn arxiv() -> Result<()> {
        check_output("arxiv")
    }

    #[test]
    fn book() -> Result<()> {
        check_output("book")
    }
}
