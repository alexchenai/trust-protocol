use crate::errors::TrustError;
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

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
    identity.volume_sol = 0;
    identity.disputes_lost = 0;
    identity.disputes_won = 0;
    identity.tasks_abandoned = 0;
    identity.fraud_flags = 0;
    identity.total_deliveries = 0;
    identity.corrections_received = 0;
    identity.active_contracts = 0;
    identity.last_task_completed_at = 0;
    identity.sponsor_bonus = 0;
    identity.banned = false;
    identity.bump = ctx.bumps.agent_identity;

    // Increment global agent counter
    let config = &mut ctx.accounts.protocol_config;
    config.total_agents = config
        .total_agents
        .checked_add(1)
        .ok_or(TrustError::MathOverflow)?;

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
    require!(
        sponsor.trust_score >= 50,
        TrustError::InsufficientJuryReputation
    );

    // Cap sponsor bonus at 5 points (Whitepaper Section 10.3: W_sponsor = 5 max)
    let capped = bonus_points.min(5);
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
        seeds = [b"agent-identity" as &[u8], agent.key().as_ref()],
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
    /// Uses bond-vault-v2 seeds after v1→v2 migration.
    #[account(
        mut,
        seeds = [b"bond-vault-v2"],
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
        seeds = [b"agent-identity" as &[u8], sponsor.key().as_ref()],
        bump = sponsor_identity.bump,
        constraint = sponsor_identity.authority == sponsor.key(),
    )]
    pub sponsor_identity: Account<'info, AgentIdentity>,

    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], agent_identity.authority.as_ref()],
        bump = agent_identity.bump,
    )]
    pub agent_identity: Account<'info, AgentIdentity>,
}

/// Permissionless: anyone can call this to finalize maturation of an agent.
/// Whitepaper Section 10.2: Identity matures after 14 days AND >= 5 completed tasks.
/// This allows agents to self-mature without relying on admin.
pub fn handler_check_maturation(ctx: Context<CheckMaturation>) -> Result<()> {
    let identity = &mut ctx.accounts.agent_identity;
    let config = &ctx.accounts.protocol_config;

    require!(!identity.matured, TrustError::IdentityNotMatured); // already matured

    let now = Clock::get()?.unix_timestamp;
    let elapsed = now - identity.registered_at;
    require!(
        elapsed >= config.maturation_period,
        TrustError::IdentityNotMatured
    );
    require!(
        identity.tasks_completed >= 5,
        TrustError::IdentityNotMatured
    );

    identity.matured = true;
    msg!(
        "Agent {} matured. Elapsed: {}s, Tasks: {}",
        identity.authority,
        elapsed,
        identity.tasks_completed
    );
    Ok(())
}

#[derive(Accounts)]
pub struct CheckMaturation<'info> {
    // permissionless — any payer can trigger
    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], agent_identity.authority.as_ref()],
        bump = agent_identity.bump,
    )]
    pub agent_identity: Account<'info, AgentIdentity>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
}

