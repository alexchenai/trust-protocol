use crate::errors::TrustError;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Burn, Token, Transfer};
use anchor_lang::solana_program::pubkey::Pubkey;

/// Initiate a dispute on a delivered contract.
/// Whitepaper Section 5: Dispute Resolution - starts at Level 1 (Direct Correction).
pub fn handler_initiate(ctx: Context<InitiateDispute>, evidence_hash: [u8; 32]) -> Result<()> {
    let contract = &mut ctx.accounts.contract;
    require!(
        contract.status == ContractStatus::Delivered || contract.status == ContractStatus::Active,
        TrustError::InvalidContractStatus
    );
    require!(
        contract.requester == ctx.accounts.requester.key(),
        TrustError::UnauthorizedRequester
    );

    contract.status = ContractStatus::Disputed;
    contract.dispute_level = 1;

    let dispute = &mut ctx.accounts.dispute;
    dispute.contract = contract.key();
    dispute.initiator = ctx.accounts.requester.key();
    dispute.level = DisputeLevel::DirectCorrection;
    dispute.status = DisputeStatus::Open;
    dispute.evidence_hash = evidence_hash;
    dispute.response_hash = [0u8; 32];
    dispute.votes_provider = 0;
    dispute.votes_requester = 0;
    dispute.jury_size = 0;
    dispute.created_at = Clock::get()?.unix_timestamp;
    // Direct correction: 7-day deadline
    dispute.deadline = dispute.created_at + 7 * 86_400;
    dispute.resolved_at = 0;
    dispute.bump = ctx.bumps.dispute;
    dispute.corrections_count = 0;

    msg!(
        "Dispute initiated on contract #{}. Level: DirectCorrection. Deadline: {}",
        contract.id,
        dispute.deadline
    );
    Ok(())
}

/// Provider responds to dispute with correction (Level 1) or counter-evidence.
pub fn handler_respond(ctx: Context<RespondDispute>, response_hash: [u8; 32]) -> Result<()> {
    let dispute = &mut ctx.accounts.dispute;
    require!(
        dispute.status == DisputeStatus::Open,
        TrustError::InvalidContractStatus
    );

    let contract = &ctx.accounts.contract;
    require!(
        contract.provider == ctx.accounts.provider.key(),
        TrustError::UnauthorizedProvider
    );

    dispute.response_hash = response_hash;
    dispute.status = DisputeStatus::Responded;

    msg!(
        "Provider responded to dispute on contract #{}.",
        contract.id
    );
    Ok(())
}

/// Escalate dispute to the next level.
/// Level 1 -> 2 (Private Rounds), 2 -> 3 (Public Jury), 3 -> 4 (Appeal).
/// Whitepaper: Appeal is double-or-nothing with larger jury.
pub fn handler_escalate(ctx: Context<EscalateDispute>) -> Result<()> {
    let dispute = &mut ctx.accounts.dispute;
    let now = Clock::get()?.unix_timestamp;

    // Can only escalate after deadline or if responded
    require!(
        now >= dispute.deadline || dispute.status == DisputeStatus::Responded,
        TrustError::DisputeDeadlineNotReached
    );

    let new_level = match dispute.level {
        DisputeLevel::DirectCorrection => DisputeLevel::PrivateRounds,
        DisputeLevel::PrivateRounds => DisputeLevel::PublicJury,
        DisputeLevel::PublicJury => DisputeLevel::Appeal,
        DisputeLevel::Appeal => return Err(TrustError::MaxDisputeLevel.into()),
    };

    let deadline_days = match new_level {
        DisputeLevel::PrivateRounds => 5, // 5 days for private negotiation
        DisputeLevel::PublicJury => 7,    // 7 days for jury voting
        DisputeLevel::Appeal => 10,       // 10 days for appeal jury
        _ => 7,
    };

    dispute.level = new_level;
    dispute.status = DisputeStatus::Open;
    dispute.deadline = now + deadline_days * 86_400;

    // Set jury size based on contract value (Whitepaper Section 5.3)
    // PublicJury: 3 (<100 SWORN units), 5 (100-1000), 7 (>1000)
    // Appeal: always 9 (larger independent jury)
    match dispute.level {
        DisputeLevel::PublicJury => {
            let contract_value = ctx.accounts.contract.value;
            // 1 SWORN unit = 1_000_000 lamports (6 decimals)
            dispute.jury_size = if contract_value < 100_000_000 { 3 }
                else if contract_value < 1_000_000_000 { 5 }
                else { 7 };
        }
        DisputeLevel::Appeal => dispute.jury_size = 9, // independent larger jury
        _ => {}
    }

    let contract = &mut ctx.accounts.contract;
    contract.dispute_level = match dispute.level {
        DisputeLevel::DirectCorrection => 1,
        DisputeLevel::PrivateRounds => 2,
        DisputeLevel::PublicJury => 3,
        DisputeLevel::Appeal => 4,
    };

    msg!(
        "Dispute escalated to level {}. New deadline: {}",
        contract.dispute_level,
        dispute.deadline
    );
    Ok(())
}

