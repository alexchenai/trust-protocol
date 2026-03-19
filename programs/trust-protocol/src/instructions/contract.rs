use crate::errors::TrustError;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Burn, Mint, Token, TokenAccount, Transfer};

/// Calculate stake factor based on TrustScore using CONVEX curve.
/// Whitepaper: factor_stake(ts) = max(0.05, 1.0 - 0.95 * (ts/100)^1.5)
/// Returns basis points (10000 = 100%, 500 = 5%).
/// Uses integer approximation of the convex curve with a lookup table
/// to avoid floating-point operations on-chain.
pub(crate) fn calculate_stake_factor(trust_score: u16, min_bps: u16, _max_bps: u16) -> u16 {
    if trust_score >= 100 {
        return min_bps; // 500 bps = 5%
    }
    if trust_score == 0 {
        return 10_000; // 100%
    }
    // Convex curve: f(ts) = max(min_bps, 10000 - 9500 * (ts/100)^1.5)
    // Integer approximation: (ts/100)^1.5 = (ts^3)^0.5 / 100^1.5 = sqrt(ts^3) / 1000
    // We compute: reduction = 9500 * sqrt(ts^3) / 1000
    let ts = trust_score as u64;
    let ts_cubed = ts * ts * ts; // ts^3, max 100^3 = 1_000_000
    let sqrt_ts_cubed = integer_sqrt(ts_cubed); // sqrt(ts^3)
    let reduction = (9_500u64 * sqrt_ts_cubed) / 1_000;
    let factor = 10_000u64.saturating_sub(reduction);
    let factor = factor.max(min_bps as u64);
    factor as u16
}

/// Calculate escrow factor for requester based on their TrustScore.
/// Whitepaper §7.7: escrow_factor(ts) = max(0.30, 1.0 - 0.70*(ts/100)^1.5)
/// Returns basis points (10000 = 100%, 3000 = 30% floor).
/// New requesters (TS=0) always deposit 100%. Experienced requesters pay less.
pub(crate) fn calculate_escrow_factor(trust_score: u16) -> u16 {
    if trust_score == 0 {
        return 10_000; // 100% — no history, full escrow required
    }
    if trust_score >= 100 {
        return 3_000; // 30% floor (minimum, even for perfect score)
    }
    // escrow_factor = 1.0 - 0.70 * (ts/100)^1.5
    // Integer approximation: reduction = 7000 * sqrt(ts^3) / 1000
    let ts = trust_score as u64;
    let ts_cubed = ts * ts * ts;
    let sqrt_ts_cubed = integer_sqrt(ts_cubed);
    let reduction = (7_000u64 * sqrt_ts_cubed) / 1_000;
    let factor = 10_000u64.saturating_sub(reduction);
    let factor = factor.max(3_000); // 30% floor
    factor as u16
}

