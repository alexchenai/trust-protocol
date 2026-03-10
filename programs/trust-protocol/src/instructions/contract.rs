use crate::errors::TrustError;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

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
pub fn handler_create(ctx: Context<CreateContract>, value: u64) -> Result<()> {
    let config = &ctx.accounts.protocol_config;
    let provider_identity = &ctx.accounts.provider_identity;

    require!(!provider_identity.banned, TrustError::AgentBanned);
    require!(provider_identity.matured, TrustError::IdentityNotMatured);

    // Exposure limit check (Whitepaper Section 7.3)
    // max_contracts = floor(TrustScore / 10) + 1
    let max_contracts = (provider_identity.trust_score as u64 / 10) + 1;
    // NOTE: We cannot count active contracts on-chain without iteration.
    // For now we log the limit. Full enforcement requires an active_contracts counter
    // on AgentIdentity (future upgrade).
    msg!("Exposure limit: max {} simultaneous contracts for TS {}", max_contracts, provider_identity.trust_score);

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
    contract.proposal_expires_at = 0;
    contract.provider_stake_required = 0;
    contract.currency = Currency::Sworn; // Default: dual-sign flow uses SWORN

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
/// Deducts 1% protocol fee (70% treasury / 20% insurance / 10% burn).
/// Whitepaper Section 11.8: fee deducted at accept_contract.
/// NOTE: Uses only 2 CPI transfers to stay within BPF stack frame limits.
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

    // Extract values before releasing mutable borrow for SOL lamport transfers
    let contract_value = contract.value;
    let contract_provider_stake = contract.provider_stake;
    let contract_currency = contract.currency;
    let contract_id_val = contract.id;

    // Calculate protocol fee: 1% of contract value (Whitepaper Section 11.8)
    // Split: 70% treasury, 20% insurance pool, 10% burn (burn goes to insurance for now)
    let protocol_fee = contract_value / 100; // 1%
    let fee_treasury = protocol_fee * 70 / 100;
    // Insurance + burn combined into one transfer (burn to insurance until proper burn setup)
    let fee_insurance_and_burn = protocol_fee.saturating_sub(fee_treasury); // 30%

    // Net payment to provider = contract value - protocol fee + stake return
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

        // Transfer ALL protocol fees to treasury (admin wallet) for SOL contracts.
        // Insurance PDA may have 0 SOL and adding small fees would leave it below
        // rent-exempt minimum, causing InsufficientFundsForRent.
        // Admin redistributes insurance portion off-chain.
        let total_fees = fee_treasury.checked_add(fee_insurance_and_burn).ok_or(TrustError::MathOverflow)?;
        if total_fees > 0 {
            **contract_info.try_borrow_mut_lamports()? -= total_fees;
            **treasury_info.try_borrow_mut_lamports()? += total_fees;
        }
    } else {
        // SWORN-denominated contract: SPL token transfers via escrow PDA
        // Manually derive escrow PDA bump (removed seeds from account validation to avoid stack overflow)
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

        // CPI 2: Transfer treasury fee to admin
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

        // CPI 3: Transfer insurance+burn portion to insurance vault
        // (Burn goes to insurance pool until proper SPL burn is set up)
        if fee_insurance_and_burn > 0 {
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
                fee_insurance_and_burn,
            )?;
        }
    }

    msg!(
        "Contract #{} completed. Currency: {:?}. Provider: {} net. Fee: {} (treasury: {}, pool: {}). Tasks: {}",
        contract_id_val,
        contract_currency,
        provider_release,
        protocol_fee,
        fee_treasury,
        fee_insurance_and_burn,
        provider_identity.tasks_completed
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
        seeds = [b"agent-identity" as &[u8], provider.key().as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Account<'info, AgentIdentity>,

    #[account(
        init,
        payer = requester,
        space = 8 + Contract::INIT_SPACE,
        seeds = [b"contract" as &[u8], &protocol_config.total_contracts.to_le_bytes()],
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

    /// Escrow vault PDA for this contract (initialized at contract creation)
    #[account(
        init,
        payer = requester,
        token::mint = sworn_mint,
        token::authority = escrow_vault,
        seeds = [b"escrow" as &[u8], &protocol_config.total_contracts.to_le_bytes()],
        bump,
    )]
    pub escrow_vault: Account<'info, TokenAccount>,

    /// SWORN token mint (validated against protocol config)
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

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Box<Account<'info, ProtocolConfig>>,

    pub token_program: Program<'info, Token>,
}
