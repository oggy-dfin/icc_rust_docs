use candid::{Nat, Principal};
use ic_cdk::api::management_canister::ecdsa::SignWithEcdsaResponse;
use ic_cdk::api::time;
use ic_cdk::call::{Call, CallError, RejectCode};
use ic_cdk::management_canister::{EcdsaCurve, EcdsaKeyId, SignWithEcdsaArgs};
use ic_cdk_macros::update;
use sha2::{Digest, Sha256};

// When calling other canisters:
// 1. The simplest is to mark your function as `update`. Then you can always call any public
//    endpoint on any other canister.
// 2. Mark the function as `async`. Then you can use the `Call` API to call other canisters.
// We expect the caller to provide the principal (i.e., ID) of the counter canister.
#[update]
pub async fn call_get_and_set(counter: Principal, new_value: Nat) -> Nat {
    // To make a call, you must provide the principal (i.e., ID) of the canister you're
    // calling, and the method name that you're calling. Here, we require our own caller to provide
    // the principal of the counter canister as an argument to our function.
    // When making a call, you must choose between bounded and unbounded wait calls. These call
    // types have different failure modes that we will explain later.
    let old = Call::unbounded_wait(counter, "get_and_set")
        // `Call` follows the builder pattern; we can customize call options before we finalize
        // the call by issuing the `call()` method. Here, we provide an argument of type that
        // get_and_set expects, a Nat (non-negative integer). The Rust CDK serializes the argument
        // for us using the Candid format, so we just need to provide the Rust value.
        .with_arg(&new_value)
        // Call automatically deserializes a Candid-encoded response into its type argument. Here,
        // we use the turbofish syntax to specify that we expect a Candid Nat (i.e., a non-negative
        // integer) as the response.
        // The Rust CDK will also deserialize the result for us, but we have to tell it what type of
        // response we are expecting. Here we use Rust turbofish syntax to specify this type.
        .call::<Nat>()
        .await
        // Calls can *always* fail. Robust applications must handle failures properly, but for this
        // first example we just panic if an error happens.
        .expect("Failed to get the old value. Bail out");
    old
}

#[update]
pub async fn set_then_get(counter: Principal, new_value: Nat) -> Nat {
    Call::unbounded_wait(counter, "set")
        .with_arg(&new_value)
        .call::<()>()
        .await
        // Again, we ignore error handling in these early examples.
        .expect("Failed to set the value. Bail ing out");

    let current_value: Nat = Call::unbounded_wait(counter, "get")
        .call()
        .await
        .expect("Failed to get the current_value value. Bail out");

    // It looks like we should be able to assert:
    // assert!(current_value == new_value);
    // But this is *NOT* guaranteed to hold!
    current_value
}