/// Integer square root (Newton's method).
pub(crate) fn integer_sqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Create a new contract between requester and provider.
/// Provider must stake: contract_value * factor_stake(TrustScore).
/// Whitepaper Section 3: Dynamic Staking + Exposure limits (3x capital).
/// currency: 0=SWORN (SPL token, default), 1=SOL (native lamports). Whitepaper §11.8b.
pub fn handler_create(ctx: Context<CreateContract>, value: u64, currency: u8) -> Result<()> {
    let config = &ctx.accounts.protocol_config;
    let provider_identity = &ctx.accounts.provider_identity;

    require!(!provider_identity.banned, TrustError::AgentBanned);
    require!(provider_identity.matured, TrustError::IdentityNotMatured);
    require!(!provider_identity.is_hibernating, TrustError::AgentHibernating);

    // Exposure limit check (Whitepaper Section 7.3)
    // max_contracts = floor(TrustScore / 10) + 1
    let max_contracts = (provider_identity.trust_score as u64 / 10) + 1;
    require!(
        (provider_identity.active_contracts as u64) < max_contracts,
        TrustError::ExposureLimitExceeded
    );

    // Capture trust_score before mutable borrow
    let ts = provider_identity.trust_score;
    let min_bps = config.min_stake_factor_bps;
    let max_bps = config.max_stake_factor_bps;

    // Increment active contract counter
    ctx.accounts.provider_identity.active_contracts =
        ctx.accounts.provider_identity.active_contracts.saturating_add(1);

    // Calculate required provider stake (Whitepaper §7.2)
    let stake_factor = calculate_stake_factor(ts, min_bps, max_bps);
    let stake_required = (value as u128)
        .checked_mul(stake_factor as u128)
        .ok_or(TrustError::MathOverflow)?
        / 10_000;

    // Calculate requester escrow factor from requester TrustScore (Whitepaper §7.7)
    // New requesters (TS=0) deposit 100%; experienced requesters deposit less (floor 30%)
    let requester_ts = ctx.accounts.requester_identity.trust_score;
    let escrow_factor = calculate_escrow_factor(requester_ts);
    let escrow_deposit = (value as u128)
        .checked_mul(escrow_factor as u128)
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
    // Transfer partial escrow from requester (§7.7: reduced by TrustScore, floor 30%)
    let transfer_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        Transfer {
            from: ctx.accounts.requester_token_account.to_account_info(),
            to: ctx.accounts.escrow_vault.to_account_info(),
            authority: ctx.accounts.requester.to_account_info(),
        },
    );
    token::transfer(transfer_ctx, escrow_deposit as u64)?;

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
    contract.proposal_expires_at = 0;
    contract.provider_stake_required = 0;
    // Parse currency parameter — GAP-7 fix: allow SOL denomination
    contract.escrow_factor_bps = escrow_factor; // §7.7: stored for payout reference
    contract.currency = match currency {
        1 => Currency::Sol,
        _ => Currency::Sworn, // 0 or any unknown = SWORN (safe default)
    };

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

    // Track total deliveries on provider identity (Whitepaper: quality_factor denominator)
    ctx.accounts.provider_identity.total_deliveries =
        ctx.accounts.provider_identity.total_deliveries.saturating_add(1);

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

    msg!("Contract #{} delivered. PoE submitted. Total deliveries: {}", contract.id, ctx.accounts.provider_identity.total_deliveries);
    Ok(())
}

