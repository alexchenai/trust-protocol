#![allow(clippy::result_large_err)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::needless_pass_by_ref_mut)]
#![allow(unexpected_cfgs)]
#![allow(deprecated)]
use anchor_lang::prelude::*;

pub mod errors;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("CSBAc1SiMALr4rnuCoB17BsddzthB4RAhjibGvyt6p6S");

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
    /// Creates DID: did:trust:{pubkey}. 14-day maturation period (+ 5 tasks).
    pub fn register_agent(ctx: Context<RegisterAgent>, bond_amount: u64) -> Result<()> {
        identity::handler_register(ctx, bond_amount)
    }

    /// Sponsor an agent (established agent vouches for newcomer).
    /// Sponsor must have TrustScore >= 50 and matured identity.
    pub fn sponsor_agent(ctx: Context<SponsorAgent>, bonus_points: u16) -> Result<()> {
        identity::handler_sponsor(ctx, bonus_points)
    }

    /// Permissionless maturation check: marks identity as matured if 14 days + 5 tasks.
    /// Whitepaper Section 10.2: agents self-mature without oracle dependency.
    pub fn check_maturation(ctx: Context<CheckMaturation>) -> Result<()> {
        identity::handler_check_maturation(ctx)
    }

    /// Compute and store TrustScore on-chain using the whitepaper formula.
    /// Permissionless: any caller triggers recalculation.
    /// sol_to_sworn_rate: SOL lamport to SWORN lamport exchange rate for volume normalization.
    pub fn calculate_trust_score(ctx: Context<CalculateTrustScore>, sol_to_sworn_rate: u64) -> Result<()> {
        identity::handler_calculate_trust_score(ctx, sol_to_sworn_rate)
    }

    // === Hibernation (Whitepaper §8.6: Declared hibernation for seasonal agents) ===

    /// Declare hibernation: reduced decay (0.5/month) for up to 12 months.
    /// Must be called BEFORE going inactive. Requires 5-task cooldown between hibernations.
    pub fn hibernate_agent(ctx: Context<HibernateAgent>, duration_months: u8) -> Result<()> {
        identity::handler_hibernate(ctx, duration_months)
    }

    /// Wake from hibernation early (or can be called by anyone after expiry via expire_hibernation).
    pub fn wake_agent(ctx: Context<WakeAgent>) -> Result<()> {
        identity::handler_wake(ctx)
    }

    /// Permissionless: expire hibernation once max duration has passed.
    /// Resets hibernation state so standard decay resumes automatically.
    pub fn expire_hibernation(ctx: Context<WakeAgent>) -> Result<()> {
        identity::handler_expire_hibernation(ctx)
    }

    // === Contract Lifecycle (Whitepaper Section 3: Dynamic Staking) ===

    /// Create contract. Provider stakes based on TrustScore.
    /// stake = value * factor_stake(score). Score 0 = 100%, Score 100 = 5%.
    /// currency: 0=SWORN (default), 1=SOL. Whitepaper §11.8b.
    pub fn create_contract(ctx: Context<CreateContract>, value: u64, currency: u8) -> Result<()> {
        contract::handler_create(ctx, value, currency)
    }

    /// Propose a contract. Only requester signs; deposits escrow.
    /// Provider must call accept_proposal to activate.
    /// currency: 0=SWORN (SPL token), 1=SOL (native lamports). Whitepaper Section 11.8b.
    pub fn propose_contract(ctx: Context<ProposeContract>, value: u64, expiry_seconds: u64, currency: u8) -> Result<()> {
        proposal::handler_propose(ctx, value, expiry_seconds, currency)
    }

    /// Provider accepts a proposed contract by depositing stake.
    /// Transitions contract from Proposed to Active.
    pub fn accept_proposal(ctx: Context<AcceptProposal>) -> Result<()> {
        proposal::handler_accept_proposal(ctx)
    }

    /// Cancel an expired proposal. Requester reclaims escrowed funds.
    pub fn cancel_proposal(ctx: Context<CancelProposal>) -> Result<()> {
        proposal::handler_cancel_proposal(ctx)
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

    /// Permissionless timeout: resolves abandoned contract after 72h delivery deadline.
    /// Provider's stake is confiscated (60% insurance, 25% requester bonus, 15% burn).
    /// Requester receives full escrow + 25% stake bonus.
    /// Whitepaper Section 3/5: failed delivery = tasks_abandoned++, active_contracts--.
    pub fn timeout_contract(ctx: Context<TimeoutContract>) -> Result<()> {
        contract::handler_timeout(ctx)
    }

    /// Permissionless: auto-accept Delivered contract if requester ignores it for 72h.
    /// Whitepaper Section 3.5: protects provider from requester ghosting. GAP-11.
    pub fn timeout_delivery(ctx: Context<TimeoutDelivery>) -> Result<()> {
        contract::handler_timeout_delivery(ctx)
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

    /// Escalate dispute to next level (Level 1→2→3 only, no stake required).
    pub fn escalate_dispute(ctx: Context<EscalateDispute>) -> Result<()> {
        dispute::handler_escalate(ctx)
    }

    /// Escalate a Level 3 (PublicJury) dispute to Level 4 (Appeal).
    /// Whitepaper Section 5.4: escalating party deposits 50% of contract value (double-or-nothing).
    /// On win: deposit returned. On loss: deposit confiscated (60% insurance, 25% winner, 15% burn).
    /// Records depositor in dispute.initiator for correct refund/confiscation in resolve_dispute.
    pub fn escalate_to_appeal(ctx: Context<EscalateToAppeal>) -> Result<()> {
        dispute::handler_escalate_to_appeal(ctx)
    }

    /// Jury vote (Public Jury / Appeal only, TrustScore > 70 required).
    pub fn jury_vote(ctx: Context<JuryVote>, vote_for_provider: bool) -> Result<()> {
        dispute::handler_vote(ctx, vote_for_provider)
    }

    /// Resolve dispute. Confiscated stakes: 15% burned, 60% insurance, 25% winner.
    pub fn resolve_dispute(ctx: Context<ResolveDispute>, provider_wins: bool) -> Result<()> {
        dispute::handler_resolve(ctx, provider_wins)
    }

    /// Provider re-delivers corrected work during Level 1 dispute.
    /// Max 3 corrections before auto-escalation to Level 2 (PrivateRounds).
    pub fn redeliver_in_dispute(ctx: Context<RedeliverInDispute>, output_hash: [u8; 32], arweave_tx: String) -> Result<()> {
        dispute::handler_redeliver(ctx, output_hash, arweave_tx)
    }

    /// Requester accepts provider's correction during dispute.
    /// Resolves dispute + completes contract (releases payment).
    pub fn accept_correction(ctx: Context<AcceptCorrection>) -> Result<()> {
        dispute::handler_accept_correction(ctx)
    }

    /// Migrate old Dispute accounts to include corrections_count field.
    /// Reallocs from 169 to 170 bytes. Call once per old dispute.
    pub fn migrate_dispute_size(ctx: Context<MigrateDisputeSize>) -> Result<()> {
        dispute::handler_migrate_dispute(ctx)
    }

    /// Migrate Dispute accounts to include appeal_stake field (u64).
    /// Reallocs from 170 to 178 bytes. The new 8 bytes are zero-filled (appeal_stake=0).
    /// appeal_stake is populated by escalate_to_appeal when Level 4 is reached.
    /// Whitepaper Section 5.4: double-or-nothing stake deposit by escalating party.
    /// Call once per old dispute. Idempotent for already-migrated accounts.
    pub fn migrate_dispute_appeal_stake(ctx: Context<MigrateDisputeAppealStake>) -> Result<()> {
        dispute::handler_migrate_dispute_appeal_stake(ctx)
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

    // === Admin Operations ===

    /// Set up bond vault token account (one-time, after initialize).
    /// Required before any agent can register.
    pub fn setup_bond_vault(ctx: Context<SetupBondVault>) -> Result<()> {
        admin::handler_setup_bond_vault(ctx)
    }

    /// Update SWORN mint address (v1 -> v2 migration).
    /// Admin only (Phase 0-2 governance).
    pub fn update_sworn_mint(ctx: Context<UpdateSwornMint>) -> Result<()> {
        admin::handler_update_sworn_mint(ctx)
    }

    /// Migrate bond vault: close old (v1) vault, create new (v2) vault, update config.
    /// Drains old tokens to admin, closes account, inits new vault with new mint.
    pub fn migrate_bond_vault(ctx: Context<MigrateBondVault>) -> Result<()> {
        admin::handler_migrate_bond_vault(ctx)
    }

    /// Force-mature an agent (devnet testing only).
    /// Bypasses maturation period for testing contract lifecycle.
    pub fn force_mature(ctx: Context<ForceMatureAgent>) -> Result<()> {
        admin::handler_force_mature(ctx)
    }

    /// Admin: update protocol config parameters (min_bond, maturation_period, etc.).
    /// Whitepaper §8: Phase 0-2 admin-controlled. GAP-1/GAP-2 fix.
    pub fn update_config(ctx: Context<UpdateConfig>, params: UpdateConfigParams) -> Result<()> {
        admin::handler_update_config(ctx, params)
    }

    /// Migrate AgentIdentity v1 (95 bytes) to v2 (123 bytes).
    /// Inserts volume_sol, total_deliveries, corrections_received, active_contracts,
    /// last_task_completed_at fields (all zero) at correct Borsh offsets.
    /// Idempotent: already-migrated accounts (123 bytes) are skipped.
    pub fn migrate_agent_identity(ctx: Context<MigrateAgentIdentity>) -> Result<()> {
        admin::handler_migrate_agent_identity(ctx)
    }

    /// Migrate ProtocolConfig from v1 (133 bytes) to v2 (146 bytes).
    /// Adds protocol_fee_sworn_bps, protocol_fee_sol_bps, max_corrections,
    /// deadline_validation fields with whitepaper defaults.
    /// Idempotent: already-migrated accounts (146 bytes) are skipped.
    pub fn migrate_protocol_config(ctx: Context<MigrateProtocolConfig>) -> Result<()> {
        admin::handler_migrate_protocol_config(ctx)
    }
}
