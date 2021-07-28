//! This crate can run in one of 2 modes:
//! * Synchronous read (using `csv-async`) to an async stream
//! * Async read (using `tokio-uring`) to an async stream
//!
//! In either case the processing happens asynchronously.
//!
//!
//! Note that the `main` functions are feature-wise overloaded because at
//! least for now, using `tokio-uring` requires starting a reactor instance
//! that is part of the `tokio-uring` crate rather than the one provided by
//! the `tokio` crate.
//! Writing separate `main` functions is a reasonable
//! way of papering over the different code paths.

use giant_squid::core::*;
use giant_squid::error::{AppError, AppResult};
use std::path::PathBuf;

#[cfg(not(feature = "async_file_reads"))]
#[tokio::main]
async fn main() -> AppResult<()> {
    tokio::spawn(process_transactions_future()).await?
}

#[cfg(feature = "async_file_reads")]
// Note the absence of the `#[tokio::main]` attribute.
// This fn is also not async.
fn main() -> AppResult<()> {
    tokio_uring::start(process_transactions_future())
}

async fn process_transactions_future() -> AppResult<()> {
    let filepath = get_filepath_from_cli_arg()?;
    let mut transactor = Transactor::new();
    transactor.process_csv_file(filepath).await?;
    // NOTE: Unslash this println!() call for a peek at the `transactor`
    //       state after it's done processing all the transactions:
    // println!("transactor: {:#?}", transactor);
    transactor.print_output().await;
    Ok(())
}

fn get_filepath_from_cli_arg() -> AppResult<PathBuf> {
    match std::env::args_os().nth(1) {
        None => Err(AppError::NoFileNameCliArgFound),
        Some(path) => Ok(PathBuf::from(path)),
    }
}
