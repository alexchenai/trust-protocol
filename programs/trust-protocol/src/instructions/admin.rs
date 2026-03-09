use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};

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
