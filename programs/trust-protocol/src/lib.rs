#![allow(clippy::result_large_err)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::needless_pass_by_ref_mut)]
#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;

pub mod errors;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("TRSTpRoToCoL1111111111111111111111111111111");

#[program]
pub mod trust_protocol {
    use super::*;

    // === Protocol Initialization ===

    /// Initialize the Trust Protocol with SWORN token and global config.
    /// Whitepaper Section 8: Governance Phase 0 (founding team).
    pub fn initialize(ctx: Context<Initialize>, params: InitializeParams) -> Result<()> {
        initialize::handler(ctx, params)
    }

    // === Identity Management (Whitepaper Section 2) ===

    /// Register a new agent with soulbound identity bond (2-5 SWORN).
    /// Creates DID: did:trust:{pubkey}. 30-day maturation period.
    pub fn register_agent(ctx: Context<RegisterAgent>, bond_amount: u64) -> Result<()> {
        identity::handler_register(ctx, bond_amount)
    }

    /// Sponsor an agent (established agent vouches for newcomer).
    /// Sponsor must have TrustScore >= 50 and matured identity.
    pub fn sponsor_agent(ctx: Context<SponsorAgent>, bonus_points: u16) -> Result<()> {
        identity::handler_sponsor(ctx, bonus_points)
    }

    // === Contract Lifecycle (Whitepaper Section 3: Dynamic Staking) ===

    /// Create contract. Provider stakes based on TrustScore.
    /// stake = value * factor_stake(score). Score 0 = 100%, Score 100 = 5%.
    pub fn create_contract(ctx: Context<CreateContract>, value: u64) -> Result<()> {
        contract::handler_create(ctx, value)
    }

    /// Provider submits deliverable with Proof of Execution (PoE).
    /// Whitepaper Section 1: immutable PoE with input/output hashes.
    pub fn deliver_contract(
        ctx: Context<DeliverContract>,
        output_hash: [u8; 32],
        arweave_tx: String,
    ) -> Result<()> {
        contract::handler_deliver(ctx, output_hash, arweave_tx)
    }

    /// Requester accepts deliverable. Releases payment + returns stake.
    pub fn accept_contract(ctx: Context<AcceptContract>) -> Result<()> {
        contract::handler_accept(ctx)
    }

    // === Dispute Resolution (Whitepaper Section 5) ===
    // 4 levels: Direct Correction -> Private Rounds -> Public Jury -> Appeal

    /// Initiate dispute on a contract.
    pub fn initiate_dispute(ctx: Context<InitiateDispute>, evidence_hash: [u8; 32]) -> Result<()> {
        dispute::handler_initiate(ctx, evidence_hash)
    }

    /// Provider responds to dispute with correction/counter-evidence.
    pub fn respond_dispute(ctx: Context<RespondDispute>, response_hash: [u8; 32]) -> Result<()> {
        dispute::handler_respond(ctx, response_hash)
    }

    /// Escalate dispute to next level.
    pub fn escalate_dispute(ctx: Context<EscalateDispute>) -> Result<()> {
        dispute::handler_escalate(ctx)
    }

    /// Jury vote (Public Jury / Appeal only, TrustScore > 70 required).
    pub fn jury_vote(ctx: Context<JuryVote>, vote_for_provider: bool) -> Result<()> {
        dispute::handler_vote(ctx, vote_for_provider)
    }

    /// Resolve dispute. Confiscated stakes: 15% burned, 60% insurance, 25% winner.
    pub fn resolve_dispute(ctx: Context<ResolveDispute>, provider_wins: bool) -> Result<()> {
        dispute::handler_resolve(ctx, provider_wins)
    }

    // === Insurance Pool (Whitepaper Section 6) ===

    /// File retroactive claim within 90-day window. Max 80% of contract value.
    pub fn file_insurance_claim(
        ctx: Context<FileInsuranceClaim>,
        amount: u64,
        evidence_hash: [u8; 32],
    ) -> Result<()> {
        insurance::handler_file_claim(ctx, amount, evidence_hash)
    }

    /// Approve insurance claim (admin Phase 0-2, DAO Phase 3+).
    pub fn approve_insurance_claim(ctx: Context<ApproveInsuranceClaim>) -> Result<()> {
        insurance::handler_approve_claim(ctx)
    }

    /// Deny insurance claim. Collateral forfeited as anti-spam.
    pub fn deny_insurance_claim(ctx: Context<ApproveInsuranceClaim>) -> Result<()> {
        insurance::handler_deny_claim(ctx)
    }
}
