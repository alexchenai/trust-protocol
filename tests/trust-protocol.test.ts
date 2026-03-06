import { expect } from 'chai';
import BN from 'bn.js';
import {
  calculateStakeFactor,
  calculateRequiredStake,
  swornToLamports,
  lamportsToSworn,
  calculateConfiscationSplit,
  agentDID,
  findAgentIdentityPDA,
  findContractPDA,
  findDisputePDA,
  findProtocolConfigPDA,
  findInsurancePoolPDA,
  findEscrowVaultPDA,
  findInsuranceClaimPDA,
  findPoEPDA,
} from '../sdk/src/utils';
import {
  SWORN_DECIMALS,
  SWORN_TOTAL_SUPPLY,
  MIN_STAKE_FACTOR_BPS,
  MAX_STAKE_FACTOR_BPS,
  BURN_RATE_BPS,
  INSURANCE_RATE_BPS,
  MIN_IDENTITY_BOND,
  MAX_IDENTITY_BOND,
  MATURATION_SECONDS,
  CLAIM_WINDOW_SECONDS,
  MAX_CLAIM_PAYOUT_BPS,
  EXPOSURE_LIMIT_MULTIPLIER,
  JURY_MIN_TRUST_SCORE,
} from '../sdk/src/types';
import { Keypair, PublicKey } from '@solana/web3.js';