/// Requester accepts deliverable. Releases payment + returns provider stake.
/// Deducts 0.5% (SWORN contracts) or 1.0% (SOL contracts) protocol fee (70% treasury / 20% insurance / 10% burn).
/// Whitepaper Section 11.8: fee deducted at accept_contract.
pub fn handler_accept(ctx: Context<AcceptContract>) -> Result<()> {
    // --- Phase 1: Read contract fields immutably for top-up calculation ---
    {
        let contract = &ctx.accounts.contract;
        require!(
            contract.status == ContractStatus::Delivered,
            TrustError::InvalidContractStatus
        );
        require!(
            contract.requester == ctx.accounts.requester.key(),
            TrustError::UnauthorizedRequester
        );

        // Whitepaper §7.7 Top-up: requester deposits the difference before payment release
        let contract_value = contract.value;
        let contract_currency = contract.currency;
        let escrow_factor_bps = contract.escrow_factor_bps;
        let escrow_factor = if escrow_factor_bps > 0 { escrow_factor_bps } else { 10_000u16 };
        let escrow_deposit = (contract_value as u128 * escrow_factor as u128 / 10_000) as u64;
        let top_up_amount = contract_value.saturating_sub(escrow_deposit);

        if top_up_amount > 0 {
            if contract_currency == Currency::Sol {
                let requester_info = ctx.accounts.requester.to_account_info();
                let contract_info = ctx.accounts.contract.to_account_info();
                let ix = anchor_lang::solana_program::system_instruction::transfer(
                    requester_info.key,
                    contract_info.key,
                    top_up_amount,
                );
                anchor_lang::solana_program::program::invoke(
                    &ix,
                    &[requester_info, contract_info, ctx.accounts.system_program.to_account_info()],
                )?;
            } else {
                token::transfer(
                    CpiContext::new(
                        ctx.accounts.token_program.to_account_info(),
                        Transfer {
                            from: ctx.accounts.requester_token_account.to_account_info(),
                            to: ctx.accounts.escrow_vault.to_account_info(),
                            authority: ctx.accounts.requester.to_account_info(),
                        },
                    ),
                    top_up_amount,
                )?;
            }
        }
    } // immutable borrow of contract ends here

    // --- Phase 2: Mutate contract state ---
    let contract = &mut ctx.accounts.contract;
    let now = Clock::get()?.unix_timestamp;
    contract.status = ContractStatus::Completed;
    contract.resolved_at = now;

    // Mark PoE as validated
    let poe = &mut ctx.accounts.proof_of_execution;
    poe.validated = true;

    // Update provider stats
    let provider_identity = &mut ctx.accounts.provider_identity;
    provider_identity.tasks_completed = provider_identity.tasks_completed.saturating_add(1);
    provider_identity.last_task_completed_at = now;
    provider_identity.active_contracts = provider_identity.active_contracts.saturating_sub(1);
    // Hibernation cooldown counter: count tasks completed since last hibernation (§8.6)
    provider_identity.tasks_since_last_hibernation =
        provider_identity.tasks_since_last_hibernation.saturating_add(1);
    // Track volume separately by currency (Whitepaper: volume_factor uses SWORN-equivalent)
    if contract.currency == Currency::Sol {
        provider_identity.volume_sol = provider_identity.volume_sol.saturating_add(contract.value);
    } else {
        provider_identity.volume_processed =
            provider_identity.volume_processed.saturating_add(contract.value);
    }

    // Extract values before releasing mutable borrow for SOL lamport transfers
    let contract_value = contract.value;
    let contract_provider_stake = contract.provider_stake;
    let contract_currency = contract.currency;
    let contract_id_val = contract.id;
    let tasks_completed = provider_identity.tasks_completed;

    // Calculate protocol fee: 0.5% for SWORN contracts, 1.0% for SOL (Whitepaper §11.8)
    // SWORN fee < SOL fee incentivises organic migration to SWORN. Split: 70/20/10.
    let fee_bps = if contract_currency == Currency::Sworn {
        ctx.accounts.protocol_config.protocol_fee_sworn_bps as u64 // 50 bps = 0.5%
    } else {
        ctx.accounts.protocol_config.protocol_fee_sol_bps as u64   // 100 bps = 1.0%
    };
    let protocol_fee = contract_value * fee_bps / 10_000;
    let fee_treasury = protocol_fee * 70 / 100;
    let fee_insurance = protocol_fee * 20 / 100;
    let fee_burn = protocol_fee.saturating_sub(fee_treasury).saturating_sub(fee_insurance); // 10%

    // Net payment to provider = full contract_value - protocol fee + stake return
    // Provider always receives full value regardless of escrow_factor (top-up done in Phase 1)
    let net_payment = contract_value.saturating_sub(protocol_fee);
    let provider_release = net_payment
        .checked_add(contract_provider_stake)
        .ok_or(TrustError::MathOverflow)?;

    if contract_currency == Currency::Sol {
        // SOL-denominated contract: transfer lamports from contract PDA
        // Validate destinations: provider wallet, admin wallet
        require!(
            ctx.accounts.provider_token_account.key() == contract.provider,
            TrustError::InvalidDestination
        );
        require!(
            ctx.accounts.treasury_token_account.key() == ctx.accounts.protocol_config.admin,
            TrustError::InvalidDestination
        );
        let contract_info = ctx.accounts.contract.to_account_info();
        let provider_info = ctx.accounts.provider_token_account.to_account_info();
        let treasury_info = ctx.accounts.treasury_token_account.to_account_info();

        // Transfer net payment + stake to provider
        **contract_info.try_borrow_mut_lamports()? -= provider_release;
        **provider_info.try_borrow_mut_lamports()? += provider_release;

        // Transfer ALL protocol fees to treasury for SOL contracts.
        // Insurance PDA may have 0 SOL — adding small amounts causes InsufficientFundsForRent.
        // Admin redistributes insurance portion off-chain.
        let total_fees = fee_treasury.saturating_add(fee_insurance).saturating_add(fee_burn);
        if total_fees > 0 {
            **contract_info.try_borrow_mut_lamports()? -= total_fees;
            **treasury_info.try_borrow_mut_lamports()? += total_fees;
        }
    } else {
        // SWORN-denominated contract: SPL token transfers via escrow PDA
        // Manually derive escrow PDA bump to avoid BPF stack overflow
        let contract_id_bytes = contract_id_val.to_le_bytes();
        let (expected_escrow, escrow_bump) = Pubkey::find_program_address(
            &[b"escrow", &contract_id_bytes],
            ctx.program_id,
        );
        require!(
            ctx.accounts.escrow_vault.key() == expected_escrow,
            TrustError::InvalidEscrowVault
        );
        let escrow_seeds: &[&[u8]] = &[b"escrow", &contract_id_bytes, &[escrow_bump]];
        let signer_seeds = &[escrow_seeds];

        // CPI 1: Transfer net payment + stake to provider
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.escrow_vault.to_account_info(),
                    to: ctx.accounts.provider_token_account.to_account_info(),
                    authority: ctx.accounts.escrow_vault.to_account_info(),
                },
                signer_seeds,
            ),
            provider_release,
        )?;

        // CPI 2: Transfer 70% fee to treasury
        if fee_treasury > 0 {
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.escrow_vault.to_account_info(),
                        to: ctx.accounts.treasury_token_account.to_account_info(),
                        authority: ctx.accounts.escrow_vault.to_account_info(),
                    },
                    signer_seeds,
                ),
                fee_treasury,
            )?;
        }

        // CPI 3: Transfer 20% fee to insurance vault
        if fee_insurance > 0 {
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.escrow_vault.to_account_info(),
                        to: ctx.accounts.insurance_vault.to_account_info(),
                        authority: ctx.accounts.escrow_vault.to_account_info(),
                    },
                    signer_seeds,
                ),
                fee_insurance,
            )?;
        }

        // CPI 4: Burn 10% of fee (deflationary — Whitepaper Section 11.8)
        if fee_burn > 0 {
            require!(
                ctx.accounts.sworn_mint.key() == ctx.accounts.protocol_config.sworn_mint,
                TrustError::InvalidDestination
            );
            token::burn(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Burn {
                        mint: ctx.accounts.sworn_mint.to_account_info(),
                        from: ctx.accounts.escrow_vault.to_account_info(),
                        authority: ctx.accounts.escrow_vault.to_account_info(),
                    },
                    signer_seeds,
                ),
                fee_burn,
            )?;
        }
    }

    msg!(
        "Contract #{} completed. Currency: {:?}. Provider: {} net. Fee: {} (treasury: {}, insurance: {}, burn: {}). Tasks: {}",
        contract_id_val,
        contract_currency,
        provider_release,
        protocol_fee,
        fee_treasury,
        fee_insurance,
        fee_burn,
        tasks_completed
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
        mut,
        seeds = [b"agent-identity" as &[u8], provider.key().as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Box<Account<'info, AgentIdentity>>,

    /// Requester identity — used to calculate escrow_factor (Whitepaper §7.7)
    #[account(
        seeds = [b"agent-identity" as &[u8], requester.key().as_ref()],
        bump = requester_identity.bump,
    )]
    pub requester_identity: Box<Account<'info, AgentIdentity>>,

    #[account(
        init,
        payer = requester,
        space = 8 + Contract::INIT_SPACE,
        seeds = [b"contract" as &[u8], &protocol_config.total_contracts.to_le_bytes()],
        bump
    )]
    pub contract: Box<Account<'info, Contract>>,

    #[account(
        mut,
        constraint = requester_token_account.owner == requester.key(),
        constraint = requester_token_account.mint == protocol_config.sworn_mint,
    )]
    pub requester_token_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = provider_token_account.owner == provider.key(),
        constraint = provider_token_account.mint == protocol_config.sworn_mint,
    )]
    pub provider_token_account: Box<Account<'info, TokenAccount>>,

    /// Escrow vault PDA for this contract (initialized at contract creation)
    #[account(
        init,
        payer = requester,
        token::mint = sworn_mint,
        token::authority = escrow_vault,
        seeds = [b"escrow" as &[u8], &protocol_config.total_contracts.to_le_bytes()],
        bump,
    )]
    pub escrow_vault: Box<Account<'info, TokenAccount>>,

    /// SWORN token mint (validated against protocol config)
    #[account(
        constraint = sworn_mint.key() == protocol_config.sworn_mint,
    )]
    pub sworn_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Box<Account<'info, ProtocolConfig>>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