/// Jury member casts vote (Public Jury / Appeal only).
/// Whitepaper: Only agents with TrustScore > 70 can serve as jurors.
/// Voting is weighted by reputation (validated via TrustScore check).
pub fn handler_vote(ctx: Context<JuryVote>, vote_for_provider: bool) -> Result<()> {
    let dispute = &mut ctx.accounts.dispute;
    let juror = &ctx.accounts.juror_identity;

    require!(
        dispute.level == DisputeLevel::PublicJury || dispute.level == DisputeLevel::Appeal,
        TrustError::InvalidContractStatus
    );
    require!(
        dispute.status == DisputeStatus::Open || dispute.status == DisputeStatus::Voting,
        TrustError::InvalidContractStatus
    );
    require!(
        juror.trust_score > 70,
        TrustError::InsufficientJuryReputation
    );
    require!(!juror.banned, TrustError::AgentBanned);
    require!(juror.matured, TrustError::IdentityNotMatured);

    let now = Clock::get()?.unix_timestamp;
    require!(now <= dispute.deadline, TrustError::DisputeDeadlineExpired);

    if vote_for_provider {
        dispute.votes_provider = dispute.votes_provider.saturating_add(1);
    } else {
        dispute.votes_requester = dispute.votes_requester.saturating_add(1);
    }
    dispute.status = DisputeStatus::Voting;

    msg!(
        "Juror {} voted for {}. Tally: provider={}, requester={}",
        ctx.accounts.juror.key(),
        if vote_for_provider {
            "provider"
        } else {
            "requester"
        },
        dispute.votes_provider,
        dispute.votes_requester
    );
    Ok(())
}