/// On-chain TrustScore computation.
/// Whitepaper Section 4: TrustScore formula with 5 factors + penalties + decay.
/// sol_to_sworn_rate: how many SWORN micro-lamports equal 1 SOL lamport.
///   Use 0 to treat SOL volume as 0 (conservative). Typical: 1_000_000 (1:1 micro scale).
pub fn handler_calculate_trust_score(
    ctx: Context<CalculateTrustScore>,
    sol_to_sworn_rate: u64,
) -> Result<()> {
    let identity = &mut ctx.accounts.agent_identity;
    let now = Clock::get()?.unix_timestamp;

    // Whitepaper §4.4: S_penalty += 100 * fraud_flags.
    // GAP-5: Early return is mathematically equivalent to the formula:
    // 100*fraud_flags dominates all S_base factors (max=100), clamping score to 0.
    // This is an optimization over computing the full formula, not a semantic deviation.
    if identity.fraud_flags > 0 {
        identity.trust_score = 0;
        msg!("TrustScore for {}: 0 (fraud flag, whitepaper section 4.4 P_fraud=100*flags)", identity.authority);
        return Ok(());
    }

    // --- Factor 1: task_factor = min(1.0, log10(1 + tasks) / 3.0), scaled to bps ---
    // log10(1001) ~ 3.0 => task_factor reaches 1.0 at ~1000 tasks
    let task_factor_bps = {
        let n = identity.tasks_completed.saturating_add(1);
        let log_bps = integer_log10_bps(n); // log10(n) * 10000
        (log_bps / 3).min(10_000)
    };

    // --- Factor 2: volume_factor = min(1.0, log10(1 + total_volume_usd_equiv) / 6.0) ---
    // We use SWORN lamports as proxy (1M lamports = 1 SWORN unit).
    // SOL volume is converted using sol_to_sworn_rate.
    let total_volume = identity.volume_processed.saturating_add(
        identity.volume_sol.saturating_mul(sol_to_sworn_rate).min(u64::MAX / 2)
    );
    let volume_factor_bps = {
        let n = total_volume.saturating_add(1);
        let log_bps = integer_log10_bps(n);
        (log_bps / 6).min(10_000)
    };

    // --- Factor 3: quality_factor ---
    // max(0, 1.0 - 2*correction_ratio - 5*dispute_loss_ratio) * ramp(tasks/20)
    // correction_ratio = corrections_received / max(1, total_deliveries)
    // dispute_loss_ratio = disputes_lost / max(1, tasks_completed)
    let quality_factor_bps = {
        let total_del = identity.total_deliveries.max(1) as u64;
        let corr_ratio_bps = ((identity.corrections_received as u64) * 10_000 / total_del).min(10_000);
        let dl_ratio_bps = ((identity.disputes_lost as u64) * 10_000
            / identity.tasks_completed.max(1)).min(10_000);
        // penalty: 2 * correction_ratio + 5 * dispute_loss_ratio (in bps)
        let penalty_bps = (2 * corr_ratio_bps).saturating_add(5 * dl_ratio_bps);
        let raw_bps = 10_000u64.saturating_sub(penalty_bps);
        // ramp: * min(1, tasks / 20)
        let ramp_bps = (identity.tasks_completed * 10_000 / 20).min(10_000);
        (raw_bps * ramp_bps / 10_000).min(10_000)
    };

    // --- Factor 4: age_factor = min(1.0, months_since_creation / 24) ---
    let months_elapsed = ((now - identity.registered_at).max(0) as u64) / (30 * 86_400);
    let age_factor_bps = ((months_elapsed * 10_000) / 24).min(10_000);

    // --- Factor 5: sponsor_bonus = 1.0 if sponsored (bonus > 0), else 0 ---
    let sponsor_factor_bps = if identity.sponsor_bonus > 0 { 10_000u64 } else { 0u64 };

    // --- S_base (unweighted, out of 100 * 10000 bps = 1_000_000) ---
    // W: tasks=30, volume=20, quality=25, age=20, sponsor=5
    let s_base_scaled = task_factor_bps * 30
        + volume_factor_bps * 20
        + quality_factor_bps * 25
        + age_factor_bps * 20
        + sponsor_factor_bps * 5;
    // Divide by 10000 to get S_base in [0, 100]
    let s_base = (s_base_scaled / 10_000).min(100);

    // --- Penalties ---
    // P_dispute = 50 * (disputes_lost / max(1, total_tasks))
    // P_abandon = 150 * (abandonos / max(1, total_tasks))
    let total_tasks = identity.tasks_completed.max(1);
    let p_dispute = 50u64 * identity.disputes_lost as u64 / total_tasks;
    let p_abandon = 150u64 * identity.tasks_abandoned as u64 / total_tasks;
    let s_penalty = p_dispute.saturating_add(p_abandon);

    let ts_raw = s_base.saturating_sub(s_penalty) as u16;

    // --- Decay: 2 pts/month inactive, max -40 ---
    let ts_after_decay = if identity.last_task_completed_at > 0 {
        let months_inactive = ((now - identity.last_task_completed_at).max(0) as u64) / (30 * 86_400);
        let decay = (months_inactive * 2).min(40);
        ts_raw.saturating_sub(decay as u16)
    } else {
        ts_raw
    };

    identity.trust_score = ts_after_decay;
    msg!(
        "TrustScore for {}: {} (task_f={}/10000, vol_f={}/10000, qual_f={}/10000, age_f={}/10000, spons_f={}/10000, s_base={}, p_dispute={}, p_abandon={})",
        identity.authority, ts_after_decay,
        task_factor_bps, volume_factor_bps, quality_factor_bps, age_factor_bps, sponsor_factor_bps,
        s_base, p_dispute, p_abandon
    );
    Ok(())
}