/// Anyone can call this to trigger a timed-out contract.
/// Whitepaper Section 3 / Section 5: If provider fails to deliver within 72h,
/// requester recovers escrow and provider's stake is confiscated (60% insurance,
/// 25% winner, 15% burn).
/// Permissionless: any caller can trigger after the deadline.
pub fn handler_timeout(
    ctx: Context<TimeoutContract>,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let contract = &ctx.accounts.contract;

    require!(
        contract.status == ContractStatus::Active,
        TrustError::InvalidContractStatus
    );

    // Delivery deadline from config (default 72h — Whitepaper §7.5 / §12.4.1)
    let timeout_seconds = ctx.accounts.protocol_config.deadline_validation;
    require!(
        now > contract.created_at + timeout_seconds,
        TrustError::TimeoutNotReached
    );

    let contract_id_val = contract.id;
    let contract_value = contract.value;
    let provider_stake = contract.provider_stake;
    let contract_currency = contract.currency;
    let contract_escrow_factor_bps_to = contract.escrow_factor_bps;
    let burn_rate_bps = ctx.accounts.protocol_config.burn_rate_bps;
    let insurance_rate_bps = ctx.accounts.protocol_config.insurance_rate_bps;

    // Compute escrowed amount (Whitepaper §7.7)
    let escrow_factor_to = if contract_escrow_factor_bps_to > 0 { contract_escrow_factor_bps_to } else { 10_000u16 };
    let escrow_deposit_to = (contract_value as u128 * escrow_factor_to as u128 / 10_000) as u64;

    // Stake confiscation split: 15% burn, 60% insurance, 25% requester bonus
    let burn_amount = (provider_stake as u128 * burn_rate_bps as u128 / 10_000) as u64;
    let insurance_amount = (provider_stake as u128 * insurance_rate_bps as u128 / 10_000) as u64;
    let winner_amount = provider_stake
        .saturating_sub(burn_amount)
        .saturating_sub(insurance_amount);
    // Refund = escrowed amount (not full contract value) + 25% confiscation bonus
    let refund = escrow_deposit_to
        .checked_add(winner_amount)
        .ok_or(TrustError::MathOverflow)?;

    // Update contract state
    {
        let contract = &mut ctx.accounts.contract;
        contract.status = ContractStatus::ResolvedRequester;
        contract.resolved_at = now;
    }

    // Update provider identity: timed out = abandoned task
    {
        let provider = &mut ctx.accounts.provider_identity;
        provider.active_contracts = provider.active_contracts.saturating_sub(1);
        provider.tasks_abandoned = provider.tasks_abandoned.saturating_add(1);
    }

    if contract_currency == Currency::Sol {
        // SOL-denominated: lamport transfers from contract PDA
        require!(
            ctx.accounts.requester_token_account.key() == ctx.accounts.contract.requester,
            TrustError::InvalidDestination
        );

        let contract_info = ctx.accounts.contract.to_account_info();
        let requester_info = ctx.accounts.requester_token_account.to_account_info();

        // Return contract value + 25% winner bonus to requester
        **contract_info.try_borrow_mut_lamports()? -= refund;
        **requester_info.try_borrow_mut_lamports()? += refund;

        // For SOL: insurance + burn portions go to requester (admin redistributes off-chain)
        let remainder = insurance_amount
            .checked_add(burn_amount)
            .ok_or(TrustError::MathOverflow)?;
        if remainder > 0 {
            **contract_info.try_borrow_mut_lamports()? -= remainder;
            **requester_info.try_borrow_mut_lamports()? += remainder;
            ctx.accounts.insurance_pool.total_balance =
                ctx.accounts.insurance_pool.total_balance.saturating_add(insurance_amount);
        }
    } else {
        // SWORN-denominated: SPL token transfers from escrow PDA
        let contract_id_bytes = contract_id_val.to_le_bytes();
        let (expected_escrow, escrow_bump) = Pubkey::find_program_address(
            &[b"escrow", &contract_id_bytes],
            ctx.program_id,
        );
        require!(
            ctx.accounts.escrow_vault.key() == expected_escrow,
            TrustError::InvalidEscrowVault
        );
        let escrow_seeds: &[&[u8]] = &[b"escrow", &contract_id_bytes, &[escrow_bump]];
        let signer_seeds = &[escrow_seeds];

        // CPI 1: Return value + 25% stake bonus to requester
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
            refund,
        )?;

        // CPI 2: 60% stake to insurance vault
        if insurance_amount > 0 {
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.escrow_vault.to_account_info(),
                        to: ctx.accounts.insurance_vault.to_account_info(),
                        authority: ctx.accounts.escrow_vault.to_account_info(),
                    },
                    signer_seeds,
                ),
                insurance_amount,
            )?;
            ctx.accounts.insurance_pool.total_balance =
                ctx.accounts.insurance_pool.total_balance.saturating_add(insurance_amount);
        }

        // CPI 3: Burn 15% stake
        if burn_amount > 0 {
            require!(
                ctx.accounts.sworn_mint.key() == ctx.accounts.protocol_config.sworn_mint,
                TrustError::InvalidDestination
            );
            token::burn(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Burn {
                        mint: ctx.accounts.sworn_mint.to_account_info(),
                        from: ctx.accounts.escrow_vault.to_account_info(),
                        authority: ctx.accounts.escrow_vault.to_account_info(),
                    },
                    signer_seeds,
                ),
                burn_amount,
            )?;
        }
    }

    msg!(
        "Contract #{} timed out (72h). Refund: {}, Insurance: {}, Burn: {}. Currency: {:?}",
        contract_id_val,
        refund,
        insurance_amount,
        burn_amount,
        contract_currency,
    );
    Ok(())
}

