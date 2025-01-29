use candid::Principal;
use ic_cdk::call::{CallError, RejectCode};
use ic_cdk::{api::msg_caller, call::Call};
use ic_ledger_types::{AccountIdentifier, BlockIndex, Memo, Tokens, TransferArgs, TransferError};
use icrc_ledger_types::icrc1::account::Account;

// Hard-coded owner principal for illustration purposes
const OWNER: &str = "gl542-2r2m3-znmmo-cjhz7-p332z-mbe6x-hmrnu-rv37c-mncas-i46u2-sqe";

const ICP_LEDGER_CANISTER_ID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";

/// Transfers some ICP to the specified account.
#[ic_cdk::update]
pub async fn icp_transfer(to: AccountIdentifier, amount: Tokens) -> Result<(), String> {
    let owner = Principal::from_text(OWNER).unwrap();
    let icp_ledger = Principal::from_text(ICP_LEDGER_CANISTER_ID).unwrap();
    // Check if the caller is the owner
    if msg_caller() != owner {
        return Err("Only the owner can transfer ICP".to_string());
    }

    match Call::new(icp_ledger, "transfer")
        .with_arg(TransferArgs {
            memo: Memo(0),
            to,
            amount,
            fee: Tokens::from_e8s(10_000),
            from_subaccount: None,
            created_at_time: None,
        })
        // Guaranteed response calls ensure that any error returned by the system is genuine,
        // i.e., that the transfer did either not happen, or it triggered either a rejection by the
        // ICP ledger canister or an error in the ledger canister itself.
        .with_guaranteed_response()
        .call::<Result<BlockIndex, TransferError>>()
        .await
    {
        // The transfer call succeeded
        Ok(Ok(_i)) => Ok(()),
        // The ledger canister returned an error; the transfer didn't happen
        Ok(Err(e)) => Err(format!("Ledger returned an error: {:?}", e)),
        // The Internet Computer returned an error. The error could be a system error, or it could
        // be that the ledger rejected the message, or that the ledger crashed (i.e., there is a
        // bug in the ledger canister). Apart from the buggy ledger case, we expect that the transfer
        // didn't happen, so we report an error back to the user.
        Err(e) => Err(format!("Error calling ledger canister: {:?}", e)),
    }
}

#[ic_cdk::update]
pub async fn icrc1_get_balance(ledger: Principal) -> Result<candid::Nat, String> {
    let owner = Principal::from_text(OWNER).unwrap();

    let caller = msg_caller();
    if caller != owner {
        return Err("Only the owner can query the balance".to_string());
    }

    // More sophisticated error handling: retry the call until it succeeds
    loop {
        println!("Issuing call");
        match Call::new(ledger, "icrc1_balance_of")
            .with_arg(Account {
                owner: ic_cdk::api::canister_self(),
                subaccount: None,
            })
            .call()
            .await
        {
            Ok(balance) => return Ok(balance),
            Err(CallError::CandidDecodeFailed(msg)) => {
                return Err(format!("Unable to decode the balance: {}", msg))
            }
            Err(CallError::CallRejected(rejection)) => {
                ic_cdk::println!("We got a rejection: {:?}", rejection);
                // Determine whether it makes sense to retry
                if
                // There is no point in retrying calls that fail synchronously, as they
                // will fail again.
                !rejection.is_sync() &&
                    // Calls that fail with a non-synchronous transient error are always safe to
                    // retry.
                    (rejection.reject_code() == RejectCode::SysTransient
                     // We don't know if the call succeeded or not. However, getting the 
                     // balance is idempotent, so it's safe to retry
                     || rejection.reject_code() == RejectCode::SysUnknown)
                {
                    continue;
                } else {
                    return Err(format!(
                        "Irrecoverable error: {:?}",
                        rejection
                    ));
                }
            }
        }
    }
}
