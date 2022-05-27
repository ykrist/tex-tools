use anyhow::*;
use std::fmt::Write;
use std::path::{Path, PathBuf};

fn examples_codegen() -> Result<String> {
    struct Example {
        name: String,
        bib: String,
        json: String,
    }

    let d = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/biblatex");
    let mut examples = Vec::new();
    for f in std::fs::read_dir(d)? {
        let mut f = f?.path();
        if f.extension().map(|e| e.to_str()).flatten() == Some("bib") {
            let name = f.file_stem().unwrap().to_str().unwrap().to_string();
            let bib = f.to_str().unwrap().to_string();
            f.set_extension("json");
            let json = f.to_str().unwrap().to_string();
            examples.push(Example { name, bib, json })
        }
    }

    let mut code = "const EXAMPLES: &[Example] = &[\n".to_string();
    for e in examples {
        writeln!(
            code,
            "Example {{ name: \"{}\", bib: include_str!(\"{}\"), json: include_str!(\"{}\")}},",
            &e.name, &e.bib, &e.json,
        )?;
    }
    writeln!(code, "];")?;
    Ok(code)
}

fn main() -> Result<()> {
    let mut path: PathBuf = std::env::var("OUT_DIR")?.into();
    path.push("example_files.rs");
    std::fs::write(&path, examples_codegen()?)?;
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}