#[derive(Accounts)]
pub struct TimeoutContract<'info> {
    /// Permissionless: anyone can trigger the timeout after 72h
    #[account(mut)]
    pub caller: Signer<'info>,

    #[account(
        mut,
        constraint = contract.status == ContractStatus::Active @ TrustError::InvalidContractStatus,
    )]
    pub contract: Box<Account<'info, Contract>>,

    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], contract.provider.as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Box<Account<'info, AgentIdentity>>,

    /// For SWORN: requester's ATA. For SOL: requester's wallet.
    /// CHECK: For SOL, validated key == contract.requester. For SWORN, token CPI validates.
    #[account(mut)]
    pub requester_token_account: UncheckedAccount<'info>,

    /// Escrow vault PDA for SWORN. Unused for SOL (lamports live in contract PDA).
    /// CHECK: Validated manually via find_program_address in handler.
    #[account(mut)]
    pub escrow_vault: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"insurance-pool"],
        bump = insurance_pool.bump,
    )]
    pub insurance_pool: Box<Account<'info, InsurancePool>>,

    /// For SWORN: insurance vault ATA. For SOL: not used (lamports go to requester).
    /// CHECK: Validated in handler based on currency.
    #[account(mut)]
    pub insurance_vault: UncheckedAccount<'info>,

    /// SWORN mint for burn CPI. Unused for SOL.
    /// CHECK: Validated key == protocol_config.sworn_mint in handler.
    #[account(mut)]
    pub sworn_mint: UncheckedAccount<'info>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Box<Account<'info, ProtocolConfig>>,

    pub token_program: Program<'info, Token>,
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
        mut,
        seeds = [b"agent-identity" as &[u8], provider.key().as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Account<'info, AgentIdentity>,

    #[account(
        init,
        payer = provider,
        space = 8 + ProofOfExecution::INIT_SPACE,
        seeds = [b"poe" as &[u8], contract.key().as_ref()],
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
    pub contract: Box<Account<'info, Contract>>,

    #[account(
        mut,
        seeds = [b"poe" as &[u8], contract.key().as_ref()],
        bump = proof_of_execution.bump,
    )]
    pub proof_of_execution: Box<Account<'info, ProofOfExecution>>,

    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], contract.provider.as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Box<Account<'info, AgentIdentity>>,

    /// For SWORN: provider's ATA. For SOL: provider's wallet (system account).
    /// CHECK: For SOL, validated key == contract.provider. For SWORN, token CPI validates.
    #[account(mut)]
    pub provider_token_account: UncheckedAccount<'info>,

    /// For SWORN: admin's ATA. For SOL: admin's wallet (system account).
    /// CHECK: For SOL, validated key == config.admin. For SWORN, token CPI validates.
    #[account(mut)]
    pub treasury_token_account: UncheckedAccount<'info>,

    /// For SWORN: insurance vault ATA. For SOL: pool authority PDA.
    /// CHECK: Validated in handler based on currency.
    #[account(mut)]
    pub insurance_vault: UncheckedAccount<'info>,

    /// Escrow vault PDA for SWORN contracts. Unused for SOL.
    /// CHECK: Validated manually in handler via find_program_address for SWORN path.
    #[account(mut)]
    pub escrow_vault: UncheckedAccount<'info>,

    /// SWORN mint for the burn CPI (10% of fee). Unused for SOL contracts.
    /// CHECK: Validated key == protocol_config.sworn_mint in handler.
    #[account(mut)]
    pub sworn_mint: UncheckedAccount<'info>,

    /// Requester's token account for SWORN top-up (§7.7).
    /// CHECK: For SWORN, validated by token CPI (owner == requester). Unused for SOL.
    #[account(mut)]
    pub requester_token_account: UncheckedAccount<'info>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Box<Account<'info, ProtocolConfig>>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

