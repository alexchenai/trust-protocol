use crate::errors::TrustError;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Burn, Token, TokenAccount, Transfer};

/// Calculate stake factor based on TrustScore.
/// Whitepaper: factor_stake decreases linearly from 100% (score 0) to 5% (score 100).
/// Returns basis points (10000 = 100%).
fn calculate_stake_factor(trust_score: u16, min_bps: u16, max_bps: u16) -> u16 {
    if trust_score >= 100 {
        return min_bps;
    }
    // Linear interpolation: max_bps - (trust_score * (max_bps - min_bps) / 100)
    let range = (max_bps as u32).saturating_sub(min_bps as u32);
    let reduction = range.saturating_mul(trust_score as u32) / 100;
    (max_bps as u32).saturating_sub(reduction) as u16
}

/// Create a new contract between requester and provider.
/// Provider must stake: contract_value * factor_stake(TrustScore).
/// Whitepaper Section 3: Dynamic Staking + Exposure limits (3x capital).
pub fn handler_create(ctx: Context<CreateContract>, value: u64) -> Result<()> {
    let config = &ctx.accounts.protocol_config;
    let provider_identity = &ctx.accounts.provider_identity;

    require!(!provider_identity.banned, TrustError::AgentBanned);
    require!(provider_identity.matured, TrustError::IdentityNotMatured);

    // Calculate required stake
    let stake_factor = calculate_stake_factor(
        provider_identity.trust_score,
        config.min_stake_factor_bps,
        config.max_stake_factor_bps,
    );
    let stake_required = (value as u128)
        .checked_mul(stake_factor as u128)
        .ok_or(TrustError::MathOverflow)?
        / 10_000;
    let stake_required = stake_required as u64;

    // Transfer provider stake
    let transfer_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        Transfer {
            from: ctx.accounts.provider_token_account.to_account_info(),
            to: ctx.accounts.escrow_vault.to_account_info(),
            authority: ctx.accounts.provider.to_account_info(),
        },
    );
    token::transfer(transfer_ctx, stake_required)?;

    // Transfer contract value from requester
    let transfer_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        Transfer {
            from: ctx.accounts.requester_token_account.to_account_info(),
            to: ctx.accounts.escrow_vault.to_account_info(),
            authority: ctx.accounts.requester.to_account_info(),
        },
    );
    token::transfer(transfer_ctx, value)?;

    // Create contract
    let contract_id = ctx.accounts.protocol_config.total_contracts;
    let contract = &mut ctx.accounts.contract;
    contract.id = contract_id;
    contract.requester = ctx.accounts.requester.key();
    contract.provider = ctx.accounts.provider.key();
    contract.value = value;
    contract.provider_stake = stake_required;
    contract.requester_stake = 0;
    contract.status = ContractStatus::Active;
    contract.created_at = Clock::get()?.unix_timestamp;
    contract.resolved_at = 0;
    contract.poe_hash = [0u8; 32];
    contract.poe_arweave_tx = String::new();
    contract.dispute_level = 0;
    contract.bump = ctx.bumps.contract;

    // Increment contract counter
    let config = &mut ctx.accounts.protocol_config;
    config.total_contracts = config
        .total_contracts
        .checked_add(1)
        .ok_or(TrustError::MathOverflow)?;

    msg!(
        "Contract #{} created. Value: {}, Stake: {} (factor: {}bps). Requester: {}, Provider: {}",
        contract_id,
        value,
        stake_required,
        stake_factor,
        contract.requester,
        contract.provider
    );
    Ok(())
}

/// Provider submits deliverable with Proof of Execution.
/// Whitepaper Section 1: PoE - immutable record with input/output hashes.
pub fn handler_deliver(
    ctx: Context<DeliverContract>,
    output_hash: [u8; 32],
    arweave_tx: String,
) -> Result<()> {
    let contract = &mut ctx.accounts.contract;
    require!(
        contract.status == ContractStatus::Active,
        TrustError::InvalidContractStatus
    );
    require!(
        contract.provider == ctx.accounts.provider.key(),
        TrustError::UnauthorizedProvider
    );

    contract.poe_hash = output_hash;
    contract.poe_arweave_tx = arweave_tx.clone();
    contract.status = ContractStatus::Delivered;

    // Create PoE record
    let poe = &mut ctx.accounts.proof_of_execution;
    poe.contract = contract.key();
    poe.provider = ctx.accounts.provider.key();
    poe.input_hash = [0u8; 32]; // Set by requester at contract creation in future version
    poe.output_hash = output_hash;
    poe.submitted_at = Clock::get()?.unix_timestamp;
    poe.validated = false;
    poe.arweave_tx = arweave_tx;
    poe.bump = ctx.bumps.proof_of_execution;

    msg!("Contract #{} delivered. PoE submitted.", contract.id);
    Ok(())
}

