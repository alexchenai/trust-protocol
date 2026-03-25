use anchor_lang::prelude::*;
use crate::state::*;
use crate::errors::TrustError;

/// Place a bid on a public contract (Whitepaper §6.5).
/// bid_score = W_price * price_score + W_trust * trust_norm + W_speed * speed_score
/// Weights are governable via ProtocolConfig (default: 0.35/0.45/0.20).
pub fn handler_place_bid(
    ctx: Context<PlaceBid>,
    proposed_price: u64,
    proposed_deadline: u64,
    stake_offered: u64,
    message_hash: [u8; 32],
    sla_deadline_hours: u64,
) -> Result<()> {
    let contract = &ctx.accounts.contract;
    let identity = &ctx.accounts.bidder_identity;
    let config = &mut ctx.accounts.protocol_config;

    // Contract must be public (visibility == 1)
    require!(contract.visibility == 1, TrustError::ContractNotPublic);

    // Contract must be in Created or Proposed status
    require!(
        contract.status == ContractStatus::Created || contract.status == ContractStatus::Proposed,
        TrustError::ContractNotBiddable,
    );

    // Proposed price must not exceed contract value (escrow amount)
    require!(proposed_price <= contract.value, TrustError::BidPriceTooHigh);

    // Check minimum stake requirement for bidder's TrustScore
    let ts = identity.trust_score as u64;
    let ts_f = ts as f64 / 100.0;
    let stake_factor = f64::max(0.05, 1.0 - 0.95 * ts_f.powf(1.5));
    let min_stake = ((contract.value as f64) * stake_factor) as u64;
    require!(stake_offered >= min_stake, TrustError::BidStakeInsufficient);

    // Compute bid_score (§6.5), all in bps (10000 = 1.0)
    let w_price = config.bid_weight_price_bps as u64; // 3500
    let w_trust = config.bid_weight_trust_bps as u64; // 4500
    let w_speed = config.bid_weight_speed_bps as u64; // 2000

    // price_score = 1.0 - (proposed_price / escrow_amount), clamped [0, 10000]
    let price_score = if contract.value == 0 {
        0u64
    } else {
        let ratio = (proposed_price as u128)
            .checked_mul(10000)
            .unwrap()
            .checked_div(contract.value as u128)
            .unwrap() as u64;
        10000u64.saturating_sub(ratio)
    };

    // trust_score_norm = bidder_ts / 100, scaled to 10000
    let trust_norm = (identity.trust_score as u64)
        .checked_mul(100)
        .unwrap(); // TS 100 => 10000

    // speed_score = max(0, 1.0 - proposed_deadline / sla_deadline), scaled to 10000
    let speed_score = if sla_deadline_hours == 0 {
        0u64
    } else {
        let ratio = (proposed_deadline as u128)
            .checked_mul(10000)
            .unwrap()
            .checked_div(sla_deadline_hours as u128)
            .unwrap() as u64;
        10000u64.saturating_sub(ratio)
    };

    // bid_score = W_price * price_score + W_trust * trust_norm + W_speed * speed_score
    // All divided by 10000 to normalize weights
    let bid_score = w_price
        .checked_mul(price_score)
        .unwrap()
        .checked_add(w_trust.checked_mul(trust_norm).unwrap())
        .unwrap()
        .checked_add(w_speed.checked_mul(speed_score).unwrap())
        .unwrap()
        .checked_div(10000)
        .unwrap();

    let clock = Clock::get()?;
    let bid = &mut ctx.accounts.bid;
    bid.bid_id = config.total_contracts; // reuse as monotonic counter
    bid.task_id = ctx.accounts.contract.key();
    bid.bidder = ctx.accounts.bidder_identity.key();
    bid.proposed_price = proposed_price;
    bid.proposed_deadline = proposed_deadline;
    bid.bidder_ts = identity.trust_score;
    bid.stake_offered = stake_offered;
    bid.message_hash = message_hash;
    bid.timestamp = clock.unix_timestamp;
    bid.bid_score = bid_score;
    bid.active = true;
    bid.bump = ctx.bumps.bid;

    msg!(
        "Bid placed: score={}, price={}, deadline={}h, ts={}",
        bid_score, proposed_price, proposed_deadline, identity.trust_score
    );

    Ok(())
}

/// Withdraw a bid (bidder cancels their bid).
pub fn handler_withdraw_bid(ctx: Context<WithdrawBid>) -> Result<()> {
    let bid = &mut ctx.accounts.bid;
    require!(bid.active, TrustError::BidNotActive);
    bid.active = false;
    msg!("Bid {} withdrawn by bidder", bid.bid_id);
    Ok(())
}

/// Requester selects a winning bid (§6.5). Sets provider on contract.
pub fn handler_select_bid(ctx: Context<SelectBid>) -> Result<()> {
    let bid = &ctx.accounts.bid;
    require!(bid.active, TrustError::BidNotActive);

    let contract = &mut ctx.accounts.contract;
    require!(
        contract.status == ContractStatus::Created || contract.status == ContractStatus::Proposed,
        TrustError::ContractNotBiddable,
    );

    // Set provider to the bidder's authority
    contract.provider = ctx.accounts.bidder_authority.key();
    contract.value = bid.proposed_price;
    contract.status = ContractStatus::Proposed;

    msg!(
        "Bid {} selected for contract {}. Provider set to {}",
        bid.bid_id, contract.id, contract.provider
    );

    Ok(())
}

// === Account structs ===

#[derive(Accounts)]
pub struct PlaceBid<'info> {
    #[account(mut)]
    pub bidder: Signer<'info>,

    #[account(
        seeds = [b"agent-identity", bidder.key().as_ref()],
        bump = bidder_identity.bump,
    )]
    pub bidder_identity: Account<'info, AgentIdentity>,

    #[account(mut)]
    pub contract: Account<'info, Contract>,

    #[account(
        init,
        payer = bidder,
        space = 8 + Bid::INIT_SPACE,
        seeds = [b"bid", contract.key().as_ref(), bidder.key().as_ref()],
        bump,
    )]
    pub bid: Account<'info, Bid>,

    #[account(
        mut,
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WithdrawBid<'info> {
    #[account(mut)]
    pub bidder: Signer<'info>,

    #[account(
        mut,
        seeds = [b"bid", bid.task_id.as_ref(), bidder.key().as_ref()],
        bump = bid.bump,
        constraint = bid.bidder == bidder_identity.key() @ TrustError::UnauthorizedBidder,
    )]
    pub bid: Account<'info, Bid>,

    #[account(
        seeds = [b"agent-identity", bidder.key().as_ref()],
        bump = bidder_identity.bump,
    )]
    pub bidder_identity: Account<'info, AgentIdentity>,
}

#[derive(Accounts)]
pub struct SelectBid<'info> {
    #[account(mut)]
    pub requester: Signer<'info>,

    #[account(
        mut,
        constraint = contract.requester == requester.key() @ TrustError::UnauthorizedBidSelector,
    )]
    pub contract: Account<'info, Contract>,

    #[account(
        seeds = [b"bid", contract.key().as_ref(), bidder_authority.key().as_ref()],
        bump = bid.bump,
    )]
    pub bid: Account<'info, Bid>,

    /// The authority wallet of the bidder (used as PDA seed)
    /// CHECK: Verified via bid PDA derivation
    pub bidder_authority: UncheckedAccount<'info>,
}
