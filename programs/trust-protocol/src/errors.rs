use anchor_lang::prelude::*;

#[error_code]
pub enum TrustError {
    #[msg("Identity bond must be between 2 and 5 SWORN tokens")]
    InvalidBondAmount,
    #[msg("Agent identity has not matured (14-day + 5-task requirement)")]
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
    #[msg("Invalid escrow vault PDA")]
    InvalidEscrowVault,
    #[msg("Contract proposal has expired")]
    ProposalExpired,
    #[msg("Proposal has not expired yet (cannot cancel before expiry)")]
    ProposalNotExpired,
    #[msg("Invalid currency value (must be 0=SWORN or 1=SOL)")]
    InvalidCurrency,
    #[msg("Currency mismatch: operation requires SOL-denominated contract")]
    CurrencyMismatchSol,
    #[msg("Currency mismatch: operation requires SWORN-denominated contract")]
    CurrencyMismatchSworn,
    #[msg("Maximum corrections reached (3). Dispute auto-escalated to Level 2.")]
    MaxCorrectionsReached,
    #[msg("Destination account does not match expected recipient")]
    InvalidDestination,
    #[msg("Account size is not the expected old format size")]
    InvalidAccountSize,
    #[msg("Contract delivery timeout (72h) has not been reached yet")]
    TimeoutNotReached,
    #[msg("Jury is still voting: quorum not reached and deadline has not passed")]
    JuryStillVoting,
    #[msg("Agent is currently hibernating and cannot accept new contracts")]
    AgentHibernating,
    #[msg("Agent is not currently hibernating")]
    AgentNotHibernating,
    #[msg("Hibernation already active for this agent")]
    AlreadyHibernating,
    #[msg("Hibernation cooldown: must complete 5 tasks after last hibernation")]
    HibernationCooldown,
    #[msg("Hibernation duration must be between 1 and 12 months")]
    InvalidHibernationDuration,
    #[msg("Claim exceeds 5% of InsurancePool balance (§11.5b per-claim cap)")]
    ClaimExceedsPoolCap,
    #[msg("InsurancePool in crisis (solvency < 0.5): only proven fraud claims accepted (§11.5c)")]
    InsurancePoolCrisis,
    #[msg("Contract is not public (visibility must be 1 for bidding, §6.5)")]
    ContractNotPublic,
    #[msg("Contract is not in Created/Proposed status (cannot bid on active contracts)")]
    ContractNotBiddable,
    #[msg("Proposed price exceeds contract escrow amount")]
    BidPriceTooHigh,
    #[msg("Bid stake offered is below the minimum required for bidder TrustScore")]
    BidStakeInsufficient,
    #[msg("Bid not found or already withdrawn")]
    BidNotActive,
    #[msg("Only the bid owner can withdraw this bid")]
    UnauthorizedBidder,
    #[msg("Only the contract requester can select a bid")]
    UnauthorizedBidSelector,
    #[msg("Insufficient liquid reserve for withdrawal (§11.10)")]
    InsufficientLiquidReserve,
    #[msg("Withdrawal amount exceeds staked balance")]
    WithdrawExceedsStake,
    #[msg("No LP fees to harvest")]
    NoFeesToHarvest,
    #[msg("Deposit amount must be greater than zero")]
    ZeroDeposit,
}