/// Resolve a dispute. Distributes stakes according to outcome.
/// Whitepaper: Confiscated stakes -> 15% burned, 60% insurance pool, 25% to winner.
/// Fraud: complete capital confiscation + permanent TrustScore reset + ban.
pub fn handler_resolve(ctx: Context<ResolveDispute>, provider_wins: bool) -> Result<()> {
    // Determine final outcome (jury overrides manual input for jury levels)
    let final_provider_wins = {
        let dispute = &ctx.accounts.dispute;
        if dispute.level == DisputeLevel::PublicJury || dispute.level == DisputeLevel::Appeal {
            let total_votes = dispute
                .votes_provider
                .saturating_add(dispute.votes_requester);
            require!(total_votes > 0, TrustError::InvalidContractStatus);
            dispute.votes_provider > dispute.votes_requester
        } else {
            provider_wins
        }
    };

    let now = Clock::get()?.unix_timestamp;
    ctx.accounts.dispute.resolved_at = now;
    ctx.accounts.contract.resolved_at = now;

    // Manually derive escrow PDA bump (seeds removed from struct to avoid BPF stack overflow)
    let contract_id_bytes = ctx.accounts.contract.id.to_le_bytes();
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

    // Dispute resolution always terminates the contract — decrement provider's active counter
    ctx.accounts.provider_identity.active_contracts =
        ctx.accounts.provider_identity.active_contracts.saturating_sub(1);

    let is_sol = ctx.accounts.contract.currency == Currency::Sol;

    if is_sol {
        // SOL-denominated dispute resolution: lamport manipulation from contract PDA
        // Validate destinations
        require!(
            ctx.accounts.provider_token_account.key() == ctx.accounts.contract.provider,
            TrustError::InvalidDestination
        );
        require!(
            ctx.accounts.requester_token_account.key() == ctx.accounts.contract.requester,
            TrustError::InvalidDestination
        );

        if final_provider_wins {
            ctx.accounts.dispute.status = DisputeStatus::ResolvedProvider;
            ctx.accounts.contract.status = ContractStatus::ResolvedProvider;

            let total = ctx.accounts.contract.value
                .checked_add(ctx.accounts.contract.provider_stake)
                .ok_or(TrustError::MathOverflow)?;

            let contract_info = ctx.accounts.contract.to_account_info();
            let provider_info = ctx.accounts.provider_token_account.to_account_info();
            **contract_info.try_borrow_mut_lamports()? -= total;
            **provider_info.try_borrow_mut_lamports()? += total;

            ctx.accounts.requester_identity.disputes_lost = ctx.accounts.requester_identity.disputes_lost.saturating_add(1);
            ctx.accounts.provider_identity.disputes_won = ctx.accounts.provider_identity.disputes_won.saturating_add(1);
            ctx.accounts.provider_identity.last_task_completed_at = now;
            msg!("Dispute resolved (SOL): PROVIDER wins. {} lamports released.", total);
        } else {
            ctx.accounts.dispute.status = DisputeStatus::ResolvedRequester;
            ctx.accounts.contract.status = ContractStatus::ResolvedRequester;

            let confiscated = ctx.accounts.contract.provider_stake;
            let contract_value = ctx.accounts.contract.value;
            let burn_rate_bps = ctx.accounts.protocol_config.burn_rate_bps;
            let insurance_rate_bps = ctx.accounts.protocol_config.insurance_rate_bps;
            let burn_amount = (confiscated as u128 * burn_rate_bps as u128 / 10_000) as u64;
            let insurance_amount = (confiscated as u128 * insurance_rate_bps as u128 / 10_000) as u64;
            let winner_amount = confiscated.saturating_sub(burn_amount).saturating_sub(insurance_amount);

            // For SOL: burn portion goes to insurance (can't burn SOL)
            let total_insurance = insurance_amount.checked_add(burn_amount).ok_or(TrustError::MathOverflow)?;
            let refund = contract_value.checked_add(winner_amount).ok_or(TrustError::MathOverflow)?;

            let contract_info = ctx.accounts.contract.to_account_info();
            let requester_info = ctx.accounts.requester_token_account.to_account_info();

            // Refund + confiscation winner portion to requester
            let total_to_requester = refund.checked_add(total_insurance).ok_or(TrustError::MathOverflow)?;
            **contract_info.try_borrow_mut_lamports()? -= total_to_requester;
            **requester_info.try_borrow_mut_lamports()? += total_to_requester;

            // For SOL disputes: insurance portion goes to requester (admin redistributes).
            // Insurance PDA may have 0 SOL — sending small amounts causes InsufficientFundsForRent.
            if total_insurance > 0 {
                ctx.accounts.insurance_pool.total_balance = ctx.accounts.insurance_pool.total_balance.saturating_add(total_insurance);
            }

            ctx.accounts.provider_identity.disputes_lost = ctx.accounts.provider_identity.disputes_lost.saturating_add(1);
            ctx.accounts.requester_identity.disputes_won = ctx.accounts.requester_identity.disputes_won.saturating_add(1);
            msg!("Dispute resolved (SOL): REQUESTER wins. Confiscated: {}, Insurance: {}, Winner: {}", confiscated, total_insurance, winner_amount);
        }
    } else {
        // SWORN-denominated dispute resolution: SPL token transfers via escrow PDA
        if final_provider_wins {
            ctx.accounts.dispute.status = DisputeStatus::ResolvedProvider;
            ctx.accounts.contract.status = ContractStatus::ResolvedProvider;

            let total = ctx.accounts.contract.value
                .checked_add(ctx.accounts.contract.provider_stake)
                .ok_or(TrustError::MathOverflow)?;

            let transfer_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.escrow_vault.to_account_info(),
                    to: ctx.accounts.provider_token_account.to_account_info(),
                    authority: ctx.accounts.escrow_vault.to_account_info(),
                },
                signer_seeds,
            );
            token::transfer(transfer_ctx, total)?;

            ctx.accounts.requester_identity.disputes_lost = ctx.accounts.requester_identity.disputes_lost.saturating_add(1);
            ctx.accounts.provider_identity.disputes_won = ctx.accounts.provider_identity.disputes_won.saturating_add(1);
            ctx.accounts.provider_identity.last_task_completed_at = now;
            msg!("Dispute resolved (SWORN): PROVIDER wins. {} released.", total);
        } else {
            ctx.accounts.dispute.status = DisputeStatus::ResolvedRequester;
            ctx.accounts.contract.status = ContractStatus::ResolvedRequester;

            let confiscated = ctx.accounts.contract.provider_stake;
            let contract_value = ctx.accounts.contract.value;
            let burn_rate_bps = ctx.accounts.protocol_config.burn_rate_bps;
            let insurance_rate_bps = ctx.accounts.protocol_config.insurance_rate_bps;
            let burn_amount = (confiscated as u128 * burn_rate_bps as u128 / 10_000) as u64;
            let insurance_amount = (confiscated as u128 * insurance_rate_bps as u128 / 10_000) as u64;
            let winner_amount = confiscated.saturating_sub(burn_amount).saturating_sub(insurance_amount);

            let refund = contract_value.checked_add(winner_amount).ok_or(TrustError::MathOverflow)?;
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
                ctx.accounts.insurance_pool.total_balance = ctx.accounts.insurance_pool.total_balance.saturating_add(insurance_amount);
            }

            if burn_amount > 0 {
                // Validate sworn_mint for burn
                require!(
                    ctx.accounts.sworn_mint.key() == ctx.accounts.protocol_config.sworn_mint,
                    TrustError::InvalidDestination
                );
                let burn_ctx = CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Burn {
                        mint: ctx.accounts.sworn_mint.to_account_info(),
                        from: ctx.accounts.escrow_vault.to_account_info(),
                        authority: ctx.accounts.escrow_vault.to_account_info(),
                    },
                    signer_seeds,
                );
                token::burn(burn_ctx, burn_amount)?;
            }

            ctx.accounts.provider_identity.disputes_lost = ctx.accounts.provider_identity.disputes_lost.saturating_add(1);
            ctx.accounts.requester_identity.disputes_won = ctx.accounts.requester_identity.disputes_won.saturating_add(1);
            msg!("Dispute resolved (SWORN): REQUESTER wins. Confiscated: {}, Burned: {}, Insurance: {}, Winner: {}", confiscated, burn_amount, insurance_amount, winner_amount);
        }
    }

    Ok(())
}

