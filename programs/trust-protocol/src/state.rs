use anchor_lang::prelude::*;

/// Agent identity account - soulbound, non-transferable (Whitepaper Section 2: Identity Model)
/// Created on registration with a 2-5 SWORN identity bond.
/// DID format: did:trust:{pubkey}
#[account]
#[derive(InitSpace)]
pub struct AgentIdentity {
    /// The agent's wallet authority
    pub authority: Pubkey,
    /// SWORN tokens locked as identity bond (2-5 tokens, anti-Sybil)
    pub identity_bond: u64,
    /// Unix timestamp of registration
    pub registered_at: i64,
    /// Whether this identity has matured (30 days after registration)
    pub matured: bool,
    /// Current TrustScore (0-100, updated by oracle)
    pub trust_score: u16,
    /// Total tasks completed (logarithmic weight in TrustScore)
    pub tasks_completed: u64,
    /// Total transaction volume in SWORN (lamports)
    pub volume_processed: u64,
    /// Disputes lost count
    pub disputes_lost: u32,
    /// Disputes won count
    pub disputes_won: u32,
    /// Tasks abandoned count
    pub tasks_abandoned: u32,
    /// Fraud flags count
    pub fraud_flags: u32,
    /// Sponsor bonus points from established agents
    pub sponsor_bonus: u16,
    /// Whether identity is permanently banned (fraud)
    pub banned: bool,
    /// Bump seed for PDA derivation
    pub bump: u8,
}

