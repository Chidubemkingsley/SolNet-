//! SolNet: Yield Auction Contract
//! Description: A localized double-sided matching engine that lets prosumers with
//! generation surplus auction excess power to neighbouring commercial loads.
//!
//! A *listing* (`Auction`) is opened by a prosumer for `energy_mwh` milli-watt-hours
//! at a floor price of `min_price` per milli-watt-hour. Neighbouring loads submit
//! all-or-nothing `Bid`s against the listing. The engine clears listings under a
//! price-time priority rule: the cheapest live bid is filled first, ties broken by
//! arrival order, because the prosumer is indifferent between clearing the whole
//! lot and prefers the cheapest load to absorb it.
//!
//! Consideration is reported as `energy_matched * clearing_price`. Every figure is
//! an `i128` integer because Soroban exposes no floating point.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotSeller = 3,
    AuctionNotFound = 4,
    AuctionClosed = 5,
    NoEnergy = 6,
    NoBids = 7,
    AlreadyCancelled = 8,
    InvalidEnergy = 9,
    InvalidPrice = 10,
    BidBelowMinimum = 11,
    SellerCannotBid = 12,
    DuplicateBid = 13,
    ArithmeticOverflow = 14,
}

/// A single open listing of surplus generation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Auction {
    pub id: u64,
    pub seller: Address,
    /// Milli-watt-hours still available to be cleared.
    pub energy_mwh: i128,
    /// Milli-watt-hours the seller originally listed.
    pub listed_mwh: i128,
    /// Floor price per milli-watt-hour the seller will accept.
    pub min_price: i128,
    /// True once the matching engine has run against this listing.
    pub cleared: bool,
    /// True once the seller has withdrawn the listing before it cleared.
    pub cancelled: bool,
}

/// A live bid from a consuming load against an open listing.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bid {
    pub buyer: Address,
    /// Price per milli-watt-hour offered by this load.
    pub price: i128,
    /// Global arrival sequence, used to break price ties in favour of the earlier bid.
    pub seq: u64,
}

/// Outcome of a clearing run against one listing.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchResult {
    pub auction_id: u64,
    /// Number of live bids that were filled.
    pub matches: u32,
    /// Total milli-watt-hours transferred to the matched loads.
    pub energy_matched: i128,
    /// Total consideration moved from the matched loads to the prosumer.
    pub consideration: i128,
    /// True when the whole listing was absorbed by matched loads.
    pub seller_cleared: bool,
}

#[contract]
pub struct YieldAuction;