#[derive(Accounts)]
pub struct InitiateDispute<'info> {
    #[account(mut)]
    pub requester: Signer<'info>,

    #[account(
        mut,
        constraint = contract.requester == requester.key() @ TrustError::UnauthorizedRequester,
    )]
    pub contract: Account<'info, Contract>,

    #[account(
        init,
        payer = requester,
        space = 8 + Dispute::INIT_SPACE,
        seeds = [b"dispute" as &[u8], contract.key().as_ref()],
        bump
    )]
    pub dispute: Account<'info, Dispute>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RespondDispute<'info> {
    pub provider: Signer<'info>,

    pub contract: Account<'info, Contract>,

    #[account(
        mut,
        seeds = [b"dispute" as &[u8], contract.key().as_ref()],
        bump = dispute.bump,
    )]
    pub dispute: Account<'info, Dispute>,
}

#[derive(Accounts)]
pub struct EscalateDispute<'info> {
    /// Either the requester or provider may escalate (Whitepaper Section 5.2).
    pub initiator: Signer<'info>,

    #[account(mut)]
    pub contract: Account<'info, Contract>,

    #[account(
        mut,
        seeds = [b"dispute" as &[u8], contract.key().as_ref()],
        bump = dispute.bump,
        // Either contract party can escalate
        constraint = (
            initiator.key() == contract.requester ||
            initiator.key() == contract.provider
        ) @ TrustError::UnauthorizedRequester,
    )]
    pub dispute: Account<'info, Dispute>,
}

