package trustprotocol

import (
	"crypto/sha256"
	"encoding/binary"

	"github.com/gagliardetto/solana-go"
)

// AnchorDiscriminator computes the 8-byte Anchor instruction discriminator.
//
//	SHA256("global:<instruction_name>")[0:8]
func AnchorDiscriminator(instructionName string) [8]byte {
	h := sha256.Sum256([]byte("global:" + instructionName))
	var disc [8]byte
	copy(disc[:], h[:8])
	return disc
}

// Well-known Solana program IDs.
var (
	TokenProgramID    = solana.MustPublicKeyFromBase58("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
	ATAProgramID      = solana.MustPublicKeyFromBase58("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
	SystemProgramID   = solana.SystemProgramID
	RentSysvarID      = solana.MustPublicKeyFromBase58("SysvarRent111111111111111111111111111111111")
	MetaplexProgramID = solana.MustPublicKeyFromBase58("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s")
)

// ---------------------------------------------------------------------------
// Instruction builders — return solana.Instruction ready for a transaction.
// Each builder takes the programID + relevant accounts + args.
// ---------------------------------------------------------------------------

// NewInitializeInstruction builds the "initialize" instruction.
// params is the Borsh-encoded InitializeParams struct.
func NewInitializeInstruction(
	programID solana.PublicKey,
	admin solana.PublicKey,
	swornMint solana.PublicKey,
	configPDA solana.PublicKey,
	insurancePoolPDA solana.PublicKey,
	params []byte,
) solana.Instruction {
	disc := AnchorDiscriminator("initialize")
	data := make([]byte, 8+len(params))
	copy(data[0:8], disc[:])
	copy(data[8:], params)
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(admin).SIGNER().WRITE(),
			solana.Meta(swornMint),
			solana.Meta(configPDA).WRITE(),
			solana.Meta(insurancePoolPDA).WRITE(),
			solana.Meta(SystemProgramID),
		},
		DataBytes: data,
	}
}

// NewRegisterAgentInstruction builds "register_agent" with bond_amount arg.
func NewRegisterAgentInstruction(
	programID solana.PublicKey,
	agent solana.PublicKey,
	agentIdentityPDA solana.PublicKey,
	agentTokenAccount solana.PublicKey,
	bondVaultPDA solana.PublicKey,
	configPDA solana.PublicKey,
	bondLamports uint64,
) solana.Instruction {
	disc := AnchorDiscriminator("register_agent")
	var data [16]byte
	copy(data[0:8], disc[:])
	binary.LittleEndian.PutUint64(data[8:16], bondLamports)
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(agent).SIGNER().WRITE(),
			solana.Meta(agentIdentityPDA).WRITE(),
			solana.Meta(agentTokenAccount).WRITE(),
			solana.Meta(bondVaultPDA).WRITE(),
			solana.Meta(configPDA).WRITE(),
			solana.Meta(TokenProgramID),
			solana.Meta(SystemProgramID),
		},
		DataBytes: data[:],
	}
}

// NewForceMatureInstruction builds the admin "force_mature" instruction.
func NewForceMatureInstruction(
	programID solana.PublicKey,
	admin solana.PublicKey,
	configPDA solana.PublicKey,
	agentIdentityPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("force_mature")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(admin).SIGNER().WRITE(),
			solana.Meta(configPDA),
			solana.Meta(agentIdentityPDA).WRITE(),
		},
		DataBytes: disc[:],
	}
}

// NewCreateContractInstruction builds "create_contract" with value arg.
// The escrow vault is init'd in this instruction (requires system_program).
func NewCreateContractInstruction(
	programID solana.PublicKey,
	requester solana.PublicKey,
	provider solana.PublicKey,
	providerIdentityPDA solana.PublicKey,
	contractPDA solana.PublicKey,
	requesterTokenAccount solana.PublicKey,
	providerTokenAccount solana.PublicKey,
	escrowVaultPDA solana.PublicKey,
	swornMint solana.PublicKey,
	configPDA solana.PublicKey,
	value uint64,
) solana.Instruction {
	disc := AnchorDiscriminator("create_contract")
	var data [16]byte
	copy(data[0:8], disc[:])
	binary.LittleEndian.PutUint64(data[8:16], value)
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(requester).SIGNER().WRITE(),
			solana.Meta(provider),
			solana.Meta(providerIdentityPDA),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(requesterTokenAccount).WRITE(),
			solana.Meta(providerTokenAccount).WRITE(),
			solana.Meta(escrowVaultPDA).WRITE(),
			solana.Meta(swornMint),
			solana.Meta(configPDA).WRITE(),
			solana.Meta(TokenProgramID),
			solana.Meta(SystemProgramID),
		},
		DataBytes: data[:],
	}
}