#[contractimpl]
impl YieldAuction {
    /// Initializes the yield auction contract state configuration.
    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&symbol_short!("admin"), &admin);
        env.storage().instance().set(&symbol_short!("nAuction"), &0u64);
        env.storage().instance().set(&symbol_short!("nBidSeq"), &0u64);
        Ok(())
    }

    /// Returns the administrator address recorded at initialization.
    pub fn admin(env: Env) -> Result<Address, Error> {
        Self::require_initialized(&env)?;
        Ok(env.storage().instance().get(&symbol_short!("admin")).unwrap())
    }

    /// Number of listings ever opened by the engine.
    pub fn auction_count(env: Env) -> Result<u64, Error> {
        Self::require_initialized(&env)?;
        Ok(env
            .storage()
            .instance()
            .get(&symbol_short!("nAuction"))
            .unwrap_or(0))
    }

    /// Lists `energy_mwh` milli-watt-hours of surplus generation and returns the
    /// new auction id. Both `energy_mwh` and `min_price` must be strictly positive.
    pub fn create_auction(
        env: Env,
        seller: Address,
        energy_mwh: i128,
        min_price: i128,
    ) -> Result<u64, Error> {
        Self::require_initialized(&env)?;
        if energy_mwh <= 0 {
            return Err(Error::InvalidEnergy);
        }
        if min_price <= 0 {
            return Err(Error::InvalidPrice);
        }
        seller.require_auth();

        let id: u64 = env
            .storage()
            .instance()
            .get(&symbol_short!("nAuction"))
            .unwrap_or(0);
        let next = id.checked_add(1).ok_or(Error::ArithmeticOverflow)?;

        env.storage()
            .persistent()
            .set(&(symbol_short!("seller"), id), &seller);
        env.storage()
            .persistent()
            .set(&(symbol_short!("listed"), id), &energy_mwh);
        env.storage()
            .persistent()
            .set(&(symbol_short!("energy"), id), &energy_mwh);
        env.storage()
            .persistent()
            .set(&(symbol_short!("minprice"), id), &min_price);
        env.storage()
            .persistent()
            .set(&(symbol_short!("cleared"), id), &false);
        env.storage()
            .persistent()
            .set(&(symbol_short!("canceld"), id), &false);
        env.storage()
            .persistent()
            .set(&(symbol_short!("bids"), id), &Vec::<Bid>::new(&env));
        env.storage()
            .persistent()
            .set(&(symbol_short!("buyers"), id), &Vec::<Address>::new(&env));

        env.storage().instance().set(&symbol_short!("nAuction"), &next);
        Ok(id)
    }

    /// Submits an all-or-nothing bid for the whole remaining listing at `price`
    /// per milli-watt-hour. A load may hold only one live bid per listing.
    pub fn place_bid(env: Env, buyer: Address, auction_id: u64, price: i128) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        if price <= 0 {
            return Err(Error::InvalidPrice);
        }
        let seller: Address = env
            .storage()
            .persistent()
            .get(&(symbol_short!("seller"), auction_id))
            .ok_or(Error::AuctionNotFound)?;

        if buyer == seller {
            return Err(Error::SellerCannotBid);
        }
        let cleared: bool = env
            .storage()
            .persistent()
            .get(&(symbol_short!("cleared"), auction_id))
            .unwrap();
        if cleared {
            return Err(Error::AuctionClosed);
        }
        let remaining: i128 = env
            .storage()
            .persistent()
            .get(&(symbol_short!("energy"), auction_id))
            .unwrap();
        if remaining <= 0 {
            return Err(Error::NoEnergy);
        }
        let min_price: i128 = env
            .storage()
            .persistent()
            .get(&(symbol_short!("minprice"), auction_id))
            .unwrap();
        if price < min_price {
            return Err(Error::BidBelowMinimum);
        }
        let mut buyers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&(symbol_short!("buyers"), auction_id))
            .unwrap_or(Vec::new(&env));
        if buyers.contains(&buyer) {
            return Err(Error::DuplicateBid);
        }
        buyer.require_auth();

        let seq: u64 = env
            .storage()
            .instance()
            .get(&symbol_short!("nBidSeq"))
            .unwrap_or(0);
        let next_seq = seq.checked_add(1).ok_or(Error::ArithmeticOverflow)?;

        buyers.push_back(buyer.clone());
        env.storage()
            .persistent()
            .set(&(symbol_short!("buyers"), auction_id), &buyers);

        let mut bids: Vec<Bid> = env
            .storage()
            .persistent()
            .get(&(symbol_short!("bids"), auction_id))
            .unwrap_or(Vec::new(&env));
        bids.push_back(Bid {
            buyer,
            price,
            seq,
        });
        env.storage()
            .persistent()
            .set(&(symbol_short!("bids"), auction_id), &bids);

        env.storage()
            .instance()
            .set(&symbol_short!("nBidSeq"), &next_seq);
        Ok(())
    }

    /// Clears the listing under price-time priority: cheapest live bid first,
    /// arrival order breaking ties, filling the seller's available energy.
    /// Permissionless: any observer may trigger the clearing run.
    pub fn match_auction(env: Env, auction_id: u64) -> Result<MatchResult, Error> {
        if !env
            .storage()
            .persistent()
            .has(&(symbol_short!("seller"), auction_id))
        {
            return Err(Error::AuctionNotFound);
        }
        let cleared: bool = env
            .storage()
            .persistent()
            .get(&(symbol_short!("cleared"), auction_id))
            .unwrap();
        if cleared {
            return Err(Error::AuctionClosed);
        }
        let mut remaining: i128 = env
            .storage()
            .persistent()
            .get(&(symbol_short!("energy"), auction_id))
            .unwrap();
        if remaining <= 0 {
            return Err(Error::NoEnergy);
        }

        let bids: Vec<Bid> = env
            .storage()
            .persistent()
            .get(&(symbol_short!("bids"), auction_id))
            .unwrap_or(Vec::new(&env));
        let order = Self::priority_order(&env, &bids);

        let mut matches: u32 = 0;
        let mut energy_matched: i128 = 0;
        let mut consideration: i128 = 0;

        let mut slot: u32 = 0;
        while slot < order.len() && remaining > 0 {
            let idx = order.get(slot).unwrap();
            let bid = bids.get(idx).unwrap();
            let fill = remaining;
            let fill_cost = fill.checked_mul(bid.price).ok_or(Error::ArithmeticOverflow)?;
            energy_matched = energy_matched
                .checked_add(fill)
                .ok_or(Error::ArithmeticOverflow)?;
            consideration = consideration
                .checked_add(fill_cost)
                .ok_or(Error::ArithmeticOverflow)?;
            matches = matches.checked_add(1).ok_or(Error::ArithmeticOverflow)?;
            remaining = 0;
            slot = slot.checked_add(1).ok_or(Error::ArithmeticOverflow)?;
        }

        env.storage()
            .persistent()
            .set(&(symbol_short!("energy"), auction_id), &remaining);
        env.storage()
            .persistent()
            .set(&(symbol_short!("cleared"), auction_id), &true);

        Ok(MatchResult {
            auction_id,
            matches,
            energy_matched,
            consideration,
            seller_cleared: energy_matched > 0 && remaining == 0,
        })
    }

    /// Withdraws an open listing before it clears. Only the seller may do so, and
    /// the listing's remaining energy is destroyed so it can never be matched.
    pub fn cancel_auction(env: Env, seller: Address, auction_id: u64) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        let listed_seller: Address = env
            .storage()
            .persistent()
            .get(&(symbol_short!("seller"), auction_id))
            .ok_or(Error::AuctionNotFound)?;
        if listed_seller != seller {
            return Err(Error::NotSeller);
        }
        let cleared: bool = env
            .storage()
            .persistent()
            .get(&(symbol_short!("cleared"), auction_id))
            .unwrap();
        if cleared {
            return Err(Error::AuctionClosed);
        }
        let cancelled: bool = env
            .storage()
            .persistent()
            .get(&(symbol_short!("canceld"), auction_id))
            .unwrap();
        if cancelled {
            return Err(Error::AlreadyCancelled);
        }
        seller.require_auth();

        env.storage()
            .persistent()
            .set(&(symbol_short!("canceld"), auction_id), &true);
        env.storage()
            .persistent()
            .set(&(symbol_short!("energy"), auction_id), &0i128);
        env.storage()
            .persistent()
            .set(&(symbol_short!("bids"), auction_id), &Vec::<Bid>::new(&env));
        env.storage()
            .persistent()
            .set(&(symbol_short!("buyers"), auction_id), &Vec::<Address>::new(&env));
        Ok(())
    }

    /// Returns the full record for a listing.
    pub fn auction(env: Env, auction_id: u64) -> Result<Auction, Error> {
        let seller: Address = env
            .storage()
            .persistent()
            .get(&(symbol_short!("seller"), auction_id))
            .ok_or(Error::AuctionNotFound)?;
        Ok(Auction {
            id: auction_id,
            seller,
            energy_mwh: env
                .storage()
                .persistent()
                .get(&(symbol_short!("energy"), auction_id))
                .unwrap(),
            listed_mwh: env
                .storage()
                .persistent()
                .get(&(symbol_short!("listed"), auction_id))
                .unwrap(),
            min_price: env
                .storage()
                .persistent()
                .get(&(symbol_short!("minprice"), auction_id))
                .unwrap(),
            cleared: env
                .storage()
                .persistent()
                .get(&(symbol_short!("cleared"), auction_id))
                .unwrap(),
            cancelled: env
                .storage()
                .persistent()
                .get(&(symbol_short!("canceld"), auction_id))
                .unwrap(),
        })
    }

    /// Returns the highest-priority live bid: lowest price, earliest arrival on ties.
    pub fn best_bid(env: Env, auction_id: u64) -> Result<Bid, Error> {
        if !env
            .storage()
            .persistent()
            .has(&(symbol_short!("seller"), auction_id))
        {
            return Err(Error::AuctionNotFound);
        }
        let bids: Vec<Bid> = env
            .storage()
            .persistent()
            .get(&(symbol_short!("bids"), auction_id))
            .unwrap_or(Vec::new(&env));
        if bids.is_empty() {
            return Err(Error::NoBids);
        }
        let order = Self::priority_order(&env, &bids);
        Ok(bids.get(order.get(0).unwrap()).unwrap())
    }

    /// Number of live bids currently resting on a listing.
    pub fn bid_count(env: Env, auction_id: u64) -> Result<u32, Error> {
        if !env
            .storage()
            .persistent()
            .has(&(symbol_short!("seller"), auction_id))
        {
            return Err(Error::AuctionNotFound);
        }
        let bids: Vec<Bid> = env
            .storage()
            .persistent()
            .get(&(symbol_short!("bids"), auction_id))
            .unwrap_or(Vec::new(&env));
        Ok(bids.len())
    }

    /// Reports whether the matching engine has already run against a listing.
    pub fn is_cleared(env: Env, auction_id: u64) -> bool {
        env.storage()
            .persistent()
            .get(&(symbol_short!("cleared"), auction_id))
            .unwrap_or(false)
    }

    fn require_initialized(env: &Env) -> Result<(), Error> {
        if env.storage().instance().has(&symbol_short!("admin")) {
            Ok(())
        } else {
            Err(Error::NotInitialized)
        }
    }

    fn bid_precedes(bids: &Vec<Bid>, a: u32, b: u32) -> bool {
        let x = bids.get(a).unwrap();
        let y = bids.get(b).unwrap();
        if x.price != y.price {
            x.price < y.price
        } else {
            x.seq < y.seq
        }
    }

    fn priority_order(env: &Env, bids: &Vec<Bid>) -> Vec<u32> {
        let n = bids.len();
        let mut order: Vec<u32> = Vec::new(env);
        let mut i: u32 = 0;
        while i < n {
            order.push_back(i);
            i += 1;
        }
        let mut i: u32 = 0;
        while i < n {
            let mut best = i;
            let mut j = i + 1;
            while j < n {
                if Self::bid_precedes(bids, order.get(j).unwrap(), order.get(best).unwrap()) {
                    best = j;
                }
                j += 1;
            }
            if best != i {
                let held = order.get(i).unwrap();
                order.set(i, order.get(best).unwrap());
                order.set(best, held);
            }
            i += 1;
        }
        order
    }
}