#[derive(Accounts)]
pub struct JuryVote<'info> {
    pub juror: Signer<'info>,

    pub contract: Account<'info, Contract>,

    #[account(
        mut,
        seeds = [b"dispute" as &[u8], contract.key().as_ref()],
        bump = dispute.bump,
    )]
    pub dispute: Account<'info, Dispute>,

    #[account(
        seeds = [b"agent-identity" as &[u8], juror.key().as_ref()],
        bump = juror_identity.bump,
        constraint = juror_identity.authority == juror.key(),
    )]
    pub juror_identity: Account<'info, AgentIdentity>,
}

/// ResolveDispute uses Box<Account> to avoid BPF stack overflow.
/// escrow_vault seeds removed — validated manually in handler (same fix as AcceptContract).
#[derive(Accounts)]
pub struct ResolveDispute<'info> {
    #[account(mut)]
    pub resolver: Signer<'info>,

    #[account(mut)]
    pub contract: Box<Account<'info, Contract>>,

    #[account(
        mut,
        seeds = [b"dispute" as &[u8], contract.key().as_ref()],
        bump = dispute.bump,
    )]
    pub dispute: Box<Account<'info, Dispute>>,

    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], contract.provider.as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Box<Account<'info, AgentIdentity>>,

    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], contract.requester.as_ref()],
        bump = requester_identity.bump,
    )]
    pub requester_identity: Box<Account<'info, AgentIdentity>>,

    /// For SWORN: provider's ATA. For SOL: provider's wallet.
    /// CHECK: For SOL, validated key == contract.provider. For SWORN, token CPI validates.
    #[account(mut)]
    pub provider_token_account: UncheckedAccount<'info>,

    /// For SWORN: requester's ATA. For SOL: requester's wallet.
    /// CHECK: For SOL, validated key == contract.requester. For SWORN, token CPI validates.
    #[account(mut)]
    pub requester_token_account: UncheckedAccount<'info>,

    /// Escrow vault PDA for SWORN. Unused for SOL.
    /// CHECK: Validated manually in handler via find_program_address.
    #[account(mut)]
    pub escrow_vault: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"insurance-pool"],
        bump = insurance_pool.bump,
    )]
    pub insurance_pool: Box<Account<'info, InsurancePool>>,

    /// For SWORN: insurance vault ATA. For SOL: pool authority PDA.
    /// CHECK: Validated in handler based on currency.
    #[account(mut)]
    pub insurance_vault: UncheckedAccount<'info>,

    /// SWORN mint. Only used for SWORN burn path.
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

// ---------------------------------------------------------------------------
// Level 1: Provider re-delivers corrected work during dispute
// ---------------------------------------------------------------------------

/// Provider re-delivers corrected work during a Level 1 (DirectCorrection) dispute.
/// Updates the PoE hash and increments corrections_count.
/// After 3 corrections, auto-escalates to Level 2 (PrivateRounds).
/// Whitepaper Section 5.1: Level 1 Direct Correction flow.
pub fn handler_redeliver(
    ctx: Context<RedeliverInDispute>,
    output_hash: [u8; 32],
    arweave_tx: String,
) -> Result<()> {
    let dispute = &mut ctx.accounts.dispute;

    require!(
        dispute.level == DisputeLevel::DirectCorrection,
        TrustError::InvalidContractStatus
    );
    require!(
        dispute.status == DisputeStatus::Open || dispute.status == DisputeStatus::Responded,
        TrustError::InvalidContractStatus
    );

    let contract = &mut ctx.accounts.contract;
    require!(
        contract.provider == ctx.accounts.provider.key(),
        TrustError::UnauthorizedProvider
    );

    let now = Clock::get()?.unix_timestamp;

    // Update contract PoE hash
    contract.poe_hash = output_hash;
    contract.poe_arweave_tx = arweave_tx.clone();
    // Return contract to Delivered so requester can review
    contract.status = ContractStatus::Delivered;

    // Update the PoE record
    let poe = &mut ctx.accounts.proof_of_execution;
    poe.output_hash = output_hash;
    poe.arweave_tx = arweave_tx;
    poe.submitted_at = now;
    poe.validated = false;

    // Track re-delivery on provider identity (Whitepaper: total_deliveries denominator)
    ctx.accounts.provider_identity.total_deliveries =
        ctx.accounts.provider_identity.total_deliveries.saturating_add(1);

    // Increment corrections count
    dispute.corrections_count = dispute.corrections_count.saturating_add(1);

    if dispute.corrections_count >= 3 {
        // Auto-escalate to Level 2 (PrivateRounds)
        dispute.level = DisputeLevel::PrivateRounds;
        dispute.status = DisputeStatus::Open;
        contract.dispute_level = 2;
        // 5-day deadline for private rounds
        dispute.deadline = now + 5 * 86_400;

        msg!(
            "Contract #{}: 3 corrections exhausted. Auto-escalated to Level 2 (PrivateRounds). Deadline: {}",
            contract.id,
            dispute.deadline
        );
    } else {
        // Reset deadline for requester review (7 days)
        dispute.status = DisputeStatus::Open;
        dispute.deadline = now + 7 * 86_400;

        msg!(
            "Contract #{}: Re-delivery #{} submitted. Awaiting requester review. Deadline: {}",
            contract.id,
            dispute.corrections_count,
            dispute.deadline
        );
    }

    Ok(())
}