// NewDeliverContractInstruction builds "deliver_contract" with PoE data.
func NewDeliverContractInstruction(
	programID solana.PublicKey,
	provider solana.PublicKey,
	contractPDA solana.PublicKey,
	poePDA solana.PublicKey,
	outputHash [32]byte,
	arweaveTx string,
) solana.Instruction {
	disc := AnchorDiscriminator("deliver_contract")
	arweaveBytes := []byte(arweaveTx)
	data := make([]byte, 8+32+4+len(arweaveBytes))
	copy(data[0:8], disc[:])
	copy(data[8:40], outputHash[:])
	binary.LittleEndian.PutUint32(data[40:44], uint32(len(arweaveBytes)))
	copy(data[44:], arweaveBytes)
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(provider).SIGNER().WRITE(),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(poePDA).WRITE(),
			solana.Meta(SystemProgramID),
		},
		DataBytes: data,
	}
}

// NewAcceptContractInstruction builds "accept_contract" (no extra args).
func NewAcceptContractInstruction(
	programID solana.PublicKey,
	requester solana.PublicKey,
	contractPDA solana.PublicKey,
	poePDA solana.PublicKey,
	providerIdentityPDA solana.PublicKey,
	providerTokenAccount solana.PublicKey,
	treasuryTokenAccount solana.PublicKey,
	insuranceVault solana.PublicKey,
	escrowVaultPDA solana.PublicKey,
	configPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("accept_contract")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(requester).SIGNER(),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(poePDA).WRITE(),
			solana.Meta(providerIdentityPDA).WRITE(),
			solana.Meta(providerTokenAccount).WRITE(),
			solana.Meta(treasuryTokenAccount).WRITE(),
			solana.Meta(insuranceVault).WRITE(),
			solana.Meta(escrowVaultPDA).WRITE(),
			solana.Meta(configPDA),
			solana.Meta(TokenProgramID),
		},
		DataBytes: disc[:],
	}
}

// NewSetupBondVaultInstruction builds the admin "setup_bond_vault" instruction.
func NewSetupBondVaultInstruction(
	programID solana.PublicKey,
	admin solana.PublicKey,
	configPDA solana.PublicKey,
	swornMint solana.PublicKey,
	bondVaultPDA solana.PublicKey,
	poolAuthorityPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("setup_bond_vault")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(admin).SIGNER().WRITE(),
			solana.Meta(configPDA),
			solana.Meta(swornMint),
			solana.Meta(bondVaultPDA).WRITE(),
			solana.Meta(poolAuthorityPDA),
			solana.Meta(TokenProgramID),
			solana.Meta(SystemProgramID),
			solana.Meta(RentSysvarID),
		},
		DataBytes: disc[:],
	}
}

// NewUpdateSwornMintInstruction builds the admin "update_sworn_mint" instruction.
func NewUpdateSwornMintInstruction(
	programID solana.PublicKey,
	admin solana.PublicKey,
	configPDA solana.PublicKey,
	newMint solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("update_sworn_mint")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(admin).SIGNER().WRITE(),
			solana.Meta(configPDA).WRITE(),
			solana.Meta(newMint),
		},
		DataBytes: disc[:],
	}
}

// NewSPLTransferInstruction builds a raw SPL Token Transfer instruction.
func NewSPLTransferInstruction(
	source, destination, authority solana.PublicKey,
	amount uint64,
) solana.Instruction {
	data := make([]byte, 9)
	data[0] = 3 // SPL Token Transfer index
	binary.LittleEndian.PutUint64(data[1:9], amount)
	return &solana.GenericInstruction{
		ProgID: TokenProgramID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(source).WRITE(),
			solana.Meta(destination).WRITE(),
			solana.Meta(authority).SIGNER(),
		},
		DataBytes: data,
	}
}

// NewCreateATAInstruction builds an Associated Token Account creation instruction.
func NewCreateATAInstruction(
	payer, ata, owner, mint solana.PublicKey,
) solana.Instruction {
	return &solana.GenericInstruction{
		ProgID: ATAProgramID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(payer).SIGNER().WRITE(),
			solana.Meta(ata).WRITE(),
			solana.Meta(owner),
			solana.Meta(mint),
			solana.Meta(SystemProgramID),
			solana.Meta(TokenProgramID),
		},
		DataBytes: []byte{},
	}
}

