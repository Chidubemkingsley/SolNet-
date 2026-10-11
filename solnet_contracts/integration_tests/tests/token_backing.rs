use soroban_sdk::{
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};
use solnet_account_ledger::{AccountLedger, AccountLedgerClient, Error};

const RATE: i128 = 1_000_000;

struct BackedLedger {
    env: Env,
    ledger: AccountLedgerClient<'static>,
    ledger_id: Address,
    token: StellarAssetClient<'static>,
    balances: TokenClient<'static>,
}

fn setup() -> BackedLedger {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token_id = sac.address();
    let token = StellarAssetClient::new(&env, &token_id);
    let balances = TokenClient::new(&env, &token_id);

    let ledger_id = env.register(AccountLedger, ());
    let ledger = AccountLedgerClient::new(&env, &ledger_id);
    ledger.init(&admin, &RATE);
    ledger.set_token(&admin, &token_id);

    BackedLedger {
        env,
        ledger,
        ledger_id,
        token,
        balances,
    }
}

#[test]
fn deposit_secured_moves_real_tokens_and_mints_credits() {
    let g = setup();
    let user = Address::generate(&g.env);
    g.token.mint(&user, &10_000);

    let minted = g.ledger.deposit_secured(&user, &1_000);
    assert_eq!(minted, 1_000);

    assert_eq!(g.balances.balance(&user), 9_000);
    assert_eq!(g.balances.balance(&g.ledger_id.clone()), 1_000);
    assert_eq!(g.ledger.watt_balance(&user), 1_000);
    assert_eq!(g.ledger.stable_balance(&user), 1_000);
}

#[test]
fn withdraw_secured_returns_real_tokens_and_burns_credits() {
    let g = setup();
    let user = Address::generate(&g.env);
    g.token.mint(&user, &10_000);
    g.ledger.deposit_secured(&user, &1_000);

    let paid = g.ledger.withdraw_secured(&user, &400);
    assert_eq!(paid, 400);

    assert_eq!(g.balances.balance(&user), 9_400);
    assert_eq!(g.balances.balance(&g.ledger_id.clone()), 600);
    assert_eq!(g.ledger.watt_balance(&user), 600);
    assert_eq!(g.ledger.stable_balance(&user), 600);
}

#[test]
fn full_round_trip_conserves_token_supply() {
    let g = setup();
    let user = Address::generate(&g.env);
    g.token.mint(&user, &10_000);

    g.ledger.deposit_secured(&user, &2_500);
    g.ledger.withdraw_secured(&user, &2_500);

    assert_eq!(g.balances.balance(&user), 10_000);
    assert_eq!(g.balances.balance(&g.ledger_id.clone()), 0);
    assert_eq!(g.ledger.watt_balance(&user), 0);
    assert_eq!(g.ledger.stable_balance(&user), 0);
}

#[test]
fn deposit_secured_fails_cleanly_without_tokens() {
    let g = setup();
    let broke = Address::generate(&g.env);

    assert_eq!(
        g.ledger.try_deposit_secured(&broke, &100),
        Err(Ok(Error::TokenTransferFailed))
    );
    assert_eq!(g.balances.balance(&broke), 0);
    assert_eq!(g.ledger.watt_balance(&broke), 0);
    assert_eq!(g.ledger.stable_balance(&broke), 0);
}

#[test]
fn withdraw_secured_rejects_overdraw_without_touching_tokens() {
    let g = setup();
    let user = Address::generate(&g.env);
    g.token.mint(&user, &10_000);
    g.ledger.deposit_secured(&user, &1_000);

    assert_eq!(
        g.ledger.try_withdraw_secured(&user, &5_000),
        Err(Ok(Error::InsufficientCredits))
    );
    assert_eq!(g.balances.balance(&user), 9_000);
    assert_eq!(g.balances.balance(&g.ledger_id.clone()), 1_000);
}

#[test]
fn plain_deposit_still_mints_without_moving_tokens() {
    let g = setup();
    let user = Address::generate(&g.env);
    g.token.mint(&user, &10_000);

    // The legacy path is untouched: credits appear, tokens do not move.
    // This documents the asymmetry — new integrations must use the secured pair.
    let minted = g.ledger.deposit(&user, &1_000);
    assert_eq!(minted, 1_000);
    assert_eq!(g.balances.balance(&user), 10_000);
    assert_eq!(g.balances.balance(&g.ledger_id.clone()), 0);
}