/// Contract between two agents (Whitepaper Section 3: Dynamic Staking)
/// stake_required = contract_value * factor_stake(TrustScore)
/// factor_stake: TrustScore 0 => 100%, TrustScore 100 => 5%
#[account]
#[derive(InitSpace)]
pub struct Contract {
    /// Unique contract ID
    pub id: u64,
    /// The agent requesting work
    pub requester: Pubkey,
    /// The agent performing work
    pub provider: Pubkey,
    /// Contract value in SWORN lamports
    pub value: u64,
    /// Stake locked by provider
    pub provider_stake: u64,
    /// Stake locked by requester (for dispute anti-spam)
    pub requester_stake: u64,
    /// Contract status
    pub status: ContractStatus,
    /// Creation timestamp
    pub created_at: i64,
    /// Completion/resolution timestamp
    pub resolved_at: i64,
    /// Proof of Execution hash (SHA-256 of deliverable)
    pub poe_hash: [u8; 32],
    /// Arweave TX ID for full PoE data (43 bytes base64url)
    #[max_len(43)]
    pub poe_arweave_tx: String,
    /// Dispute level if in dispute (0 = no dispute)
    pub dispute_level: u8,
    /// Bump seed
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum ContractStatus {
    /// Contract created, awaiting provider stake
    Created,
    /// Both parties staked, work in progress
    Active,
    /// Provider submitted deliverable with PoE
    Delivered,
    /// Requester accepted, funds released
    Completed,
    /// In dispute resolution
    Disputed,
    /// Cancelled before work started
    Cancelled,
    /// Resolved via dispute (provider won)
    ResolvedProvider,
    /// Resolved via dispute (requester won)
    ResolvedRequester,
}

/// Proof of Execution record (Whitepaper Section 1: PoE)
/// Immutable record of task execution with input/output hashes
#[account]
#[derive(InitSpace)]
pub struct ProofOfExecution {
    /// Associated contract
    pub contract: Pubkey,
    /// Provider who executed
    pub provider: Pubkey,
    /// SHA-256 hash of input specification
    pub input_hash: [u8; 32],
    /// SHA-256 hash of output/deliverable
    pub output_hash: [u8; 32],
    /// Timestamp of submission
    pub submitted_at: i64,
    /// Whether validated by requester
    pub validated: bool,
    /// Arweave TX for full payload
    #[max_len(43)]
    pub arweave_tx: String,
    /// Bump seed
    pub bump: u8,
}

/// Dispute account (Whitepaper Section 5: Dispute Resolution)
/// 4 levels: Direct Correction -> Private Rounds -> Public Jury -> Appeal
#[account]
#[derive(InitSpace)]
pub struct Dispute {
    /// Associated contract
    pub contract: Pubkey,
    /// Who initiated the dispute
    pub initiator: Pubkey,
    /// Current dispute level
    pub level: DisputeLevel,
    /// Dispute status
    pub status: DisputeStatus,
    /// Evidence hash from initiator
    pub evidence_hash: [u8; 32],
    /// Response hash from respondent
    pub response_hash: [u8; 32],
    /// Jury votes for provider (Public Jury / Appeal only)
    pub votes_provider: u16,
    /// Jury votes for requester
    pub votes_requester: u16,
    /// Total jury members assigned
    pub jury_size: u16,
    /// Escalation deadline (unix timestamp)
    pub deadline: i64,
    /// Created at
    pub created_at: i64,
    /// Resolved at
    pub resolved_at: i64,
    /// Bump seed
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum DisputeLevel {
    /// Level 1: Direct correction by provider (90% of cases)
    DirectCorrection,
    /// Level 2: Private negotiation rounds (8%)
    PrivateRounds,
    /// Level 3: Public jury of high-rep agents, TrustScore > 70 (1.5%)
    PublicJury,
    /// Level 4: Appeal - double-or-nothing with larger jury (0.5%)
    Appeal,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum DisputeStatus {
    /// Awaiting response
    Open,
    /// Respondent submitted correction/response
    Responded,
    /// Jury voting in progress
    Voting,
    /// Resolved in favor of provider
    ResolvedProvider,
    /// Resolved in favor of requester
    ResolvedRequester,
    /// Escalated to next level
    Escalated,
}

/// Insurance Pool (Whitepaper Section 6)
/// Accumulates 60% of confiscated stakes. Enables 90-day retroactive claims.
#[account]
#[derive(InitSpace)]
pub struct InsurancePool {
    /// Total SWORN in the pool
    pub total_balance: u64,
    /// Total claims paid out
    pub total_claims_paid: u64,
    /// Number of active claims
    pub active_claims: u32,
    /// Authority (program PDA)
    pub authority: Pubkey,
    /// Bump seed
    pub bump: u8,
}

/// Retroactive claim against the Insurance Pool
#[account]
#[derive(InitSpace)]
pub struct InsuranceClaim {
    /// Claimant (requester who discovered defect)
    pub claimant: Pubkey,
    /// Original contract
    pub contract: Pubkey,
    /// Amount claimed (up to 80% of contract value)
    pub amount: u64,
    /// Anti-spam collateral deposited
    pub collateral: u64,
    /// Evidence hash
    pub evidence_hash: [u8; 32],
    /// Claim status
    pub status: ClaimStatus,
    /// Filed at timestamp
    pub filed_at: i64,
    /// Original contract completion timestamp (must be within 90 days)
    pub contract_completed_at: i64,
    /// Bump seed
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum ClaimStatus {
    Filed,
    UnderReview,
    Approved,
    Denied,
}

/// Protocol configuration (Whitepaper Section 8: Governance)
/// Phase 0-2: Centralized (founding team), Phase 3+: Decentralized (sqrt TrustScore voting)
#[account]
#[derive(InitSpace)]
pub struct ProtocolConfig {
    /// Protocol admin (founding team in Phase 0-2)
    pub admin: Pubkey,
    /// SWORN token mint
    pub sworn_mint: Pubkey,
    /// Minimum identity bond (2 SWORN in lamports)
    pub min_identity_bond: u64,
    /// Maximum identity bond (5 SWORN in lamports)
    pub max_identity_bond: u64,
    /// Identity maturation period in seconds (30 days = 2592000)
    pub maturation_period: i64,
    /// Minimum stake factor at TrustScore 100 (500 = 5.00%)
    pub min_stake_factor_bps: u16,
    /// Maximum stake factor at TrustScore 0 (10000 = 100%)
    pub max_stake_factor_bps: u16,
    /// Burn rate of confiscated tokens (1500 = 15%)
    pub burn_rate_bps: u16,
    /// Insurance pool rate of confiscated tokens (6000 = 60%)
    pub insurance_rate_bps: u16,
    /// Retroactive claim window in seconds (90 days = 7776000)
    pub claim_window: i64,
    /// Max claim payout as % of contract value (8000 = 80%)
    pub max_claim_payout_bps: u16,
    /// Exposure limit multiplier (3x capital)
    pub exposure_limit_multiplier: u8,
    /// Governance phase (0, 1, 2 = centralized; 3+ = decentralized)
    pub governance_phase: u8,
    /// Total contracts created (counter)
    pub total_contracts: u64,
    /// Total agents registered
    pub total_agents: u64,
    /// Bump seed
    pub bump: u8,
}
