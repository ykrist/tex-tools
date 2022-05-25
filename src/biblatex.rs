use super::*;
use std::fmt::{Display, Formatter, Result as FmtResult};

pub struct FmtBiblatex<'a, T: ?Sized>(pub &'a T);

pub trait ToBiblatex {
    fn biblatex<'a>(&'a self) -> FmtBiblatex<'a, Self>;
}

macro_rules! impl_tobiblatex {
    ($($t:path),+ $(,)?) => {
        $(
            impl ToBiblatex for $t {
                fn biblatex<'a>(&'a self) -> FmtBiblatex<'a, Self> {
                    FmtBiblatex(self)
                }
            }
        )*
    };
}

pub mod types {
    use super::*;

    pub type Int = i32;

    impl<'a> Display for FmtBiblatex<'a, Int> {
        fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
            write!(f, "{}", self.0)
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Date {
        pub year: Int,
        pub month: Option<Int>,
        pub day: Option<Int>,
    }

    impl Date {
        pub fn year(year: Int) -> Self {
            Date {
                year,
                month: None,
                day: None,
            }
        }

        pub fn year_month(year: Int, month: Int) -> Self {
            Date {
                year,
                month: Some(month),
                day: None,
            }
        }

        pub fn full(year: Int, month: Int, day: Int) -> Self {
            Date {
                year,
                month: Some(month),
                day: Some(day),
            }
        }
    }

    impl<'a> Display for FmtBiblatex<'a, Date> {
        fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
            write!(f, "{}", self.0.year)?;
            if let Some(m) = self.0.month {
                write!(f, "-{}", m)?;
                if let Some(d) = self.0.day {
                    write!(f, "-{}", d)?;
                }
            }
            Ok(())
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub enum PubState {
        /// The manuscript is being prepared for publication
        InPreparation,
        /// The manuscript has been submitted to a journal or conference
        Submitted,
        /// The manuscript has been accepted by a press or journal
        Forthcoming,
        /// The manuscript is fully copyedited and out of the author’s hands; it is in the final stages of the production process).
        InPress,
        /// The manuscript is published in a preliminary form or location, such as online version in advance of print publication
        Prepublished,
    }

    impl<'a> Display for FmtBiblatex<'a, PubState> {
        fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
            let s = match self.0 {
                PubState::InPreparation => "in preperation",
                PubState::Submitted => "submitted",
                PubState::Forthcoming => "forthcoming",
                PubState::InPress => "in press",
                PubState::Prepublished => "pre-published",
            };
            f.write_str(s)
        }
    }

    macro_rules! tranparent_string_wrapper {
        ($name:ident) => {
            #[derive(Clone, Debug)]
            pub struct $name(pub String);

            impl From<String> for $name {
                fn from(s: String) -> Self {
                    $name(s)
                }
            }

            impl From<&str> for $name {
                fn from(s: &str) -> Self {
                    $name(s.to_string())
                }
            }
        };
    }

    tranparent_string_wrapper!(Literal);

    impl<'a> Display for FmtBiblatex<'a, Literal> {
        fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
            crate::escape::utf8_to_tex(&self.0 .0).fmt(f)
        }
    }

    tranparent_string_wrapper!(Verbatim);

    impl<'a> Display for FmtBiblatex<'a, Verbatim> {
        fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
            f.write_str(&self.0 .0)
        }
    }

    tranparent_string_wrapper!(Uri);

    impl<'a> Display for FmtBiblatex<'a, Uri> {
        fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
            f.write_str(&self.0 .0)
        }
    }

    #[derive(Debug, Clone, Copy)]
    pub enum Range {
        Single(Int),
        Multi { start: Int, end: Option<Int> },
    }

    impl<'a> Display for FmtBiblatex<'a, Range> {
        fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
            match self.0 {
                Range::Single(i) => i.fmt(f),
                Range::Multi {
                    start,
                    end: Some(end),
                } => write!(f, "{}-{}", start, end),
                Range::Multi { start, end: None } => write!(f, "{}-", start),
            }
        }
    }

    #[derive(Debug, Clone)]
    pub struct Name {
        given: String,
        family: String,
    }

    impl Name {
        pub fn new(given: String, family: String) -> Self {
            Name { given, family }
        }
    }

    impl<'a> Display for FmtBiblatex<'a, Name> {
        fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
            let n = self.0;
            write!(f, "{}, {}", utf8_to_tex(&n.family), utf8_to_tex(&n.given),)
        }
    }

    #[derive(Debug, Clone)]
    pub struct List<T>(pub Vec<T>);

    impl<T> ToBiblatex for List<T> {
        fn biblatex<'a>(&'a self) -> FmtBiblatex<'a, Self> {
            FmtBiblatex(self)
        }
    }

    impl<'a, T> Display for FmtBiblatex<'a, List<T>>
    where
        T: ToBiblatex,
        FmtBiblatex<'a, T>: Display,
    {
        fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
            let mut values = self.0 .0.iter();
            if let Some(first) = values.next() {
                write!(f, "{}", first.biblatex())?;
            }
            for v in values {
                write!(f, " and {}", v.biblatex())?;
            }
            Ok(())
        }
    }

    impl_tobiblatex! {
        Date,
        Int,
        Literal,
        Name,
        PubState,
        Range,
        Uri,
        Verbatim,
    }
}

