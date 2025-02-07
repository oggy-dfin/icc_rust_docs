use candid::{Nat, Principal};
use ic_cdk::call::{Call, CallError};
use ic_cdk::api::time;
use ic_cdk_macros::update;

// When calling other canisters:
// 1. The simplest is to mark your function as `update`. Then you can always call any public
//    endpoint on any other canister.
// 2. Mark the function as `async`. Then you can use the `Call` API to call other canisters.
// We expect the caller to provide the principal (i.e., ID) of the counter canister.
#[update]
pub async fn increment_twice(counter: Principal) -> (Nat, Nat) {
    // Let's check the initial value of the counter
    // To make a call, you must provide the principal (i.e., ID) of the canister you're
    // calling, and the method name that you're calling. We'll ask our caller to provide the
    // principal of the counter canister.
    // We must choose between bounded and unbounded wait calls. Unbounded wait calls have a simple
    // failure semantics, so we start with them.
    let initial = Call::unbounded_wait(counter, "get")
        // `Call` follows the builder pattern; we can customize call options before we finalize
        // the call by issuing the `call()` method. We don't need to set any options for `get` so we
        // just issue `call()`.
        // Call automatically deserializes a Candid-encoded response into its type argument. Here,
        // we use the turbofish syntax to specify that we expect a Candid Nat (i.e., a non-negative
        // integer) as the response.
        .call::<Nat>()
        // The `call` method only creates a Future, but it doesn't actually run it. To issue the
        // call, you must `await` it.
        .await
        // Calls can *always* fail. Robust applications must handle failures properly, but for this
        // first example we just panic if an error happens.
        .expect("An error happened during the call. Bail out in this simple example");

    // Following the exact same pattern, we can increment the counter.
    let _ = Call::unbounded_wait(counter, "increment")
        .call::<()>()
        .await
        .expect("Error in the first increment. Bail out");

    // An alternative pattern to turbofish is to specify the type of the variable being assigned.
    let _: () = Call::unbounded_wait(counter, "increment")
        .call()
        .await
        .expect("Failed in the second increment. Bail out");

    let end: Nat = Call::unbounded_wait(counter, "get")
        .call()
        .await
        .expect("Failed to get the final value. Bail out");

    // It looks like we should be able to assert:
    // assert!(final == initial + 2);
    // But this is *NOT* guaranteed to hold!
    (initial, end)
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
                    // In the `CallRejected` case, we know that the call wasn't executed. Retrying
                    // is safe in the sense that it will never execute the call more than once.
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
    }
}