// NewProposeContractInstruction builds "propose_contract" with value and expiry args.
// Only the requester signs. Provider must accept separately via accept_proposal.
func NewProposeContractInstruction(
	programID solana.PublicKey,
	requester solana.PublicKey,
	provider solana.PublicKey,
	providerIdentityPDA solana.PublicKey,
	contractPDA solana.PublicKey,
	requesterTokenAccount solana.PublicKey,
	escrowVaultPDA solana.PublicKey,
	swornMint solana.PublicKey,
	configPDA solana.PublicKey,
	value uint64,
	expirySeconds int64,
	currency uint8,
) solana.Instruction {
	disc := AnchorDiscriminator("propose_contract")
	var data [25]byte
	copy(data[0:8], disc[:])
	binary.LittleEndian.PutUint64(data[8:16], value)
	binary.LittleEndian.PutUint64(data[16:24], uint64(expirySeconds))
	data[24] = currency
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(requester).SIGNER().WRITE(),
			solana.Meta(provider),
			solana.Meta(providerIdentityPDA),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(requesterTokenAccount).WRITE(),
			solana.Meta(escrowVaultPDA).WRITE(),
			solana.Meta(swornMint),
			solana.Meta(configPDA).WRITE(),
			solana.Meta(TokenProgramID),
			solana.Meta(SystemProgramID),
			solana.Meta(RentSysvarID),
		},
		DataBytes: data[:],
	}
}

// NewAcceptProposalInstruction builds "accept_proposal" (no extra args).
// Provider signs to accept a proposed contract by depositing stake.
func NewAcceptProposalInstruction(
	programID solana.PublicKey,
	provider solana.PublicKey,
	providerIdentityPDA solana.PublicKey,
	contractPDA solana.PublicKey,
	providerTokenAccount solana.PublicKey,
	escrowVaultPDA solana.PublicKey,
	swornMint solana.PublicKey,
	configPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("accept_proposal")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(provider).SIGNER().WRITE(),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(providerTokenAccount).WRITE(),
			solana.Meta(escrowVaultPDA).WRITE(),
			solana.Meta(configPDA),
			solana.Meta(TokenProgramID),
		},
		DataBytes: disc[:],
	}
}

// NewCancelProposalInstruction builds "cancel_proposal" (no extra args).
// Requester signs to cancel an expired proposal and reclaim escrowed funds.
func NewCancelProposalInstruction(
	programID solana.PublicKey,
	requester solana.PublicKey,
	contractPDA solana.PublicKey,
	requesterTokenAccount solana.PublicKey,
	escrowVaultPDA solana.PublicKey,
	swornMint solana.PublicKey,
	configPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("cancel_proposal")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(requester).SIGNER(),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(requesterTokenAccount).WRITE(),
			solana.Meta(escrowVaultPDA).WRITE(),
			solana.Meta(TokenProgramID),
		},
		DataBytes: disc[:],
	}
}

// NewInitiateDisputeInstruction builds "initiate_dispute" with evidence_hash arg.
// The initiator (requester) signs and pays for the dispute PDA creation.
// Accounts: requester (signer, writable), contract (writable), dispute (writable, init), system_program.
func NewInitiateDisputeInstruction(
	programID solana.PublicKey,
	initiator solana.PublicKey,
	contractPDA solana.PublicKey,
	disputePDA solana.PublicKey,
	evidenceHash [32]byte,
) solana.Instruction {
	disc := AnchorDiscriminator("initiate_dispute")
	var data [40]byte
	copy(data[0:8], disc[:])
	copy(data[8:40], evidenceHash[:])
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(initiator).SIGNER().WRITE(),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(disputePDA).WRITE(),
			solana.Meta(SystemProgramID),
		},
		DataBytes: data[:],
	}
}

// NewRespondDisputeInstruction builds "respond_dispute" with response_hash arg.
// The provider signs to respond to a dispute with correction or counter-evidence.
// Accounts: provider (signer), contract (read), dispute (writable).
func NewRespondDisputeInstruction(
	programID solana.PublicKey,
	responder solana.PublicKey,
	contractPDA solana.PublicKey,
	disputePDA solana.PublicKey,
	responseHash [32]byte,
) solana.Instruction {
	disc := AnchorDiscriminator("respond_dispute")
	var data [40]byte
	copy(data[0:8], disc[:])
	copy(data[8:40], responseHash[:])
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(responder).SIGNER(),
			solana.Meta(contractPDA),
			solana.Meta(disputePDA).WRITE(),
		},
		DataBytes: data[:],
	}
}

// NewEscalateDisputeInstruction builds "escalate_dispute" (no extra args).
// The initiator signs to escalate a dispute to the next level.
// Accounts: initiator (signer), contract (writable), dispute (writable).
func NewEscalateDisputeInstruction(
	programID solana.PublicKey,
	initiator solana.PublicKey,
	contractPDA solana.PublicKey,
	disputePDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("escalate_dispute")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(initiator).SIGNER(),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(disputePDA).WRITE(),
		},
		DataBytes: disc[:],
	}
}

