use crate::errors::TrustError;
use crate::instructions::contract::calculate_stake_factor;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

/// Propose a contract. Only requester signs; deposits escrow value.
/// Provider must call accept_proposal to activate the contract.
pub fn handler_propose(ctx: Context<ProposeContract>, value: u64, expiry_seconds: u64) -> Result<()> {
    let config = &ctx.accounts.protocol_config;
    let provider_identity = &ctx.accounts.provider_identity;

    // Validate provider is a registered, matured, non-banned agent
    require!(!provider_identity.banned, TrustError::AgentBanned);
    require!(provider_identity.matured, TrustError::IdentityNotMatured);

    // Calculate the stake the provider will need to deposit on accept
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

    // Transfer contract value from requester to escrow
    let transfer_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        Transfer {
            from: ctx.accounts.requester_token_account.to_account_info(),
            to: ctx.accounts.escrow_vault.to_account_info(),
            authority: ctx.accounts.requester.to_account_info(),
        },
    );
    token::transfer(transfer_ctx, value)?;

    // Initialize contract in Proposed state
    let contract_id = ctx.accounts.protocol_config.total_contracts;
    let now = Clock::get()?.unix_timestamp;
    let contract = &mut ctx.accounts.contract;
    contract.id = contract_id;
    contract.requester = ctx.accounts.requester.key();
    contract.provider = ctx.accounts.provider.key();
    contract.value = value;
    contract.provider_stake = 0;
    contract.requester_stake = 0;
    contract.status = ContractStatus::Proposed;
    contract.created_at = now;
    contract.resolved_at = 0;
    contract.poe_hash = [0u8; 32];
    contract.poe_arweave_tx = String::new();
    contract.dispute_level = 0;
    contract.bump = ctx.bumps.contract;
    contract.proposal_expires_at = if expiry_seconds > 0 {
        now.checked_add(expiry_seconds as i64).ok_or(TrustError::MathOverflow)?
    } else {
        0
    };
    contract.provider_stake_required = stake_required;

    // Increment contract counter
    let config = &mut ctx.accounts.protocol_config;
    config.total_contracts = config
        .total_contracts
        .checked_add(1)
        .ok_or(TrustError::MathOverflow)?;

    msg!(
        "Contract #{} proposed. Value: {}, Required stake: {} (factor: {}bps). Requester: {}, Provider: {}",
        contract_id, value, stake_required, stake_factor,
        contract.requester, contract.provider
    );
    Ok(())
}

/// Provider accepts a proposed contract by depositing stake.
/// Transitions contract from Proposed to Active.
pub fn handler_accept_proposal(ctx: Context<AcceptProposal>) -> Result<()> {
    let contract = &mut ctx.accounts.contract;

    require!(
        contract.status == ContractStatus::Proposed,
        TrustError::InvalidContractStatus
    );
    require!(
        contract.provider == ctx.accounts.provider.key(),
        TrustError::UnauthorizedProvider
    );

    // Check expiry
    if contract.proposal_expires_at > 0 {
        let now = Clock::get()?.unix_timestamp;
        require!(
            now <= contract.proposal_expires_at,
            TrustError::ProposalExpired
        );
    }

    // Transfer provider stake to escrow
    let stake_required = contract.provider_stake_required;
    let transfer_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        Transfer {
            from: ctx.accounts.provider_token_account.to_account_info(),
            to: ctx.accounts.escrow_vault.to_account_info(),
            authority: ctx.accounts.provider.to_account_info(),
        },
    );
    token::transfer(transfer_ctx, stake_required)?;

    // Transition to Active
    contract.provider_stake = stake_required;
    contract.status = ContractStatus::Active;

    msg!(
        "Contract #{} accepted by provider {}. Stake deposited: {}",
        contract.id, contract.provider, stake_required
    );
    Ok(())
}

