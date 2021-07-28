//! This module defines core data types.

#[cfg(test)]
mod tests;

use crate::error::{AppError, AppResult, TransactionError, TransactionResult};
use rust_decimal::prelude::Decimal;
use serde_derive::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use tokio_stream::StreamExt;

#[cfg(not(feature = "async_file_reads"))]
use csv_async::AsyncReaderBuilder;
#[cfg(feature = "async_file_reads")]
use {
    async_stream::{stream, AsyncStream},
    std::future::Future,
};

/// An instance of this type acts as a transaction engine.
/// It is fed CSV files, which are read and processed asynchronously.
#[derive(Debug, Deserialize)]
pub struct Transactor {
    pub(crate) accounts: BTreeMap<ClientId, Account>,
}

impl Transactor {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            accounts: BTreeMap::new(),
        }
    }

    #[cfg(not(feature = "async_file_reads"))]
    /// Synchronously read, deserialize and process the transactions in a
    /// `CSV` file to an async Stream.
    ///
    /// It is assumed that the last transaction in one `CSV` file is ordered
    /// in time strictly before the first item of the next CSV file.
    pub async fn process_csv_file(&mut self, filepath: PathBuf) -> AppResult<()> {
        let file = tokio::fs::File::open(filepath).await?;
        let reader = AsyncReaderBuilder::new()
            .trim(csv_async::Trim::All) // Allow nicely aligned columns
            .flexible(true) // Allow rows of type dispute, resolve & chargeback
            .comment(Some(b'#')) // Allow #-prefixed line comments
            .create_deserializer(file);
        let mut transactions_stream: csv_async::DeserializeRecordsIntoStream<_, _> =
            reader.into_deserialize::<Transaction>();
        while let Some(csv_async_result) = transactions_stream.next().await {
            let transaction: Transaction = csv_async_result?;
            let result = self.process_transaction(transaction).await;
            if let Err(_transaction_error) = result {
                // NOTE: The transaction failed. To prevent producing
                //       undesirable output, for now both the error
                //       and the transaction itself are ignored.
                //       This would be inadvisable in a real-world system,
                //       of course, and this note would be replaced by
                //       error handling code and logging.
                // return Err(_transaction_error);
            }
        }
        Ok(())
    }

    #[cfg(feature = "async_file_reads")]
    /// Asynchronously read, deserialize and process the transactions
    /// in a `CSV` file to an async Stream using `tokio-uring` (which in turn
    /// is built on the Linux kernel `io_uring` feature, which provides truly
    /// async functionality, including async I/O. When not using the `io_uring`
    /// APIs, all I/O is scheduled in a kernel-level thread pool, but still
    /// fundamentally synchronously executed).
    ///
    /// It is assumed that the last transaction in one `CSV` file is ordered
    /// in time strictly before the first item of the next CSV file.
    pub async fn process_csv_file(&mut self, filepath: PathBuf) -> AppResult<()> {
        let transaction_results: AsyncStream<AppResult<Transaction>, _> =
            Transaction::stream_from_csv_file(filepath).await?;
        tokio::pin!(transaction_results);
        while let Some(transaction_result) = transaction_results.next().await {
            let transaction: Transaction = transaction_result?;
            let result = self.process_transaction(transaction).await;
            if let Err(_transaction_error) = result {
                // NOTE: The transaction failed. To prevent producing
                //       undesirable output, for now both the error
                //       and the transaction itself are ignored.
                //       This would be inadvisable in a real-world system,
                //       of course, and this note would be replaced by
                //       error handling code and logging.
                // return Err(_transaction_error);
            }
        }
        Ok(())
    }

    #[rustfmt::skip]
    /// Process a single transaction.
    pub(crate) async fn process_transaction(
        &mut self,
        t: Transaction
    ) -> TransactionResult<()> {
        match t.ttype {
            TransactionType::Deposit    => self.deposit(&t).await,
            TransactionType::Withdrawal => self.withdraw(&t).await,
            TransactionType::Dispute    => self.dispute(&t).await,
            TransactionType::Resolve    => self.resolve(&t).await,
            TransactionType::Chargeback => self.chargeback(&t).await,
        }
    }

    /// Handle a deposit transaction.
    async fn deposit(&mut self, t: &Transaction) -> TransactionResult<()> {
        let account = self.account_mut(t.cid).await?;
        let amount = t.amount.ok_or(TransactionError::MalformedInputData)?;
        account.available = account.available + amount;
        account.total = account.total + amount;
        Self::ensure_account_balance_invariant(&account).await?;
        account.processed_transactions.insert(t.tid, *t);
        Ok(())
    }

    /// Handle a withdrawal transaction.
    async fn withdraw(&mut self, t: &Transaction) -> TransactionResult<()> {
        let account = self.account_mut(t.cid).await?;
        let amount = t.amount.ok_or(TransactionError::MalformedInputData)?;
        Self::ensure_account_has_sufficient_funds_available(&account, amount).await?;
        account.available = account.available - amount;
        account.total = account.total - amount;
        Self::ensure_account_balance_invariant(&account).await?;
        account.processed_transactions.insert(t.tid, *t);
        Ok(())
    }

    /// Handle a dispute transaction.
    async fn dispute(&mut self, dispute: &Transaction) -> TransactionResult<()> {
        let account = self.account_mut(dispute.cid).await?;
        if let Some(disputed) = account.processed_transactions.get(&dispute.tid) {
            // NOTE: Found the `disputed` transaction that the `dispute` refers to
            let disputed_amount = disputed.amount.unwrap(
                // This should be safe as long as `disputed.ttype` is either
                // TransactionType::Deposit or TransactionType::Withdrawal.
                // The data is malformed if the field equals neither value.
            );
            Self::ensure_account_balance_invariant(&account).await?;
            account.available = account.available - disputed_amount;
            account.held = account.held + disputed_amount;
            Self::ensure_account_balance_invariant(&account).await?;
            // NOTE: mark the `dispute` transaction as disputed:
            account.disputed_transactions.insert(dispute.tid, *disputed);
            let _ = account.processed_transactions.remove(&dispute.tid);
            Ok(())
        } else {
            // NOTE: The account mentioned in the dispute doesn't exist.
            Err(TransactionError::NoSuchProcessedTransactionForClient {
                tid: dispute.tid,
                cid: account.id,
            })
        }
    }

    /// Handle a dispute resolution transaction.
    async fn resolve(&mut self, dispute: &Transaction) -> TransactionResult<()> {
        let account = self.account_mut(dispute.cid).await?;
        if let Some(disputed) = account.disputed_transactions.get(&dispute.tid) {
            // NOTE: Found the `disputed` transaction that the `dispute` refers to
            let disputed_amount = disputed.amount.unwrap(
                // This should be safe as long as `disputed.ttype` is either
                // TransactionType::Deposit or TransactionType::Withdrawal.
                // The data is malformed if the field equals neither value.
            );
            Self::ensure_account_balance_invariant(&account).await?;
            account.available = account.available + disputed_amount;
            account.held = account.held - disputed_amount;
            Self::ensure_account_balance_invariant(&account).await?;
            // NOTE: mark the `dispute` transaction as resolved:
            account.resolved_transactions.insert(dispute.tid, *disputed);
            let _ = account.disputed_transactions.remove(&dispute.tid);
            Ok(())
        } else {
            // NOTE: The account mentioned in the dispute doesn't exist.
            Err(TransactionError::NoSuchDisputedTransactionForClient {
                tid: dispute.tid,
                cid: account.id,
            })
        }
    }

    /// Handle a chargeback transaction.
    async fn chargeback(&mut self, dispute: &Transaction) -> TransactionResult<()> {
        let account = self.account_mut(dispute.cid).await?;
        if let Some(disputed) = account.resolved_transactions.get(&dispute.tid) {
            // NOTE: Found the `disputed` transaction that the `dispute` refers to
            let disputed_amount = disputed.amount.unwrap(
                // This should be safe as long as `disputed.ttype` is either
                // TransactionType::Deposit or TransactionType::Withdrawal.
                // The data is malformed if the field equals neither value.
            );
            Self::ensure_account_balance_invariant(&account).await?;
            account.total = account.total - disputed_amount;
            account.held = account.held - disputed_amount;
            Self::ensure_account_balance_invariant(&account).await?;
            // NOTE: mark the `dispute` transaction as charged back:
            account
                .charged_back_transactions
                .insert(dispute.tid, *disputed);
            let _ = account.resolved_transactions.remove(&dispute.tid);
            account.freeze();
            Ok(())
        } else {
            // NOTE: The account mentioned in the dispute doesn't exist.
            Err(TransactionError::NoSuchResolvedTransactionForClient {
                tid: dispute.tid,
                cid: account.id,
            })
        }
    }

    pub async fn print_output(&self) {
        println!("client,available,held,total,locked");
        for (ClientId(cid), account) in self.accounts.iter() {
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

    #[inline]
    /// Access an account based on `ClientId`. If successful, several checks
    /// are performed to ensure that the account is in the correct state.
    async fn account_mut(&mut self, cid: ClientId) -> TransactionResult<&mut Account> {
        self.ensure_client_account_exists(cid).await?;
        let account = self.accounts.get_mut(&cid).unwrap(
            // NOTE: Should be safe b/c of the `ensure_client_account_exists()`
            //       call above. If this panicks, then that's definitely a bug.
        );
        Self::ensure_account_is_not_locked(&account).await?;
        Self::ensure_account_balance_invariant(&account).await?;
        Ok(account)
    }

    #[inline]
    /// Ensure a client account exists. This is accomplished by opening
    /// an account for the client `id` if no such account exists yet.
    async fn ensure_client_account_exists(&mut self, cid: ClientId) -> TransactionResult<()> {
        if !self.accounts.contains_key(&cid) {
            self.accounts.insert(cid, Account::new(cid));
        }
        Ok(())
    }

    #[inline]
    /// Ensure a client account exists. This is accomplished by opening
    /// an account for the client `id` if no such account exists yet.
    async fn ensure_account_is_not_locked(account: &Account) -> TransactionResult<()> {
        if account.is_locked {
            Err(TransactionError::AccountIsLocked { cid: account.id })
        } else {
            Ok(())
        }
    }

    #[inline]
    /// Ensure that the addition of available funds + held funds
    /// for a given `account` equals its total funds.
    /// This should hold before and after any transaction.
    async fn ensure_account_balance_invariant(account: &Account) -> TransactionResult<()> {
        if account.available + account.held == account.total {
            Ok(())
        } else {
            Err(TransactionError::AccountBalanceInvariantViolated { cid: account.id })
        }
    }

    #[inline]
    /// Ensure that an `account` has >= `amount` of funds available.
    async fn ensure_account_has_sufficient_funds_available(
        account: &Account,
        amount: Currency,
    ) -> TransactionResult<()> {
        if account.available >= amount {
            Ok(())
        } else {
            Err(TransactionError::AccountHasInsufficientFundsAvailable { cid: account.id })
        }
    }
}

// NOTE: The `*_transactions` fields are of type `BTreeMap<_, _>`
//       to preserve ordering (which is temporal) while also allowing
//       non-sequential storage of transactions.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct Account {
    pub(crate) id: ClientId,
    pub(crate) available: Currency,
    pub(crate) held: Currency,
    pub(crate) total: Currency,
    pub(crate) is_locked: bool,
    /// Transactions that have been processed, and are not disputed
    pub(crate) processed_transactions: BTreeMap<TransactionId, Transaction>,
    /// Transactions that have been disputed
    pub(crate) disputed_transactions: BTreeMap<TransactionId, Transaction>,
    /// Transactions that have been disputed, and the dispute has been resolved
    pub(crate) resolved_transactions: BTreeMap<TransactionId, Transaction>,
    /// Transactions that have been charged back
    pub(crate) charged_back_transactions: BTreeMap<TransactionId, Transaction>,
}