describe('Trust Protocol - Unit Tests', () => {
  // Use a deterministic program ID for tests
  const PROGRAM_ID = new PublicKey('TRSTpRoToCoL1111111111111111111111111111111');

  describe('Whitepaper Constants', () => {
    it('SWORN total supply is 100M', () => {
      expect(SWORN_TOTAL_SUPPLY).to.equal(100_000_000);
    });

    it('SWORN has 9 decimals', () => {
      expect(SWORN_DECIMALS).to.equal(9);
    });

    it('Identity bond range is 2-5 SWORN', () => {
      expect(MIN_IDENTITY_BOND).to.equal(2);
      expect(MAX_IDENTITY_BOND).to.equal(5);
    });

    it('Maturation period is 30 days', () => {
      expect(MATURATION_SECONDS).to.equal(30 * 24 * 60 * 60);
    });

    it('Stake factor range: 5% (score 100) to 100% (score 0)', () => {
      expect(MIN_STAKE_FACTOR_BPS).to.equal(500);
      expect(MAX_STAKE_FACTOR_BPS).to.equal(10_000);
    });

    it('Burn rate is 15%', () => {
      expect(BURN_RATE_BPS).to.equal(1_500);
    });

    it('Insurance rate is 60%', () => {
      expect(INSURANCE_RATE_BPS).to.equal(6_000);
    });

    it('Claim window is 90 days', () => {
      expect(CLAIM_WINDOW_SECONDS).to.equal(90 * 24 * 60 * 60);
    });

    it('Max claim payout is 80%', () => {
      expect(MAX_CLAIM_PAYOUT_BPS).to.equal(8_000);
    });

    it('Exposure limit is 3x', () => {
      expect(EXPOSURE_LIMIT_MULTIPLIER).to.equal(3);
    });

    it('Jury minimum TrustScore is 70', () => {
      expect(JURY_MIN_TRUST_SCORE).to.equal(70);
    });
  });

  describe('Dynamic Staking (Whitepaper Section 3)', () => {
    it('TrustScore 0 => 100% stake (10000 bps)', () => {
      expect(calculateStakeFactor(0)).to.equal(10_000);
    });

    it('TrustScore 100 => 5% stake (500 bps)', () => {
      expect(calculateStakeFactor(100)).to.equal(500);
    });

    it('TrustScore 50 => ~52.5% stake (5250 bps)', () => {
      const factor = calculateStakeFactor(50);
      expect(factor).to.equal(5_250);
    });

    it('TrustScore 75 => ~28.75% stake (2875 bps)', () => {
      const factor = calculateStakeFactor(75);
      expect(factor).to.equal(2_875);
    });

    it('TrustScore > 100 clamps to 5%', () => {
      expect(calculateStakeFactor(150)).to.equal(500);
    });

    it('Negative TrustScore clamps to 100%', () => {
      expect(calculateStakeFactor(-10)).to.equal(10_000);
    });

    it('calculates required stake correctly', () => {
      const value = new BN(1_000_000_000); // 1 SWORN
      // Score 0 => 100% stake
      expect(calculateRequiredStake(value, 0).toString()).to.equal('1000000000');
      // Score 100 => 5% stake
      expect(calculateRequiredStake(value, 100).toString()).to.equal('50000000');
      // Score 50 => 52.5% stake
      expect(calculateRequiredStake(value, 50).toString()).to.equal('525000000');
    });

    it('stake factor is linear interpolation', () => {
      // Verify linearity: each point of TrustScore reduces by 95 bps
      const f0 = calculateStakeFactor(0);
      const f1 = calculateStakeFactor(1);
      const step = f0 - f1; // Should be 95
      expect(step).to.equal(95);

      // Verify intermediate points
      for (let score = 0; score <= 100; score++) {
        const expected = 10_000 - score * 95;
        expect(calculateStakeFactor(score)).to.equal(expected);
      }
    });
  });

  describe('SWORN Token Conversions', () => {
    it('converts 1 SWORN to 1_000_000_000 lamports', () => {
      const lamports = swornToLamports(1);
      expect(lamports.toString()).to.equal('1000000000');
    });

    it('converts 5 SWORN to 5_000_000_000 lamports', () => {
      const lamports = swornToLamports(5);
      expect(lamports.toString()).to.equal('5000000000');
    });

    it('converts lamports back to SWORN', () => {
      const sworn = lamportsToSworn(new BN('2000000000'));
      expect(sworn).to.equal(2);
    });

    it('identity bond bounds in lamports', () => {
      const minBond = swornToLamports(MIN_IDENTITY_BOND);
      const maxBond = swornToLamports(MAX_IDENTITY_BOND);
      expect(minBond.toString()).to.equal('2000000000');
      expect(maxBond.toString()).to.equal('5000000000');
    });
  });

  describe('Confiscation Split (Whitepaper Section 4+6)', () => {
    it('splits confiscated stake: 15% burned, 60% insurance, 25% winner', () => {
      const confiscated = new BN(10_000);
      const { burned, insurance, winner } = calculateConfiscationSplit(confiscated);

      expect(burned.toNumber()).to.equal(1_500);    // 15%
      expect(insurance.toNumber()).to.equal(6_000);  // 60%
      expect(winner.toNumber()).to.equal(2_500);     // 25%

      // Verify total = confiscated
      const total = burned.add(insurance).add(winner);
      expect(total.toNumber()).to.equal(confiscated.toNumber());
    });

    it('works with large amounts', () => {
      const confiscated = swornToLamports(100); // 100 SWORN
      const { burned, insurance, winner } = calculateConfiscationSplit(confiscated);
      const total = burned.add(insurance).add(winner);
      expect(total.toString()).to.equal(confiscated.toString());
    });

    it('handles zero confiscation', () => {
      const { burned, insurance, winner } = calculateConfiscationSplit(new BN(0));
      expect(burned.toNumber()).to.equal(0);
      expect(insurance.toNumber()).to.equal(0);
      expect(winner.toNumber()).to.equal(0);
    });
  });

  describe('DID Generation (Whitepaper Section 2)', () => {
    it('generates correct DID format', () => {
      const kp = Keypair.generate();
      const did = agentDID(kp.publicKey);
      expect(did).to.equal(`did:trust:${kp.publicKey.toBase58()}`);
      expect(did.startsWith('did:trust:')).to.be.true;
    });
  });

  describe('PDA Derivation', () => {
    const agent = Keypair.generate().publicKey;

    it('derives agent identity PDA deterministically', () => {
      const [pda1] = findAgentIdentityPDA(agent, PROGRAM_ID);
      const [pda2] = findAgentIdentityPDA(agent, PROGRAM_ID);
      expect(pda1.equals(pda2)).to.be.true;
    });

    it('different agents get different PDAs', () => {
      const agent2 = Keypair.generate().publicKey;
      const [pda1] = findAgentIdentityPDA(agent, PROGRAM_ID);
      const [pda2] = findAgentIdentityPDA(agent2, PROGRAM_ID);
      expect(pda1.equals(pda2)).to.be.false;
    });

    it('derives protocol config PDA', () => {
      const [pda] = findProtocolConfigPDA(PROGRAM_ID);
      expect(pda).to.be.instanceOf(PublicKey);
    });

    it('derives insurance pool PDA', () => {
      const [pda] = findInsurancePoolPDA(PROGRAM_ID);
      expect(pda).to.be.instanceOf(PublicKey);
    });

    it('derives contract PDA from ID', () => {
      const [pda1] = findContractPDA(new BN(0), PROGRAM_ID);
      const [pda2] = findContractPDA(new BN(1), PROGRAM_ID);
      expect(pda1.equals(pda2)).to.be.false;
    });

    it('derives escrow vault PDA from contract ID', () => {
      const [pda] = findEscrowVaultPDA(new BN(42), PROGRAM_ID);
      expect(pda).to.be.instanceOf(PublicKey);
    });

    it('derives dispute PDA from contract', () => {
      const contract = Keypair.generate().publicKey;
      const [pda] = findDisputePDA(contract, PROGRAM_ID);
      expect(pda).to.be.instanceOf(PublicKey);
    });

    it('derives PoE PDA from contract', () => {
      const contract = Keypair.generate().publicKey;
      const [pda] = findPoEPDA(contract, PROGRAM_ID);
      expect(pda).to.be.instanceOf(PublicKey);
    });

    it('derives insurance claim PDA from contract', () => {
      const contract = Keypair.generate().publicKey;
      const [pda] = findInsuranceClaimPDA(contract, PROGRAM_ID);
      expect(pda).to.be.instanceOf(PublicKey);
    });
  });

  describe('Security: Long Con Fraud (Whitepaper Section 7)', () => {
    it('minimum 5% stake even at max TrustScore prevents risk-free fraud', () => {
      // Agent with perfect score still stakes 5%
      const contractValue = swornToLamports(1000); // 1000 SWORN contract
      const stake = calculateRequiredStake(contractValue, 100);
      // 5% of 1000 = 50 SWORN
      expect(lamportsToSworn(stake)).to.equal(50);
      // Fraud is never risk-free
      expect(stake.gt(new BN(0))).to.be.true;
    });

    it('exposure limit: max 3x capital at risk', () => {
      // Agent with 100 SWORN can only have 300 SWORN in active contracts
      const capital = swornToLamports(100);
      const maxExposure = capital.muln(EXPOSURE_LIMIT_MULTIPLIER);
      expect(lamportsToSworn(maxExposure)).to.equal(300);
    });
  });

  describe('Security: Sybil Attack (Whitepaper Section 7)', () => {
    it('identity bond creates economic friction', () => {
      // Creating 1000 fake identities costs 2000-5000 SWORN
      const minCostPer = swornToLamports(MIN_IDENTITY_BOND);
      const sybilCount = 1000;
      const totalCost = minCostPer.muln(sybilCount);
      // 2000 SWORN minimum to create 1000 identities
      expect(lamportsToSworn(totalCost)).to.equal(2000);
    });

    it('30-day maturation prevents instant reputation farming', () => {
      // Each identity needs 30 days to mature
      expect(MATURATION_SECONDS).to.equal(2_592_000);
      // Plus real task completion required to build TrustScore
    });
  });

  describe('Insurance Pool (Whitepaper Section 6)', () => {
    it('max claim is 80% of contract value', () => {
      const contractValue = new BN(10_000);
      const maxClaim = contractValue.muln(MAX_CLAIM_PAYOUT_BPS).divn(10_000);
      expect(maxClaim.toNumber()).to.equal(8_000);
    });

    it('90-day retroactive window', () => {
      const completedAt = Math.floor(Date.now() / 1000) - (89 * 86_400); // 89 days ago
      const now = Math.floor(Date.now() / 1000);
      const windowEnd = completedAt + CLAIM_WINDOW_SECONDS;
      expect(now < windowEnd).to.be.true; // Still within window

      const tooLate = Math.floor(Date.now() / 1000) - (91 * 86_400); // 91 days ago
      const lateEnd = tooLate + CLAIM_WINDOW_SECONDS;
      expect(now > lateEnd).to.be.true; // Window expired
    });
  });
});
