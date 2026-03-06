use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use crate::state::*;
use crate::errors::TrustError;

/// Register a new agent identity with a SWORN identity bond (2-5 tokens).
/// Identity is soulbound (non-transferable) and requires 30-day maturation.
/// Whitepaper Section 2: Identity Model + Anti-Sybil
pub fn handler_register(ctx: Context<RegisterAgent>, bond_amount: u64) -> Result<()> {
    let config = &ctx.accounts.protocol_config;

    // Validate bond amount (2-5 SWORN)
    require!(
        bond_amount >= config.min_identity_bond && bond_amount <= config.max_identity_bond,
        TrustError::InvalidBondAmount
    );

    // Transfer SWORN tokens as identity bond (locked permanently)
    let transfer_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(),
        Transfer {
            from: ctx.accounts.agent_token_account.to_account_info(),
            to: ctx.accounts.bond_vault.to_account_info(),
            authority: ctx.accounts.agent.to_account_info(),
        },
    );
    token::transfer(transfer_ctx, bond_amount)?;

    // Initialize agent identity (soulbound)
    let identity = &mut ctx.accounts.agent_identity;
    identity.authority = ctx.accounts.agent.key();
    identity.identity_bond = bond_amount;
    identity.registered_at = Clock::get()?.unix_timestamp;
    identity.matured = false;
    identity.trust_score = 0;
    identity.tasks_completed = 0;
    identity.volume_processed = 0;
    identity.disputes_lost = 0;
    identity.disputes_won = 0;
    identity.tasks_abandoned = 0;
    identity.fraud_flags = 0;
    identity.sponsor_bonus = 0;
    identity.banned = false;
    identity.bump = ctx.bumps.agent_identity;

    // Increment global agent counter
    let config = &mut ctx.accounts.protocol_config;
    config.total_agents = config.total_agents.checked_add(1).ok_or(TrustError::MathOverflow)?;

    msg!(
        "Agent registered: {}. Bond: {} SWORN lamports. DID: did:trust:{}",
        ctx.accounts.agent.key(),
        bond_amount,
        ctx.accounts.agent.key()
    );
    Ok(())
}

/// Sponsor an agent to boost their TrustScore (established agents vouch for newcomers).
/// Sponsor must have TrustScore >= 50 and matured identity.
pub fn handler_sponsor(ctx: Context<SponsorAgent>, bonus_points: u16) -> Result<()> {
    let sponsor = &ctx.accounts.sponsor_identity;
    require!(!sponsor.banned, TrustError::AgentBanned);
    require!(sponsor.matured, TrustError::IdentityNotMatured);
    require!(sponsor.trust_score >= 50, TrustError::InsufficientJuryReputation);

    // Cap sponsor bonus at 10 points
    let capped = bonus_points.min(10);
    let agent = &mut ctx.accounts.agent_identity;
    agent.sponsor_bonus = agent.sponsor_bonus.saturating_add(capped);

    msg!(
        "Agent {} sponsored by {} with {} bonus points",
        agent.authority,
        sponsor.authority,
        capped
    );
    Ok(())
}

#[derive(Accounts)]
pub struct RegisterAgent<'info> {
    #[account(mut)]
    pub agent: Signer<'info>,

    #[account(
        init,
        payer = agent,
        space = 8 + AgentIdentity::INIT_SPACE,
        seeds = [b"agent-identity", agent.key().as_ref()],
        bump
    )]
    pub agent_identity: Account<'info, AgentIdentity>,

    /// Agent's SWORN token account (source of bond)
    #[account(
        mut,
        constraint = agent_token_account.owner == agent.key(),
        constraint = agent_token_account.mint == protocol_config.sworn_mint,
    )]
    pub agent_token_account: Account<'info, TokenAccount>,

    /// Bond vault (PDA-controlled, tokens locked permanently)
    #[account(
        mut,
        seeds = [b"bond-vault"],
        bump,
    )]
    pub bond_vault: Account<'info, TokenAccount>,

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
pub struct SponsorAgent<'info> {
    pub sponsor: Signer<'info>,

    #[account(
        seeds = [b"agent-identity", sponsor.key().as_ref()],
        bump = sponsor_identity.bump,
        constraint = sponsor_identity.authority == sponsor.key(),
    )]
    pub sponsor_identity: Account<'info, AgentIdentity>,

    #[account(
        mut,
        seeds = [b"agent-identity", agent_identity.authority.as_ref()],
        bump = agent_identity.bump,
    )]
    pub agent_identity: Account<'info, AgentIdentity>,
}