#[update]
pub async fn call_increment(counter: Principal) -> Result<(), String> {
    match Call::new(counter, "increment")
        .call::<()>()
        .await {
        // The counter canister successfully responded. Here, it means that our call was successful,
        // and we can return an "OK" to the caller.
        // A more complicated target than the counter (e.g., a ledger) could also return
        // "user-level" errors that you should handle.
        Ok(()) => Ok(()),
        // Let's look into errors in more detail
        Err(e) => match e {
            // In the `CallRejected` case, we know that the call wasn't executed.
            // One possible way to handle errors is retrying. Retrying on a `CallRejected` is safe
            // in the sense that it will never execute the call more than once.
            // However, it's not always sensible to retry immediately.
            CallError::CallRejected(e) => match e.reject_code() {
                // This error is likely a bug in the system. Retries are usually not useful. We
                // have to clean up ourselves, or pass the error to our caller. Since there's no
                // sensible error recovery here, we'll just report the error back.
                RejectCode::SysFatal => Err(format!("The call was rejected with a fatal error: {:?}", e.reject_message())),
                // A transient error may go away upon a retry. However, we also have to distinguish
                // between the "synchronous" and "asynchronous" transient errors. A synchronous
                // error means that the system is out of resources to even accept our call. Thus,
                // retrying immediately is pointless and would just burn our cycles. Any retries
                // should be done in a background task (e.g., using canister timers).
                RejectCode::SysTransient if e.is_sync() => {
                    Err(format!("The call was rejected with a synchronous transient error: {:?}", e.reject_message()))
                }
                // An asynchronous transient error can normally be retried.
                RejectCode::SysTransient => {
                    // An asynchronous transient error means that the system is overloaded, but
                    // it might be able to accept our call in the future. For the increment example,
                    // we could retry immediately. However, when retrying,
                    // For simplicity, we'll just return the error.
                    Err(format!("The call was rejected with an asynchronous transient error: {:?}", e.reject_message()))
                }
                RejectCode::CanisterReject => {
                    // The call made it to the callee but was rejected (e.g., because the callee
                    // was out of cycles, or because it was uninstalled). Retrying may be possible
                    // in some cases but
                    Err(format!("The call made it to the canister but was rejected: {:?}", e.reject_message()))
                }
            }
            // If the immediately_retryable() method
            if e.immediately_retryable() => {
                // Even if we can retry, don't if we're out of time
                if time() > deadline {
                    return Err("Timed out while trying to set the value".to_string());
                } else {
                    continue
                }
            },
            // We can't immediately retry. We could retry in the background using timers,
            // and provide some means of informing the caller once the call succeeds.
            CallError::CallRejected(_) =>
                return Err(format!("Failed to get the value and cannot retry: {:?}", e)),
            // In the `OutcomeUnknown` case, we don't know whether the call was executed.
            // The counter may be set to the value we provided, or it may not.
            CallError::OutcomeUnknown(e) => match e {
                // The first case is that the callee returned a result, but the
                // deserialization of the result failed because the result wasn't of the
                // type we specified. For example, we could have been passed a wrong
                // principal for the counter canister, and could have called a canister that
                // has a `set` method that returns a non-unit value.
                OutcomeUnknown::CandidDecodingFailed(err, payload) =>
                // We can't do much in this case; just report the error.
                    return Err(format!("The counter canister returned an non-unit response: {:?} {:?}", err, payload)),
                // The callee trapped while processing our request. Our call may or may
                // not have taken effect.
                OutcomeUnknown::CanisterError(err) =>
                // We could try to get the value to see if it was set as a form of error
                // recovery. But for now, we'll just report the error back.
                    return Err(format!("The counter canister returned an error while trying to get the value: {:?}", err)),
                // This error type distinguishes bounded-wait calls from unbounded-wait calls. It means that the
                // system gave up waiting for the response, and the call may or may not have been executed.
                OutcomeUnknown::SysUnknown => {
                    // Since the `set` method is idempotent, we can retry, even if it
                    // already executed. However, let's first check if we're out of time.
                    if time() > deadline {
                        return Err("Timed out while trying to set the value".to_string());
                    } else {
                        continue
                    }
                },
            },
        }
    }
}