// NewResolveDisputeInstruction builds "resolve_dispute" with provider_wins bool arg.
// The resolver signs. Confiscated stakes: 15% burned, 60% insurance, 25% winner.
// Accounts: resolver, contract, dispute, provider_identity, requester_identity,
//
//	provider_token_account, requester_token_account, escrow_vault,
//	insurance_pool, insurance_vault, sworn_mint, protocol_config, token_program.
func NewResolveDisputeInstruction(
	programID solana.PublicKey,
	resolver solana.PublicKey,
	contractPDA solana.PublicKey,
	disputePDA solana.PublicKey,
	providerIdentityPDA solana.PublicKey,
	requesterIdentityPDA solana.PublicKey,
	providerTokenAccount solana.PublicKey,
	requesterTokenAccount solana.PublicKey,
	escrowVaultPDA solana.PublicKey,
	insurancePoolPDA solana.PublicKey,
	insuranceVaultAccount solana.PublicKey,
	swornMint solana.PublicKey,
	configPDA solana.PublicKey,
	providerWins bool,
) solana.Instruction {
	disc := AnchorDiscriminator("resolve_dispute")
	var data [9]byte
	copy(data[0:8], disc[:])
	if providerWins {
		data[8] = 1
	}
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(resolver).SIGNER().WRITE(),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(disputePDA).WRITE(),
			solana.Meta(providerIdentityPDA).WRITE(),
			solana.Meta(requesterIdentityPDA).WRITE(),
			solana.Meta(providerTokenAccount).WRITE(),
			solana.Meta(requesterTokenAccount).WRITE(),
			solana.Meta(escrowVaultPDA).WRITE(),
			solana.Meta(insurancePoolPDA).WRITE(),
			solana.Meta(insuranceVaultAccount).WRITE(),
			solana.Meta(swornMint).WRITE(),
			solana.Meta(configPDA),
			solana.Meta(TokenProgramID),
		},
		DataBytes: data[:],
	}
}

// NewRedeliverInDisputeInstruction builds "redeliver_in_dispute" with output_hash and arweave_tx args.
// The provider signs to re-deliver corrected work during a Level 1 dispute.
// Accounts: provider (signer, writable), contract (writable), dispute (writable), proof_of_execution (writable).
func NewRedeliverInDisputeInstruction(
	programID solana.PublicKey,
	provider solana.PublicKey,
	contractPDA solana.PublicKey,
	disputePDA solana.PublicKey,
	poePDA solana.PublicKey,
	outputHash [32]byte,
	arweaveTx string,
) solana.Instruction {
	disc := AnchorDiscriminator("redeliver_in_dispute")
	// data: 8 disc + 32 hash + 4 strlen + bytes
	arweaveBytes := []byte(arweaveTx)
	data := make([]byte, 8+32+4+len(arweaveBytes))
	copy(data[0:8], disc[:])
	copy(data[8:40], outputHash[:])
	binary.LittleEndian.PutUint32(data[40:44], uint32(len(arweaveBytes)))
	copy(data[44:], arweaveBytes)
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(provider).SIGNER().WRITE(),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(disputePDA).WRITE(),
			solana.Meta(poePDA).WRITE(),
		},
		DataBytes: data,
	}
}

// NewAcceptCorrectionInstruction builds "accept_correction" (no extra args).
// The requester signs to accept a provider's correction during dispute.
// Resolves dispute + completes contract + releases payment with protocol fee.
// Accounts: requester, contract, dispute, proof_of_execution, provider_identity,
//
//	provider_token_account, treasury_token_account, insurance_vault,
//	escrow_vault, protocol_config, token_program.
func NewAcceptCorrectionInstruction(
	programID solana.PublicKey,
	requester solana.PublicKey,
	contractPDA solana.PublicKey,
	disputePDA solana.PublicKey,
	poePDA solana.PublicKey,
	providerIdentityPDA solana.PublicKey,
	providerTokenAccount solana.PublicKey,
	treasuryTokenAccount solana.PublicKey,
	insuranceVault solana.PublicKey,
	escrowVaultPDA solana.PublicKey,
	configPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("accept_correction")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(requester).SIGNER(),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(disputePDA).WRITE(),
			solana.Meta(poePDA).WRITE(),
			solana.Meta(providerIdentityPDA).WRITE(),
			solana.Meta(providerTokenAccount).WRITE(),
			solana.Meta(treasuryTokenAccount).WRITE(),
			solana.Meta(insuranceVault).WRITE(),
			solana.Meta(escrowVaultPDA).WRITE(),
			solana.Meta(configPDA),
			solana.Meta(TokenProgramID),
		},
		DataBytes: disc[:],
	}
}
