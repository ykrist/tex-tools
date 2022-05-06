use std::fmt::Display;

pub struct Utf8ToTex<'a> {
    original: &'a str,
}

impl<'a> Display for Utf8ToTex<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

pub fn utf8_to_tex(s: &str) -> Utf8ToTex {
    todo!()
}
