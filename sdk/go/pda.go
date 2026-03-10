package trustprotocol

import (
	"encoding/binary"

	"github.com/gagliardetto/solana-go"
)

// FindProtocolConfigPDA derives the singleton protocol-config PDA.
func FindProtocolConfigPDA(programID solana.PublicKey) (solana.PublicKey, uint8) {
	addr, bump, _ := solana.FindProgramAddress(
		[][]byte{[]byte("protocol-config")}, programID,
	)
	return addr, bump
}

// FindInsurancePoolPDA derives the insurance-pool PDA.
func FindInsurancePoolPDA(programID solana.PublicKey) (solana.PublicKey, uint8) {
	addr, bump, _ := solana.FindProgramAddress(
		[][]byte{[]byte("insurance-pool")}, programID,
	)
	return addr, bump
}

// FindBondVaultPDA derives the bond-vault token account PDA.
func FindBondVaultPDA(programID solana.PublicKey) (solana.PublicKey, uint8) {
	addr, bump, _ := solana.FindProgramAddress(
		[][]byte{[]byte("bond-vault")}, programID,
	)
	return addr, bump
}

// FindBondVaultV2PDA derives the bond-vault-v2 token account PDA (after migration).
func FindBondVaultV2PDA(programID solana.PublicKey) (solana.PublicKey, uint8) {
	addr, bump, _ := solana.FindProgramAddress(
		[][]byte{[]byte("bond-vault-v2")}, programID,
	)
	return addr, bump
}

// FindPoolAuthorityPDA derives the pool-authority PDA (signer for vault ops).
func FindPoolAuthorityPDA(programID solana.PublicKey) (solana.PublicKey, uint8) {
	addr, bump, _ := solana.FindProgramAddress(
		[][]byte{[]byte("pool-authority")}, programID,
	)
	return addr, bump
}

// FindAgentIdentityPDA derives the agent-identity PDA for a given wallet.
func FindAgentIdentityPDA(programID solana.PublicKey, agent solana.PublicKey) (solana.PublicKey, uint8) {
	addr, bump, _ := solana.FindProgramAddress(
		[][]byte{[]byte("agent-identity"), agent.Bytes()}, programID,
	)
	return addr, bump
}

// FindContractPDA derives the contract PDA for a given sequential contract ID.
func FindContractPDA(programID solana.PublicKey, contractID uint64) (solana.PublicKey, uint8) {
	idBytes := make([]byte, 8)
	binary.LittleEndian.PutUint64(idBytes, contractID)
	addr, bump, _ := solana.FindProgramAddress(
		[][]byte{[]byte("contract"), idBytes}, programID,
	)
	return addr, bump
}

// FindEscrowPDA derives the escrow vault PDA for a given contract ID.
func FindEscrowPDA(programID solana.PublicKey, contractID uint64) (solana.PublicKey, uint8) {
	idBytes := make([]byte, 8)
	binary.LittleEndian.PutUint64(idBytes, contractID)
	addr, bump, _ := solana.FindProgramAddress(
		[][]byte{[]byte("escrow"), idBytes}, programID,
	)
	return addr, bump
}

// FindPoePDA derives the Proof-of-Execution PDA seeded by contract account pubkey.
func FindPoePDA(programID solana.PublicKey, contractPubkey solana.PublicKey) (solana.PublicKey, uint8) {
	addr, bump, _ := solana.FindProgramAddress(
		[][]byte{[]byte("poe"), contractPubkey.Bytes()}, programID,
	)
	return addr, bump
}

// FindDisputePDA derives the dispute PDA seeded by contract account pubkey.
func FindDisputePDA(programID solana.PublicKey, contractPubkey solana.PublicKey) (solana.PublicKey, uint8) {
	addr, bump, _ := solana.FindProgramAddress(
		[][]byte{[]byte("dispute"), contractPubkey.Bytes()}, programID,
	)
	return addr, bump
}

// FindInsuranceClaimPDA derives the insurance-claim PDA seeded by contract account pubkey.
// Seeds: [b"insurance-claim", contractPDA]
func FindInsuranceClaimPDA(programID solana.PublicKey, contractPDA solana.PublicKey) (solana.PublicKey, uint8) {
	addr, bump, _ := solana.FindProgramAddress(
		[][]byte{[]byte("insurance-claim"), contractPDA.Bytes()}, programID,
	)
	return addr, bump
}

// DeriveATA derives the Associated Token Account for a wallet + mint.
func DeriveATA(wallet, mint solana.PublicKey) (solana.PublicKey, error) {
	ataProgramID := solana.MustPublicKeyFromBase58("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
	tokenProgramID := solana.MustPublicKeyFromBase58("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
	addr, _, err := solana.FindProgramAddress(
		[][]byte{wallet.Bytes(), tokenProgramID.Bytes(), mint.Bytes()},
		ataProgramID,
	)
	return addr, err
}
