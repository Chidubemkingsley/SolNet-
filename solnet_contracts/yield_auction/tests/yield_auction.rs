use soroban_sdk::{
    symbol_short,
    testutils::{storage::Instance as _, storage::Persistent as _, Address as _, Ledger as _},
    Address, Env, Symbol,
};
use solnet_yield_auction::{
    Auction, Bid, Error, MatchResult, YieldAuction, YieldAuctionClient,
};

fn setup() -> (Env, Address, Address, YieldAuctionClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let seller = Address::generate(&e);
    let id = e.register(YieldAuction, ());
    let client = YieldAuctionClient::new(&e, &id);
    client.init(&admin);
    (e, admin, seller, client)
}

/// Mirrors `setup` but also hands back the contract address, which the
/// lifetime assertions below need in order to inspect contract storage.
fn setup_tracked() -> (Env, Address, Address, Address, YieldAuctionClient<'static>) {
    let e = Env::default();
    e.mock_all_auths();
    let admin = Address::generate(&e);
    let seller = Address::generate(&e);
    let id = e.register(YieldAuction, ());
    let client = YieldAuctionClient::new(&e, &id);
    client.init(&admin);
    (e, admin, seller, id, client)
}

fn network_max_ttl(e: &Env, id: &Address) -> u32 {
    e.as_contract(id, || e.storage().max_ttl())
}

fn entry_ttl(e: &Env, id: &Address, key: &(Symbol, u64)) -> u32 {
    e.as_contract(id, || e.storage().persistent().get_ttl(key))
}

fn instance_ttl(e: &Env, id: &Address) -> u32 {
    e.as_contract(id, || e.storage().instance().get_ttl())
}

fn result(
    auction_id: u64,
    matches: u32,
    energy_matched: i128,
    consideration: i128,
    seller_cleared: bool,
) -> MatchResult {
    MatchResult {
        auction_id,
        matches,
        energy_matched,
        consideration,
        seller_cleared,
    }
}

