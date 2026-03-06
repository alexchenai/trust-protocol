import { PublicKey } from '@solana/web3.js';
import BN from 'bn.js';
import { MIN_STAKE_FACTOR_BPS, MAX_STAKE_FACTOR_BPS, SWORN_DECIMALS } from './types';

/**
 * Calculate stake factor based on TrustScore.
 * Whitepaper: Linear interpolation from 100% (score 0) to 5% (score 100).
 * Returns basis points (10000 = 100%).
 */
export function calculateStakeFactor(trustScore: number): number {
  if (trustScore >= 100) return MIN_STAKE_FACTOR_BPS;
  if (trustScore <= 0) return MAX_STAKE_FACTOR_BPS;
  const range = MAX_STAKE_FACTOR_BPS - MIN_STAKE_FACTOR_BPS;
  const reduction = Math.floor((range * trustScore) / 100);
  return MAX_STAKE_FACTOR_BPS - reduction;
}

/**
 * Calculate required stake for a contract value and TrustScore.
 * stake_required = contract_value * factor_stake(TrustScore) / 10000
 */
export function calculateRequiredStake(contractValue: BN, trustScore: number): BN {
  const factor = calculateStakeFactor(trustScore);
  return contractValue.mul(new BN(factor)).div(new BN(10_000));
}

/**
 * Convert SWORN tokens to lamports (9 decimals).
 */
export function swornToLamports(amount: number): BN {
  return new BN(amount).mul(new BN(10).pow(new BN(SWORN_DECIMALS)));
}

/**
 * Convert lamports to SWORN tokens.
 */
export function lamportsToSworn(lamports: BN): number {
  return lamports.div(new BN(10).pow(new BN(SWORN_DECIMALS))).toNumber();
}

/**
 * Derive PDA for agent identity.
 */
export function findAgentIdentityPDA(
  agent: PublicKey,
  programId: PublicKey
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('agent-identity'), agent.toBuffer()],
    programId
  );
}

/**
 * Derive PDA for contract.
 */
export function findContractPDA(
  contractId: BN,
  programId: PublicKey
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('contract'), contractId.toArrayLike(Buffer, 'le', 8)],
    programId
  );
}

/**
 * Derive PDA for dispute.
 */
export function findDisputePDA(
  contract: PublicKey,
  programId: PublicKey
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('dispute'), contract.toBuffer()],
    programId
  );
}

/**
 * Derive PDA for proof of execution.
 */
export function findPoEPDA(
  contract: PublicKey,
  programId: PublicKey
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('poe'), contract.toBuffer()],
    programId
  );
}

/**
 * Derive PDA for protocol config.
 */
export function findProtocolConfigPDA(programId: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('protocol-config')],
    programId
  );
}

/**
 * Derive PDA for insurance pool.
 */
export function findInsurancePoolPDA(programId: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('insurance-pool')],
    programId
  );
}

/**
 * Derive PDA for escrow vault.
 */
export function findEscrowVaultPDA(
  contractId: BN,
  programId: PublicKey
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('escrow'), contractId.toArrayLike(Buffer, 'le', 8)],
    programId
  );
}

/**
 * Derive PDA for insurance claim.
 */
export function findInsuranceClaimPDA(
  contract: PublicKey,
  programId: PublicKey
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('insurance-claim'), contract.toBuffer()],
    programId
  );
}

/**
 * Generate DID string for an agent.
 * Format: did:trust:{base58_pubkey}
 */
export function agentDID(pubkey: PublicKey): string {
  return `did:trust:${pubkey.toBase58()}`;
}

/**
 * Calculate confiscation distribution (whitepaper Section 4+6).
 * 15% burned, 60% insurance pool, 25% to winner.
 */
export function calculateConfiscationSplit(confiscated: BN): {
  burned: BN;
  insurance: BN;
  winner: BN;
} {
  const burned = confiscated.mul(new BN(1_500)).div(new BN(10_000));
  const insurance = confiscated.mul(new BN(6_000)).div(new BN(10_000));
  const winner = confiscated.sub(burned).sub(insurance);
  return { burned, insurance, winner };
}