/// Retries setting the counter to the provided value even if errors appear, until it succeeds,
/// times out, or hits an unrecoverable error.
#[update]
pub async fn stubborn_set(counter: Principal, value: Nat) -> Result<(), String> {
    // Let's set a timeout to 10 minutes.
    let timeout = std::time::Duration::from_secs(10 * 60).as_nanos() as u64;
    // Compute the deadline based on the current IC time.
    let deadline = time() + timeout;
    // We'll try to set the counter to the provided value, retrying where possible.
    loop {
        // Bounded-wait calls are guaranteed to respond even if the callee takes a long
        // time to respond (or never responds). This is useful when you want to always provide
        // an answer quickly, and also when calling canisters that you don't trust to respond
        // in a timely manner. They are also very scalable. However, they have more complex
        // failure semantics than unbounded-wait calls.
        match Call::bounded_wait(counter, "set")
            .with_arg(&value)
            .call().await {

            Ok(()) => return (),
            // Let's look into errors in more detail
            Err(e) => match e {
                // In the `CallRejected` case, we know that the call wasn't executed.
                // In our concrete example, it means that the counter wasn't set to the value we
                // provided.
                // Retrying is safe in the sense that it will never execute the call more than once.
                // However, it's not always sensible to retry immediately. For example, the
                // system might be at its capacity limit and unable to take more calls. Retrying
                // would just waste the cycles balance of the caller.
                CallError::CallRejected(e)
                    // Check if we can retry immediately
                    if e.immediately_retryable() => {
                    // Even if we can retry, don't if we're out of time
                    if time() > deadline {
                        return Err("Timed out while trying to set the value".to_string());
                    } else {
                        continue
                    }
                },
                // We can't immediately retry. We could retry in the background using timers,
                // and provide some means of informing the caller once the call succeeds.
                CallError::CallRejected(_) =>
                    return Err(format!("Failed to get the value and cannot retry: {:?}", e)),
                // In the `OutcomeUnknown` case, we don't know whether the call was executed.
                // The counter may be set to the value we provided, or it may not.
                CallError::OutcomeUnknown(e) => match e {
                    // The first case is that the callee returned a result, but the
                    // deserialization of the result failed because the result wasn't of the
                    // type we specified. For example, we could have been passed a wrong
                    // principal for the counter canister, and could have called a canister that
                    // has a `set` method that returns a non-unit value.
                    // We don't expect this to happen in our example because we tightly control
                    // the callee (counter).
                    OutcomeUnknown::CandidDecodingFailed(err, payload) =>
                        // We can't do much in this case; just report the error.
                        return Err(format!("The counter canister returned an non-unit response: {:?} {:?}", err, payload)),
                    // The callee trapped while processing our request. Our call may or may
                    // not have taken effect, i.e., the counter may or may not have been set.
                    // However, our callee is so simple that we can reasonably expect this case
                    // not to happen.
                    OutcomeUnknown::CanisterError(err) =>
                        // There is no immediately clear recovery action for our example.
                        // For example, we could retry setting the counter, but that might fail with
                        // the same error again. Let's just report the error back.
                        return Err(format!("The counter canister returned an error while trying to get the value: {:?}", err)),
                    // This error means that the system gave up waiting for the response, and the
                    // call may or may not have been executed. That is, the counter may or may not
                    // have been set.
                    // This error type distinguishes bounded-wait calls from unbounded-wait calls.
                    // That is, unbounded-wait calls have the exact same error cases, except that
                    // this one is unreachable.
                    OutcomeUnknown::SysUnknown => {
                        // We can safely retry here, but only because the `set` method is idempotent.
                        // Even if it was already executed, there is no harm in executing it again.
                        // Let's do that, but let's first check if we're out of time, since we don't
                        // want to retry forever.
                        if time() > deadline {
                            return Err("Timed out while trying to set the value".to_string());
                        } else {
                            continue
                        }
                    },
                },
            }
        }
    }
}

#[update]
pub async fn sign_message(message: String) -> Result<String, String> {
    let message_hash = Sha256::digest(&message).to_vec();

    let request = SignWithEcdsaArgs {
        message_hash,
        // We don't use the fancier signing features here
        derivation_path: vec![],
        key_id: EcdsaKeyId {
            curve: EcdsaCurve::Secp256k1,
            // This is the key name used for local testing; different
            // key names are needed for the mainnet
            name: "dfx_test_key".to_string(),
        },
    };

    // We use bounded-wait calls in this example, since the amount attached is
    // fairly low, and losing the attached cycles isn't catastrophic.
    match Call::bounded_wait(Principal::management_canister(), "sign_with_ecdsa")
        .with_arg(&request)
        // Signing with a test key requires 10 billion cycles
        .with_cycles(10_000_000_000)
        .call::<SignWithEcdsaResponse>()
        .await
    {
        Ok(signature) => Ok(hex::encode(signature.signature)),
        Err(e) => match e {
            // A SysUnknown error means that we won't get any cycles refunded, even
            // if the call didn't make it to the callee. But we don't care here since
            // we only attached a small amount of cycles.
            CallError::OutComeUnknown(OutcomeUnknown::SysUnknown(err)) => Err(format!(
                "Got a SysUnknown error while signing message: {:?}; cycles are not refunded",
                err
            )),
            _ => Err(format!("Error signing message: {:?}", e)),
        },
    }
}