#[macro_use]
pub mod field {
    use super::types::*;
    use super::*;

    #[rustfmt::skip]
    macro_rules! field_ty {
        (addendum) => { Literal };
        (annotator) => { List<Name> };
        (author) => { List<Name> };
        (book_title) => { Literal };
        (book_title_addon) => { Literal };
        (book_subtitle) => { Literal };
        (chapter) => { Literal };
        (commentator) => { List<Name> };
        (doi) => { Verbatim };
        (edition) => { Literal };
        (editor) => { List<Name> };
        (eid) => { Literal };
        (eprint) => { Verbatim };
        (eprint_class) => { Literal };
        (eprint_type) => { Literal };
        (event_date) => { Date };
        (event_title) => { Literal };
        (event_title_addon) => { Literal };
        (how_published) => { Literal };
        (institution) => { List<Literal> };
        (isbn) => { Literal }; 
        (issn) => { Literal }; 
        (issue) => { Literal }; 
        (issue_subtitle) => { Literal }; 
        (issue_title) => { Literal }; 
        (location) => { List<Literal> };
        (journal_subtitle) => { Literal }; 
        (journal_title) => { Literal };
        (main_title) => { Literal };
        (main_title_addon) => { Literal };
        (main_subtitle) => { Literal };
        (month) => { Int };
        (note) => { Literal };        
        (number) => { Literal };
        (organization) => { List<Literal> };
        (pages) => { Range };
        (page_total) => { Literal };
        (part) => { Literal }; 
        (pubstate) => { PubState };
        (publisher) => { List<Literal> };
        (series) => { Literal };
        (subtitle) => { Literal };
        (title) => { Literal };
        (title_addon) => { Literal };
        (translator) => { List<Name> };
        (type_) => { Literal };
        (url) => { Uri };
        (url_date) => { Date };
        (venue) => { Literal };
        (version) => { Literal };
        (volume) => { Literal };
        (volumes) => { Literal };
        (year) => { Int };
    }

    #[rustfmt::skip]
    macro_rules! field_id {
        (book_title) => { "booktitle" };
        (book_title_addon) => { "booktitleaddon" };
        (book_subtitle) => { "booksubtitle" };
        (eprint_class) => { "eprintclass" };
        (eprint_type) => { "eprinttype" };
        (event_date) => { "eventdate" };
        (event_title) => { "eventtitle" };
        (event_title_addon) => { "eventtitleaddon" };
        (how_published) => { "howpublished" };
        (issue_subtitle) => { "issuesubtitle" }; 
        (issue_title) => { "issuetitle" }; 
        (journal_subtitle) => { "journalsubtitle" }; 
        (journal_title) => { "journaltitle" };
        (main_title) => { "maintitle" };
        (main_title_addon) => { "maintitleaddon" };
        (main_subtitle) => { "mainsubtitle" };
        (page_total) => { "pagetotal" };
        (title_addon) => { "titleaddon" };
        (type_) => { "type" };
        (url_date) => { "urldate" };
        ($f:ident) => { stringify!($f) }
    }

    macro_rules! field_rename {
        ($field:ident) => {
            $field
        };
    }

    macro_rules! req_field {
        ($f:ident) => {
            field_rename!($f): $field_ty,
        };
    }

    macro_rules! field {
        ($f:ident) => {
            field_rename!($f): Option<$field_ty>,
        };
    }
}

