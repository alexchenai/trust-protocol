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