impl Account {
    #[inline(always)]
    fn new(id: ClientId) -> Self {
        Self {
            id,
            available: Currency::ZERO,
            held: Currency::ZERO,
            total: Currency::ZERO,
            is_locked: false,
            processed_transactions: BTreeMap::new(),
            disputed_transactions: BTreeMap::new(),
            resolved_transactions: BTreeMap::new(),
            charged_back_transactions: BTreeMap::new(),
        }
    }

    #[inline(always)]
    fn freeze(&mut self) {
        self.is_locked = true;
    }
}

// NOTE: I purposely left out the actual currency designation, since the
// assignment has done so as well. It's a unicurrency, unibank world.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub(crate) struct Currency(Decimal);

impl Currency {
    const ZERO: Self = Self(Decimal::ZERO);

    #[allow(unused)]
    pub fn from_str(amount: &str) -> AppResult<Self> {
        // NOTE: used for testing purposes
        use std::str::FromStr;
        match Decimal::from_str(amount) {
            Ok(decimal) => Ok(Self(decimal)),
            Err(_) => Err(AppError::FailedToParseDecimal {
                decimal: amount.to_string(),
            }),
        }
    }
}

impl fmt::Debug for Currency {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // NOTE: These Debug printouts are so short that it's more useful and
        //       comprehensible to always print them on 1 line, rather
        //       than 3 lines (as is the case with deriving the Debug impl).
        write!(f, "{:.4?}", self.0)
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // NOTE: The choice of $ is fairly arbitrary here, since the
        //       actual currency has been abstracted away (note the
        //       lack of a dollar/euro/whatever designation in the
        //       type definition).
        write!(f, "${:.4?}", self.0)
    }
}

