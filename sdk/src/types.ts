import { PublicKey } from '@solana/web3.js';
import BN from 'bn.js';

// === Account Types (mirror on-chain state.rs) ===

export interface AgentIdentity {
  authority: PublicKey;
  identityBond: BN;
  registeredAt: BN;
  matured: boolean;
  trustScore: number;
  tasksCompleted: BN;
  volumeProcessed: BN;
  disputesLost: number;
  disputesWon: number;
  tasksAbandoned: number;
  fraudFlags: number;
  sponsorBonus: number;
  banned: boolean;
  bump: number;
}

export enum ContractStatus {
  Created = 0,
  Active = 1,
  Delivered = 2,
  Completed = 3,
  Disputed = 4,
  Cancelled = 5,
  ResolvedProvider = 6,
  ResolvedRequester = 7,
}

export interface Contract {
  id: BN;
  requester: PublicKey;
  provider: PublicKey;
  value: BN;
  providerStake: BN;
  requesterStake: BN;
  status: ContractStatus;
  createdAt: BN;
  resolvedAt: BN;
  poeHash: number[];
  poeArweaveTx: string;
  disputeLevel: number;
  bump: number;
}

export enum DisputeLevel {
  DirectCorrection = 0,
  PrivateRounds = 1,
  PublicJury = 2,
  Appeal = 3,
}

export enum DisputeStatus {
  Open = 0,
  Responded = 1,
  Voting = 2,
  ResolvedProvider = 3,
  ResolvedRequester = 4,
  Escalated = 5,
}

export interface Dispute {
  contract: PublicKey;
  initiator: PublicKey;
  level: DisputeLevel;
  status: DisputeStatus;
  evidenceHash: number[];
  responseHash: number[];
  votesProvider: number;
  votesRequester: number;
  jurySize: number;
  deadline: BN;
  createdAt: BN;
  resolvedAt: BN;
  bump: number;
}

export interface InsurancePool {
  totalBalance: BN;
  totalClaimsPaid: BN;
  activeClaims: number;
  authority: PublicKey;
  bump: number;
}

export interface InsuranceClaim {
  claimant: PublicKey;
  contract: PublicKey;
  amount: BN;
  collateral: BN;
  evidenceHash: number[];
  status: ClaimStatus;
  filedAt: BN;
  contractCompletedAt: BN;
  bump: number;
}

export enum ClaimStatus {
  Filed = 0,
  UnderReview = 1,
  Approved = 2,
  Denied = 3,
}

export interface ProtocolConfig {
  admin: PublicKey;
  swornMint: PublicKey;
  minIdentityBond: BN;
  maxIdentityBond: BN;
  maturationPeriod: BN;
  minStakeFactorBps: number;
  maxStakeFactorBps: number;
  burnRateBps: number;
  insuranceRateBps: number;
  claimWindow: BN;
  maxClaimPayoutBps: number;
  exposureLimitMultiplier: number;
  governancePhase: number;
  totalContracts: BN;
  totalAgents: BN;
  bump: number;
}

// === SDK Config ===

export interface TrustProtocolConfig {
  programId: PublicKey;
  swornMint: PublicKey;
}

// === Constants from whitepaper ===

export const SWORN_DECIMALS = 9;
export const SWORN_TOTAL_SUPPLY = 100_000_000;
export const MIN_IDENTITY_BOND = 2; // SWORN tokens
export const MAX_IDENTITY_BOND = 5;
export const MATURATION_SECONDS = 2_592_000; // 30 days
export const MIN_STAKE_FACTOR_BPS = 500; // 5%
export const MAX_STAKE_FACTOR_BPS = 10_000; // 100%
export const BURN_RATE_BPS = 1_500; // 15%
export const INSURANCE_RATE_BPS = 6_000; // 60%
export const CLAIM_WINDOW_SECONDS = 7_776_000; // 90 days
export const MAX_CLAIM_PAYOUT_BPS = 8_000; // 80%
export const EXPOSURE_LIMIT_MULTIPLIER = 3;
export const JURY_MIN_TRUST_SCORE = 70;
