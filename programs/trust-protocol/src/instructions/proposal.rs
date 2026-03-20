use crate::errors::TrustError;
use crate::instructions::contract::calculate_stake_factor;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

/// Propose a contract. Only requester signs; deposits escrow value.
/// Provider must call accept_proposal to activate the contract.
pub fn handler_propose(ctx: Context<ProposeContract>, value: u64, expiry_seconds: u64, currency: u8) -> Result<()> {
    let config = &ctx.accounts.protocol_config;
    let provider_identity = &ctx.accounts.provider_identity;

    // Parse currency parameter (0=SWORN, 1=SOL)
    let currency_enum = match currency {
        0 => Currency::Sworn,
        1 => Currency::Sol,
        _ => return Err(TrustError::InvalidCurrency.into()),
    };

    // Validate provider is a registered, matured, non-banned, non-hibernating agent
    require!(!provider_identity.banned, TrustError::AgentBanned);
    require!(provider_identity.matured, TrustError::IdentityNotMatured);
    require!(!provider_identity.is_hibernating, TrustError::AgentHibernating);

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

    // Calculate requester escrow deposit using reduced escrow factor (Whitepaper §7.7)
    let requester_ts = ctx.accounts.requester_identity.trust_score;
    let escrow_factor = crate::instructions::contract::calculate_escrow_factor(requester_ts);
    let escrow_deposit = (value as u128)
        .checked_mul(escrow_factor as u128)
        .ok_or(TrustError::MathOverflow)?
        / 10_000;
    let escrow_deposit = escrow_deposit as u64;

    // Transfer reduced escrow from requester (Whitepaper §7.7)
    if currency_enum == Currency::Sol {
        // SOL escrow: transfer lamports from requester to the contract PDA
        let ix = anchor_lang::solana_program::system_instruction::transfer(
            &ctx.accounts.requester.key(),
            &ctx.accounts.contract.to_account_info().key,
            escrow_deposit,
        );
        anchor_lang::solana_program::program::invoke(
            &ix,
            &[
                ctx.accounts.requester.to_account_info(),
                ctx.accounts.contract.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
    } else {
        // SWORN escrow: SPL token transfer to escrow token account
        let transfer_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.requester_token_account.to_account_info(),
                to: ctx.accounts.escrow_vault.to_account_info(),
                authority: ctx.accounts.requester.to_account_info(),
            },
        );
        token::transfer(transfer_ctx, escrow_deposit)?;
    }

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
    contract.currency = currency_enum;
    // Store escrow factor (Whitepaper §7.7) — already computed above
    contract.escrow_factor_bps = escrow_factor;
    contract.spec_hash = [0u8; 32]; // §6.1: spec_hash set later or via separate instruction
    contract.corrections_used = 0; // §6.2: no corrections yet
    contract.max_corrections_contract = config.max_corrections; // §6.1: from protocol config
    contract.deadline_validation_contract = config.deadline_validation; // §6.1: per-contract validation timeout
    contract.visibility = 0; // §6.4: private by default

    // Increment contract counter
    let config = &mut ctx.accounts.protocol_config;
    config.total_contracts = config
        .total_contracts
        .checked_add(1)
        .ok_or(TrustError::MathOverflow)?;

    msg!(
        "Contract #{} proposed. Value: {}, Currency: {}, Required stake: {} (factor: {}bps). Requester: {}, Provider: {}",
        contract_id, value, currency, stake_required, stake_factor,
        contract.requester, contract.provider
    );
    Ok(())
}

