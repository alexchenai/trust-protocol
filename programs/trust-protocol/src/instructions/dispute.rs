use crate::errors::TrustError;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Burn, Mint, Token, TokenAccount, Transfer};

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

    // Set jury size for Public Jury and Appeal
    match dispute.level {
        DisputeLevel::PublicJury => dispute.jury_size = 5, // 5 jurors
        DisputeLevel::Appeal => dispute.jury_size = 11,    // 11 jurors (double-or-nothing)
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

    // Save values we need before multiple borrows
    let contract_id_bytes = ctx.accounts.contract.id.to_le_bytes();
    let escrow_bump = ctx.bumps.escrow_vault;
    let escrow_seeds: &[&[u8]] = &[b"escrow", &contract_id_bytes, &[escrow_bump]];
    let signer_seeds = &[escrow_seeds];

    if final_provider_wins {
        ctx.accounts.dispute.status = DisputeStatus::ResolvedProvider;
        ctx.accounts.contract.status = ContractStatus::ResolvedProvider;

        // Provider gets payment + stake back (same as accept)
        let total = ctx
            .accounts
            .contract
            .value
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

        // Update requester stats (lost dispute)
        ctx.accounts.requester_identity.disputes_lost = ctx
            .accounts
            .requester_identity
            .disputes_lost
            .saturating_add(1);

        // Update provider stats (won dispute)
        ctx.accounts.provider_identity.disputes_won = ctx
            .accounts
            .provider_identity
            .disputes_won
            .saturating_add(1);

        msg!("Dispute resolved: PROVIDER wins. {} SWORN released.", total);
    } else {
        ctx.accounts.dispute.status = DisputeStatus::ResolvedRequester;
        ctx.accounts.contract.status = ContractStatus::ResolvedRequester;

        // Confiscate provider's stake
        let confiscated = ctx.accounts.contract.provider_stake;
        let contract_value = ctx.accounts.contract.value;

        // 15% burned (deflationary)
        let burn_rate_bps = ctx.accounts.protocol_config.burn_rate_bps;
        let insurance_rate_bps = ctx.accounts.protocol_config.insurance_rate_bps;
        let burn_amount = (confiscated as u128 * burn_rate_bps as u128 / 10_000) as u64;
        // 60% to insurance pool
        let insurance_amount = (confiscated as u128 * insurance_rate_bps as u128 / 10_000) as u64;
        // 25% to requester (winner)
        let winner_amount = confiscated
            .saturating_sub(burn_amount)
            .saturating_sub(insurance_amount);

        // Return contract value to requester
        let refund = contract_value
            .checked_add(winner_amount)
            .ok_or(TrustError::MathOverflow)?;
        let transfer_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.escrow_vault.to_account_info(),
                to: ctx.accounts.requester_token_account.to_account_info(),
                authority: ctx.accounts.escrow_vault.to_account_info(),
            },
            signer_seeds,
        );
        token::transfer(transfer_ctx, refund)?;

        // Transfer insurance portion to pool
        if insurance_amount > 0 {
            let transfer_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.escrow_vault.to_account_info(),
                    to: ctx.accounts.insurance_vault.to_account_info(),
                    authority: ctx.accounts.escrow_vault.to_account_info(),
                },
                signer_seeds,
            );
            token::transfer(transfer_ctx, insurance_amount)?;

            ctx.accounts.insurance_pool.total_balance = ctx
                .accounts
                .insurance_pool
                .total_balance
                .saturating_add(insurance_amount);
        }

        // Burn tokens (15% deflationary mechanic)
        if burn_amount > 0 {
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

        // Update provider stats (lost dispute)
        ctx.accounts.provider_identity.disputes_lost = ctx
            .accounts
            .provider_identity
            .disputes_lost
            .saturating_add(1);

        // Update requester stats (won dispute)
        ctx.accounts.requester_identity.disputes_won = ctx
            .accounts
            .requester_identity
            .disputes_won
            .saturating_add(1);

        msg!(
            "Dispute resolved: REQUESTER wins. Confiscated: {}. Burned: {}, Insurance: {}, Winner: {}",
            confiscated, burn_amount, insurance_amount, winner_amount
        );
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
    pub initiator: Signer<'info>,

    #[account(mut)]
    pub contract: Account<'info, Contract>,

    #[account(
        mut,
        seeds = [b"dispute" as &[u8], contract.key().as_ref()],
        bump = dispute.bump,
        constraint = dispute.initiator == initiator.key(),
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

#[derive(Accounts)]
pub struct ResolveDispute<'info> {
    #[account(mut)]
    pub resolver: Signer<'info>,

    #[account(mut)]
    pub contract: Account<'info, Contract>,

    #[account(
        mut,
        seeds = [b"dispute" as &[u8], contract.key().as_ref()],
        bump = dispute.bump,
    )]
    pub dispute: Account<'info, Dispute>,

    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], contract.provider.as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Account<'info, AgentIdentity>,

    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], contract.requester.as_ref()],
        bump = requester_identity.bump,
    )]
    pub requester_identity: Account<'info, AgentIdentity>,

    #[account(
        mut,
        constraint = provider_token_account.owner == contract.provider,
    )]
    pub provider_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = requester_token_account.owner == contract.requester,
    )]
    pub requester_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"escrow" as &[u8], &contract.id.to_le_bytes()],
        bump,
    )]
    pub escrow_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"insurance-pool"],
        bump = insurance_pool.bump,
    )]
    pub insurance_pool: Account<'info, InsurancePool>,

    /// Insurance pool token vault
    #[account(mut)]
    pub insurance_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = sworn_mint.key() == protocol_config.sworn_mint,
    )]
    pub sworn_mint: Account<'info, Mint>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    pub token_program: Program<'info, Token>,
}
