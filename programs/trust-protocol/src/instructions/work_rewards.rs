use crate::errors::TrustError;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

/// Emit work reward for a completed task.
/// Whitepaper 11.3b: Work Rewards emission with halving schedule.
///
/// work_reward = base_reward * 0.5^floor(total_protocol_tasks / halving_interval)
///
/// Schedule (with defaults: base_reward=10 SWORN, halving_interval=50,000):
///   Tasks 0-49,999:       10 SWORN/task   (500,000 SWORN emitted)
///   Tasks 50,000-99,999:   5 SWORN/task   (250,000 SWORN emitted)
///   Tasks 100,000-149,999: 2.5 SWORN/task (125,000 SWORN emitted)
///   ...converges to 1,000,000 SWORN total
///
/// Called by the protocol after a contract is completed (handler_accept in contract.rs).
/// Admin-only in Phase 0-2 to prevent abuse; permissionless via CPI in Phase 3+.
pub fn handler_emit_work_reward(ctx: Context<EmitWorkReward>) -> Result<()> {
    // Read all needed values before any mutable borrow
    let governance_phase = ctx.accounts.protocol_config.governance_phase;
    let admin_key = ctx.accounts.protocol_config.admin;
    let total_emitted = ctx.accounts.protocol_config.total_work_rewards_emitted;
    let max_rewards = ctx.accounts.protocol_config.max_work_rewards;
    let halving_interval = ctx.accounts.protocol_config.halving_interval;
    let total_tasks = ctx.accounts.protocol_config.total_protocol_tasks;
    let base_reward = ctx.accounts.protocol_config.base_work_reward;
    let config_bump = ctx.accounts.protocol_config.bump;

    // Anti-gaming: only admin can trigger in Phase 0-2
    if governance_phase < 3 {
        require!(
            ctx.accounts.authority.key() == admin_key,
            TrustError::UnauthorizedAdmin
        );
    }

    // Check cap: automatic emission is limited to max_work_rewards (1M SWORN)
    if total_emitted >= max_rewards {
        msg!("Work rewards cap reached: {} >= {}", total_emitted, max_rewards);
        return Ok(());
    }

    // Calculate reward with halving: base_reward * 0.5^floor(total_tasks / halving_interval)
    let halvings = if halving_interval > 0 {
        total_tasks / halving_interval
    } else {
        0
    };

    // After 63 halvings the reward is effectively 0
    let reward = if halvings >= 64 {
        0
    } else {
        base_reward >> halvings
    };

    if reward == 0 {
        msg!("Work reward is 0 after {} halvings", halvings);
        return Ok(());
    }

    // Ensure we do not exceed the cap
    let actual_reward = reward.min(max_rewards.saturating_sub(total_emitted));

    if actual_reward == 0 {
        return Ok(());
    }

    // Transfer reward from work rewards vault to provider
    let config_seeds: &[&[u8]] = &[b"protocol-config", &[config_bump]];
    let signer_seeds = &[config_seeds];
    let transfer_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        Transfer {
            from: ctx.accounts.work_rewards_vault.to_account_info(),
            to: ctx.accounts.provider_token_account.to_account_info(),
            authority: ctx.accounts.protocol_config.to_account_info(),
        },
        signer_seeds,
    );
    token::transfer(transfer_ctx, actual_reward)?;

    // Now mutate config counters
    let config = &mut ctx.accounts.protocol_config;
    config.total_protocol_tasks = config.total_protocol_tasks.saturating_add(1);
    config.total_work_rewards_emitted = config
        .total_work_rewards_emitted
        .saturating_add(actual_reward);

    // Update provider identity task counter (for hibernation cooldown tracking)
    let provider = &mut ctx.accounts.provider_identity;
    provider.tasks_since_last_hibernation = provider.tasks_since_last_hibernation.saturating_add(1);

    msg!(
        "Work reward emitted: {} lamports to {}. Tasks: {}, Emitted: {}, Halvings: {}",
        actual_reward,
        provider.authority,
        config.total_protocol_tasks,
        config.total_work_rewards_emitted,
        halvings
    );
    Ok(())
}

#[derive(Accounts)]
pub struct EmitWorkReward<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    /// Work rewards reserve vault (holds the 15M SWORN allocation)
    #[account(mut)]
    pub work_rewards_vault: Account<'info, TokenAccount>,

    /// Provider SWORN token account (receives the reward)
    #[account(
        mut,
        constraint = provider_token_account.owner == provider_identity.authority,
    )]
    pub provider_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], provider_identity.authority.as_ref()],
        bump = provider_identity.bump,
    )]
    pub provider_identity: Account<'info, AgentIdentity>,

    pub token_program: Program<'info, Token>,
}