impl std::ops::Add<Self> for Currency {
    type Output = Self;

    #[inline(always)]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl std::ops::Sub<Self> for Currency {
    type Output = Self;

    #[inline(always)]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct IgnoredTransaction {
    /// The actual transaction being ignored.
    transaction: Transaction,
    /// The reason that `transaction` was ignored.
    reason: TransactionError,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct Transaction {
    #[serde(rename = "type")]
    ttype: TransactionType,
    #[serde(rename = "client")]
    cid: ClientId,
    #[serde(rename = "tx")]
    tid: TransactionId,
    amount: Option<Currency>,
}

impl Transaction {
    #[cfg(feature = "async_file_reads")]
    /// Stream transactions from a CSV file located @ `filepath`.
    async fn stream_from_csv_file(
        filepath: PathBuf,
    ) -> AppResult<AsyncStream<AppResult<Self>, impl Future<Output = ()>>> {
        Ok(stream! {
            const CAPACITY: usize = 8192;
            let file = tokio_uring::fs::File::open(filepath).await?;
            // NOTE: The `buffer` is used to communicate with the kernel...
            let mut buffer: Vec<u8> = vec![0; CAPACITY];
            // NOTE: ... but the kernel has no idea where Unicode char
            // boundaries are located.  So in order to prevent broken
            // graphemes, `accumulator` is used as a circular buffer.
            let mut accumulator: Vec<u8> = Vec::with_capacity(CAPACITY);
            let mut headers: Vec<String> = vec![];
            let mut byte_offset: u64 = 0;
            let mut lineno: usize = 0;
            loop {
                // NOTE: Read some data, the `buffer` is passed by ownership
                // and submitted to the kernel. When the operation completes,
                // the kernel gives ownership of the buffer back.
                let (result, buf) = file.read_at(buffer, byte_offset).await;
                buffer = buf;
                let num_bytes_read = result?;
                if num_bytes_read == 0 {
                    break; // NOTE: Reached EOF
                }
                byte_offset += num_bytes_read as u64;
                accumulator.extend(&buffer[.. num_bytes_read]);
                const NEWLINE: &[u8] = "\n".as_bytes();
                while let Some(newline_idx) = find(NEWLINE, &accumulator) {
                    let line: &[u8] = &accumulator[.. newline_idx];
                    let line: &str = std::str::from_utf8(line)?;
                    if lineno == 0 { // NOTE: parse the headers
                        lineno += 1;
                        let columns = line.split(',');
                        headers = columns
                            .map(str::trim)
                            .map(String::from)
                            .collect();
                        let _ = accumulator
                            .drain(.. newline_idx + NEWLINE.len()) // drain the line
                            .collect::<Vec<_>>();
                    } else {
                        lineno += 1;
                        // NOTE: create a `Transaction` value and stream it:
                        let transaction =
                            Transaction::from_csv_line(&*headers, line).await;
                        let _ = accumulator
                            .drain(.. newline_idx + NEWLINE.len()) // drain the line
                            .collect::<Vec<_>>();
                        yield transaction;
                    }
                }
            }
            assert!(accumulator.is_empty(), "The accumulator isn't empty");
        })
    }

