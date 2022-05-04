#![allow(unused)]

use std::{fmt::Display, path::Path};

pub use anyhow::{bail, anyhow, Result, Context as ErrContext};
pub use clap::{Parser, Args};
pub use tracing::{
    error,
    error_span,
    warn,
    warn_span,
    info,
    info_span,
    debug,
    debug_span,
    trace,
    trace_span,
    instrument,
};

mod escape;
pub use escape::*;

mod crossref;
pub use crossref::*;
use serde::de::DeserializeOwned;

pub fn read_json<T, P>(path: P) -> Result<T> 
where 
    T: DeserializeOwned,
    P: AsRef<Path>
{
    let path = path.as_ref();
    let f = std::fs::File::open(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    
    let val =serde_json::from_reader(f)?;
    Ok(val)
}


pub fn logging_init() {
    tracing_subscriber::fmt()
        // .pretty()
        .without_time()
        .with_target(false)
        .init()
}