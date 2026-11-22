// Integration test for escalate_to_appeal on a SOL-denominated contract.
// Needs a local validator with the program loaded (`anchor test`); skipped when
// ANCHOR_PROVIDER_URL is not set, e.g. in the unit-test CI job.
import * as anchor from '@coral-xyz/anchor';
import BN from 'bn.js';
import { expect } from 'chai';
import { Keypair, LAMPORTS_PER_SOL, PublicKey, SystemProgram } from '@solana/web3.js';
import {
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccount,
  createMint,
  mintTo,
} from '@solana/spl-token';

const describeWithValidator = process.env.ANCHOR_PROVIDER_URL ? describe : describe.skip;

describeWithValidator('escalate_to_appeal (SOL contract)', () => {
  let program: any; // untyped: the generated IDL type is too deep for tsc here
  let conn: anchor.web3.Connection;
  let admin: Keypair;

  const pda = (...seeds: Buffer[]) => PublicKey.findProgramAddressSync(seeds, program.programId)[0];
  const u64le = (n: BN) => n.toArrayLike(Buffer, 'le', 8);

  const providerKp = Keypair.generate();
  const requesterKp = Keypair.generate();
  const VALUE = new BN(0.2 * LAMPORTS_PER_SOL);

  let mint: PublicKey;
  let contract: PublicKey;
  let dispute: PublicKey;
  let escrowVault: PublicKey;
  let requesterAta: PublicKey;

  const airdrop = async (to: PublicKey, sol: number) => {
    const sig = await conn.requestAirdrop(to, sol * LAMPORTS_PER_SOL);
    await conn.confirmTransaction(sig, 'confirmed');
  };

  before(async () => {
    const provider = anchor.AnchorProvider.env();
    anchor.setProvider(provider);
    program = anchor.workspace.TrustProtocol;
    conn = provider.connection;
    admin = (provider.wallet as anchor.Wallet).payer;
    const protocolConfig = pda(Buffer.from('protocol-config'));
    const insurancePool = pda(Buffer.from('insurance-pool'));
    const poolAuthority = pda(Buffer.from('pool-authority'));
    const bondVaultV1 = pda(Buffer.from('bond-vault'));
    const bondVaultV2 = pda(Buffer.from('bond-vault-v2'));

    await airdrop(providerKp.publicKey, 5);
    await airdrop(requesterKp.publicKey, 5);

    mint = await createMint(conn, admin, admin.publicKey, null, 6);
    const adminAta = await createAssociatedTokenAccount(conn, admin, mint, admin.publicKey);
    const providerAta = await createAssociatedTokenAccount(conn, admin, mint, providerKp.publicKey);
    requesterAta = await createAssociatedTokenAccount(conn, admin, mint, requesterKp.publicKey);
    await mintTo(conn, admin, mint, providerAta, admin, 10_000_000);

    await program.methods
      .initialize({ minIdentityBond: new BN(3_000_000), maxIdentityBond: new BN(5_000_000) })
      .accounts({
        admin: admin.publicKey, swornMint: mint, protocolConfig, insurancePool, poolAuthority,
        systemProgram: SystemProgram.programId, tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();
    await program.methods.setupBondVault()
      .accounts({
        admin: admin.publicKey, protocolConfig, swornMint: mint, bondVault: bondVaultV1,
        vaultAuthority: poolAuthority, tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
      })
      .rpc();
    await program.methods.migrateBondVault()
      .accounts({
        admin: admin.publicKey, protocolConfig, oldBondVault: bondVaultV1, adminOldTokenAccount: adminAta,
        newSwornMint: mint, newBondVault: bondVaultV2, vaultAuthority: poolAuthority,
        tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
      })
      .rpc();

    const providerIdentity = pda(Buffer.from('agent-identity'), providerKp.publicKey.toBuffer());
    await program.methods.registerAgent(new BN(3_000_000))
      .accounts({
        agent: providerKp.publicKey, agentIdentity: providerIdentity, agentTokenAccount: providerAta,
        bondVault: bondVaultV2, protocolConfig, tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
      })
      .signers([providerKp])
      .rpc();
    await program.methods.forceMature()
      .accounts({ admin: admin.publicKey, protocolConfig, agentIdentity: providerIdentity })
      .rpc();

    // SOL-denominated contract: propose (requester escrows SOL) and accept (provider stakes SOL).
    const cfg: any = await (program.account as any).protocolConfig.fetch(protocolConfig);
    const id = new BN(cfg.totalContracts.toString());
    contract = pda(Buffer.from('contract'), u64le(id));
    escrowVault = pda(Buffer.from('escrow'), u64le(id));
    dispute = pda(Buffer.from('dispute'), contract.toBuffer());

    await program.methods.proposeContract(VALUE, new BN(0), 1, 10_000)
      .accounts({
        requester: requesterKp.publicKey, provider: providerKp.publicKey, providerIdentity, contract,
        requesterTokenAccount: requesterAta, escrowVault, swornMint: mint, protocolConfig,
        tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
      })
      .signers([requesterKp])
      .rpc();
    await program.methods.acceptProposal()
      .accounts({
        provider: providerKp.publicKey, contract, providerIdentity, providerTokenAccount: providerAta,
        escrowVault, protocolConfig, tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
      })
      .signers([providerKp])
      .rpc();

    // Dispute: L1 -> L2 -> L3 (PublicJury), using provider responses to unlock each escalation.
    await program.methods.initiateDispute(Array(32).fill(1))
      .accounts({ requester: requesterKp.publicKey, contract, dispute, systemProgram: SystemProgram.programId })
      .signers([requesterKp])
      .rpc();
    for (let level = 1; level <= 2; level++) {
      await program.methods.respondDispute(Array(32).fill(2))
        .accounts({ provider: providerKp.publicKey, contract, dispute })
        .signers([providerKp])
        .rpc();
      await program.methods.escalateDispute()
        .accounts({ initiator: requesterKp.publicKey, contract, dispute })
        .signers([requesterKp])
        .rpc();
    }
    const d: any = await (program.account as any).dispute.fetch(dispute);
    expect(Object.keys(d.level)[0]).to.equal('publicJury');
  });

  it('moves the 50% appeal stake from the escalator to the contract PDA', async () => {
    const stake = VALUE.divn(2).toNumber();
    const contractBefore = await conn.getBalance(contract);
    const requesterBefore = await conn.getBalance(requesterKp.publicKey);

    let err: any = null;
    try {
      await program.methods.escalateToAppeal()
        .accounts({
          escalator: requesterKp.publicKey, contract, dispute,
          escalatorTokenAccount: requesterAta, escrowVault,
          tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
        })
        .signers([requesterKp])
        .rpc();
    } catch (e) {
      err = e;
    }
    expect(err, `escalate_to_appeal failed: ${err}`).to.equal(null);

    const d: any = await (program.account as any).dispute.fetch(dispute);
    expect(Object.keys(d.level)[0]).to.equal('appeal');
    expect(d.appealStake.toNumber()).to.equal(stake);
    expect(d.initiator.toBase58()).to.equal(requesterKp.publicKey.toBase58());
    expect((await conn.getBalance(contract)) - contractBefore).to.equal(stake);
    // escalator pays the stake plus the transaction fee
    expect(requesterBefore - (await conn.getBalance(requesterKp.publicKey))).to.be.gte(stake);
  });
});
