//! This module defines core data types.

use crate::error::{AppError, AppResult, TransactionError, TransactionResult};
use csv_async::AsyncReaderBuilder;
use rust_decimal::prelude::Decimal;
use serde_derive::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use tokio::fs::File;
use tokio_stream::StreamExt;

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

    /// Asynchronously read and process the transactions in a `CSV` file.
    /// It is assumed that the last transaction in one `CSV` file is ordered
    /// in time strictly before the first item of the next CSV file.
    pub async fn process_csv_file<P>(&mut self, filepath: P) -> AppResult<()>
    where
        P: AsRef<Path>,
    {
        let file = File::open(filepath).await?;
        let reader = AsyncReaderBuilder::new()
            .trim(csv_async::Trim::All) // Allow nicely aligned columns
            .flexible(true) // Allow rows of type dispute, resolve & chargeback
            .comment(Some(b'#')) // Allow #-prefixed line comments
            .create_deserializer(file);
        let mut transactions_stream: csv_async::DeserializeRecordsIntoStream<_, _> =
            reader.into_deserialize::<Transaction>();
        while let Some(csv_async_result) = transactions_stream.next().await {
            let transaction: Transaction = csv_async_result?;
            self.process_transaction(transaction).await?;
        }
        Ok(())
    }

    /// Process a single transaction.
    pub(crate) async fn process_transaction(&mut self, t: Transaction) -> TransactionResult<()> {
        let result: TransactionResult<()> = match t.ttype {
            TransactionType::Deposit => self.deposit(&t).await,
            TransactionType::Withdrawal => self.withdraw(&t).await,
            TransactionType::Dispute => self.dispute(&t).await,
            TransactionType::Resolve => self.resolve(&t).await,
            TransactionType::Chargeback => self.chargeback(&t).await,
        };
        if let Err(transaction_error) = result {
            // NOTE: The transaction failed. To prevent producing
            //       undesirable output, for now both the error
            //       and the transaction itself are ignored.
            //       This would be inadvisable in a real-world system,
            //       of course, and this note would be replaced by
            //       error handling code and logging.
            let account = self.account_mut(t.cid).await?;
            account.ignored_transactions.insert(
                t.tid,
                IgnoredTransaction {
                    transaction: t,
                    reason: transaction_error,
                },
            );
        }
        Ok(())
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
        if account.available - amount >= Currency::ZERO {
            Ok(())
        } else {
            Err(TransactionError::AccountHasInsufficientFundsAvailable)
        }
    }
}

// NOTE: The `*_transactions` fields are of type `BTreeMap<_, _>`
//       to preserve ordering (which is temporal) while also allowing
//       non-sequential storage of transactions.
#[derive(Debug, Deserialize)]
pub(crate) struct Account {
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
    /// Transactions that have been ignored
    pub(crate) ignored_transactions: BTreeMap<TransactionId, IgnoredTransaction>,
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
            ignored_transactions: BTreeMap::new(),
        }
    }

    #[inline(always)]
    fn freeze(&mut self) {
        self.is_locked = true;
    }
}

