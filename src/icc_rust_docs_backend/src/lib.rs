use candid::Principal;
use ic_cdk::call::{CallError, RejectCode};
use ic_cdk::{api::msg_caller, call::Call};
use ic_cdk::api::canister_self;
use ic_ledger_types::{AccountIdentifier, BlockIndex, Memo, Tokens, TransferArgs, TransferError};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{NumTokens, TransferArg};

// Hard-coded owner principal for illustration purposes
const OWNER: &str = "gl542-2r2m3-znmmo-cjhz7-p332z-mbe6x-hmrnu-rv37c-mncas-i46u2-sqe";


/// Transfers some ICP to the specified account.
#[ic_cdk::update]
pub async fn icp_transfer(to: AccountIdentifier, amount: Tokens) -> Result<(), String> {
    if msg_caller != Principal::from_text(OWNER).unwrap() {
        return Err("Only the owner can call this method".to_string());
    }

    const ICP_LEDGER_CANISTER_ID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
    let icp_ledger = Principal::from_text(ICP_LEDGER_CANISTER_ID).unwrap();

    // Unbounded wait calls ensure that the system doesn't give up waiting on the response from the
    // ledger, and thus never returns a `SysUnknown` error. The response might still be a different
    // kind of error, either coming from the ledger or from the system (it's possible that the system
    // fails the call before it reaches the ledger)
    match Call::unbounded_wait(icp_ledger, "transfer")
        .with_arg(TransferArgs {
            memo: Memo(0),
            to,
            amount,
            fee: Tokens::from_e8s(10_000),
            from_subaccount: None,
            created_at_time: None,
        })

        .call::<Result<BlockIndex, TransferError>>()
        .await
    {
        // The transfer call succeeded
        Ok(Ok(_i)) => Ok(()),
        // The ledger canister returned an error, for example because our balance was too low.
        // The transfer didn't happen and we can report an error back to the user.
        Ok(Err(e)) => Err(format!("Ledger returned an error: {:?}", e)),
        // The Internet Computer rejected our call, for example because the system is overloaded.
        // We know that the transfer didn't happen and return an error to the user.
        Err(CallError::CallRejected(_)) => Err(format!("Error calling ledger canister: {:?}", e)),
        // An error might happen because the response could not be decoded. We mark this as
        // unreachable here because we assume that the ledger's response type is known and stable.
        Err(CallError::StateUnknown(CandidDecodeFailed(msg))) => unreachable!("Decoding failed: {}", msg),
        // The ledger crashed while processing our request. We don't know if the transfer happened.
        // Here, we assume that the ICP ledger is sufficiently well tested that it doesn't crash.
        // For other canisters, more sophisticated error handling might be necessary (for example,
        // they may fail because of a bug or running out of cycles to perform some operations).
        Err(CallError::StateUnknown(CanisterError(err))) => unreachable!("Ledger crashed: {:?}", err),
        // This case is unreachable when using unbounded wait calls.
        Err(CallError::StateUnknown(SysUnknown(_))) => unreachable!("SysUnknown errors cannot happen when using"),
    }
}

#[ic_cdk::update]
pub async fn icrc1_get_fee(ledger: Principal) -> Result<NumTokens, String> {
    if msg_caller() != Principal::from_text(OWNER).unwrap() && msg_caller() != canister_self()  {
        return Err("Only the owner can call this method".to_string());
    }

    // In this example we'll use more sophisticated error handling: retrying the call if possible
    loop {
        let res = Call::bounded_wait(ledger, "icrc1_fee")
            .with_arg(Account {
                owner: ic_cdk::api::canister_self(),
                subaccount: None,
            })
            .call()
            .await;
        match res {
            Ok(balance) => return Ok(balance),
            Err(CallError::CallRejected(rejection)) => {
                // Determine whether it makes sense to retry
                if
                // Calls that fail with a non-synchronous transient error are always safe to retry
                rejection.is_sync() && rejection.reject_code() == RejectCode::SysTransient
                {
                    continue;
                } else {
                    return Err(format!(
                        "Irrecoverable error: {:?}",
                        rejection
                    ));
                }
            }
            // Since getting the fee doesn't change the ledger state we can simply retry if the system
            // returns an error with the ledger canister state being unknown
            Err(CallError::StateUnknown(SysUnknown(_))) => continue,
            // We don't expect Candid decoding to fail, as we assume that the ledger's return value
            // is stable
            Err(CallError::StateUknown(CandidDecodeFailed(msg))) => {
                unreachable!("Unable to decode the balance: {}", msg)
            }
            // Again, we assume that the ledger is stable and doesn't crash
            Err(CallError::StateUknown(CanisterError(err))) => {
                unreachable!("Ledger crashed: {:?}", err)
            }
        }
    }
}

#[ic_cdk::update]
pub async fn icrc1_transfer(ledger: Principal, to: Account, amount: NumTokens) -> Result<(), String> {
    if msg_caller() != Principal::from_text(OWNER).unwrap() {
        return Err("Only the owner can call this method".to_string());
    }

    let fee: NumTokens = Call::bounded_wait(canister_self, "icrc1_get_fee")
        .call()
        .await
        // For simplicity, we won't retry here, but you might want to do so in a real application
        .map_err(|e| format!("Error obtaining the fee from the ledger canister: {:?}", e))?;

    let arg = TransferArg {
        from_subaccount: None,
        to,
        fee: None,
        // Setting the created time ensures that the ledger performs deduplication of transactions,
        // such that they can be safely retried
        created_at_time: Some(ic_cdk::api::time()),
        memo: None,
        amount,
    };

    loop {
        match Call::bounded_wait(ledger, "icrc1_transfer")
            .with_arg(&arg)
            .call::<Result<BlockIndex, TransferError>>()
            .await {
            Ok(Ok(_)) => Ok(()),
            // The ledger canister returned an error. This could be because the transaction didn't
            // happen, for example because our balance was too low, but it could also happen in the
            // case where we were retrying for too long and the `created_at_time` was too old.
            // In the later case, the transaction may or may not have happened. We could do more
            // sophisticated error handling here, for example by querying the ledger, but for
            // simplicity we'll just return the error to the caller.
            Ok(Err(e)) => Err(format!("Ledger returned an error: {:?}", e)),
            Err(CallError::CallRejected(rejection)) => {
                if rejection.is_sync() && rejection.reject_code() == RejectCode::SysTransient {
                    continue
                } else {
                    return Err(format!("Irrecoverable error: {:?}", rejection));
                }
            }
            // Since the call is idempotent, we can safely retry if the system returns an error with
            // the ledger canister state being unknown.
            Err(CallError::StateUnknown(SysUnknown(_))) => continue,
            // This should not happen if the ledger correctly implements the ICRC-1 standard.
            // We could try to query the ledger to determine the state of the transaction, but
            // if the ledger is incorrect, it is unlikely to work anyway
            Err(CallError::StateUnknown(CandidDecodeFailed(msg))) => {
                return Err(format!("Unable to decode the ledger response: {}", msg))
            }
            // This should not happen if the ledger is correct. Same as for Candid decoding, we could
            // try to query the ledger, but if the ledger is incorrect, it is unlikely to work, so
            // we just report an error to the user
            Err(CallError::StateUnknown(CanisterError(err))) => {
                return Err(format!("Ledger crashed: {:?}", err))
            }
        }
    }
}