// ---------------------------------------------------------------------------
// GAP-11: Requester-validation timeout
// Whitepaper Section 3.5: if requester ignores a Delivered contract for 72h,
// any caller can trigger auto-accept (protects provider from ghosting).
// Uses poe.submitted_at as the delivery timestamp.
// ---------------------------------------------------------------------------

/// Permissionless: auto-accept a delivered contract if requester ignores it for 72h.
/// Whitepaper Section 3.5: provider protected from requester ghosting.
pub fn handler_timeout_delivery(ctx: Context<TimeoutDelivery>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let contract = &ctx.accounts.contract;

    require!(
        contract.status == ContractStatus::Delivered,
        TrustError::InvalidContractStatus
    );

    // Requester validation timeout from config (default 72h — Whitepaper §7.5 / §12.4.1)
    let validation_timeout: i64 = ctx.accounts.protocol_config.deadline_validation;
    require!(
        now > ctx.accounts.proof_of_execution.submitted_at + validation_timeout,
        TrustError::TimeoutNotReached
    );

    let contract_id_val = contract.id;
    let contract_value = contract.value;
    let contract_provider_stake = contract.provider_stake;
    let contract_currency = contract.currency;
    let contract_escrow_factor_bps_td = contract.escrow_factor_bps;

    // Protocol fee (Whitepaper §11.8: 0.5% SWORN, 1.0% SOL)
    let fee_bps_td = if contract_currency == Currency::Sworn {
        ctx.accounts.protocol_config.protocol_fee_sworn_bps as u64
    } else {
        ctx.accounts.protocol_config.protocol_fee_sol_bps as u64
    };
    // Compute escrowed amount (Whitepaper §7.7: reduced escrow for experienced requesters)
    let escrow_factor_td = if contract_escrow_factor_bps_td > 0 { contract_escrow_factor_bps_td } else { 10_000u16 };
    let escrow_deposit_td = (contract_value as u128 * escrow_factor_td as u128 / 10_000) as u64;
    let protocol_fee = escrow_deposit_td * fee_bps_td / 10_000;
    let fee_treasury = protocol_fee * 70 / 100;
    let fee_insurance = protocol_fee * 20 / 100;
    let fee_burn = protocol_fee.saturating_sub(fee_treasury).saturating_sub(fee_insurance);
    let net_payment = escrow_deposit_td.saturating_sub(protocol_fee);
    let provider_release = net_payment
        .checked_add(contract_provider_stake)
        .ok_or(TrustError::MathOverflow)?;

    // Mark PoE validated
    ctx.accounts.proof_of_execution.validated = true;

    // Update contract state
    {
        let contract = &mut ctx.accounts.contract;
        contract.status = ContractStatus::Completed;
        contract.resolved_at = now;
    }

    // Update provider identity stats
    {
        let provider = &mut ctx.accounts.provider_identity;
        provider.tasks_completed = provider.tasks_completed.saturating_add(1);
        provider.last_task_completed_at = now;
        provider.active_contracts = provider.active_contracts.saturating_sub(1);
        // Hibernation cooldown counter (§8.6)
        provider.tasks_since_last_hibernation =
            provider.tasks_since_last_hibernation.saturating_add(1);
        if contract_currency == Currency::Sol {
            provider.volume_sol = provider.volume_sol.saturating_add(contract_value);
        } else {
            provider.volume_processed = provider.volume_processed.saturating_add(contract_value);
        }
    }

    if contract_currency == Currency::Sol {
        require!(
            ctx.accounts.provider_token_account.key() == ctx.accounts.contract.provider,
            TrustError::InvalidDestination
        );
        require!(
            ctx.accounts.treasury_token_account.key() == ctx.accounts.protocol_config.admin,
            TrustError::InvalidDestination
        );
        let contract_info = ctx.accounts.contract.to_account_info();
        let provider_info = ctx.accounts.provider_token_account.to_account_info();
        let treasury_info = ctx.accounts.treasury_token_account.to_account_info();

        **contract_info.try_borrow_mut_lamports()? -= provider_release;
        **provider_info.try_borrow_mut_lamports()? += provider_release;

        let total_fees = fee_treasury.saturating_add(fee_insurance).saturating_add(fee_burn);
        if total_fees > 0 {
            **contract_info.try_borrow_mut_lamports()? -= total_fees;
            **treasury_info.try_borrow_mut_lamports()? += total_fees;
        }
    } else {
        let contract_id_bytes = contract_id_val.to_le_bytes();
        let (expected_escrow, escrow_bump) = Pubkey::find_program_address(
            &[b"escrow", &contract_id_bytes],
            ctx.program_id,
        );
        require!(
            ctx.accounts.escrow_vault.key() == expected_escrow,
            TrustError::InvalidEscrowVault
        );
        let escrow_seeds: &[&[u8]] = &[b"escrow", &contract_id_bytes, &[escrow_bump]];
        let signer_seeds = &[escrow_seeds];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.escrow_vault.to_account_info(),
                    to: ctx.accounts.provider_token_account.to_account_info(),
                    authority: ctx.accounts.escrow_vault.to_account_info(),
                },
                signer_seeds,
            ),
            provider_release,
        )?;
        if fee_treasury > 0 {
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.escrow_vault.to_account_info(),
                        to: ctx.accounts.treasury_token_account.to_account_info(),
                        authority: ctx.accounts.escrow_vault.to_account_info(),
                    },
                    signer_seeds,
                ),
                fee_treasury,
            )?;
        }
        if fee_insurance > 0 {
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.escrow_vault.to_account_info(),
                        to: ctx.accounts.insurance_vault.to_account_info(),
                        authority: ctx.accounts.escrow_vault.to_account_info(),
                    },
                    signer_seeds,
                ),
                fee_insurance,
            )?;
        }
        if fee_burn > 0 {
            require!(
                ctx.accounts.sworn_mint.key() == ctx.accounts.protocol_config.sworn_mint,
                TrustError::InvalidDestination
            );
            token::burn(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Burn {
                        mint: ctx.accounts.sworn_mint.to_account_info(),
                        from: ctx.accounts.escrow_vault.to_account_info(),
                        authority: ctx.accounts.escrow_vault.to_account_info(),
                    },
                    signer_seeds,
                ),
                fee_burn,
            )?;
        }
    }

    msg!(
        "Contract #{} auto-completed via delivery timeout (72h). Provider: {} net. Currency: {:?}",
        contract_id_val, provider_release, contract_currency,
    );
    Ok(())
}

