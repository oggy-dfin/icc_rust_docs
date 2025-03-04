use candid::Principal;
use ic_cdk::call::{CallError, RejectCode};
use ic_cdk::{api::msg_caller, call::Call};
use ic_cdk::api::canister_self;
use ic_ledger_types::{AccountIdentifier, BlockIndex, Memo, Tokens, TransferArgs, TransferError};
use ic_xrc_types::{Asset, GetExchangeRateRequest, GetExchangeRateResult};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{NumTokens, TransferArg};

// Hard-coded owner principal for illustration purposes
const OWNER: &str = "gl542-2r2m3-znmmo-cjhz7-p332z-mbe6x-hmrnu-rv37c-mncas-i46u2-sqe";


/// Transfers some ICP to the specified account.
// Methods that call other canisters can use the async/await syntax to perform calls, and we thus
// mark them as async.
#[ic_cdk::update]
pub async fn icp_transfer(to: AccountIdentifier, amount: Tokens) -> Result<(), String> {
    // msg_caller() returns the identity of the user or canister who initiated the call.
    // Only allow the owner to transfer.
    if msg_caller() != Principal::from_text(OWNER).unwrap() {
        return Err("Only the owner can ask to transfer ICP".to_string());
    }

    // The ID of the ledger canister on the IC mainnet.
    const ICP_LEDGER_CANISTER_ID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
    let icp_ledger = Principal::from_text(ICP_LEDGER_CANISTER_ID).unwrap();
    let args = TransferArgs {
        // A "memo" is an arbitrary blob that has no meaning to the ledger, but can be used by
        // the sender or receiver to attach additional information to the transaction. We
        // just use the number 0 here as an example.
        memo: Memo(0),
        to,
        amount,
        // The ICP ledger canister charges a fee for transfers, which is deducted from the
        // sender's account. The fee is fixed to 10_000 e8s (0.0001 ICP).
        fee: Tokens::from_e8s(10_000),
        // The ledger supports subaccounts, but we don't use them in this example.
        from_subaccount: None,
        // The created_at_time is used for deduplication, which we don't use in this example.
        created_at_time: None,
    };

    // Unbounded wait calls ensure that the system doesn't give up waiting on the response from the
    // ledger, though the call might still fail.
    // We will match on the result to show how to properly handle errors.
    // Unbounded wait calls never return a `SysUnknown` error. The response might still be a different
    // kind of error, either coming from the ledger or from the system (it's possible that the system
    // fails the call before it reaches the ledger)
    match Call::unbounded_wait(icp_ledger, "transfer")
        // Sets the call argument, that the recipient will process.
        .with_arg(&args)
        // We are ready to execute the call. The type parameter specifies the expected return type
        // of the call. In this case, we expect the ledger to return a `BlockIndex` if the transfer
        // was successful, or a `TransferError` if it failed. The result of the entire await is a
        // nested `Result`, which can contain an error if the call itself failed, or the value
        // returned by the ledger (which is in itself a `Result`).
        // Note that calls must be awaited to actually send them.
        .call::<Result<BlockIndex, TransferError>>()
        .await
    {
        // The transfer call succeeded
        Ok(Ok(_i)) => Ok(()),
        // The ledger canister returned an error, for example because our balance was too low.
        // The transfer didn't happen, and we can report an error back to the user.
        Ok(Err(e)) => Err(format!("Ledger returned an error: {:?}", e)),
        // The Internet Computer rejected our call, for example because the system is overloaded.
        // We know that the transfer didn't happen and return an error to the user.
        Err(CallError::CallRejected(_)) => Err(format!("Error calling ledger canister: {:?}", e)),
        // An error might happen because the response could not be decoded. We panic
        // here because we assume that the ledger's response type is known and stable.
        Err(CallError::StateUnknown(StateUnknown::CandidDecodeFailed(msg))) => panic!("Decoding failed: {}", msg),
        // The ledger crashed while processing our request. We don't know if the transfer happened.
        // Here, we assume that the ICP ledger is sufficiently well tested that it doesn't crash.
        // For other canisters, more sophisticated error handling might be necessary (for example,
        // they may fail because of a bug or running out of cycles to perform some operations).
        Err(CallError::StateUnknown(StateUnknown::CanisterError(err))) => panic!("Ledger crashed: {:?}", err),
        // This case is unreachable when using unbounded wait calls.
        Err(CallError::StateUnknown(StateUnknown::SysUnknown(_))) => unreachable!("SysUnknown errors cannot happen when using"),
    }
}

