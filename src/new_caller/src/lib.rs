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
    let initial = Call::new(counter, "get")
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
    let _ = Call::new(counter, "inc")
        .call::<()>()
        .await
        .expect("Error in the first increment. Bail out");

    // An alternative pattern to turbofish is to specify the type of the variable being assigned.
    let _: () = Call::new(counter, "inc")
        .call()
        .await
        .expect("Failed in the second increment. Bail out");

    let end: Nat = Call::new(counter, "get")
        .call()
        .await
        .expect("Failed to get the final value. Bail out");

    // It looks like we should be able to assert:
    // assert!(final == initial + 2);
    // But this is *NOT* guaranteed to hold!
    (initial, end)
}