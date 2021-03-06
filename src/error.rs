//!

use crate::core::{ClientId, TransactionId};
use csv_async::Error as CsvAsyncError;
use serde_derive::Deserialize;
use std::io::Error as IoError;
use std::num::ParseIntError;
use std::str::Utf8Error;
use tokio::task::JoinError as TokioJoinError;

pub type AppResult<T> = std::result::Result<T, AppError>;

#[derive(Debug)]
pub enum AppError {
    CsvAsyncError(CsvAsyncError),
    FailedToParseDecimal { decimal: String },
    IoError(IoError),
    NoFileNameCliArgFound,
    ParseIntError(ParseIntError),
    TokioJoinError(TokioJoinError),
    TransactionError(TransactionError),
    Utf8Error(Utf8Error),
}

impl From<CsvAsyncError> for AppError {
    #[inline(always)]
    fn from(e: CsvAsyncError) -> Self {
        Self::CsvAsyncError(e)
    }
}

impl From<IoError> for AppError {
    #[inline(always)]
    fn from(e: IoError) -> Self {
        Self::IoError(e)
    }
}

impl From<ParseIntError> for AppError {
    #[inline(always)]
    fn from(e: ParseIntError) -> Self {
        Self::ParseIntError(e)
    }
}

impl From<TokioJoinError> for AppError {
    #[inline(always)]
    fn from(e: TokioJoinError) -> Self {
        Self::TokioJoinError(e)
    }
}

impl From<TransactionError> for AppError {
    #[inline(always)]
    fn from(e: TransactionError) -> Self {
        Self::TransactionError(e)
    }
}

impl From<Utf8Error> for AppError {
    #[inline(always)]
    fn from(e: Utf8Error) -> Self {
        Self::Utf8Error(e)
    }
}

pub type TransactionResult<T> = std::result::Result<T, TransactionError>;

// NOTE: `TransactionError`s have been split off into their own error type
// rather than being incorporated directly into AppError, because these errors
// can derive additional useful traits that some of the AppError variants (and
// therefore the AppError type as a whole) cannot.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum TransactionError {
    AccountBalanceInvariantViolated {
        cid: ClientId,
    },
    AccountHasInsufficientFundsAvailable {
        cid: ClientId,
    },
    AccountIsLocked {
        cid: ClientId,
    },
    MalformedInputData,
    /// There is no processed transaction with the given `TransactionId` for the
    /// client account with the given `ClientId`.
    NoSuchProcessedTransactionForClient {
        tid: TransactionId,
        cid: ClientId,
    },
    /// There is no disputed transaction with the given `TransactionId` for the
    /// client account with the given `ClientId`.
    NoSuchDisputedTransactionForClient {
        tid: TransactionId,
        cid: ClientId,
    },
    /// There is no resolved transaction with the given `TransactionId` for the
    /// client account with the given `ClientId`.
    NoSuchResolvedTransactionForClient {
        tid: TransactionId,
        cid: ClientId,
    },
}