/// Integer log10 approximation in basis points (log10(n) * 10000).
/// Uses digit-count + fractional lookup for smooth approximation.
fn integer_log10_bps(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    // Count integer digits -> floor(log10(n))
    let mut digits = 0u64;
    let mut tmp = n;
    while tmp > 0 {
        digits += 1;
        tmp /= 10;
    }
    let floor_log10 = digits - 1;
    // Approximate fractional part: f = (n / 10^floor - 1) * ln(10) / 10 * 10000
    // Lookup table for fractional part * 10000 for mantissa 1.0-9.9
    // mantissa = n / 10^floor_log10, in range [1, 10)
    let power_of_10 = 10u64.pow(floor_log10 as u32);
    let mantissa_10 = (n * 10) / power_of_10; // mantissa * 10, range [10, 99]
    let frac_bps: u64 = match mantissa_10 {
        10 => 0,
        11 => 414,
        12 => 792,
        13 => 1139,
        14 => 1461,
        15 => 1761,
        16 => 2041,
        17 => 2304,
        18 => 2553,
        19 => 2788,
        20 => 3010,
        21 => 3222,
        22 => 3424,
        23 => 3617,
        24 => 3802,
        25 => 3979,
        26 => 4150,
        27 => 4314,
        28 => 4472,
        29 => 4624,
        30 => 4771,
        31 => 4914,
        32 => 5051,
        33 => 5185,
        34 => 5315,
        35 => 5441,
        36 => 5563,
        37 => 5682,
        38 => 5798,
        39 => 5911,
        40 => 6021,
        41 => 6128,
        42 => 6232,
        43 => 6335,
        44 => 6435,
        45 => 6532,
        46 => 6628,
        47 => 6721,
        48 => 6812,
        49 => 6902,
        50 => 6990,
        51 => 7076,
        52 => 7160,
        53 => 7243,
        54 => 7324,
        55 => 7404,
        56 => 7482,
        57 => 7559,
        58 => 7634,
        59 => 7709,
        60 => 7782,
        61 => 7853,
        62 => 7924,
        63 => 7993,
        64 => 8062,
        65 => 8129,
        66 => 8195,
        67 => 8261,
        68 => 8325,
        69 => 8388,
        70 => 8451,
        71 => 8513,
        72 => 8573,
        73 => 8633,
        74 => 8692,
        75 => 8751,
        76 => 8808,
        77 => 8865,
        78 => 8921,
        79 => 8976,
        80 => 9031,
        81 => 9085,
        82 => 9138,
        83 => 9191,
        84 => 9243,
        85 => 9294,
        86 => 9345,
        87 => 9395,
        88 => 9445,
        89 => 9494,
        90 => 9542,
        91 => 9590,
        92 => 9638,
        93 => 9685,
        94 => 9731,
        95 => 9777,
        96 => 9823,
        97 => 9868,
        98 => 9912,
        _ => 9956, // 99 or overflow
    };
    floor_log10 * 10_000 + frac_bps
}

#[derive(Accounts)]
pub struct CalculateTrustScore<'info> {
    // permissionless
    #[account(
        mut,
        seeds = [b"agent-identity" as &[u8], agent_identity.authority.as_ref()],
        bump = agent_identity.bump,
    )]
    pub agent_identity: Account<'info, AgentIdentity>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
}