    #[rustfmt::skip]
    #[cfg(feature = "async_file_reads")]
    async fn from_csv_line<S: AsRef<str>>(
        headers: &[S],
        line: &str
    ) -> AppResult<Self> {
        let mut transaction = Self::default();
        let columns = line.split(',');
        for (value, header) in columns.map(str::trim).zip(headers.iter()) {
            match header.as_ref() {
                "type" => {
                    transaction.ttype = match value {
                        "deposit" => TransactionType::Deposit,
                        "withdrawal" => TransactionType::Withdrawal,
                        "dispute" => TransactionType::Dispute,
                        "resolve" => TransactionType::Resolve,
                        "chargeback" => TransactionType::Chargeback,
                        _ => panic!("unknown type '{}'", value),
                    }
                }
                "client" => transaction.cid = ClientId(value.parse()?),
                "tx" => transaction.tid = TransactionId(value.parse()?),
                "amount" => {
                    transaction.amount = match transaction.ttype {
                        TransactionType::Deposit | TransactionType::Withdrawal => {
                            Some(Currency::from_str(&value)?)
                        }
                        _ => None,
                    }
                },
                header => panic!("unknown header '{}'", header),
            }
        }
        Ok(transaction)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum TransactionType {
    #[serde(rename = "deposit")]
    Deposit,
    #[serde(rename = "withdrawal")]
    Withdrawal,
    #[serde(rename = "dispute")]
    Dispute,
    #[serde(rename = "resolve")]
    Resolve,
    #[serde(rename = "chargeback")]
    Chargeback,
}

impl Default for TransactionType {
    #[inline(always)]
    fn default() -> Self {
        Self::Chargeback
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
pub struct ClientId(pub(crate) u16); // Newtyped for type safety reasons

impl fmt::Debug for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // NOTE: These Debug printouts are so short that it's more useful and
        //       comprehensible to always print them on 1 line, rather
        //       than 3 lines (as is the case with deriving the Debug impl and
        //       then using the alternate flag i.e. {:#?} rather than {:?} for
        //       debug formatting).
        write!(f, "ClientId({})", self.0)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
pub struct TransactionId(u32); // Newtyped for type safety reasons

impl fmt::Debug for TransactionId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // NOTE: These Debug printouts are so short that it's more useful and
        //       comprehensible to always print them on 1 line, rather
        //       than 3 lines (as is the case with deriving the Debug impl and
        //       then using the alternate flag i.e. {:#?} rather than {:?} for
        //       debug formatting).
        write!(f, "TransactionId({})", self.0)
    }
}

#[cfg(feature = "async_file_reads")]
/// Find a `needle` in a `haystack`.
fn find(needle: &[u8], haystack: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
