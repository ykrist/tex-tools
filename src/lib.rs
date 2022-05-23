#![allow(unused)]

use std::{fmt::Display, path::Path};

pub use anyhow::{anyhow, bail, Context as ErrContext, Result};
pub use clap::{Args, Parser};
pub use tracing::{
    debug, debug_span, error, error_span, info, info_span, instrument, trace, trace_span, warn,
    warn_span,
};

mod escape;
pub use escape::*;

use serde::{de::DeserializeOwned, Serialize};

pub mod biblatex;

use posix_cli_utils::IoContext;

pub fn read_json<T, P>(path: P) -> Result<T>
where
    T: DeserializeOwned,
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let f = std::fs::File::open(path).context_read(&path)?;
    let val = serde_json::from_reader(f)?;
    Ok(val)
}

pub fn write_json<T, P>(path: P, val: T) -> Result<()>
where
    T: Serialize,
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let f = std::fs::File::create(path).context_write(&path)?;
    serde_json::to_writer(f, &val)?;
    Ok(())
}

pub fn write_json_pretty<T, P>(path: P, val: T) -> Result<()>
where
    T: Serialize,
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let f = std::fs::File::create(path).context_write(&path)?;
    serde_json::to_writer_pretty(f, &val)?;
    Ok(())
}

pub fn logging_init() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    tracing_subscriber::registry()
        .with(
            fmt::layer()
                // .pretty()
                .without_time(),
        )
        .with(EnvFilter::from_default_env())
        .init();
}

pub fn logging_init_test() {
    use tracing_subscriber::{fmt, prelude::*};
    tracing_subscriber::fmt().without_time().try_init().ok();
}
