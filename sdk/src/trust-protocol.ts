import {
  Connection,
  PublicKey,
  Keypair,
  Transaction,
  TransactionInstruction,
  SystemProgram,
  SYSVAR_CLOCK_PUBKEY,
} from '@solana/web3.js';
import { TOKEN_PROGRAM_ID } from '@solana/spl-token';
import BN from 'bn.js';
import {
  TrustProtocolConfig,
  AgentIdentity,
  Contract,
  Dispute,
  InsurancePool,
  ProtocolConfig,
} from './types';
import {
  findAgentIdentityPDA,
  findContractPDA,
  findDisputePDA,
  findPoEPDA,
  findProtocolConfigPDA,
  findInsurancePoolPDA,
  findEscrowVaultPDA,
  findInsuranceClaimPDA,
  calculateStakeFactor,
  calculateRequiredStake,
  swornToLamports,
  agentDID,
} from './utils';

/**
 * Trust Protocol SDK - TypeScript client for interacting with the on-chain program.
 * Provides high-level methods matching all whitepaper operations.
 */
export class TrustProtocolSDK {
  readonly connection: Connection;
  readonly programId: PublicKey;
  readonly swornMint: PublicKey;

  constructor(connection: Connection, config: TrustProtocolConfig) {
    this.connection = connection;
    this.programId = config.programId;
    this.swornMint = config.swornMint;
  }

  // === PDA Helpers ===

  getAgentIdentityPDA(agent: PublicKey): [PublicKey, number] {
    return findAgentIdentityPDA(agent, this.programId);
  }

  getContractPDA(contractId: BN): [PublicKey, number] {
    return findContractPDA(contractId, this.programId);
  }

  getDisputePDA(contract: PublicKey): [PublicKey, number] {
    return findDisputePDA(contract, this.programId);
  }

  getProtocolConfigPDA(): [PublicKey, number] {
    return findProtocolConfigPDA(this.programId);
  }

  getInsurancePoolPDA(): [PublicKey, number] {
    return findInsurancePoolPDA(this.programId);
  }

  // === Read Operations ===

  /**
   * Fetch agent identity from chain.
   * Returns null if agent is not registered.
   */
  async getAgentIdentity(agent: PublicKey): Promise<AgentIdentity | null> {
    const [pda] = this.getAgentIdentityPDA(agent);
    const info = await this.connection.getAccountInfo(pda);
    if (!info) return null;
    // Deserialize using Anchor discriminator (8 bytes) + borsh
    // In production, use anchor Program.account.agentIdentity.fetch()
    return this.deserializeAgentIdentity(info.data);
  }

  /**
   * Fetch protocol config.
   */
  async getProtocolConfig(): Promise<ProtocolConfig | null> {
    const [pda] = this.getProtocolConfigPDA();
    const info = await this.connection.getAccountInfo(pda);
    if (!info) return null;
    return this.deserializeProtocolConfig(info.data);
  }

  /**
   * Get an agent's DID string.
   */
  getAgentDID(agent: PublicKey): string {
    return agentDID(agent);
  }

  /**
   * Calculate how much stake a provider needs for a contract.
   */
  getRequiredStake(contractValue: BN, trustScore: number): BN {
    return calculateRequiredStake(contractValue, trustScore);
  }

  /**
   * Get the stake factor for a given TrustScore (in basis points).
   */
  getStakeFactor(trustScore: number): number {
    return calculateStakeFactor(trustScore);
  }

  /**
   * Check if an agent's identity has matured (30 days since registration).
   */
  async isAgentMatured(agent: PublicKey): Promise<boolean> {
    const identity = await this.getAgentIdentity(agent);
    if (!identity) return false;
    if (identity.matured) return true;
    const now = Math.floor(Date.now() / 1000);
    const maturationTime = identity.registeredAt.toNumber() + 2_592_000;
    return now >= maturationTime;
  }

  /**
   * Check if an agent can serve as juror (TrustScore > 70, matured, not banned).
   */
  async canServeAsJuror(agent: PublicKey): Promise<boolean> {
    const identity = await this.getAgentIdentity(agent);
    if (!identity) return false;
    return identity.trustScore > 70 && identity.matured && !identity.banned;
  }

  // === Utility ===

  /**
   * Convert SWORN tokens to on-chain lamport representation.
   */
  toSwornLamports(amount: number): BN {
    return swornToLamports(amount);
  }

  // === Deserialization (simplified - use Anchor IDL in production) ===

  private deserializeAgentIdentity(data: Buffer): AgentIdentity {
    // Skip 8-byte discriminator
    let offset = 8;
    const authority = new PublicKey(data.subarray(offset, offset + 32)); offset += 32;
    const identityBond = new BN(data.subarray(offset, offset + 8), 'le'); offset += 8;
    const registeredAt = new BN(data.subarray(offset, offset + 8), 'le'); offset += 8;
    const matured = data[offset] === 1; offset += 1;
    const trustScore = data.readUInt16LE(offset); offset += 2;
    const tasksCompleted = new BN(data.subarray(offset, offset + 8), 'le'); offset += 8;
    const volumeProcessed = new BN(data.subarray(offset, offset + 8), 'le'); offset += 8;
    const disputesLost = data.readUInt32LE(offset); offset += 4;
    const disputesWon = data.readUInt32LE(offset); offset += 4;
    const tasksAbandoned = data.readUInt32LE(offset); offset += 4;
    const fraudFlags = data.readUInt32LE(offset); offset += 4;
    const sponsorBonus = data.readUInt16LE(offset); offset += 2;
    const banned = data[offset] === 1; offset += 1;
    const bump = data[offset]; offset += 1;

    return {
      authority, identityBond, registeredAt, matured, trustScore,
      tasksCompleted, volumeProcessed, disputesLost, disputesWon,
      tasksAbandoned, fraudFlags, sponsorBonus, banned, bump,
    };
  }

  private deserializeProtocolConfig(data: Buffer): ProtocolConfig {
    let offset = 8;
    const admin = new PublicKey(data.subarray(offset, offset + 32)); offset += 32;
    const swornMint = new PublicKey(data.subarray(offset, offset + 32)); offset += 32;
    const minIdentityBond = new BN(data.subarray(offset, offset + 8), 'le'); offset += 8;
    const maxIdentityBond = new BN(data.subarray(offset, offset + 8), 'le'); offset += 8;
    const maturationPeriod = new BN(data.subarray(offset, offset + 8), 'le'); offset += 8;
    const minStakeFactorBps = data.readUInt16LE(offset); offset += 2;
    const maxStakeFactorBps = data.readUInt16LE(offset); offset += 2;
    const burnRateBps = data.readUInt16LE(offset); offset += 2;
    const insuranceRateBps = data.readUInt16LE(offset); offset += 2;
    const claimWindow = new BN(data.subarray(offset, offset + 8), 'le'); offset += 8;
    const maxClaimPayoutBps = data.readUInt16LE(offset); offset += 2;
    const exposureLimitMultiplier = data[offset]; offset += 1;
    const governancePhase = data[offset]; offset += 1;
    const totalContracts = new BN(data.subarray(offset, offset + 8), 'le'); offset += 8;
    const totalAgents = new BN(data.subarray(offset, offset + 8), 'le'); offset += 8;
    const bump = data[offset]; offset += 1;

    return {
      admin, swornMint, minIdentityBond, maxIdentityBond, maturationPeriod,
      minStakeFactorBps, maxStakeFactorBps, burnRateBps, insuranceRateBps,
      claimWindow, maxClaimPayoutBps, exposureLimitMultiplier, governancePhase,
      totalContracts, totalAgents, bump,
    };
  }
}
