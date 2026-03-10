use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, CloseAccount, Mint, Token, TokenAccount, Transfer};

/// Admin: Set up the bond vault token account (one-time).
/// Must be called after initialize, before any agent can register.
pub fn handler_setup_bond_vault(ctx: Context<SetupBondVault>) -> Result<()> {
    msg!(
        "Bond vault initialized at {}. Mint: {}",
        ctx.accounts.bond_vault.key(),
        ctx.accounts.sworn_mint.key()
    );
    Ok(())
}

/// Admin: Update the SWORN mint address in ProtocolConfig.
/// Used when migrating from v1 mint (no metadata) to v2 mint (with Metaplex metadata).
pub fn handler_update_sworn_mint(ctx: Context<UpdateSwornMint>) -> Result<()> {
    let config = &mut ctx.accounts.protocol_config;
    let old_mint = config.sworn_mint;
    config.sworn_mint = ctx.accounts.new_sworn_mint.key();

    msg!(
        "SWORN mint updated: {} -> {}",
        old_mint,
        config.sworn_mint
    );
    Ok(())
}

#[derive(Accounts)]
pub struct SetupBondVault<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
        constraint = protocol_config.admin == admin.key(),
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    /// The SWORN token mint referenced by protocol config
    #[account(
        constraint = sworn_mint.key() == protocol_config.sworn_mint,
    )]
    pub sworn_mint: Account<'info, Mint>,

    /// Bond vault PDA — initialized here as a token account
    #[account(
        init,
        payer = admin,
        token::mint = sworn_mint,
        token::authority = vault_authority,
        seeds = [b"bond-vault"],
        bump,
    )]
    pub bond_vault: Account<'info, TokenAccount>,

    /// CHECK: PDA authority for the bond vault
    #[account(
        seeds = [b"pool-authority"],
        bump,
    )]
    pub vault_authority: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateSwornMint<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
        constraint = protocol_config.admin == admin.key(),
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    /// The new SWORN mint to set
    pub new_sworn_mint: Account<'info, Mint>,
}

/// Admin: Migrate bond vault from old mint to new mint.
/// Steps: 1) Transfer all tokens from old vault to admin ATA
///        2) Close old vault (rent returned to admin)
///        3) Create new vault with new mint
///        4) Update protocol_config.sworn_mint
/// For devnet: bonds are effectively "forgiven" during migration.
pub fn handler_migrate_bond_vault(ctx: Context<MigrateBondVault>) -> Result<()> {
    let config = &mut ctx.accounts.protocol_config;
    let old_mint = config.sworn_mint;
    let new_mint = ctx.accounts.new_sworn_mint.key();

    // PDA signer seeds for pool-authority
    let pool_auth_bump = ctx.bumps.vault_authority;
    let pool_seeds: &[&[u8]] = &[b"pool-authority", &[pool_auth_bump]];
    let signer_seeds = &[pool_seeds];

    // Step 1: Transfer all tokens from old vault to admin's old-mint ATA
    let old_balance = ctx.accounts.old_bond_vault.amount;
    if old_balance > 0 {
        let transfer_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.old_bond_vault.to_account_info(),
                to: ctx.accounts.admin_old_token_account.to_account_info(),
                authority: ctx.accounts.vault_authority.to_account_info(),
            },
            signer_seeds,
        );
        token::transfer(transfer_ctx, old_balance)?;
        msg!("Transferred {} old tokens to admin", old_balance);
    }

    // Step 2: Close old vault (rent SOL returned to admin)
    let close_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        CloseAccount {
            account: ctx.accounts.old_bond_vault.to_account_info(),
            destination: ctx.accounts.admin.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        },
        signer_seeds,
    );
    token::close_account(close_ctx)?;
    msg!("Old bond vault closed");

    // Step 3: New bond vault is initialized via Anchor `init` constraint
    // (see new_bond_vault in the accounts struct)

    // Step 4: Update config
    config.sworn_mint = new_mint;
    msg!("SWORN mint migrated: {} -> {}", old_mint, new_mint);

    Ok(())
}

#[derive(Accounts)]
pub struct MigrateBondVault<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
        constraint = protocol_config.admin == admin.key(),
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    /// Old bond vault (will be closed)
    #[account(
        mut,
        seeds = [b"bond-vault"],
        bump,
        token::authority = vault_authority,
    )]
    pub old_bond_vault: Account<'info, TokenAccount>,

    /// Admin's token account for the OLD mint (receives drained tokens)
    #[account(
        mut,
        constraint = admin_old_token_account.owner == admin.key(),
        constraint = admin_old_token_account.mint == protocol_config.sworn_mint,
    )]
    pub admin_old_token_account: Account<'info, TokenAccount>,

    /// The new SWORN mint (v2 with metadata)
    pub new_sworn_mint: Account<'info, Mint>,

    /// New bond vault PDA — initialized with new mint.
    /// Uses different seeds ("bond-vault-v2") since old PDA can't be reused in same TX.
    #[account(
        init,
        payer = admin,
        token::mint = new_sworn_mint,
        token::authority = vault_authority,
        seeds = [b"bond-vault-v2"],
        bump,
    )]
    pub new_bond_vault: Account<'info, TokenAccount>,

    /// CHECK: PDA authority for both vaults
    #[account(
        seeds = [b"pool-authority"],
        bump,
    )]
    pub vault_authority: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