/// Requester accepts provider's correction during L1 or L2 dispute.
/// Resolves dispute + completes contract + releases payment (same as accept_contract).
/// Whitepaper Section 5.1: Requester accepts correction → dispute resolved.
pub fn handler_accept_correction(ctx: Context<AcceptCorrection>) -> Result<()> {
    let contract = &mut ctx.accounts.contract;
    require!(
        contract.status == ContractStatus::Delivered,
        TrustError::InvalidContractStatus
    );
    require!(
        contract.requester == ctx.accounts.requester.key(),
        TrustError::UnauthorizedRequester
    );

    let dispute = &mut ctx.accounts.dispute;
    require!(
        dispute.level == DisputeLevel::DirectCorrection
            || dispute.level == DisputeLevel::PrivateRounds,
        TrustError::InvalidContractStatus
    );

    let now = Clock::get()?.unix_timestamp;

    // Resolve the dispute
    dispute.status = DisputeStatus::ResolvedProvider;
    dispute.resolved_at = now;

    // Complete the contract
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
    // Track corrections_received (requester accepted = provider needed correction)
    provider_identity.corrections_received =
        provider_identity.corrections_received.saturating_add(dispute.corrections_count as u32);
    // Track volume by currency
    if contract.currency == Currency::Sol {
        provider_identity.volume_sol = provider_identity.volume_sol.saturating_add(contract.value);
    } else {
        provider_identity.volume_processed =
            provider_identity.volume_processed.saturating_add(contract.value);
    }

    // Extract values before releasing mutable borrow for payment logic
    let contract_value = contract.value;
    let contract_provider_stake = contract.provider_stake;
    let contract_currency = contract.currency;
    let contract_id_val = contract.id;
    let corrections_count = dispute.corrections_count;
    let tasks_completed = provider_identity.tasks_completed;

    // Calculate protocol fee: 1% of contract value (Whitepaper Section 11.8)
    // Split: 70% treasury, 20% insurance pool, 10% burn
    let protocol_fee = contract_value / 100; // 1%
    let fee_treasury = protocol_fee * 70 / 100;
    let fee_insurance = protocol_fee * 20 / 100;
    let fee_burn = protocol_fee.saturating_sub(fee_treasury).saturating_sub(fee_insurance); // 10%

    // Net payment to provider = contract value - protocol fee + stake return
    let net_payment = contract_value.saturating_sub(protocol_fee);
    let provider_release = net_payment
        .checked_add(contract_provider_stake)
        .ok_or(TrustError::MathOverflow)?;

    if contract_currency == Currency::Sol {
        // SOL-denominated contract: transfer lamports from contract PDA
        // Validate destinations
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
        // Insurance PDA may have 0 SOL — sending small amounts causes InsufficientFundsForRent.
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

        // CPI 4: Burn 10% of fee (Whitepaper Section 11.8: deflationary)
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
        "Contract #{} completed via dispute correction. Currency: {:?}. Provider: {} net. Fee: {} (treasury: {}, insurance: {}, burn: {}). Corrections: {}. Tasks: {}",
        contract_id_val,
        contract_currency,
        provider_release,
        protocol_fee,
        fee_treasury,
        fee_insurance,
        fee_burn,
        corrections_count,
        tasks_completed
    );
    Ok(())
}