pub mod entry {
    macro_rules! entry_struct {
        (
            $tyname:ident $biber_name:literal ;
            $($req_field:ident),* $(,)? ;
            $($opt_field:ident),* $(,)?
        ) => {



            #[derive(Debug, Clone)]
            #[non_exhaustive]
            pub struct $tyname {
                pub id: String,
                $(
                    pub $req_field : field_ty!($req_field),
                )*
                $(
                    pub $opt_field : Option<field_ty!($opt_field)>,
                )*
            }

            impl_tobiblatex!{$tyname}

            impl $tyname {
                pub fn new(
                    id: String,
                    $(
                        $req_field : field_ty!($req_field),
                    )*
                ) -> Self {
                    Self {
                        id,
                        $($req_field,)*
                        $($opt_field: None),*
                    }
                }
            }

            impl<'a> Display for FmtBiblatex<'a, $tyname> {
                fn fmt(&self, f: &mut Formatter) -> FmtResult {
                    let e = self.0;
                    writeln!(f, "@{}{{{},", $biber_name, &e.id)?;

                    $(
                        writeln!(f, "    {} = {{{}}},", field_id!($req_field), e.$req_field.biblatex())?;
                    )*
                    $(
                        if let Some(val) = e.$opt_field.as_ref() {
                            writeln!(f, "    {} = {{{}}},", field_id!($opt_field), val.biblatex())?;
                        }
                    )*
                    f.write_str("}\n")
                }
            }

        };
    }

    use super::types::*;
    use super::*;

    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum Entry {
        Article(Article),
        Thesis(Thesis),
        InProceedings(InProceedings),
        Report(Report),
        Misc(Misc),
    }

    impl_tobiblatex! {Entry}

    impl Entry {
        pub fn id(&self) -> &str {
            match self {
                Entry::Article(e) => &e.id,
                Entry::Thesis(e) => &e.id,
                Entry::InProceedings(e) => &e.id,
                Entry::Report(e) => &e.id,
                Entry::Misc(e) => &e.id,
            }
        }
    }

    impl<'a> Display for FmtBiblatex<'a, Entry> {
        fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
            match &self.0 {
                Entry::Article(e) => e.biblatex().fmt(f),
                Entry::Thesis(e) => e.biblatex().fmt(f),
                Entry::InProceedings(e) => e.biblatex().fmt(f),
                Entry::Report(e) => e.biblatex().fmt(f),
                Entry::Misc(e) => e.biblatex().fmt(f),
            }
        }
    }

    entry_struct! {
        Article "article";
        author,
        title,
        journal_title,
        year,
        ;
        addendum,
        annotator,
        commentator,
        doi,
        editor,
        // editora,
        // editorb,
        // editorc,
        eid,
        eprint,
        eprint_class,
        eprint_type,
        issn,
        issue,
        issue_subtitle,
        issue_title,
        journal_subtitle,
        // language
        month,
        note,
        number,
        // origlanguage,
        pages,
        pubstate,
        series,
        subtitle,
        title_addon,
        translator,
        url,
        url_date,
        version,
        volume,
    }

    entry_struct! {
        Thesis "thesis";
        author,
        title,
        type_,
        institution,
        year,
        ;
        addendum,
        chapter,
        doi,
        eprint_class,
        eprint_type,
        eprint,
        isbn,
        // language,
        location,
        month,
        note,
        page_total,
        pages,
        pubstate,
        subtitle,
        title_addon,
        url_date,
        url,
    }

    entry_struct! {
        InProceedings "inproceedings";
        author,
        title,
        book_title,
        year,
        ;
        addendum,
        book_subtitle,
        book_title_addon,
        chapter,
        doi,
        editor,
        eprint,
        eprint_class,
        eprint_type,
        event_date,
        event_title,
        event_title_addon,
        isbn,
        // language,
        location,
        main_subtitle,
        main_title,
        main_title_addon,
        month,
        note,
        number,
        organization,
        pages,
        part,
        publisher,
        pubstate,
        series,
        subtitle,
        title_addon,
        url,
        url_date,
        venue,
        volume,
        volumes,
    }

    entry_struct! {
        Report "report";
        author,
        title,
        type_,
        institution,
        year,
        ;
        addendum,
        chapter,
        doi,
        eprint,
        eprint_class,
        eprint_type,
        // isrn,
        // language,
        location,
        month,
        note,
        number,
        pages,
        page_total,
        pubstate,
        subtitle,
        title_addon,
        url,
        url_date,
        version,
    }

    entry_struct! {
        Misc "misc";
        author,
        title,
        year,
        ;
        addendum,
        chapter,
        doi,
        edition,
        eprint,
        eprint_class,
        eprint_type,
        isbn,
        // language,
        location,
        note,
        number,
        organization,
        pages,
        page_total,
        publisher,
        pubstate,
        series,
        subtitle,
        title_addon,
        type_,
        url,
        url_date,
        version,
    }
}