/// Admin: Migrate AgentIdentity v1 (95 bytes) to v2 (123 bytes).
///
/// v1 layout (87 data bytes after 8-byte discriminator):
///   [8..40]  authority (32)
///   [40..48] identity_bond (8)
///   [48..56] registered_at (8)
///   [56]     matured (1)
///   [57..59] trust_score (2)
///   [59..67] tasks_completed (8)
///   [67..75] volume_processed (8)
///   [75..79] disputes_lost (4)
///   [79..83] disputes_won (4)
///   [83..87] tasks_abandoned (4)
///   [87..91] fraud_flags (4)
///   [91..93] sponsor_bonus (2)
///   [93]     banned (1)
///   [94]     bump (1)
///
/// v2 layout inserts 28 bytes in the middle:
///   [75..83]   volume_sol (8) = 0
///   [99..103]  total_deliveries (4) = 0
///   [103..107] corrections_received (4) = 0
///   [107..111] active_contracts (4) = 0
///   [111..119] last_task_completed_at (8) = 0
/// Old fields [75..95] shift to [83..99] and [119..123] accordingly.
pub fn handler_migrate_agent_identity(ctx: Context<MigrateAgentIdentity>) -> Result<()> {
    use crate::errors::TrustError;

    let account_info = ctx.accounts.agent_identity.to_account_info();

    // Check owner is this program
    require!(
        account_info.owner == ctx.program_id,
        TrustError::UnauthorizedAdmin
    );

    let old_len = account_info.data_len();
    let new_len: usize = 123; // 8 discriminator + 115 data

    // Already migrated — idempotent skip
    if old_len == new_len {
        msg!("AgentIdentity already at v2 size (123), skipping");
        return Ok(());
    }

    require!(old_len == 95, TrustError::InvalidAccountSize);

    // Top up rent if needed
    let rent = Rent::get()?;
    let new_min_balance = rent.minimum_balance(new_len);
    let old_balance = account_info.lamports();
    if old_balance < new_min_balance {
        let diff = new_min_balance - old_balance;
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.admin.to_account_info(),
                    to: account_info.clone(),
                },
            ),
            diff,
        )?;
    }

    // Save bytes that must be relocated (positions relative to start of account data)
    //   old[75..91] = disputes_lost + disputes_won + tasks_abandoned + fraud_flags (16 bytes)
    //   old[91..95] = sponsor_bonus + banned + bump (4 bytes)
    let disputes_to_fraud: [u8; 16];
    let tail: [u8; 4];
    {
        let data = account_info.try_borrow_data()?;
        disputes_to_fraud = data[75..91].try_into().unwrap();
        tail = data[91..95].try_into().unwrap();
    }

    // Realloc: bytes 0..95 preserved, bytes 95..123 zeroed
    account_info.realloc(new_len, false)?;

    {
        let mut data = account_info.try_borrow_mut_data()?;
        // 1. Move tail (sponsor_bonus + banned + bump) to 119..123
        data[119..123].copy_from_slice(&tail);
        // 2. Move disputes_lost..fraud_flags to 83..99 (shifts +8 due to volume_sol insertion)
        data[83..99].copy_from_slice(&disputes_to_fraud);
        // 3. Zero volume_sol slot at 75..83 (old data at these offsets is now garbage)
        data[75..83].fill(0);
        // 4. Zero new fields total_deliveries..last_task_completed_at at 99..119
        //    (bytes 99..119 partially have old data after the shift; clear all)
        data[99..119].fill(0);
    }

    msg!("AgentIdentity migrated to v2: {} -> {} bytes", old_len, new_len);
    Ok(())
}

#[derive(Accounts)]
pub struct MigrateAgentIdentity<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
        constraint = protocol_config.admin == admin.key(),
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    /// CHECK: Raw account — validated via owner == program_id check in handler.
    /// Cannot use Account<AgentIdentity> because old accounts have 95 bytes
    /// and would fail Anchor deserialization against the new 123-byte struct.
    #[account(mut)]
    pub agent_identity: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

/// Admin: Force-mature an agent (devnet testing only).
/// In production, maturation happens after 30 days automatically.
pub fn handler_force_mature(ctx: Context<ForceMatureAgent>) -> Result<()> {
    let agent = &mut ctx.accounts.agent_identity;
    agent.matured = true;
    msg!("Agent {} force-matured by admin", agent.authority);
    Ok(())
}

#[derive(Accounts)]
pub struct ForceMatureAgent<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
        constraint = protocol_config.admin == admin.key(),
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(mut)]
    pub agent_identity: Account<'info, AgentIdentity>,
}

/// Admin: Update configurable ProtocolConfig parameters.
/// Whitepaper §8: Phase 0-2 admin-controlled; Phase 3+ via DAO.
/// Each field is Option<T> — pass None to leave unchanged.
pub fn handler_update_config(ctx: Context<UpdateConfig>, params: UpdateConfigParams) -> Result<()> {
    let config = &mut ctx.accounts.protocol_config;
    if let Some(v) = params.min_identity_bond { config.min_identity_bond = v; }
    if let Some(v) = params.max_identity_bond { config.max_identity_bond = v; }
    if let Some(v) = params.maturation_period  { config.maturation_period = v; }
    msg!(
        "ProtocolConfig updated: min_bond={} max_bond={} maturation_period={}",
        config.min_identity_bond, config.max_identity_bond, config.maturation_period
    );
    Ok(())
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct UpdateConfigParams {
    /// New min identity bond in SWORN lamports. None = no change.
    pub min_identity_bond: Option<u64>,
    /// New max identity bond in SWORN lamports. None = no change.
    pub max_identity_bond: Option<u64>,
    /// New maturation period in seconds. None = no change.
    pub maturation_period: Option<i64>,
}

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [b"protocol-config"],
        bump = protocol_config.bump,
        constraint = protocol_config.admin == admin.key(),
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
}