#[derive(Accounts)]
pub struct RedeliverInDispute<'info> {
    #[account(mut)]
    pub provider: Signer<'info>,

    #[account(
        mut,
        constraint = contract.provider == provider.key() @ TrustError::UnauthorizedProvider,
        constraint = contract.status == ContractStatus::Disputed @ TrustError::InvalidContractStatus,
    )]
    pub contract: Account<'info, Contract>,

    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], provider.key().as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Account<'info, AgentIdentity>,

    #[account(
        mut,
        seeds = [b"dispute" as &[u8], contract.key().as_ref()],
        bump = dispute.bump,
    )]
    pub dispute: Account<'info, Dispute>,

    #[account(
        mut,
        seeds = [b"poe" as &[u8], contract.key().as_ref()],
        bump = proof_of_execution.bump,
    )]
    pub proof_of_execution: Account<'info, ProofOfExecution>,
}

#[derive(Accounts)]
pub struct AcceptCorrection<'info> {
    pub requester: Signer<'info>,

    #[account(
        mut,
        constraint = contract.requester == requester.key() @ TrustError::UnauthorizedRequester,
    )]
    pub contract: Box<Account<'info, Contract>>,

    #[account(
        mut,
        seeds = [b"dispute" as &[u8], contract.key().as_ref()],
        bump = dispute.bump,
    )]
    pub dispute: Box<Account<'info, Dispute>>,

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

    /// For SWORN: insurance vault ATA. For SOL: pool authority PDA.
    /// CHECK: Validated in handler based on currency.
    #[account(mut)]
    pub insurance_vault: UncheckedAccount<'info>,

    /// Escrow vault PDA for SWORN contracts. Unused for SOL.
    /// CHECK: Validated manually in handler for SWORN path.
    #[account(mut)]
    pub escrow_vault: UncheckedAccount<'info>,

    /// SWORN mint for the burn CPI (10% of fee). Unused for SOL contracts.
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

// ---------------------------------------------------------------------------
// Migration: resize old Dispute accounts to include corrections_count
// ---------------------------------------------------------------------------

/// Migrate a Dispute account created before corrections_count was added.
/// Reallocs from 169 to 170 bytes. The new byte is zero-filled (corrections_count=0).
/// Only needs to be called once per old dispute.
pub fn handler_migrate_dispute(ctx: Context<MigrateDisputeSize>) -> Result<()> {
    let dispute_info = &ctx.accounts.dispute;
    let contract_key = ctx.accounts.contract.key();

    // Validate PDA
    let (expected_pda, _bump) = Pubkey::find_program_address(
        &[b"dispute", contract_key.as_ref()],
        ctx.program_id,
    );
    require!(dispute_info.key() == expected_pda, TrustError::InvalidEscrowVault);

    // Check owner
    require!(
        dispute_info.owner == ctx.program_id,
        TrustError::InvalidEscrowVault
    );

    let old_len = dispute_info.data_len();
    let new_len = 8 + Dispute::INIT_SPACE;

    if old_len < new_len {
        // Transfer rent difference via CPI to system program
        let rent = Rent::get()?;
        let new_min_balance = rent.minimum_balance(new_len);
        let old_balance = dispute_info.lamports();
        if old_balance < new_min_balance {
            let diff = new_min_balance - old_balance;
            anchor_lang::system_program::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    anchor_lang::system_program::Transfer {
                        from: ctx.accounts.payer.to_account_info(),
                        to: dispute_info.to_account_info(),
                    },
                ),
                diff,
            )?;
        }
        dispute_info.realloc(new_len, false)?;
        // Zero out the new byte (corrections_count = 0)
        let mut data = dispute_info.try_borrow_mut_data()?;
        data[old_len..new_len].fill(0);
        msg!("Dispute migrated for contract {}. {} -> {} bytes.", contract_key, old_len, new_len);
    } else {
        msg!("Dispute already at correct size ({} bytes). No migration needed.", old_len);
    }

    Ok(())
}

#[derive(Accounts)]
pub struct MigrateDisputeSize<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub contract: Account<'info, Contract>,

    /// CHECK: Validated manually via PDA derivation + owner check. Cannot use Account<Dispute>
    /// because old accounts have smaller size and fail deserialization.
    #[account(mut)]
    pub dispute: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}
