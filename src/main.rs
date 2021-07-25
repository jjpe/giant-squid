//!

mod core;
mod error;

use crate::core::*;
use crate::error::{AppError, AppResult};
use std::path::PathBuf;
use tokio;

#[tokio::main]
async fn main() -> AppResult<()> {
    tokio::spawn(run_transaction_engine()).await?
}

async fn run_transaction_engine() -> AppResult<()> {
    let mut transactor = Transactor::new();
    let filepath = get_filepath_from_cli_arg()?;
    transactor.process_csv_file(&filepath).await?;
    // NOTE: Unslash this println!() call for a peek at the `transactor`
    //       state after it's done processing all the transactions:
    // println!("transactor: {:#?}", transactor);
    print_output(&transactor).await;
    Ok(())
}

async fn print_output(transactor: &Transactor) {
    println!("client,available,held,total,locked");
    for (ClientId(cid), account) in transactor.accounts.iter() {
        let Account {
            available,
            held,
            total,
            is_locked,
            ..
        } = &account;
        println!(
            "{},{:?},{:?},{:?},{}",
            cid, available, held, total, is_locked
        );
    }
}

fn get_filepath_from_cli_arg() -> AppResult<PathBuf> {
    match std::env::args_os().nth(1) {
        None => Err(AppError::NoFileNameCliArgFound),
        Some(path) => Ok(PathBuf::from(path)),
    }
}