/// Cancel an expired proposal. Requester reclaims escrowed funds.
pub fn handler_cancel_proposal(ctx: Context<CancelProposal>) -> Result<()> {
    let contract = &mut ctx.accounts.contract;

    require!(
        contract.status == ContractStatus::Proposed,
        TrustError::InvalidContractStatus
    );
    require!(
        contract.requester == ctx.accounts.requester.key(),
        TrustError::UnauthorizedRequester
    );

    // If there's an expiry, requester can only cancel after expiry.
    // If no expiry (0), requester can cancel anytime.
    if contract.proposal_expires_at > 0 {
        let now = Clock::get()?.unix_timestamp;
        require!(
            now > contract.proposal_expires_at,
            TrustError::ProposalNotExpired
        );
    }

    // Return escrow to requester via PDA signer
    let contract_id_bytes = contract.id.to_le_bytes();
    let (_, escrow_bump) = Pubkey::find_program_address(
        &[b"escrow", &contract_id_bytes],
        ctx.program_id,
    );
    let escrow_seeds: &[&[u8]] = &[b"escrow", &contract_id_bytes, &[escrow_bump]];
    let signer_seeds = &[escrow_seeds];

    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.escrow_vault.to_account_info(),
                to: ctx.accounts.requester_token_account.to_account_info(),
                authority: ctx.accounts.escrow_vault.to_account_info(),
            },
            signer_seeds,
        ),
        contract.value,
    )?;

    contract.status = ContractStatus::Cancelled;
    contract.resolved_at = Clock::get()?.unix_timestamp;

    msg!("Contract #{} proposal cancelled. Escrow returned to requester.", contract.id);
    Ok(())
}

// === Account Structs ===

#[derive(Accounts)]
pub struct ProposeContract<'info> {
    #[account(mut)]
    pub requester: Signer<'info>,

    /// CHECK: Provider pubkey - does NOT sign. Used only for PDA lookup + recording.
    pub provider: UncheckedAccount<'info>,

    /// Provider's identity - read-only, used to calculate stake requirement.
    #[account(
        seeds = [b"agent-identity" as &[u8], provider.key().as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Account<'info, AgentIdentity>,

    #[account(
        init,
        payer = requester,
        space = 8 + Contract::INIT_SPACE,
        seeds = [b"contract" as &[u8], &protocol_config.total_contracts.to_le_bytes()],
        bump,
    )]
    pub contract: Account<'info, Contract>,

    #[account(
        mut,
        constraint = requester_token_account.owner == requester.key(),
        constraint = requester_token_account.mint == protocol_config.sworn_mint,
    )]
    pub requester_token_account: Account<'info, TokenAccount>,

    /// Escrow vault PDA - holds requester's deposit until provider accepts or proposal expires.
    #[account(
        init,
        payer = requester,
        token::mint = sworn_mint,
        token::authority = escrow_vault,
        seeds = [b"escrow" as &[u8], &protocol_config.total_contracts.to_le_bytes()],
        bump,
    )]
    pub escrow_vault: Account<'info, TokenAccount>,

    #[account(
        constraint = sworn_mint.key() == protocol_config.sworn_mint,
    )]
    pub sworn_mint: Account<'info, Mint>,

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
pub struct AcceptProposal<'info> {
    #[account(mut)]
    pub provider: Signer<'info>,

    #[account(
        mut,
        constraint = contract.provider == provider.key() @ TrustError::UnauthorizedProvider,
        constraint = contract.status == ContractStatus::Proposed @ TrustError::InvalidContractStatus,
    )]
    pub contract: Account<'info, Contract>,

    #[account(
        mut,
        constraint = provider_token_account.owner == provider.key(),
        constraint = provider_token_account.mint == protocol_config.sworn_mint,
    )]
    pub provider_token_account: Account<'info, TokenAccount>,

    /// Escrow vault - already initialized by propose_contract
    #[account(
        mut,
        seeds = [b"escrow" as &[u8], &contract.id.to_le_bytes()],
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

#[derive(Accounts)]
pub struct CancelProposal<'info> {
    pub requester: Signer<'info>,

    #[account(
        mut,
        constraint = contract.requester == requester.key() @ TrustError::UnauthorizedRequester,
        constraint = contract.status == ContractStatus::Proposed @ TrustError::InvalidContractStatus,
    )]
    pub contract: Account<'info, Contract>,

    #[account(
        mut,
        constraint = requester_token_account.owner == requester.key(),
    )]
    pub requester_token_account: Account<'info, TokenAccount>,

    /// Escrow vault - return funds to requester
    #[account(
        mut,
        seeds = [b"escrow" as &[u8], &contract.id.to_le_bytes()],
        bump,
    )]
    pub escrow_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}
