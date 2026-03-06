use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token};
use crate::state::*;
use crate::errors::TrustError;

/// Initialize the Trust Protocol with SWORN token mint and global config.
/// Called once by the founding team (Phase 0 governance).
pub fn handler(ctx: Context<Initialize>, params: InitializeParams) -> Result<()> {
    let config = &mut ctx.accounts.protocol_config;
    config.admin = ctx.accounts.admin.key();
    config.sworn_mint = ctx.accounts.sworn_mint.key();
    config.min_identity_bond = params.min_identity_bond;
    config.max_identity_bond = params.max_identity_bond;
    config.maturation_period = 2_592_000; // 30 days
    config.min_stake_factor_bps = 500; // 5%
    config.max_stake_factor_bps = 10_000; // 100%
    config.burn_rate_bps = 1_500; // 15%
    config.insurance_rate_bps = 6_000; // 60%
    config.claim_window = 7_776_000; // 90 days
    config.max_claim_payout_bps = 8_000; // 80%
    config.exposure_limit_multiplier = 3;
    config.governance_phase = 0;
    config.total_contracts = 0;
    config.total_agents = 0;
    config.bump = ctx.bumps.protocol_config;

    let pool = &mut ctx.accounts.insurance_pool;
    pool.total_balance = 0;
    pool.total_claims_paid = 0;
    pool.active_claims = 0;
    pool.authority = ctx.accounts.pool_authority.key();
    pool.bump = ctx.bumps.insurance_pool;

    msg!("Trust Protocol initialized. Admin: {}", config.admin);
    Ok(())
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct InitializeParams {
    /// Minimum identity bond in SWORN lamports (2 SWORN = 2_000_000_000)
    pub min_identity_bond: u64,
    /// Maximum identity bond in SWORN lamports (5 SWORN = 5_000_000_000)
    pub max_identity_bond: u64,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    /// The SWORN token mint (fixed supply 100M, decimals 9)
    pub sworn_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = admin,
        space = 8 + ProtocolConfig::INIT_SPACE,
        seeds = [b"protocol-config"],
        bump
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        init,
        payer = admin,
        space = 8 + InsurancePool::INIT_SPACE,
        seeds = [b"insurance-pool"],
        bump
    )]
    pub insurance_pool: Account<'info, InsurancePool>,

    /// CHECK: PDA authority for the insurance pool token account
    #[account(
        seeds = [b"pool-authority"],
        bump
    )]
    pub pool_authority: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}
