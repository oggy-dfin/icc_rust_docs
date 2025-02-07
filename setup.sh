#!/bin/sh

if ! command -v jq &> /dev/null
then
    echo "jq is not installed. Please install it before running this script."
    exit 1
fi
if ! command -v dfx &> /dev/null
then
    echo "dfx is not installed. Please install it before running this script."
    echo "You can install by running:"
    echo 'sh -ci "$(curl -fsSL https://internetcomputer.org/install.sh)"'
    exit 1
fi
     
dfx stop || true
dfx start --clean --background
dfx canister create --specified-id ryjl3-tyaaa-aaaaa-aaaba-cai icp_ledger_canister
dfx canister create icc_rust_docs_backend
curl -o download_latest_icp_ledger.sh https://raw.githubusercontent.com/dfinity/ic/aba60ffbc46acfc8990bf4d5685c1360bd7026b9/rs/rosetta-api/scripts/download_latest_icp_ledger.sh
chmod +x download_latest_icp_ledger.sh
dfx build icp_ledger_canister
./download_latest_icp_ledger.sh
MINTING_ACCOUNT=`dfx ledger account-id`
CANISTER_ACCOUNT=`dfx ledger account-id --of-principal $(dfx canister id icc_rust_docs_backend)`
dfx canister install icp_ledger_canister -m install --argument "(variant { Init = record { send_whitelist = vec {}; token_symbol = opt \"LICP\"; transfer_fee = opt record { e8s = 10_000 : nat64 }; minting_account = \"$MINTING_ACCOUNT\"; initial_values = vec { record { \"$CANISTER_ACCOUNT\"; record { e8s = 1_000_000_000_000 : nat64 }; }; }; token_name = opt \"Local ICP\"; } })"
