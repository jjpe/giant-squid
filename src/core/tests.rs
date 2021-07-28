use crate::error::TransactionError;
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
        let result = transactor.process_transaction(transaction).await;
        assert_eq!(result, Err(TransactionError::AccountHasInsufficientFundsAvailable {
            cid: transaction.cid
        }));
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
    let transactions = vec![Transaction {
        ttype: TransactionType::Withdrawal,
        cid: ClientId(1),
        tid: TransactionId(1),
        amount: Some(Currency::from_str("0.9975")?),
    }];
    for transaction in transactions {
        let result = transactor.process_transaction(transaction).await;
        assert_eq!(result, Err(TransactionError::AccountHasInsufficientFundsAvailable {
            cid: transaction.cid,
        }));
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
        let result = transactor.process_transaction(transaction).await;
        assert_eq!(result, Err(TransactionError::NoSuchProcessedTransactionForClient {
            tid: TransactionId(1),
            cid: ClientId(1)
        }));
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
        let result = transactor.process_transaction(transaction).await;
        assert_eq!(result, Err(TransactionError::NoSuchDisputedTransactionForClient {
            tid: TransactionId(1),
            cid: ClientId(1)
        }));
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
        let result = transactor.process_transaction(transaction).await;
        assert_eq!(result, Err(TransactionError::NoSuchResolvedTransactionForClient {
            tid: TransactionId(1),
            cid: ClientId(1)
        }));
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
