use anchor_lang::prelude::*;

#[error_code]
pub enum TrustError {
    #[msg("Identity bond must be between 2 and 5 SWORN tokens")]
    InvalidBondAmount,
    #[msg("Agent identity has not matured (30-day waiting period)")]
    IdentityNotMatured,
    #[msg("Agent is permanently banned due to fraud")]
    AgentBanned,
    #[msg("Insufficient stake for contract value and TrustScore")]
    InsufficientStake,
    #[msg("Contract is not in the expected status")]
    InvalidContractStatus,
    #[msg("Only the contract requester can perform this action")]
    UnauthorizedRequester,
    #[msg("Only the contract provider can perform this action")]
    UnauthorizedProvider,
    #[msg("Dispute level cannot be escalated further")]
    MaxDisputeLevel,
    #[msg("Dispute deadline has not passed")]
    DisputeDeadlineNotReached,
    #[msg("Dispute deadline has passed")]
    DisputeDeadlineExpired,
    #[msg("Jury member TrustScore must be > 70")]
    InsufficientJuryReputation,
    #[msg("Retroactive claim window (90 days) has expired")]
    ClaimWindowExpired,
    #[msg("Claim amount exceeds 80% of contract value")]
    ClaimAmountExceeded,
    #[msg("Agent exposure exceeds 3x capital limit")]
    ExposureLimitExceeded,
    #[msg("Only protocol admin can perform this action")]
    UnauthorizedAdmin,
    #[msg("Proof of Execution hash mismatch")]
    PoEHashMismatch,
    #[msg("Insufficient collateral for insurance claim")]
    InsufficientCollateral,
    #[msg("Governance phase does not allow this action")]
    GovernancePhaseRestricted,
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("Agent has already voted on this dispute")]
    AlreadyVoted,
    #[msg("Identity bond is soulbound and cannot be transferred")]
    SoulboundViolation,
}
