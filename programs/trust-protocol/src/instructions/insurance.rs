use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use crate::state::*;
use crate::errors::TrustError;

/// File a retroactive insurance claim within 90 days of contract completion.
/// Whitepaper Section 6: Insurance Pool - requester discovers defect post-acceptance.
/// Requires anti-spam collateral deposit. Max payout: 80% of contract value.
pub fn handler_file_claim(
    ctx: Context<FileInsuranceClaim>,
    amount: u64,
    evidence_hash: [u8; 32],
) -> Result<()> {
    let contract = &ctx.accounts.contract;
    let config = &ctx.accounts.protocol_config;
    let now = Clock::get()?.unix_timestamp;

    // Must be a completed contract
    require!(contract.status == ContractStatus::Completed, TrustError::InvalidContractStatus);
    require!(contract.requester == ctx.accounts.claimant.key(), TrustError::UnauthorizedRequester);

    // Check 90-day claim window
    let window_end = contract.resolved_at.checked_add(config.claim_window).ok_or(TrustError::MathOverflow)?;
    require!(now <= window_end, TrustError::ClaimWindowExpired);

    // Max claim: 80% of contract value
    let max_payout = (contract.value as u128 * config.max_claim_payout_bps as u128 / 10_000) as u64;
    require!(amount <= max_payout, TrustError::ClaimAmountExceeded);

    // Anti-spam collateral: 10% of claim amount
    let collateral = amount / 10;
    require!(collateral > 0, TrustError::InsufficientCollateral);

    // Transfer collateral from claimant
    let transfer_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        Transfer {
            from: ctx.accounts.claimant_token_account.to_account_info(),
            to: ctx.accounts.insurance_vault.to_account_info(),
            authority: ctx.accounts.claimant.to_account_info(),
        },
    );
    token::transfer(transfer_ctx, collateral)?;

    // Create claim
    let claim = &mut ctx.accounts.insurance_claim;
    claim.claimant = ctx.accounts.claimant.key();
    claim.contract = contract.key();
    claim.amount = amount;
    claim.collateral = collateral;
    claim.evidence_hash = evidence_hash;
    claim.status = ClaimStatus::Filed;
    claim.filed_at = now;
    claim.contract_completed_at = contract.resolved_at;
    claim.bump = ctx.bumps.insurance_claim;

    let pool = &mut ctx.accounts.insurance_pool;
    pool.active_claims = pool.active_claims.saturating_add(1);

    msg!(
        "Insurance claim filed. Amount: {}, Collateral: {}. Window closes: {}",
        amount, collateral, window_end
    );
    Ok(())
}

/// Approve an insurance claim (admin in Phase 0-2, DAO in Phase 3+).
/// Pays out from insurance pool to claimant. Returns collateral.
pub fn handler_approve_claim(ctx: Context<ApproveInsuranceClaim>) -> Result<()> {
    let config = &ctx.accounts.protocol_config;

    // Phase 0-2: only admin can approve
    if config.governance_phase < 3 {
        require!(ctx.accounts.admin.key() == config.admin, TrustError::UnauthorizedAdmin);
    }

    let claim = &mut ctx.accounts.insurance_claim;
    require!(claim.status == ClaimStatus::Filed || claim.status == ClaimStatus::UnderReview, TrustError::InvalidContractStatus);

    claim.status = ClaimStatus::Approved;

    let pool = &mut ctx.accounts.insurance_pool;
    let payout = claim.amount.min(pool.total_balance);

    // Transfer payout from insurance vault to claimant
    let pool_seeds = &[
        b"pool-authority".as_ref(),
        &[ctx.bumps.pool_authority],
    ];
    let transfer_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        Transfer {
            from: ctx.accounts.insurance_vault.to_account_info(),
            to: ctx.accounts.claimant_token_account.to_account_info(),
            authority: ctx.accounts.pool_authority.to_account_info(),
        },
        &[pool_seeds],
    );
    token::transfer(transfer_ctx, payout)?;

    // Return collateral
    let collateral_transfer = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        Transfer {
            from: ctx.accounts.insurance_vault.to_account_info(),
            to: ctx.accounts.claimant_token_account.to_account_info(),
            authority: ctx.accounts.pool_authority.to_account_info(),
        },
        &[pool_seeds],
    );
    token::transfer(collateral_transfer, claim.collateral)?;

    pool.total_balance = pool.total_balance.saturating_sub(payout);
    pool.total_claims_paid = pool.total_claims_paid.saturating_add(payout);
    pool.active_claims = pool.active_claims.saturating_sub(1);

    // Penalize provider
    let provider_identity = &mut ctx.accounts.provider_identity;
    provider_identity.fraud_flags = provider_identity.fraud_flags.saturating_add(1);

    msg!("Insurance claim approved. Payout: {}, Collateral returned: {}", payout, claim.collateral);
    Ok(())
}

/// Deny an insurance claim. Collateral is forfeited to the insurance pool.
pub fn handler_deny_claim(ctx: Context<ApproveInsuranceClaim>) -> Result<()> {
    let config = &ctx.accounts.protocol_config;
    if config.governance_phase < 3 {
        require!(ctx.accounts.admin.key() == config.admin, TrustError::UnauthorizedAdmin);
    }

    let claim = &mut ctx.accounts.insurance_claim;
    claim.status = ClaimStatus::Denied;

    // Collateral stays in insurance pool (anti-spam penalty)
    let pool = &mut ctx.accounts.insurance_pool;
    pool.total_balance = pool.total_balance.saturating_add(claim.collateral);
    pool.active_claims = pool.active_claims.saturating_sub(1);

    msg!("Insurance claim denied. Collateral {} forfeited to pool.", claim.collateral);
    Ok(())
}

#[derive(Accounts)]
pub struct FileInsuranceClaim<'info> {
    #[account(mut)]
    pub claimant: Signer<'info>,

    #[account(
        constraint = contract.requester == claimant.key() @ TrustError::UnauthorizedRequester,
        constraint = contract.status == ContractStatus::Completed @ TrustError::InvalidContractStatus,
    )]
    pub contract: Account<'info, Contract>,

    #[account(
        init,
        payer = claimant,
        space = 8 + InsuranceClaim::INIT_SPACE,
        seeds = [b"insurance-claim", contract.key().as_ref()],
        bump
    )]
    pub insurance_claim: Account<'info, InsuranceClaim>,

    #[account(
        mut,
        seeds = [b"insurance-pool"],
        bump = insurance_pool.bump,
    )]
    pub insurance_pool: Account<'info, InsurancePool>,

    #[account(
        mut,
        constraint = claimant_token_account.owner == claimant.key(),
    )]
    pub claimant_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub insurance_vault: Account<'info, TokenAccount>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ApproveInsuranceClaim<'info> {
    pub admin: Signer<'info>,

    #[account(mut)]
    pub insurance_claim: Account<'info, InsuranceClaim>,

    #[account(
        mut,
        seeds = [b"insurance-pool"],
        bump = insurance_pool.bump,
    )]
    pub insurance_pool: Account<'info, InsurancePool>,

    #[account(mut)]
    pub insurance_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = claimant_token_account.owner == insurance_claim.claimant,
    )]
    pub claimant_token_account: Account<'info, TokenAccount>,

    /// CHECK: PDA authority for insurance pool
    #[account(
        seeds = [b"pool-authority"],
        bump
    )]
    pub pool_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"agent-identity", provider_identity.authority.as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Account<'info, AgentIdentity>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    pub token_program: Program<'info, Token>,
}