#[derive(Accounts)]
pub struct TimeoutDelivery<'info> {
    /// Permissionless: any caller can trigger after 72h
    #[account(mut)]
    pub caller: Signer<'info>,

    #[account(
        mut,
        constraint = contract.status == ContractStatus::Delivered @ TrustError::InvalidContractStatus,
    )]
    pub contract: Box<Account<'info, Contract>>,

    #[account(
        mut,
        seeds = [b"poe" as &[u8], contract.key().as_ref()],
        bump = proof_of_execution.bump,
    )]
    pub proof_of_execution: Box<Account<'info, ProofOfExecution>>,

    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], contract.provider.as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Box<Account<'info, AgentIdentity>>,

    /// For SWORN: provider's ATA. For SOL: provider's wallet.
    /// CHECK: For SOL, validated key == contract.provider. For SWORN, token CPI validates.
    #[account(mut)]
    pub provider_token_account: UncheckedAccount<'info>,

    /// For SWORN: admin's ATA. For SOL: admin's wallet.
    /// CHECK: For SOL, validated key == config.admin. For SWORN, token CPI validates.
    #[account(mut)]
    pub treasury_token_account: UncheckedAccount<'info>,

    /// For SWORN: insurance vault ATA. For SOL: unused (pass dummy).
    /// CHECK: Validated in handler.
    #[account(mut)]
    pub insurance_vault: UncheckedAccount<'info>,

    /// Escrow vault PDA for SWORN contracts.
    /// CHECK: Validated via find_program_address.
    #[account(mut)]
    pub escrow_vault: UncheckedAccount<'info>,

    /// SWORN mint for burn CPI. Unused for SOL.
    /// CHECK: Validated key == protocol_config.sworn_mint in handler.
    #[account(mut)]
    pub sworn_mint: UncheckedAccount<'info>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Box<Account<'info, ProtocolConfig>>,

    pub token_program: Program<'info, Token>,
}