/// Requester accepts deliverable. Releases payment + returns provider stake.
/// Updates provider's TrustScore factors (tasks_completed, volume_processed).
pub fn handler_accept(ctx: Context<AcceptContract>) -> Result<()> {
    let contract = &mut ctx.accounts.contract;
    require!(
        contract.status == ContractStatus::Delivered,
        TrustError::InvalidContractStatus
    );
    require!(
        contract.requester == ctx.accounts.requester.key(),
        TrustError::UnauthorizedRequester
    );

    contract.status = ContractStatus::Completed;
    contract.resolved_at = Clock::get()?.unix_timestamp;

    // Mark PoE as validated
    let poe = &mut ctx.accounts.proof_of_execution;
    poe.validated = true;

    // Update provider stats
    let provider_identity = &mut ctx.accounts.provider_identity;
    provider_identity.tasks_completed = provider_identity.tasks_completed.saturating_add(1);
    provider_identity.volume_processed = provider_identity
        .volume_processed
        .saturating_add(contract.value);

    // Release escrow: payment to provider, stake back to provider
    let total_release = contract
        .value
        .checked_add(contract.provider_stake)
        .ok_or(TrustError::MathOverflow)?;
    let contract_id_bytes = contract.id.to_le_bytes();
    let escrow_bump = ctx.bumps.escrow_vault;
    let escrow_seeds: &[&[u8]] = &[b"escrow", &contract_id_bytes, &[escrow_bump]];
    let signer_seeds = &[escrow_seeds];

    let transfer_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        Transfer {
            from: ctx.accounts.escrow_vault.to_account_info(),
            to: ctx.accounts.provider_token_account.to_account_info(),
            authority: ctx.accounts.escrow_vault.to_account_info(),
        },
        signer_seeds,
    );
    token::transfer(transfer_ctx, total_release)?;

    msg!(
        "Contract #{} completed. {} SWORN released to provider. Tasks: {}, Volume: {}",
        contract.id,
        total_release,
        provider_identity.tasks_completed,
        provider_identity.volume_processed
    );
    Ok(())
}

#[derive(Accounts)]
pub struct CreateContract<'info> {
    #[account(mut)]
    pub requester: Signer<'info>,

    /// CHECK: Provider pubkey (doesn't need to sign at creation)
    pub provider: UncheckedAccount<'info>,

    #[account(
        seeds = [b"agent-identity", provider.key().as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Account<'info, AgentIdentity>,

    #[account(
        init,
        payer = requester,
        space = 8 + Contract::INIT_SPACE,
        seeds = [b"contract", &protocol_config.total_contracts.to_le_bytes()],
        bump
    )]
    pub contract: Account<'info, Contract>,

    #[account(
        mut,
        constraint = requester_token_account.owner == requester.key(),
        constraint = requester_token_account.mint == protocol_config.sworn_mint,
    )]
    pub requester_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = provider_token_account.owner == provider.key(),
        constraint = provider_token_account.mint == protocol_config.sworn_mint,
    )]
    pub provider_token_account: Account<'info, TokenAccount>,

    /// Escrow vault PDA for this contract
    #[account(
        mut,
        seeds = [b"escrow", &protocol_config.total_contracts.to_le_bytes()],
        bump,
    )]
    pub escrow_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DeliverContract<'info> {
    #[account(mut)]
    pub provider: Signer<'info>,

    #[account(
        mut,
        constraint = contract.provider == provider.key() @ TrustError::UnauthorizedProvider,
        constraint = contract.status == ContractStatus::Active @ TrustError::InvalidContractStatus,
    )]
    pub contract: Account<'info, Contract>,

    #[account(
        init,
        payer = provider,
        space = 8 + ProofOfExecution::INIT_SPACE,
        seeds = [b"poe", contract.key().as_ref()],
        bump
    )]
    pub proof_of_execution: Account<'info, ProofOfExecution>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AcceptContract<'info> {
    pub requester: Signer<'info>,

    #[account(
        mut,
        constraint = contract.requester == requester.key() @ TrustError::UnauthorizedRequester,
        constraint = contract.status == ContractStatus::Delivered @ TrustError::InvalidContractStatus,
    )]
    pub contract: Account<'info, Contract>,

    #[account(
        mut,
        seeds = [b"poe", contract.key().as_ref()],
        bump = proof_of_execution.bump,
    )]
    pub proof_of_execution: Account<'info, ProofOfExecution>,

    #[account(
        mut,
        seeds = [b"agent-identity", contract.provider.as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Account<'info, AgentIdentity>,

    /// Provider's SWORN token account (receives payment + stake return)
    #[account(
        mut,
        constraint = provider_token_account.owner == contract.provider,
        constraint = provider_token_account.mint == protocol_config.sworn_mint,
    )]
    pub provider_token_account: Account<'info, TokenAccount>,

    /// Escrow vault for this contract
    #[account(
        mut,
        seeds = [b"escrow", &contract.id.to_le_bytes()],
        bump,
    )]
    pub escrow_vault: Account<'info, TokenAccount>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    pub token_program: Program<'info, Token>,
}