#[test]
fn init_stores_admin_guards_reentry_and_gates_creation() {
    let e = Env::default();
    e.mock_all_auths();
    let contract_id = e.register(YieldAuction, ());
    let client = YieldAuctionClient::new(&e, &contract_id);
    let admin = Address::generate(&e);
    let seller = Address::generate(&e);

    assert_eq!(
        client.try_auction_count(),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(
        client.try_create_auction(&seller, &100, &5),
        Err(Ok(Error::NotInitialized))
    );

    client.init(&admin);
    assert_eq!(client.admin(), admin);
    assert_eq!(client.auction_count(), 0);

    assert_eq!(client.try_init(&admin), Err(Ok(Error::AlreadyInitialized)));
    assert_eq!(client.auction_count(), 0);
}

#[test]
fn create_auction_rejects_non_positive_inputs() {
    let (_e, _admin, seller, client) = setup();

    assert_eq!(
        client.try_create_auction(&seller, &0, &5),
        Err(Ok(Error::InvalidEnergy))
    );
    assert_eq!(
        client.try_create_auction(&seller, &-1, &5),
        Err(Ok(Error::InvalidEnergy))
    );
    assert_eq!(
        client.try_create_auction(&seller, &100, &0),
        Err(Ok(Error::InvalidPrice))
    );
    assert_eq!(
        client.try_create_auction(&seller, &100, &-5),
        Err(Ok(Error::InvalidPrice))
    );

    assert_eq!(client.auction_count(), 0);
}

#[test]
fn create_auction_assigns_ids_and_stores_records() {
    let (e, _admin, seller, client) = setup();
    let other = Address::generate(&e);

    let first = client.create_auction(&seller, &1_000, &10);
    let second = client.create_auction(&other, &250, &3);
    assert_eq!(first, 0);
    assert_eq!(second, 1);
    assert_eq!(client.auction_count(), 2);

    assert_eq!(
        client.auction(&first),
        Auction {
            id: first,
            seller: seller.clone(),
            energy_mwh: 1_000,
            listed_mwh: 1_000,
            min_price: 10,
            cleared: false,
            cancelled: false,
        }
    );
    assert_eq!(client.auction(&second).seller, other);
    assert_eq!(client.auction(&second).listed_mwh, 250);
    assert_eq!(client.try_auction(&99), Err(Ok(Error::AuctionNotFound)));
    assert!(!client.is_cleared(&99));
}

#[test]
fn place_bid_rejects_non_positive_and_under_minimum_prices() {
    let (_e, _admin, seller, client) = setup();
    let auction = client.create_auction(&seller, &1_000, &10);
    let buyer = Address::generate(&_e);

    assert_eq!(
        client.try_place_bid(&buyer, &auction, &0),
        Err(Ok(Error::InvalidPrice))
    );
    assert_eq!(
        client.try_place_bid(&buyer, &auction, &-1),
        Err(Ok(Error::InvalidPrice))
    );
    assert_eq!(
        client.try_place_bid(&buyer, &auction, &9),
        Err(Ok(Error::BidBelowMinimum))
    );
    assert_eq!(client.bid_count(&auction), 0);

    client.place_bid(&buyer, &auction, &10);
    assert_eq!(client.bid_count(&auction), 1);
}

#[test]
fn place_bid_rejects_the_seller_as_buyer() {
    let (_e, _admin, seller, client) = setup();
    let auction = client.create_auction(&seller, &1_000, &10);

    assert_eq!(
        client.try_place_bid(&seller, &auction, &50),
        Err(Ok(Error::SellerCannotBid))
    );
    assert_eq!(
        client.try_best_bid(&auction),
        Err(Ok(Error::NoBids))
    );
    assert_eq!(client.bid_count(&auction), 0);
}

#[test]
fn duplicate_live_bid_is_rejected_per_auction() {
    let (e, _admin, seller, client) = setup();
    let first = client.create_auction(&seller, &1_000, &10);
    let second = client.create_auction(&seller, &400, &10);
    let b1 = Address::generate(&e);
    let b2 = Address::generate(&e);

    client.place_bid(&b1, &first, &20);
    assert_eq!(
        client.try_place_bid(&b1, &first, &30),
        Err(Ok(Error::DuplicateBid))
    );
    client.place_bid(&b2, &first, &25);
    assert_eq!(client.bid_count(&first), 2);
    assert_eq!(client.best_bid(&first).price, 20);

    client.place_bid(&b1, &second, &30);
    assert_eq!(client.bid_count(&second), 1);
    assert_eq!(client.best_bid(&second).buyer, b1);
    assert_eq!(client.best_bid(&second).price, 30);
}

#[test]
fn bid_book_rejects_unknown_auction() {
    let (e, _admin, _seller, client) = setup();
    let buyer = Address::generate(&e);

    assert_eq!(client.try_bid_count(&42), Err(Ok(Error::AuctionNotFound)));
    assert_eq!(client.try_best_bid(&42), Err(Ok(Error::AuctionNotFound)));
    assert_eq!(
        client.try_place_bid(&buyer, &42, &5),
        Err(Ok(Error::AuctionNotFound))
    );
    assert_eq!(
        client.try_match_auction(&42),
        Err(Ok(Error::AuctionNotFound))
    );
}

#[test]
fn best_bid_prefers_lowest_price_then_earliest_arrival() {
    let (e, _admin, seller, client) = setup();
    let auction = client.create_auction(&seller, &1_000, &10);
    let dear = Address::generate(&e);
    let cheap_a = Address::generate(&e);
    let cheap_b = Address::generate(&e);

    client.place_bid(&dear, &auction, &40);
    client.place_bid(&cheap_a, &auction, &20);
    client.place_bid(&cheap_b, &auction, &20);

    let best = client.best_bid(&auction);
    assert_eq!(best.buyer, cheap_a);
    assert_eq!(best.price, 20);
    assert_eq!(best.seq, 1);
    assert_eq!(
        best,
        Bid {
            buyer: cheap_a,
            price: 20,
            seq: 1,
        }
    );

    let cheapest_alone = client.create_auction(&seller, &50, &1);
    client.place_bid(&dear, &cheapest_alone, &99);
    client.place_bid(&cheap_b, &cheapest_alone, &2);
    assert_eq!(client.best_bid(&cheapest_alone).seq, 4);
    assert_eq!(client.best_bid(&cheapest_alone).price, 2);
}

#[test]
fn match_clears_listing_to_lowest_bidder() {
    let (e, _admin, seller, client) = setup();
    let auction = client.create_auction(&seller, &1_000, &10);
    let observer = Address::generate(&e);

    client.place_bid(&observer, &auction, &40);
    let winner = Address::generate(&e);
    client.place_bid(&winner, &auction, &25);
    let runner_up = Address::generate(&e);
    client.place_bid(&runner_up, &auction, &33);

    let r = client.match_auction(&auction);
    assert_eq!(r, result(auction, 1, 1_000, 25_000, true));

    assert!(client.is_cleared(&auction));
    assert_eq!(client.auction(&auction).energy_mwh, 0);
    assert_eq!(client.auction(&auction).listed_mwh, 1_000);
    assert_eq!(client.auction_count(), 1);
}

#[test]
fn match_with_no_bids_closes_unfilled() {
    let (_e, _admin, seller, client) = setup();
    let auction = client.create_auction(&seller, &1_000, &10);

    assert_eq!(client.match_auction(&auction), result(auction, 0, 0, 0, false));

    assert!(client.is_cleared(&auction));
    assert_eq!(client.auction(&auction).energy_mwh, 1_000);
    assert_eq!(client.try_best_bid(&auction), Err(Ok(Error::NoBids)));
    assert_eq!(client.bid_count(&auction), 0);

}

#[test]
fn cleared_auction_rejects_further_bids_and_matching() {
    let (e, _admin, seller, client) = setup();
    let auction = client.create_auction(&seller, &1_000, &10);
    let winner = Address::generate(&e);
    client.place_bid(&winner, &auction, &10);
    client.match_auction(&auction);

    let latecomer = Address::generate(&e);
    assert_eq!(
        client.try_place_bid(&latecomer, &auction, &99),
        Err(Ok(Error::AuctionClosed))
    );
    assert_eq!(
        client.try_match_auction(&auction),
        Err(Ok(Error::AuctionClosed))
    );
    assert_eq!(client.bid_count(&auction), 1);
    assert_eq!(client.auction_count(), 1);
}

#[test]
fn match_rejects_consideration_that_would_overflow() {
    let (e, _admin, seller, client) = setup();
    let auction = client.create_auction(&seller, &i128::MAX, &2);
    let buyer = Address::generate(&e);
    client.place_bid(&buyer, &auction, &2);

    assert_eq!(
        client.try_match_auction(&auction),
        Err(Ok(Error::ArithmeticOverflow))
    );

    assert!(!client.is_cleared(&auction));
    assert_eq!(client.auction(&auction).energy_mwh, i128::MAX);
    assert_eq!(client.try_match_auction(&auction), Err(Ok(Error::ArithmeticOverflow)));
}

#[test]
fn cancel_withdraws_listing_and_blocks_matching() {
    let (e, _admin, seller, client) = setup();
    let auction = client.create_auction(&seller, &1_000, &10);
    let buyer = Address::generate(&e);
    client.place_bid(&buyer, &auction, &10);

    client.cancel_auction(&seller, &auction);

    let record = client.auction(&auction);
    assert!(record.cancelled);
    assert!(!record.cleared);
    assert_eq!(record.energy_mwh, 0);
    assert_eq!(record.listed_mwh, 1_000);
    assert_eq!(client.bid_count(&auction), 0);
    assert!(!client.is_cleared(&auction));

    assert_eq!(
        client.try_match_auction(&auction),
        Err(Ok(Error::NoEnergy))
    );
    assert_eq!(
        client.try_place_bid(&buyer, &auction, &50),
        Err(Ok(Error::NoEnergy))
    );
}

#[test]
fn cancel_rejects_non_seller_repeat_and_unknown_listings() {
    let (e, _admin, seller, client) = setup();
    let auction = client.create_auction(&seller, &1_000, &10);
    let stranger = Address::generate(&e);

    assert_eq!(
        client.try_cancel_auction(&stranger, &auction),
        Err(Ok(Error::NotSeller))
    );
    assert_eq!(
        client.try_cancel_auction(&stranger, &77),
        Err(Ok(Error::AuctionNotFound))
    );

    client.cancel_auction(&seller, &auction);
    assert_eq!(
        client.try_cancel_auction(&seller, &auction),
        Err(Ok(Error::AlreadyCancelled))
    );

    let cleared = client.create_auction(&seller, &100, &10);
    let buyer = Address::generate(&e);
    client.place_bid(&buyer, &cleared, &10);
    client.match_auction(&cleared);
    assert_eq!(
        client.try_cancel_auction(&seller, &cleared),
        Err(Ok(Error::AuctionClosed))
    );
}

#[test]
#[should_panic]
fn cancel_requires_the_seller_signature() {
    let (e, _admin, seller, client) = setup();
    let auction = client.create_auction(&seller, &1_000, &10);

    e.set_auths(&[]);
    client.cancel_auction(&seller, &auction);
}

#[test]
fn listing_write_paths_renew_the_lifetime_of_every_listing_entry() {
    let (e, _admin, seller, id, client) = setup_tracked();
    let max = network_max_ttl(&e, &id);
    assert!(max > 0);

    let auction = client.create_auction(&seller, &1_000, &10);
    let buyer = Address::generate(&e);
    client.place_bid(&buyer, &auction, &20);
    client.match_auction(&auction);

    for key in [
        (symbol_short!("seller"), auction),
        (symbol_short!("listed"), auction),
        (symbol_short!("energy"), auction),
        (symbol_short!("minprice"), auction),
        (symbol_short!("cleared"), auction),
        (symbol_short!("canceld"), auction),
        (symbol_short!("bids"), auction),
        (symbol_short!("buyers"), auction),
    ] {
        assert_eq!(
            entry_ttl(&e, &id, &key),
            max,
            "entry {:?} was not renewed",
            key.0
        );
    }
    assert!(entry_ttl(&e, &id, &(symbol_short!("bids"), auction))
        > e.ledger().get().min_persistent_entry_ttl);
}

#[test]
fn cancelling_a_listing_renews_the_entries_it_rewrites() {
    let (e, _admin, seller, id, client) = setup_tracked();
    let max = network_max_ttl(&e, &id);
    let auction = client.create_auction(&seller, &1_000, &10);
    let buyer = Address::generate(&e);
    client.place_bid(&buyer, &auction, &20);

    client.cancel_auction(&seller, &auction);

    for key in [
        (symbol_short!("canceld"), auction),
        (symbol_short!("energy"), auction),
        (symbol_short!("bids"), auction),
        (symbol_short!("buyers"), auction),
    ] {
        assert_eq!(
            entry_ttl(&e, &id, &key),
            max,
            "entry {:?} was not renewed",
            key.0
        );
    }
    assert!(client.auction(&auction).cancelled);
}

#[test]
fn a_renewed_entry_outlives_one_the_auction_engine_never_touched() {
    let (e, _admin, seller, id, client) = setup_tracked();
    let auction = client.create_auction(&seller, &1_000, &10);
    let buyer = Address::generate(&e);
    client.place_bid(&buyer, &auction, &20);
    client.match_auction(&auction);

    let control = (symbol_short!("ctl"), auction);
    e.as_contract(&id, || e.storage().persistent().set(&control, &0i128));

    let renewed = entry_ttl(&e, &id, &(symbol_short!("bids"), auction));
    let untouched = entry_ttl(&e, &id, &control);
    assert_eq!(renewed, network_max_ttl(&e, &id));
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
    assert!(untouched <= e.ledger().get().min_persistent_entry_ttl);
    assert!(untouched < renewed);
}
#[test]
fn init_renews_the_lifetime_of_the_contract_instance() {
    let (e, admin, _seller, id, client) = setup_tracked();
    let max = network_max_ttl(&e, &id);
    assert!(max > 0);

    let renewed = instance_ttl(&e, &id);
    assert_eq!(renewed, max);
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
    assert_eq!(client.admin(), admin);
    assert_eq!(client.auction_count(), 0);
}

#[test]
fn listing_and_bidding_renew_the_lifetime_of_the_contract_instance() {
    let (e, _admin, seller, id, client) = setup_tracked();
    let buyer = Address::generate(&e);
    let max = network_max_ttl(&e, &id);

    let auction_id = client.create_auction(&seller, &1_000, &5);
    assert_eq!(instance_ttl(&e, &id), max);

    client.place_bid(&buyer, &auction_id, &7);
    assert_eq!(instance_ttl(&e, &id), max);
    assert_eq!(client.auction_count(), 1);
}

#[test]
fn a_listing_write_restores_a_decayed_contract_instance() {
    let (e, _admin, seller, id, client) = setup_tracked();
    let max = network_max_ttl(&e, &id);

    e.ledger().with_mut(|l| l.sequence_number += max / 2 + 1);
    let decayed = instance_ttl(&e, &id);
    assert!(decayed < max);
    assert_eq!(client.auction_count(), 0);

    client.create_auction(&seller, &1_000, &5);
    assert_eq!(client.auction_count(), 1);
    let renewed = instance_ttl(&e, &id);
    assert_eq!(renewed, max);
    assert!(renewed > decayed);
    assert!(renewed > e.ledger().get().min_persistent_entry_ttl);
}

#[test]
fn a_bid_write_restores_a_decayed_contract_instance() {
    let (e, _admin, seller, id, client) = setup_tracked();
    let buyer = Address::generate(&e);
    let max = network_max_ttl(&e, &id);
    let auction_id = client.create_auction(&seller, &1_000, &5);

    e.ledger().with_mut(|l| l.sequence_number += max / 2 + 1);
    let decayed = instance_ttl(&e, &id);
    assert!(decayed < max);

    client.place_bid(&buyer, &auction_id, &7);
    assert_eq!(client.bid_count(&auction_id), 1);
    let renewed = instance_ttl(&e, &id);
    assert_eq!(renewed, max);
    assert!(renewed > decayed);
}