/// Provider accepts a proposed contract by depositing stake.
/// Transitions contract from Proposed to Active.
pub fn handler_accept_proposal(ctx: Context<AcceptProposal>) -> Result<()> {
    // Extract values we need before doing transfers (to avoid borrow conflicts)
    let is_sol = ctx.accounts.contract.currency == Currency::Sol;
    let stake_required = ctx.accounts.contract.provider_stake_required;
    let contract_status = ctx.accounts.contract.status;
    let contract_provider = ctx.accounts.contract.provider;
    let contract_expires = ctx.accounts.contract.proposal_expires_at;
    let contract_id = ctx.accounts.contract.id;

    require!(
        contract_status == ContractStatus::Proposed,
        TrustError::InvalidContractStatus
    );
    require!(
        contract_provider == ctx.accounts.provider.key(),
        TrustError::UnauthorizedProvider
    );

    // Check expiry
    if contract_expires > 0 {
        let now = Clock::get()?.unix_timestamp;
        require!(
            now <= contract_expires,
            TrustError::ProposalExpired
        );
    }

    // Transfer provider stake to escrow
    if is_sol {
        // SOL stake: transfer lamports from provider to contract PDA
        let contract_key = ctx.accounts.contract.to_account_info().key();
        let ix = anchor_lang::solana_program::system_instruction::transfer(
            &ctx.accounts.provider.key(),
            &contract_key,
            stake_required,
        );
        anchor_lang::solana_program::program::invoke(
            &ix,
            &[
                ctx.accounts.provider.to_account_info(),
                ctx.accounts.contract.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
    } else {
        // SWORN stake: SPL token transfer to escrow vault
        let transfer_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.provider_token_account.to_account_info(),
                to: ctx.accounts.escrow_vault.to_account_info(),
                authority: ctx.accounts.provider.to_account_info(),
            },
        );
        token::transfer(transfer_ctx, stake_required)?;
    }

    // Transition to Active
    let now_ts = Clock::get()?.unix_timestamp;
    let contract = &mut ctx.accounts.contract;
    contract.provider_stake = stake_required;
    contract.status = ContractStatus::Active;
    // GAP-6 fix: reset created_at to acceptance time so the 72h delivery
    // window starts from when the contract becomes Active, not from proposal time.
    contract.created_at = now_ts;

    // Enforce exposure limit and increment active_contracts counter (Whitepaper Section 7.3)
    // NOTE: AcceptProposal struct must include provider_identity (added below)
    let max_contracts = (ctx.accounts.provider_identity.trust_score as u64 / 10) + 1;
    require!(
        (ctx.accounts.provider_identity.active_contracts as u64) < max_contracts,
        TrustError::ExposureLimitExceeded
    );
    ctx.accounts.provider_identity.active_contracts =
        ctx.accounts.provider_identity.active_contracts.saturating_add(1);

    // Re-validate provider hasn't been banned since proposal was created
    require!(!ctx.accounts.provider_identity.banned, TrustError::AgentBanned);

    msg!(
        "Contract #{} accepted by provider {}. Stake deposited: {} (currency: {:?}). Active contracts: {}",
        contract_id, contract_provider, stake_required,
        if is_sol { Currency::Sol } else { Currency::Sworn },
        ctx.accounts.provider_identity.active_contracts
    );
    Ok(())
}

/// Cancel an expired proposal. Requester reclaims escrowed funds.
pub fn handler_cancel_proposal(ctx: Context<CancelProposal>) -> Result<()> {
    // Extract values before transfers to avoid borrow conflicts
    let contract_status = ctx.accounts.contract.status;
    let contract_requester = ctx.accounts.contract.requester;
    let _contract_expires = ctx.accounts.contract.proposal_expires_at;
    // Refund the actual escrowed amount, not the full value (Whitepaper §7.7)
    let refund_value = (ctx.accounts.contract.value as u128)
        .checked_mul(ctx.accounts.contract.escrow_factor_bps as u128)
        .unwrap_or(ctx.accounts.contract.value as u128)
        / 10_000;
    let refund_value = refund_value as u64;
    let is_sol = ctx.accounts.contract.currency == Currency::Sol;
    let contract_id = ctx.accounts.contract.id;

    require!(
        contract_status == ContractStatus::Proposed,
        TrustError::InvalidContractStatus
    );
    require!(
        contract_requester == ctx.accounts.requester.key(),
        TrustError::UnauthorizedRequester
    );

    // Requester can cancel a proposal at any time before it is accepted.
    // The expiry is only a soft deadline for the provider to accept.
    // No penalty for early cancellation — escrow is fully returned.

    if is_sol {
        // SOL escrow: return lamports from contract PDA to requester
        let contract_info = ctx.accounts.contract.to_account_info();
        let requester_info = ctx.accounts.requester.to_account_info();
        **contract_info.try_borrow_mut_lamports()? -= refund_value;
        **requester_info.try_borrow_mut_lamports()? += refund_value;
    } else {
        // SWORN escrow: return SPL tokens via PDA signer
        let contract_id_bytes = contract_id.to_le_bytes();
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
            refund_value,
        )?;
    }

    let contract = &mut ctx.accounts.contract;
    contract.status = ContractStatus::Cancelled;
    contract.resolved_at = Clock::get()?.unix_timestamp;

    msg!("Contract #{} proposal cancelled. Escrow returned to requester.", contract_id);
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

    /// Requester identity — used to calculate escrow_factor (Whitepaper §7.7)
    #[account(
        seeds = [b"agent-identity" as &[u8], requester.key().as_ref()],
        bump = requester_identity.bump,
    )]
    pub requester_identity: Account<'info, AgentIdentity>,

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

    /// Provider's identity — validates ban status, enforces exposure limit, tracks active_contracts
    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], provider.key().as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Account<'info, AgentIdentity>,

    /// Provider's SWORN token account (only needed for SWORN-denominated contracts)
    #[account(
        mut,
        constraint = provider_token_account.owner == provider.key(),
        constraint = provider_token_account.mint == protocol_config.sworn_mint,
    )]
    pub provider_token_account: Account<'info, TokenAccount>,

    /// Escrow vault - already initialized by propose_contract (SWORN contracts only)
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
    pub system_program: Program<'info, System>,
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