// NOTE: I purposely left out the actual currency designation, since the
// assignment has done so as well. It's a unicurrency, unibank world.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct Transaction {
    #[serde(rename = "type")]
    ttype: TransactionType,
    #[serde(rename = "client")]
    cid: ClientId,
    #[serde(rename = "tx")]
    tid: TransactionId,
    amount: Option<Currency>,
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn deposit_to_new_account() -> AppResult<()> {
        let mut transactor = Transactor::new();
        let transactions = vec![Transaction {
            ttype: TransactionType::Deposit,
            cid: ClientId(1),
            tid: TransactionId(1),
            amount: Some(Currency::from_str("1.23476")?),
        }];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("1.23476")?);
        assert_eq!(*held, Currency::from_str("0.0000")?);
        assert_eq!(*total, Currency::from_str("1.23476")?);
        assert_eq!(*is_locked, false);
        assert_eq!(
            processed_transactions.iter().collect::<Vec<_>>(),
            vec![(
                &TransactionId(1),
                &Transaction {
                    ttype: TransactionType::Deposit,
                    cid: ClientId(1),
                    tid: TransactionId(1),
                    amount: Some(Currency::from_str("1.23476")?),
                }
            )]
        );
        assert_eq!(disputed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(resolved_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(charged_back_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(ignored_transactions.iter().collect::<Vec<_>>(), vec![]);
        Ok(())
    }

    #[tokio::test]
    async fn deposit_to_preexisting_account() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let transactions = vec![Transaction {
            ttype: TransactionType::Deposit,
            cid: ClientId(1),
            tid: TransactionId(1),
            amount: Some(Currency::from_str("1.23476")?),
        }];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("1.23476")?);
        assert_eq!(*held, Currency::from_str("0.0000")?);
        assert_eq!(*total, Currency::from_str("1.23476")?);
        assert_eq!(*is_locked, false);
        assert_eq!(
            processed_transactions.iter().collect::<Vec<_>>(),
            vec![(
                &TransactionId(1),
                &Transaction {
                    ttype: TransactionType::Deposit,
                    cid: ClientId(1),
                    tid: TransactionId(1),
                    amount: Some(Currency::from_str("1.23476")?),
                }
            )]
        );
        assert_eq!(disputed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(resolved_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(charged_back_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(ignored_transactions.iter().collect::<Vec<_>>(), vec![]);
        Ok(())
    }

    #[tokio::test]
    async fn deposit_to_locked_account() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let account = transactor.account_mut(ClientId(1)).await?;
        account.freeze();
        let transactions = vec![Transaction {
            ttype: TransactionType::Deposit,
            cid: ClientId(1),
            tid: TransactionId(1),
            amount: Some(Currency::from_str("1.23476")?),
        }];
        for transaction in transactions {
            assert_eq!(
                transactor.process_transaction(transaction).await,
                Err(TransactionError::AccountIsLocked { cid: ClientId(1) })
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn successive_deposits() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let transactions = vec![
            Transaction {
                ttype: TransactionType::Deposit,
                cid: ClientId(1),
                tid: TransactionId(1),
                amount: Some(Currency::from_str("0.9975")?),
            },
            Transaction {
                ttype: TransactionType::Deposit,
                cid: ClientId(1),
                tid: TransactionId(2),
                amount: Some(Currency::from_str("49.0025")?),
            },
        ];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        // println!("{:#?}", transactor);
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("50.0000")?);
        assert_eq!(*held, Currency::from_str("0.0000")?);
        assert_eq!(*total, Currency::from_str("50.0000")?);
        assert_eq!(*is_locked, false);
        assert_eq!(
            processed_transactions.iter().collect::<Vec<_>>(),
            vec![
                (
                    &TransactionId(1),
                    &Transaction {
                        ttype: TransactionType::Deposit,
                        cid: ClientId(1),
                        tid: TransactionId(1),
                        amount: Some(Currency::from_str("0.9975")?),
                    }
                ),
                (
                    &TransactionId(2),
                    &Transaction {
                        ttype: TransactionType::Deposit,
                        cid: ClientId(1),
                        tid: TransactionId(2),
                        amount: Some(Currency::from_str("49.0025")?),
                    }
                )
            ]
        );
        assert_eq!(disputed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(resolved_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(charged_back_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(ignored_transactions.iter().collect::<Vec<_>>(), vec![]);
        Ok(())
    }

    #[tokio::test]
    async fn withdraw_from_new_account() -> AppResult<()> {
        let mut transactor = Transactor::new();
        let transactions = vec![Transaction {
            ttype: TransactionType::Withdrawal,
            cid: ClientId(1),
            tid: TransactionId(1),
            amount: Some(Currency::from_str("0.9975")?),
        }];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("0.0000")?);
        assert_eq!(*held, Currency::from_str("0.0000")?);
        assert_eq!(*total, Currency::from_str("0.0000")?);
        assert_eq!(*is_locked, false);
        assert_eq!(processed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(disputed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(resolved_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(charged_back_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(
            ignored_transactions.iter().collect::<Vec<_>>(),
            vec![(
                &TransactionId(1),
                &IgnoredTransaction {
                    transaction: Transaction {
                        ttype: TransactionType::Withdrawal,
                        cid: ClientId(1),
                        tid: TransactionId(1),
                        amount: Some(Currency::from_str("0.9975")?),
                    },
                    reason: TransactionError::AccountHasInsufficientFundsAvailable,
                }
            )]
        );
        Ok(())
    }

    #[tokio::test]
    async fn withdraw_from_locked_account() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let account = transactor.account_mut(ClientId(1)).await?;
        account.freeze();
        let transactions = vec![Transaction {
            ttype: TransactionType::Withdrawal,
            cid: ClientId(1),
            tid: TransactionId(1),
            amount: Some(Currency::from_str("1.23476")?),
        }];
        for transaction in transactions {
            assert_eq!(
                transactor.process_transaction(transaction).await,
                Err(TransactionError::AccountIsLocked { cid: ClientId(1) })
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn withdraw_from_preexisting_account_with_insufficient_funds() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let transactions = vec![
            Transaction {
                ttype: TransactionType::Withdrawal,
                cid: ClientId(1),
                tid: TransactionId(1),
                amount: Some(Currency::from_str("0.9975")?),
            },
        ];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("0.0000")?);
        assert_eq!(*held, Currency::from_str("0.0000")?);
        assert_eq!(*total, Currency::from_str("0.0000")?);
        assert_eq!(*is_locked, false);
        assert_eq!(processed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(disputed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(resolved_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(charged_back_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(
            ignored_transactions.iter().collect::<Vec<_>>(),
            vec![(
                &TransactionId(1),
                &IgnoredTransaction {
                    transaction: Transaction {
                        ttype: TransactionType::Withdrawal,
                        cid: ClientId(1),
                        tid: TransactionId(1),
                        amount: Some(Currency::from_str("0.9975")?),
                    },
                    reason: TransactionError::AccountHasInsufficientFundsAvailable,
                }
            )]
        );
        Ok(())
    }

    #[tokio::test]
    async fn withdraw_from_preexisting_account_with_sufficient_funds() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let transactions = vec![
            Transaction {
                // Ensure sufficient funds
                ttype: TransactionType::Deposit,
                cid: ClientId(1),
                tid: TransactionId(1),
                amount: Some(Currency::from_str("10.0000")?),
            },
            Transaction {
                ttype: TransactionType::Withdrawal,
                cid: ClientId(1),
                tid: TransactionId(2),
                amount: Some(Currency::from_str("1.0025")?),
            },
        ];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("8.9975")?);
        assert_eq!(*held, Currency::from_str("0.0000")?);
        assert_eq!(*total, Currency::from_str("8.9975")?);
        assert_eq!(*is_locked, false);
        assert_eq!(
            processed_transactions.iter().collect::<Vec<_>>(),
            vec![
                (
                    &TransactionId(1),
                    &Transaction {
                        ttype: TransactionType::Deposit,
                        cid: ClientId(1),
                        tid: TransactionId(1),
                        amount: Some(Currency::from_str("10.0000")?),
                    }
                ),
                (
                    &TransactionId(2),
                    &Transaction {
                        ttype: TransactionType::Withdrawal,
                        cid: ClientId(1),
                        tid: TransactionId(2),
                        amount: Some(Currency::from_str("1.0025")?),
                    }
                )
            ]
        );
        assert_eq!(disputed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(resolved_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(charged_back_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(ignored_transactions.iter().collect::<Vec<_>>(), vec![]);
        Ok(())
    }

    #[tokio::test]
    async fn successively_withdraw_from_preexisting_account() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let transactions = vec![
            Transaction {
                // Ensure sufficient funds
                ttype: TransactionType::Deposit,
                cid: ClientId(1),
                tid: TransactionId(1),
                amount: Some(Currency::from_str("10.0000")?),
            },
            Transaction {
                ttype: TransactionType::Withdrawal,
                cid: ClientId(1),
                tid: TransactionId(2),
                amount: Some(Currency::from_str("1.0025")?),
            },
            Transaction {
                ttype: TransactionType::Withdrawal,
                cid: ClientId(1),
                tid: TransactionId(3),
                amount: Some(Currency::from_str("0.9975")?),
            },
        ];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("8.0000")?);
        assert_eq!(*held, Currency::from_str("0.0000")?);
        assert_eq!(*total, Currency::from_str("8.0000")?);
        assert_eq!(*is_locked, false);
        assert_eq!(
            processed_transactions.iter().collect::<Vec<_>>(),
            vec![
                (
                    &TransactionId(1),
                    &Transaction {
                        ttype: TransactionType::Deposit,
                        cid: ClientId(1),
                        tid: TransactionId(1),
                        amount: Some(Currency::from_str("10.0000")?),
                    }
                ),
                (
                    &TransactionId(2),
                    &Transaction {
                        ttype: TransactionType::Withdrawal,
                        cid: ClientId(1),
                        tid: TransactionId(2),
                        amount: Some(Currency::from_str("1.0025")?),
                    }
                ),
                (
                    &TransactionId(3),
                    &Transaction {
                        ttype: TransactionType::Withdrawal,
                        cid: ClientId(1),
                        tid: TransactionId(3),
                        amount: Some(Currency::from_str("0.9975")?),
                    }
                )
            ]
        );
        assert_eq!(disputed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(resolved_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(charged_back_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(ignored_transactions.iter().collect::<Vec<_>>(), vec![]);
        Ok(())
    }

    #[tokio::test]
    async fn dispute_nonexistent_transaction() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let transactions = vec![Transaction {
            ttype: TransactionType::Dispute,
            cid: ClientId(1),
            tid: TransactionId(1),
            amount: None,
        }];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("0.0000")?);
        assert_eq!(*held, Currency::from_str("0.0000")?);
        assert_eq!(*total, Currency::from_str("0.0000")?);
        assert_eq!(*is_locked, false);
        assert_eq!(processed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(disputed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(resolved_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(charged_back_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(
            ignored_transactions.iter().collect::<Vec<_>>(),
            vec![(
                &TransactionId(1),
                &IgnoredTransaction {
                    transaction: Transaction {
                        ttype: TransactionType::Dispute,
                        cid: ClientId(1),
                        tid: TransactionId(1),
                        amount: None,
                    },
                    reason: TransactionError::NoSuchProcessedTransactionForClient {
                        tid: TransactionId(1),
                        cid: ClientId(1),
                    },
                }
            ),]
        );
        Ok(())
    }

    #[tokio::test]
    async fn dispute_existent_transaction() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let transactions = vec![
            Transaction {
                ttype: TransactionType::Deposit,
                cid: ClientId(1),
                tid: TransactionId(1),
                amount: Some(Currency::from_str("10.0000")?),
            },
            Transaction {
                ttype: TransactionType::Dispute,
                cid: ClientId(1),
                tid: TransactionId(1),
                amount: None,
            },
        ];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("0.0000")?);
        assert_eq!(*held, Currency::from_str("10.0000")?);
        assert_eq!(*total, Currency::from_str("10.0000")?);
        assert_eq!(*is_locked, false);
        assert_eq!(processed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(
            disputed_transactions.iter().collect::<Vec<_>>(),
            vec![(
                &TransactionId(1),
                &Transaction {
                    ttype: TransactionType::Deposit,
                    cid: ClientId(1),
                    tid: TransactionId(1),
                    amount: Some(Currency::from_str("10.0000")?),
                }
            )]
        );
        assert_eq!(resolved_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(charged_back_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(ignored_transactions.iter().collect::<Vec<_>>(), vec![]);
        Ok(())
    }

    #[tokio::test]
    async fn dispute_using_locked_account() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let account = transactor.account_mut(ClientId(1)).await?;
        account.freeze();
        let transactions = vec![Transaction {
            ttype: TransactionType::Dispute,
            cid: ClientId(1),
            tid: TransactionId(1),
            amount: None,
        }];
        for transaction in transactions {
            assert_eq!(
                transactor.process_transaction(transaction).await,
                Err(TransactionError::AccountIsLocked { cid: ClientId(1) })
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn resolve_nonexistent_disputed_transaction() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let transactions = vec![Transaction {
            ttype: TransactionType::Resolve,
            cid: ClientId(1),
            tid: TransactionId(1),
            amount: None,
        }];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("0.0000")?);
        assert_eq!(*held, Currency::from_str("0.0000")?);
        assert_eq!(*total, Currency::from_str("0.0000")?);
        assert_eq!(*is_locked, false);
        assert_eq!(processed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(disputed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(resolved_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(charged_back_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(
            ignored_transactions.iter().collect::<Vec<_>>(),
            vec![(
                &TransactionId(1),
                &IgnoredTransaction {
                    transaction: Transaction {
                        ttype: TransactionType::Resolve,
                        cid: ClientId(1),
                        tid: TransactionId(1),
                        amount: None,
                    },
                    reason: TransactionError::NoSuchDisputedTransactionForClient {
                        tid: TransactionId(1),
                        cid: ClientId(1),
                    },
                }
            ),]
        );
        Ok(())
    }

    #[tokio::test]
    async fn resolve_existent_disputed_transaction() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let transactions = vec![
            Transaction {
                ttype: TransactionType::Deposit,
                cid: ClientId(1),
                tid: TransactionId(1),
                amount: Some(Currency::from_str("10.0000")?),
            },
            Transaction {
                ttype: TransactionType::Withdrawal,
                cid: ClientId(1),
                tid: TransactionId(2),
                amount: Some(Currency::from_str("5.0000")?),
            },
            Transaction {
                ttype: TransactionType::Dispute,
                cid: ClientId(1),
                tid: TransactionId(2),
                amount: None,
            },
            Transaction {
                ttype: TransactionType::Resolve,
                cid: ClientId(1),
                tid: TransactionId(2),
                amount: None,
            },
        ];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("5.0000")?);
        assert_eq!(*held, Currency::from_str("0.0000")?);
        assert_eq!(*total, Currency::from_str("5.0000")?);
        assert_eq!(*is_locked, false);
        assert_eq!(
            processed_transactions.iter().collect::<Vec<_>>(),
            vec![(
                &TransactionId(1),
                &Transaction {
                    ttype: TransactionType::Deposit,
                    cid: ClientId(1),
                    tid: TransactionId(1),
                    amount: Some(Currency::from_str("10.0000")?)
                }
            )]
        );
        assert_eq!(disputed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(
            resolved_transactions.iter().collect::<Vec<_>>(),
            vec![(
                &TransactionId(2),
                &Transaction {
                    ttype: TransactionType::Withdrawal,
                    cid: ClientId(1),
                    tid: TransactionId(2),
                    amount: Some(Currency::from_str("5.0000")?)
                }
            )]
        );
        assert_eq!(charged_back_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(ignored_transactions.iter().collect::<Vec<_>>(), vec![]);
        Ok(())
    }

    #[tokio::test]
    async fn resolve_using_locked_account() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let account = transactor.account_mut(ClientId(1)).await?;
        account.freeze();
        let transactions = vec![Transaction {
            ttype: TransactionType::Resolve,
            cid: ClientId(1),
            tid: TransactionId(1),
            amount: None,
        }];
        for transaction in transactions {
            assert_eq!(
                transactor.process_transaction(transaction).await,
                Err(TransactionError::AccountIsLocked { cid: ClientId(1) })
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn chargeback_nonexistent_disputed_transaction() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let transactions = vec![Transaction {
            ttype: TransactionType::Chargeback,
            cid: ClientId(1),
            tid: TransactionId(1),
            amount: None,
        }];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("0.0000")?);
        assert_eq!(*held, Currency::from_str("0.0000")?);
        assert_eq!(*total, Currency::from_str("0.0000")?);
        assert_eq!(*is_locked, false);
        assert_eq!(processed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(disputed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(resolved_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(charged_back_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(
            ignored_transactions.iter().collect::<Vec<_>>(),
            vec![(
                &TransactionId(1),
                &IgnoredTransaction {
                    transaction: Transaction {
                        ttype: TransactionType::Chargeback,
                        cid: ClientId(1),
                        tid: TransactionId(1),
                        amount: None,
                    },
                    reason: TransactionError::NoSuchResolvedTransactionForClient {
                        tid: TransactionId(1),
                        cid: ClientId(1),
                    },
                }
            ),]
        );
        Ok(())
    }

    #[tokio::test]
    async fn chargeback_existent_disputed_transaction() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let transactions = vec![
            Transaction {
                ttype: TransactionType::Deposit,
                cid: ClientId(1),
                tid: TransactionId(1),
                amount: Some(Currency::from_str("10.0000")?),
            },
            Transaction {
                ttype: TransactionType::Withdrawal,
                cid: ClientId(1),
                tid: TransactionId(2),
                amount: Some(Currency::from_str("5.0000")?),
            },
            Transaction {
                ttype: TransactionType::Dispute,
                cid: ClientId(1),
                tid: TransactionId(2),
                amount: None,
            },
            Transaction {
                ttype: TransactionType::Resolve,
                cid: ClientId(1),
                tid: TransactionId(2),
                amount: None,
            },
            Transaction {
                ttype: TransactionType::Chargeback,
                cid: ClientId(1),
                tid: TransactionId(2),
                amount: None,
            },
        ];
        for transaction in transactions {
            transactor.process_transaction(transaction).await?;
        }
        let Account {
            id,
            available,
            held,
            total,
            is_locked,
            processed_transactions,
            disputed_transactions,
            resolved_transactions,
            charged_back_transactions,
            ignored_transactions,
        } = transactor.accounts.get(&ClientId(1)).unwrap();
        assert_eq!(*id, ClientId(1));
        assert_eq!(*available, Currency::from_str("5.0000")?);
        assert_eq!(*held, Currency::from_str("-5.0000")?);
        assert_eq!(*total, Currency::from_str("0.0000")?);
        assert_eq!(*is_locked, true);
        assert_eq!(
            processed_transactions.iter().collect::<Vec<_>>(),
            vec![(
                &TransactionId(1),
                &Transaction {
                    ttype: TransactionType::Deposit,
                    cid: ClientId(1),
                    tid: TransactionId(1),
                    amount: Some(Currency::from_str("10.0000")?),
                }
            ),]
        );
        assert_eq!(disputed_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(resolved_transactions.iter().collect::<Vec<_>>(), vec![]);
        assert_eq!(
            charged_back_transactions.iter().collect::<Vec<_>>(),
            vec![(
                &TransactionId(2),
                &Transaction {
                    ttype: TransactionType::Withdrawal,
                    cid: ClientId(1),
                    tid: TransactionId(2),
                    amount: Some(Currency::from_str("5.0000")?),
                }
            )]
        );
        assert_eq!(ignored_transactions.iter().collect::<Vec<_>>(), vec![]);
        Ok(())
    }

    #[tokio::test]
    async fn chargeback_using_locked_account() -> AppResult<()> {
        let mut transactor = Transactor::new();
        transactor.ensure_client_account_exists(ClientId(1)).await?;
        let account = transactor.account_mut(ClientId(1)).await?;
        account.freeze();
        let transactions = vec![Transaction {
            ttype: TransactionType::Chargeback,
            cid: ClientId(1),
            tid: TransactionId(1),
            amount: None,
        }];
        for transaction in transactions {
            assert_eq!(
                transactor.process_transaction(transaction).await,
                Err(TransactionError::AccountIsLocked { cid: ClientId(1) })
            );
        }
        Ok(())
    }

}
