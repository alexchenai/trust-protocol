use anchor_lang::prelude::*;
use crate::errors::TrustError;
use crate::state::ProtocolConfig;

// =============================================================================
// V3 Migration: realloc accounts that grew in commits ae1d481, 8171fd6, 149596e.
//
// MIGRATION ORDER (critical):
//   1. migrate_config_v3    — ProtocolConfig v2 (146) -> v3 (186)
//   2. migrate_insurance_v2 — InsurancePool v1 (61) -> v2 (69)
//   3. migrate_agent_v3     — AgentIdentity v2 (123) -> v3 (146), per agent
//   4. migrate_dispute_v2   — Dispute v1 (218) -> v2 (219), per dispute
//
// ProtocolConfig must be migrated first because other contexts use
// Account<ProtocolConfig> for admin verification (requires current schema).
// =============================================================================

// ---------------------------------------------------------------------------
// AgentIdentity  v2 (123 bytes) → v3 (146 bytes)
//
// v2 layout (123 = 8 disc + 115 data):
//   [8..122]  fields through banned
//   [122]     bump
//
// v3 inserts 23 bytes between banned and bump:
//   [122]     is_hibernating (1) = 0
//   [123..131] hibernation_started_at (8) = 0
//   [131..139] hibernation_ends_at (8) = 0
//   [139..143] tasks_since_last_hibernation (4) = 0
//   [143..145] dispute_friction_total (2) = 0
//   [145]     bump (relocated from [122])
// ---------------------------------------------------------------------------
pub fn handler_migrate_agent_v3(ctx: Context<MigrateAgentV3>) -> Result<()> {
    let account_info = ctx.accounts.agent_identity.to_account_info();

    require!(
        account_info.owner == ctx.program_id,
        TrustError::UnauthorizedAdmin
    );

    let current_len = account_info.data_len();
    let v2_len: usize = 123;
    let v3_len: usize = 146;

    if current_len >= v3_len {
        msg!("AgentIdentity already at v3 ({} bytes), skipping", current_len);
        return Ok(());
    }

    require!(current_len == v2_len, TrustError::InvalidAccountSize);

    // Save bump from v2 position [122]
    let bump_byte: u8;
    {
        let data = account_info.try_borrow_data()?;
        bump_byte = data[122];
    }

    // Realloc to v3 size
    account_info.realloc(v3_len, false)?;

    // Transfer rent delta from admin
    let rent = Rent::get()?;
    let delta = rent.minimum_balance(v3_len).saturating_sub(rent.minimum_balance(v2_len));
    if delta > 0 {
        let ix = anchor_lang::solana_program::system_instruction::transfer(
            ctx.accounts.admin.key,
            account_info.key,
            delta,
        );
        anchor_lang::solana_program::program::invoke(
            &ix,
            &[
                ctx.accounts.admin.to_account_info(),
                account_info.clone(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
    }

    // Write new fields (all default to 0) and relocate bump
    {
        let mut data = account_info.try_borrow_mut_data()?;
        data[122] = 0u8;   // is_hibernating = false
        data[123..131].copy_from_slice(&0i64.to_le_bytes());  // hibernation_started_at
        data[131..139].copy_from_slice(&0i64.to_le_bytes());  // hibernation_ends_at
        data[139..143].copy_from_slice(&0u32.to_le_bytes());  // tasks_since_last_hibernation
        data[143..145].copy_from_slice(&0u16.to_le_bytes());  // dispute_friction_total
        data[145] = bump_byte;  // bump relocated
    }

    msg!("AgentIdentity migrated v2->v3: {} -> {} bytes", v2_len, v3_len);
    Ok(())
}

#[derive(Accounts)]
pub struct MigrateAgentV3<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    /// Protocol config — must be migrated to v3 first so Anchor can deserialize it.
    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
        constraint = protocol_config.admin == admin.key() @ TrustError::UnauthorizedAdmin,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    /// CHECK: Raw account — validated via owner check in handler.
    #[account(mut)]
    pub agent_identity: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

// ---------------------------------------------------------------------------
// ProtocolConfig  v2 (146 bytes) → v3 (186 bytes)
//
// v2 layout ends at:
//   [137..145] deadline_validation (8)
//   [145]      bump
//
// v3 inserts 40 bytes between deadline_validation and bump:
//   [145..153] total_protocol_tasks (8) = 0
//   [153..161] total_work_rewards_emitted (8) = 0
//   [161..169] base_work_reward (8) = 10_000_000
//   [169..177] halving_interval (8) = 50_000
//   [177..185] max_work_rewards (8) = 1_000_000_000_000
//   [185]      bump (relocated from [145])
// ---------------------------------------------------------------------------
pub fn handler_migrate_config_v3(ctx: Context<MigrateConfigV3>) -> Result<()> {
    let account_info = ctx.accounts.protocol_config.to_account_info();

    require!(
        account_info.owner == ctx.program_id,
        TrustError::UnauthorizedAdmin
    );

    let current_len = account_info.data_len();
    let v2_len: usize = 146;
    let v3_len: usize = 186;

    if current_len >= v3_len {
        msg!("ProtocolConfig already at v3 ({} bytes), skipping", current_len);
        return Ok(());
    }

    require!(current_len == v2_len, TrustError::InvalidAccountSize);

    // Validate admin from raw bytes (can't use Account<ProtocolConfig> — still at v2)
    {
        let data = account_info.try_borrow_data()?;
        let stored_admin = Pubkey::try_from(&data[8..40]).unwrap();
        require!(
            stored_admin == ctx.accounts.admin.key(),
            TrustError::UnauthorizedAdmin
        );
    }

    // Validate PDA
    let (expected_pda, _) = Pubkey::find_program_address(&[b"protocol-config"], ctx.program_id);
    require!(
        account_info.key() == expected_pda,
        TrustError::UnauthorizedAdmin
    );

    // Save bump from v2 position [145]
    let bump_byte: u8;
    {
        let data = account_info.try_borrow_data()?;
        bump_byte = data[145];
    }

    // Realloc
    account_info.realloc(v3_len, false)?;

    let rent = Rent::get()?;
    let delta = rent.minimum_balance(v3_len).saturating_sub(rent.minimum_balance(v2_len));
    if delta > 0 {
        let ix = anchor_lang::solana_program::system_instruction::transfer(
            ctx.accounts.admin.key,
            account_info.key,
            delta,
        );
        anchor_lang::solana_program::program::invoke(
            &ix,
            &[
                ctx.accounts.admin.to_account_info(),
                account_info.clone(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
    }

    {
        let mut data = account_info.try_borrow_mut_data()?;
        data[145..153].copy_from_slice(&0u64.to_le_bytes());               // total_protocol_tasks
        data[153..161].copy_from_slice(&0u64.to_le_bytes());               // total_work_rewards_emitted
        data[161..169].copy_from_slice(&10_000_000u64.to_le_bytes());      // base_work_reward (10 SWORN)
        data[169..177].copy_from_slice(&50_000u64.to_le_bytes());          // halving_interval
        data[177..185].copy_from_slice(&1_000_000_000_000u64.to_le_bytes()); // max_work_rewards (1M SWORN)
        data[185] = bump_byte;  // bump relocated
    }

    msg!("ProtocolConfig migrated v2->v3: {} -> {} bytes", v2_len, v3_len);
    Ok(())
}

#[derive(Accounts)]
pub struct MigrateConfigV3<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    /// CHECK: Raw account — validated via owner + admin + PDA check in handler.
    #[account(mut)]
    pub protocol_config: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

// ---------------------------------------------------------------------------
// InsurancePool  v1 (61 bytes) → v2 (69 bytes)
//
// v1 layout (61 = 8 disc + 53 data):
//   [8..16]  total_balance (8)
//   [16..24] total_claims_paid (8)
//   [24..28] active_claims (4)
//   [28..60] authority (32)
//   [60]     bump (1)
//
// v2 inserts 8 bytes:
//   [60..68] total_active_exposure (8) = 0
//   [68]     bump (relocated from [60])
// ---------------------------------------------------------------------------
pub fn handler_migrate_insurance_pool(ctx: Context<MigrateInsurancePool>) -> Result<()> {
    let account_info = ctx.accounts.insurance_pool.to_account_info();

    require!(
        account_info.owner == ctx.program_id,
        TrustError::UnauthorizedAdmin
    );

    let current_len = account_info.data_len();
    let v1_len: usize = 61;
    let v2_len: usize = 69;

    if current_len >= v2_len {
        msg!("InsurancePool already at v2 ({} bytes), skipping", current_len);
        return Ok(());
    }

    require!(current_len == v1_len, TrustError::InvalidAccountSize);

    let bump_byte: u8;
    {
        let data = account_info.try_borrow_data()?;
        bump_byte = data[60];
    }

    account_info.realloc(v2_len, false)?;

    let rent = Rent::get()?;
    let delta = rent.minimum_balance(v2_len).saturating_sub(rent.minimum_balance(v1_len));
    if delta > 0 {
        let ix = anchor_lang::solana_program::system_instruction::transfer(
            ctx.accounts.admin.key,
            account_info.key,
            delta,
        );
        anchor_lang::solana_program::program::invoke(
            &ix,
            &[
                ctx.accounts.admin.to_account_info(),
                account_info.clone(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
    }

    {
        let mut data = account_info.try_borrow_mut_data()?;
        data[60..68].copy_from_slice(&0u64.to_le_bytes());  // total_active_exposure = 0
        data[68] = bump_byte;  // bump relocated
    }

    msg!("InsurancePool migrated v1->v2: {} -> {} bytes", v1_len, v2_len);
    Ok(())
}

#[derive(Accounts)]
pub struct MigrateInsurancePool<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    /// Protocol config — must be migrated to v3 first.
    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
        constraint = protocol_config.admin == admin.key() @ TrustError::UnauthorizedAdmin,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    /// CHECK: Raw account — validated via owner check in handler.
    #[account(mut)]
    pub insurance_pool: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

// ---------------------------------------------------------------------------
// Dispute  v1 (218 bytes) → v2 (219 bytes)
//
// v1 layout: fields through corrections_count at [201], then:
//   [202..210] appeal_stake (8)
//   [210..218] arbitration_fee (8)
//
// v2 inserts private_rounds_count (1 byte) at [202]:
//   [202]      private_rounds_count (1) = 0
//   [203..211] appeal_stake (shifted +1)
//   [211..219] arbitration_fee (shifted +1)
// ---------------------------------------------------------------------------
pub fn handler_migrate_dispute_v2(ctx: Context<MigrateDispute>) -> Result<()> {
    let account_info = ctx.accounts.dispute.to_account_info();

    require!(
        account_info.owner == ctx.program_id,
        TrustError::UnauthorizedAdmin
    );

    let current_len = account_info.data_len();
    let v1_len: usize = 218;
    let v2_len: usize = 219;

    if current_len >= v2_len {
        msg!("Dispute already at v2 ({} bytes), skipping", current_len);
        return Ok(());
    }

    require!(current_len == v1_len, TrustError::InvalidAccountSize);

    // Save appeal_stake + arbitration_fee (16 bytes at [202..218])
    let mut tail_16 = [0u8; 16];
    {
        let data = account_info.try_borrow_data()?;
        tail_16.copy_from_slice(&data[202..218]);
    }

    account_info.realloc(v2_len, false)?;

    let rent = Rent::get()?;
    let delta = rent.minimum_balance(v2_len).saturating_sub(rent.minimum_balance(v1_len));
    if delta > 0 {
        let ix = anchor_lang::solana_program::system_instruction::transfer(
            ctx.accounts.admin.key,
            account_info.key,
            delta,
        );
        anchor_lang::solana_program::program::invoke(
            &ix,
            &[
                ctx.accounts.admin.to_account_info(),
                account_info.clone(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
    }

    {
        let mut data = account_info.try_borrow_mut_data()?;
        data[202] = 0u8;  // private_rounds_count = 0
        data[203..219].copy_from_slice(&tail_16);  // appeal_stake + arbitration_fee shifted
    }

    msg!("Dispute migrated v1->v2: {} -> {} bytes", v1_len, v2_len);
    Ok(())
}

#[derive(Accounts)]
pub struct MigrateDispute<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    /// Protocol config — must be migrated to v3 first.
    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
        constraint = protocol_config.admin == admin.key() @ TrustError::UnauthorizedAdmin,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    /// CHECK: Raw account — validated via owner check in handler.
    #[account(mut)]
    pub dispute: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}
