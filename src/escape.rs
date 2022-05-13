use std::fmt::{Display, Error as FmtError, Formatter, Result as FmtResult, Write};
use unicode_normalization::UnicodeNormalization;

/// A lazily UTF8 to LaTeX escaped string. Call `.to_string()` or use [`Display`] to extract
///  the escaped string.
pub struct Utf8ToTex<'a> {
    original: &'a str,
}

#[derive(Clone, Copy, Debug)]
enum CharKind {
    Verbatim(char),
    Escape(&'static str),
    Combining(&'static str),
}

fn classify_char(c: char) -> CharKind {
    use CharKind::*;
    match c {
        c if c.is_ascii_alphanumeric() | c.is_ascii_whitespace() => Verbatim(c),
        '\\' => Escape(r"\textbackslash{}"),
        '~' => Escape(r"\textasciitilde{}"),
        '#' => Escape(r"\#"),
        '$' => Escape(r"\$"),
        '%' => Escape(r"\%"),
        '&' => Escape(r"\&"),
        '_' => Escape(r"\_"),
        '{' => Escape(r"\{"),
        '}' => Escape(r"\}"),
        '^' => Escape(r"\textasciicircum{}"),
        c if c.is_ascii_punctuation() => Verbatim(c),
        'ł' => Escape(r"\l{}"),
        'ø' => Escape(r"\o{}"),
        'Ø' => Escape(r"\O{}"),
        'ı' => Escape(r"\i"),
        '\u{0300}' => Combining(r"\`"),
        '\u{0301}' => Combining(r"\'"),
        '\u{0302}' => Combining(r"\^"),
        '\u{0303}' => Combining(r"\~"),
        '\u{0304}' => Combining(r"\="),
        '\u{0306}' => Combining(r"\u"),
        '\u{0307}' => Combining(r"\."),
        '\u{0308}' => Combining(r#"\""#),
        '\u{030a}' => Combining(r"\r"),
        '\u{030b}' => Combining(r"\H"),
        '\u{030c}' => Combining(r"\v"),
        '\u{0323}' => Combining(r"\d"),
        '\u{0327}' => Combining(r"\c"),
        '\u{0328}' => Combining(r"\k"),

        c => panic!(
            "unimplemented: not sure how to interpret {}: {} ",
            c.escape_unicode(),
            c
        ),
    }
}

#[must_use = "must call .finish()"]
struct CharEscaper<'a, 'b> {
    formatter: &'a mut Formatter<'b>,
    ch: CharKind,
    closing_brackets: u32,
}

impl<'a, 'b> CharEscaper<'a, 'b> {
    fn new(first: CharKind, f: &'a mut Formatter<'b>) -> Self {
        if matches!(&first, CharKind::Combining(_)) {
            panic!("first character should not be combining character");
        }
        CharEscaper {
            formatter: f,
            ch: first,
            closing_brackets: 0,
        }
    }

    fn finish_current_glyph(&mut self) -> FmtResult {
        match self.ch {
            CharKind::Verbatim(c) => self.formatter.write_char(c)?,
            CharKind::Escape(s) => self.formatter.write_str(s)?,
            CharKind::Combining(_) => unreachable!(),
        }
        for _ in 0..self.closing_brackets {
            self.formatter.write_char('}')?;
        }
        Ok(())
    }

    fn write_char(&mut self, c: CharKind) -> FmtResult {
        if let CharKind::Combining(s) = c {
            self.formatter.write_str(s)?;
            self.formatter.write_char('{')?;
            self.closing_brackets += 1;
        } else {
            self.finish_current_glyph();
            self.ch = c;
            self.closing_brackets = 0;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<&'a mut Formatter<'b>, FmtError> {
        self.finish_current_glyph()?;
        Ok(self.formatter)
    }
}

impl<'a> Display for Utf8ToTex<'a> {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        let mut chars = self.original.chars().nfkd().map(classify_char);

        let mut closing_brackets = 0;
        let mut e = match chars.next() {
            Some(c) => CharEscaper::new(c, f),
            None => return Ok(()),
        };

        for c in chars {
            e.write_char(c)?;
        }
        e.finish()?;
        Ok(())
    }
}

/// Substitute non-ASCII characters and escape TeX control characters.
pub fn utf8_to_tex(s: &str) -> Utf8ToTex {
    Utf8ToTex { original: s }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmp(input: &str, expected: &str) {
        let output = utf8_to_tex(input).to_string();
        assert_eq!(output, expected);
    }

    #[test]
    fn acute() {
        cmp("É", r"\'{E}");
        cmp("á", r"\'{a}");
    }

    #[test]
    fn grave() {
        cmp("ò", r"\`{o}");
    }

    #[test]
    fn circumflex() {
        cmp("ô", r"\^{o}")
    }

    #[test]
    fn umlaut() {
        cmp("ö", r#"\"{o}"#)
    }

    #[test]
    fn hungarian_umlaut() {
        cmp("ő", r"\H{o}");
        cmp("Ű", r"\H{U}");
    }

    #[test]
    fn tilde() {
        cmp("õ", r"\~{o}")
    }

    #[test]
    fn cedilla() {
        cmp("ç", r"\c{c}")
    }

    #[test]
    fn ogonek() {
        cmp("ą", r"\k{a}")
    }

    #[test]
    fn barred_l() {
        cmp("ł", r"\l{}")
    }

    #[test]
    fn macron() {
        cmp("ō", r"\={o}")
    }

    #[test]
    fn dot_over() {
        cmp("ȯ", r"\.{o}")
    }

    #[test]
    fn dot_under() {
        cmp("ụ", r"\d{u}")
    }

    #[test]
    fn ring() {
        cmp("å", r"\r{a}");
        cmp("\u{212b}", r"\r{A}");
        cmp("\u{00C5}", r"\r{A}");
    }

    #[test]
    fn breve() {
        cmp("ŏ", r"\u{o}")
    }

    #[test]
    fn caron() {
        cmp("š", r"\v{s}")
    }

    #[test]
    fn slashed_o() {
        cmp("ø", r"\o{}");
        cmp("Ø", r"\O{}");
    }

    #[test]
    fn dotless_i() {
        cmp("ı", r"\i")
    }

    #[test]
    fn zalgo_text() {
        cmp("ą\u{0302}\u{0304}", r"\k{\^{\={a}}}");
        cmp("\u{212b}\u{0300}\u{0301}", r"\r{\`{\'{A}}}");
    }

    #[test]
    fn unchanged() {
        fn check_unchanged(s: &str) {
            cmp(s, s);
        }

        check_unchanged("\n");
        check_unchanged("ab e asfjn23 ASADA");
        check_unchanged("(foo)");

        let no_escape = "
        Lorem ipsum dolor sit amet, consectetur adipiscing elit. Proin scelerisque eu urna in aliquet.\
        Phasellus ac nulla a urna sagittis consequat id quis est. Nullam eu ex eget erat accumsan dictum\
        ac lobortis urna. Etiam fermentum ut quam at dignissim. Curabitur vestibulum luctus tellus, sit\
        amet lobortis augue tempor faucibus. Nullam sed felis eget odio elementum euismod in sit amet massa.\
        Vestibulum sagittis purus sit amet eros auctor, sit amet pharetra purus dapibus. Donec ornare metus\
        vel dictum porta. Etiam ut nisl nisi. Nullam rutrum porttitor mi. Donec aliquam ac ipsum eget\
        hendrerit. Cras faucibus, eros ut pharetra imperdiet, est tellus aliquet felis, eget convallis\
        lacus ipsum eget quam. Vivamus orci lorem, maximus ac mi eget, bibendum vulputate massa. In\
        vestibulum dui hendrerit, vestibulum lacus sit amet, posuere erat. Vivamus euismod massa diam,\
        vulputate euismod lectus vestibulum nec. Donec sit amet massa magna. Nunc ipsum nulla, euismod\
        quis lacus at, gravida maximus elit. Duis tristique, nisl nullam.\
        ";

        cmp(no_escape, no_escape);
    }

    #[test]
    fn from_v_latexescape() {
        cmp(
            r"#$%&\^_{}~",
            r"\#\$\%\&\textbackslash{}\textasciicircum{}\_\{\}\textasciitilde{}",
        );
        cmp("", "");
        cmp("#$%&", r"\#\$\%\&");
        cmp("bar_^", r"bar\_\textasciicircum{}");
        cmp("{foo}", r"\{foo\}");
        cmp(r"~\", r"\textasciitilde{}\textbackslash{}");
        cmp(
            r"_% of do$llar an&d #HASHES {I} have in ~ \ latex",
            r"\_\% of do\$llar an\&d \#HASHES \{I\} have in \textasciitilde{} \textbackslash{} latex",
        );
        cmp(
            r#"Lorem ipsum dolor sit amet,#foo>bar&foo"bar\foo/bar"#,
            r#"Lorem ipsum dolor sit amet,\#foo>bar\&foo"bar\textbackslash{}foo/bar"#,
        );
    }
}
