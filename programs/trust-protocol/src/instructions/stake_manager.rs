use anchor_lang::prelude::*;
use crate::state::*;
use crate::errors::TrustError;

/// Deposit SWORN into the Staking-Based Liquidity Pool (Whitepaper §11.10).
/// Split: reserve_ratio -> liquid_reserve, (1-reserve_ratio) -> lp_allocation.
/// Default: 40% liquid reserve, 60% LP allocation.
/// LP deposit to Orca Whirlpool is a STUB for now.
pub fn handler_deposit_stake(ctx: Context<DepositStake>, amount: u64) -> Result<()> {
    require!(amount > 0, TrustError::ZeroDeposit);

    let config = &ctx.accounts.protocol_config;
    let stake = &mut ctx.accounts.stake_manager;

    let reserve_ratio = config.reserve_ratio_bps as u64; // default 4000 = 40%
    let liquid_amount = amount
        .checked_mul(reserve_ratio)
        .unwrap()
        .checked_div(10000)
        .unwrap();
    let lp_amount = amount.checked_sub(liquid_amount).unwrap();

    stake.agent = ctx.accounts.agent.key();
    stake.total_staked = stake.total_staked.checked_add(amount).unwrap();
    stake.liquid_reserve = stake.liquid_reserve.checked_add(liquid_amount).unwrap();
    stake.lp_allocation = stake.lp_allocation.checked_add(lp_amount).unwrap();
    stake.bump = ctx.bumps.stake_manager;

    // STUB: In production, CPI to Orca Whirlpool to deposit lp_amount
    msg!("LP deposit stub: {} SWORN to Orca pool", lp_amount);
    msg!(
        "Stake deposited: total={}, liquid={}, lp={}",
        stake.total_staked, stake.liquid_reserve, stake.lp_allocation
    );

    Ok(())
}

/// Withdraw SWORN from the liquid reserve (Whitepaper §11.10).
/// Only withdraws from liquid_reserve, not from LP allocation.
pub fn handler_withdraw_stake(ctx: Context<WithdrawStake>, amount: u64) -> Result<()> {
    require!(amount > 0, TrustError::ZeroDeposit);

    let stake = &mut ctx.accounts.stake_manager;
    require!(amount <= stake.liquid_reserve, TrustError::InsufficientLiquidReserve);
    require!(amount <= stake.total_staked, TrustError::WithdrawExceedsStake);

    stake.liquid_reserve = stake.liquid_reserve.checked_sub(amount).unwrap();
    stake.total_staked = stake.total_staked.checked_sub(amount).unwrap();

    msg!("Stake withdrawn: amount={}, remaining_liquid={}", amount, stake.liquid_reserve);

    Ok(())
}

/// Harvest LP fees earned from liquidity provision (Whitepaper §11.10).
/// Fee distribution: 70% stakers pro-rata, 20% treasury, 10% insurance.
/// STUB: logs instead of real CPI. In production this would claim fees from Orca.
pub fn handler_harvest_lp_fees(ctx: Context<HarvestLpFees>) -> Result<()> {
    let stake = &mut ctx.accounts.stake_manager;
    let config = &ctx.accounts.protocol_config;
    require!(stake.lp_fees_earned > 0, TrustError::NoFeesToHarvest);

    let total_fees = stake.lp_fees_earned;
    let staker_share = config.lp_fee_staker_bps as u64; // default 7000 = 70%
    let staker_amount = total_fees
        .checked_mul(staker_share)
        .unwrap()
        .checked_div(10000)
        .unwrap();
    let treasury_amount = total_fees
        .checked_mul(2000) // 20% treasury
        .unwrap()
        .checked_div(10000)
        .unwrap();
    let insurance_amount = total_fees
        .checked_sub(staker_amount)
        .unwrap()
        .checked_sub(treasury_amount)
        .unwrap();

    let clock = Clock::get()?;
    stake.lp_fees_earned = 0;
    stake.last_fee_harvest = clock.unix_timestamp;

    // STUB: In production, transfer staker_amount to agent, treasury_amount to treasury, insurance to pool
    msg!(
        "LP fees harvested: total={}, staker={} ({}bps), treasury={}, insurance={}",
        total_fees, staker_amount, staker_share, treasury_amount, insurance_amount
    );

    Ok(())
}

// === Account structs ===

#[derive(Accounts)]
pub struct DepositStake<'info> {
    #[account(mut)]
    pub agent: Signer<'info>,

    #[account(
        seeds = [b"agent-identity", agent.key().as_ref()],
        bump = agent_identity.bump,
    )]
    pub agent_identity: Account<'info, AgentIdentity>,

    #[account(
        init,
        payer = agent,
        space = 8 + StakeManager::INIT_SPACE,
        seeds = [b"stake-manager", agent.key().as_ref()],
        bump,
    )]
    pub stake_manager: Account<'info, StakeManager>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WithdrawStake<'info> {
    #[account(mut)]
    pub agent: Signer<'info>,

    #[account(
        mut,
        seeds = [b"stake-manager", agent.key().as_ref()],
        bump = stake_manager.bump,
        constraint = stake_manager.agent == agent.key(),
    )]
    pub stake_manager: Account<'info, StakeManager>,
}

#[derive(Accounts)]
pub struct HarvestLpFees<'info> {
    #[account(mut)]
    pub agent: Signer<'info>,

    #[account(
        mut,
        seeds = [b"stake-manager", agent.key().as_ref()],
        bump = stake_manager.bump,
        constraint = stake_manager.agent == agent.key(),
    )]
    pub stake_manager: Account<'info, StakeManager>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
}