/// Obtain the fee that the ledger canister charges for a transfer.
#[ic_cdk::update]
pub async fn icrc1_get_fee(ledger: Principal) -> Result<NumTokens, String> {
    loop {
        match Call::bounded_wait(ledger, "icrc1_fee")
            .call()
            .await
        {
            Ok(fee) => return Ok(fee),
            // The system rejected our call
            Err(CallError::CallRejected(rejection)) => {
                // Determine whether it makes sense to retry. Calls that fail with a non-synchronous
                // transient error are retryable. For a production system, one might want to limit the
                // number of retries to avoid spinning in a retry loop forever in some way.
                // We could use a fixed number of attempts, a timeout, or just check that the caller
                // isn't stopping.
                if rejection.is_sync() && rejection.reject_code() == RejectCode::SysTransient
                {
                    continue;
                } else {
                    // Other rejection types are not retryable. They could happen, for example, if
                    // the target canister explicitly rejects the call (for example, because it is
                    // stopped), if it gets deleted, or if a fatal system error occurs.
                    return Err(format!(
                        "Irrecoverable error: {:?}",
                        rejection
                    ));
                }
            }
            // Since getting the fee doesn't change the ledger state we can simply retry if the
            // system returns a `SysUnknown` error with the ledger canister state being unknown.
            // Again, we omit limiting the number of retries for simplicity.
            Err(CallError::StateUnknown(StateUnknown::SysUnknown(_))) => continue,
            // Candid decoding shouldn't fail with a correctly implemented ledger. However, since
            // we are calling an arbitrary ledger, we don't know if it's correctly implemented.
            // Return an error to the user.
            Err(CallError::StateUknown(StateUnknown::CandidDecodeFailed(msg))) =>
                return Err(format!("Unable to decode the fee: {}", msg)),
            // The ledger crashed while processing our request; report an error to the user.
            Err(CallError::StateUnknown(StateUnknown::CanisterError(err))) =>
                return Err(format!("Ledger crashed: {:?}", err))
        }
    }
}

/// Transfer the tokens on the specified ledger
#[ic_cdk::update]
pub async fn icrc1_transfer(ledger: Principal, to: Account, amount: NumTokens) -> Result<(), String> {
    // In the first step, obtain the fee. Use the method above to handle retries.
    let fee: NumTokens = Call::bounded_wait(canister_self(), "icrc1_get_fee")
        .call()
        .await
        // Since `icrc1_get_fee` already retries internally, just pass the error to the user
        // if it fails.
        .map_err(|e| format!("Error obtaining the fee from the ledger canister: {:?}", e))?;

    let arg = TransferArg {
        from_subaccount: None,
        to,
        fee: Some(fee),
        // Setting the created time ensures that the ledger performs deduplication of transactions,
        // such that they can be safely retried. This is very useful for bounded wait calls.
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
            // In the later case, the transaction may or may not have happened. See the TransferError
            // documentation to do more fine-grained  and sophisticated error handling here. For
            // example, you can query the ledger to find out whether the transaction occurred.
            Ok(Err(e)) => Err(format!("Ledger returned an error: {:?}", e)),
            // Since the call is idempotent, we can safely retry if the system returns an error with
            // the ledger canister state being unknown. For production, you likely need to limit the
            // number of retries in some way, at the very least to make sure that you don't prevent
            // your canister from stopping because it's constantly retrying this call.
            Err(CallError::StateUnknown(StateUnknown::SysUnknown(_))) => continue,
            Err(CallError::CallRejected(rejection)) => {
                // Non-synchronous transient errors can be sensibly retried
                if rejection.is_sync() && rejection.reject_code() == RejectCode::SysTransient {
                    continue
                } else {
                    // Again, we could try to query the ledger, but it's unlikely that it would
                    // work.
                    return Err(format!("Irrecoverable error: {:?}", rejection));
                }
            }
            // This should not happen if the ledger correctly implements the ICRC-1 standard.
            // We could try to query the ledger to determine the state of the transaction, but
            // if the ledger is incorrect, it is unlikely to work anyway
            Err(CallError::StateUnknown(StateUnknown::CandidDecodeFailed(msg))) => {
                return Err(format!("Unable to decode the ledger response: {}", msg))
            }
            // This should not happen if the ledger is correct. Same as for Candid decoding, we could
            // try to query the ledger, but if the ledger is incorrect, it is unlikely to work, so
            // we just report an error to the user
            Err(CallError::StateUnknown(StateUnknown::CanisterError(err))) => {
                return Err(format!("Ledger crashed: {:?}", err))
            }
        }
    }
}

/// Return the exchange rate between the base and quote assets, where the result consists of the
/// exchange rate as an integer, and the number of decimals in the exchange rate.
#[ic_cdk::update]
pub async fn get_exchange_rate(base: Asset, quote: Asset) -> Result<(u64, u32), String> {
    const XRC_CANISTER_ID: &str = "uf6dk-hyaaa-aaaaq-qaaaq-cai";
    let xrc = Principal::from_text(XRC_CANISTER_ID).unwrap();

    let args = GetExchangeRateRequest {
        base_asset: base,
        quote_asset: quote,
        timestamp: None,
    };

    // The XRC charges a fee (in cycles) for its services. The fee is currently 1 billion cycles.
    const XRC_FEES: u128 = 1_000_000_000;

    // We will use a bounded wait call here, since the attached amount of cycles isn't very large.
    // For larger cycle transfers, an unbounded wait call is safer.
    match Call::bounded_wait(xrc, "get_exchange_rate")
        .with_arg(&args)
        // We attach the fee here; it is deducted from the caller's cycles balance.
        .with_cycles(XRC_FEES)
        .call::<GetExchangeRateResult>()
        .await
    {
        Ok(Ok(rate)) => Ok((rate.rate, rate.metadata.decimals)),
        // The XRC canister returned an error. This could be because the assets are unknown,
        // because the XRC canister cannot make outgoing calls, and other reasons. We don't do
        // any sophisticated error handling here.
        Ok(Err(e)) => Err(format!("XRC returned an error: {:?}", e)),
        // For simplicity, we will bail out on any errors. In a real system, we might want to
        // retry, as we did when obtaining transfer fees.
        Err(e) => Err(format!("Error calling XRC: {:?}", e)),
    }
}