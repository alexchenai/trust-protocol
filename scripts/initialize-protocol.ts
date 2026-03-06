/**
 * Initialize Trust Protocol on devnet.
 * Creates SWORN token mint (100M fixed supply) and protocol config.
 * Run after deploy: npx ts-node scripts/initialize-protocol.ts
 */

import {
  Connection,
  Keypair,
  PublicKey,
  clusterApiUrl,
} from '@solana/web3.js';
import {
  createMint,
  mintTo,
  getOrCreateAssociatedTokenAccount,
  TOKEN_PROGRAM_ID,
} from '@solana/spl-token';
import { readFileSync } from 'fs';
import { swornToLamports } from '../sdk/src/utils';
import {
  SWORN_DECIMALS,
  SWORN_TOTAL_SUPPLY,
  MIN_IDENTITY_BOND,
  MAX_IDENTITY_BOND,
} from '../sdk/src/types';

const PROGRAM_ID = new PublicKey('TRSTpRoToCoL1111111111111111111111111111111'); // Update after deploy

async function main() {
  const connection = new Connection(clusterApiUrl('devnet'), 'confirmed');

  // Load wallet
  const walletPath = process.env.WALLET_PATH || `${process.env.HOME}/.config/solana/id.json`;
  const secretKey = JSON.parse(readFileSync(walletPath, 'utf-8'));
  const admin = Keypair.fromSecretKey(Uint8Array.from(secretKey));

  console.log(`Admin: ${admin.publicKey.toBase58()}`);
  console.log(`Balance: ${await connection.getBalance(admin.publicKey) / 1e9} SOL`);

  // Step 1: Create SWORN token mint
  console.log('\n--- Creating SWORN Token Mint ---');
  const swornMint = await createMint(
    connection,
    admin,           // payer
    admin.publicKey,  // mint authority
    null,             // freeze authority (none - no freezing)
    SWORN_DECIMALS,   // 9 decimals
  );
  console.log(`SWORN Mint: ${swornMint.toBase58()}`);

  // Step 2: Mint total supply to admin (100M SWORN)
  console.log('\n--- Minting Total Supply ---');
  const adminATA = await getOrCreateAssociatedTokenAccount(
    connection,
    admin,
    swornMint,
    admin.publicKey,
  );
  console.log(`Admin ATA: ${adminATA.address.toBase58()}`);

  const totalSupplyLamports = swornToLamports(SWORN_TOTAL_SUPPLY);
  await mintTo(
    connection,
    admin,
    swornMint,
    adminATA.address,
    admin,
    BigInt(totalSupplyLamports.toString()),
  );
  console.log(`Minted ${SWORN_TOTAL_SUPPLY} SWORN (${totalSupplyLamports.toString()} lamports)`);

  // Step 3: Revoke mint authority (fixed supply - no more minting)
  // This ensures the 100M cap from the whitepaper
  console.log('\n--- Revoking Mint Authority (Fixed Supply) ---');
  const { setAuthority, AuthorityType } = await import('@solana/spl-token');
  await setAuthority(
    connection,
    admin,
    swornMint,
    admin,
    AuthorityType.MintTokens,
    null, // Revoke - no one can mint more
  );
  console.log('Mint authority REVOKED. Supply permanently fixed at 100M SWORN.');

  // Step 4: Initialize protocol (via Anchor instruction)
  console.log('\n--- Protocol Initialization ---');
  console.log('Run anchor test or use SDK to call initialize() with:');
  console.log(`  swornMint: ${swornMint.toBase58()}`);
  console.log(`  minIdentityBond: ${swornToLamports(MIN_IDENTITY_BOND).toString()} (${MIN_IDENTITY_BOND} SWORN)`);
  console.log(`  maxIdentityBond: ${swornToLamports(MAX_IDENTITY_BOND).toString()} (${MAX_IDENTITY_BOND} SWORN)`);

  // Summary
  console.log('\n=== Initialization Summary ===');
  console.log(`Program ID: ${PROGRAM_ID.toBase58()}`);
  console.log(`SWORN Mint: ${swornMint.toBase58()}`);
  console.log(`Admin: ${admin.publicKey.toBase58()}`);
  console.log(`Total Supply: ${SWORN_TOTAL_SUPPLY} SWORN (fixed, mint authority revoked)`);
  console.log(`Identity Bond: ${MIN_IDENTITY_BOND}-${MAX_IDENTITY_BOND} SWORN`);
  console.log('\nSave these values! They are needed for SDK configuration.');
}

main().catch(console.error);
