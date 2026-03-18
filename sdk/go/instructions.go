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
// providerIdentityPDA is mutable (active_contracts increment + exposure limit check).
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
			solana.Meta(providerIdentityPDA).WRITE(), // mutable: active_contracts
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
// providerIdentityPDA is required to track total_deliveries.
func NewDeliverContractInstruction(
	programID solana.PublicKey,
	provider solana.PublicKey,
	contractPDA solana.PublicKey,
	providerIdentityPDA solana.PublicKey,
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
			solana.Meta(providerIdentityPDA).WRITE(), // tracks total_deliveries
			solana.Meta(poePDA).WRITE(),
			solana.Meta(SystemProgramID),
		},
		DataBytes: data,
	}
}

// NewAcceptContractInstruction builds "accept_contract" (no extra args).
// swornMint required for 10% burn CPI on SWORN contracts. Pass zeroed key for SOL.
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
	swornMint solana.PublicKey,
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
			solana.Meta(swornMint).WRITE(), // for burn CPI (10% fee)
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
// providerIdentityPDA: mutable - validates ban, enforces exposure limit, tracks active_contracts.
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
			solana.Meta(providerIdentityPDA).WRITE(), // exposure limit + active_contracts
			solana.Meta(providerTokenAccount).WRITE(),
			solana.Meta(escrowVaultPDA).WRITE(),
			solana.Meta(configPDA),
			solana.Meta(TokenProgramID),
			solana.Meta(SystemProgramID),
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
// providerIdentityPDA required to track total_deliveries.
func NewRedeliverInDisputeInstruction(
	programID solana.PublicKey,
	provider solana.PublicKey,
	contractPDA solana.PublicKey,
	providerIdentityPDA solana.PublicKey,
	disputePDA solana.PublicKey,
	poePDA solana.PublicKey,
	outputHash [32]byte,
	arweaveTx string,
) solana.Instruction {
	disc := AnchorDiscriminator("redeliver_in_dispute")
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
			solana.Meta(providerIdentityPDA).WRITE(), // tracks total_deliveries
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
// NewAcceptCorrectionInstruction builds "accept_correction" (no extra args).
// swornMint required for 10% burn CPI on SWORN contracts.
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
	swornMint solana.PublicKey,
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
			solana.Meta(swornMint).WRITE(), // for burn CPI (10% fee)
			solana.Meta(configPDA),
			solana.Meta(TokenProgramID),
		},
		DataBytes: disc[:],
	}
}


// NewMigrateDisputeSizeInstruction builds "migrate_dispute_size" (no extra args).
// Reallocs an old Dispute account from 169 to 170 bytes.
// Accounts: payer (signer, writable), contract (read), dispute (writable), system_program.
func NewMigrateDisputeSizeInstruction(
	programID solana.PublicKey,
	payer solana.PublicKey,
	contractPDA solana.PublicKey,
	disputePDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("migrate_dispute_size")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(payer).SIGNER().WRITE(),
			solana.Meta(contractPDA),
			solana.Meta(disputePDA).WRITE(),
			solana.Meta(SystemProgramID),
		},
		DataBytes: disc[:],
	}
}

// NewCheckMaturationInstruction builds "check_maturation" (no extra args).
// Permissionless: any caller can trigger maturation check.
// Sets matured=true if agent has 14+ days AND 5+ completed tasks.
// Accounts: agent_identity_pda (writable), config_pda (read).
func NewCheckMaturationInstruction(
	programID solana.PublicKey,
	agentIdentityPDA solana.PublicKey,
	configPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("check_maturation")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(agentIdentityPDA).WRITE(),
			solana.Meta(configPDA),
		},
		DataBytes: disc[:],
	}
}

// NewTimeoutContractInstruction builds "timeout_contract" (no extra args).
// Permissionless: any caller can trigger after the 72h delivery deadline.
// Provider's stake confiscated: 60% insurance, 25% requester bonus, 15% burn.
// Accounts (in order): caller (signer, writable), contract (writable), provider_identity (writable),
// requester_token_account (writable), escrow_vault (writable), insurance_pool (writable),
// insurance_vault (writable), sworn_mint (writable), protocol_config (read), token_program.
func NewTimeoutContractInstruction(
	programID solana.PublicKey,
	caller solana.PublicKey,
	contractPDA solana.PublicKey,
	providerIdentityPDA solana.PublicKey,
	requesterTokenAccount solana.PublicKey,
	escrowVaultPDA solana.PublicKey,
	insurancePoolPDA solana.PublicKey,
	insuranceVault solana.PublicKey,
	swornMint solana.PublicKey,
	configPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("timeout_contract")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(caller).SIGNER().WRITE(),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(providerIdentityPDA).WRITE(),
			solana.Meta(requesterTokenAccount).WRITE(),
			solana.Meta(escrowVaultPDA).WRITE(),
			solana.Meta(insurancePoolPDA).WRITE(),
			solana.Meta(insuranceVault).WRITE(),
			solana.Meta(swornMint).WRITE(),
			solana.Meta(configPDA),
			solana.Meta(TokenProgramID),
		},
		DataBytes: disc[:],
	}
}

// NewMigrateAgentIdentityInstruction builds "migrate_agent_identity" (no extra args).
// Reallocs an old AgentIdentity account from 95 bytes (v1) to 123 bytes (v2).
// Inserts volume_sol, total_deliveries, corrections_received, active_contracts,
// and last_task_completed_at fields (all zero) at the correct Borsh offsets.
// Idempotent: already-migrated accounts (123 bytes) are skipped.
// Accounts: admin (signer, writable), protocol_config (read), agent_identity (writable), system_program.
func NewMigrateAgentIdentityInstruction(
	programID solana.PublicKey,
	admin solana.PublicKey,
	protocolConfigPDA solana.PublicKey,
	agentIdentityPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("migrate_agent_identity")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(admin).SIGNER().WRITE(),
			solana.Meta(protocolConfigPDA),
			solana.Meta(agentIdentityPDA).WRITE(),
			solana.Meta(SystemProgramID),
		},
		DataBytes: disc[:],
	}
}

// NewMigrateDisputeAppealStakeInstruction builds "migrate_dispute_appeal_stake" (no extra args).
// Reallocs an old Dispute account from 170 bytes to 178 bytes by appending 8 zero bytes.
// The new bytes represent appeal_stake: u64 = 0 (only non-zero at Level 4 after escalation).
// Whitepaper Section 5.4: double-or-nothing stake for Appeal level disputes.
// Idempotent: already-migrated accounts (178 bytes) are skipped.
// Accounts: payer (signer, writable), contract (read), dispute (writable), system_program.
func NewMigrateDisputeAppealStakeInstruction(
	programID solana.PublicKey,
	payer solana.PublicKey,
	contractPDA solana.PublicKey,
	disputePDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("migrate_dispute_appeal_stake")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(payer).SIGNER().WRITE(),
			solana.Meta(contractPDA),
			solana.Meta(disputePDA).WRITE(),
			solana.Meta(SystemProgramID),
		},
		DataBytes: disc[:],
	}
}

// NewCalculateTrustScoreInstruction builds "calculate_trust_score" with sol_to_sworn_rate arg.
// Permissionless: any caller can trigger TrustScore recalculation.
// Implements full whitepaper formula (5 factors + penalties + decay) on-chain.
// solToSwornRate: SOL lamport to SWORN lamport exchange rate for volume normalization.
//   Use 0 to treat SOL volume as 0 (conservative).
// Accounts: agent_identity_pda (writable), config_pda (read).
func NewCalculateTrustScoreInstruction(
	programID solana.PublicKey,
	agentIdentityPDA solana.PublicKey,
	configPDA solana.PublicKey,
	solToSwornRate uint64,
) solana.Instruction {
	disc := AnchorDiscriminator("calculate_trust_score")
	var data [16]byte
	copy(data[0:8], disc[:])
	binary.LittleEndian.PutUint64(data[8:16], solToSwornRate)
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(agentIdentityPDA).WRITE(),
			solana.Meta(configPDA),
		},
		DataBytes: data[:],
	}
}
// NewEscalateToAppealInstruction builds the "escalate_to_appeal" instruction.
// Level 3 → Level 4 (Appeal) escalation.
// Whitepaper §5.4: the escalating party deposits 50% of the contract value as
// appeal_stake ("double-or-nothing"). If they win, it is returned; if they lose,
// it is confiscated 60/25/15 (insurance/winner/burn).
//
// For SWORN contracts: pass escalatorATA and escrowVault (SWORN token accounts).
// For SOL contracts: pass any valid pubkey for both (they are not used on-chain).
//
// Accounts: escalator SIGNER+WRITE, contractPDA WRITE, disputePDA WRITE,
// escalatorTokenAccount WRITE, escrowVault WRITE, tokenProgram, systemProgram.
func NewEscalateToAppealInstruction(
	programID solana.PublicKey,
	escalator solana.PublicKey,
	contractPDA solana.PublicKey,
	disputePDA solana.PublicKey,
	escalatorTokenAccount solana.PublicKey,
	escrowVault solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("escalate_to_appeal")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(escalator).SIGNER().WRITE(),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(disputePDA).WRITE(),
			solana.Meta(escalatorTokenAccount).WRITE(),
			solana.Meta(escrowVault).WRITE(),
			solana.Meta(TokenProgramID),
			solana.Meta(SystemProgramID),
		},
		DataBytes: disc[:],
	}
}

// NewJuryVoteInstruction builds the "jury_vote" instruction.
// Args: voteForProvider bool (Borsh: 1 byte, 0x00=false, 0x01=true)
// Accounts: juror (signer,writable), contract, dispute (writable), jurorIdentity,
//           voteRecord (writable, init PDA [b"vote",dispute,juror]), systemProgram
// Whitepaper Section 5.3: TrustScore>70 required, one vote per juror per dispute.
// double-vote prevention: init fails if voteRecord already exists.
func NewJuryVoteInstruction(
	programID solana.PublicKey,
	juror solana.PublicKey,
	contractPDA solana.PublicKey,
	disputePDA solana.PublicKey,
	jurorIdentityPDA solana.PublicKey,
	voteRecordPDA solana.PublicKey,
	voteForProvider bool,
) solana.Instruction {
	disc := AnchorDiscriminator("jury_vote")
	voteArg := uint8(0)
	if voteForProvider {
		voteArg = 1
	}
	data := make([]byte, 9) // 8 disc + 1 bool
	copy(data[:8], disc[:])
	data[8] = voteArg
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(juror).SIGNER().WRITE(),
			solana.Meta(contractPDA),
			solana.Meta(disputePDA).WRITE(),
			solana.Meta(jurorIdentityPDA),
			solana.Meta(voteRecordPDA).WRITE(),
			solana.Meta(SystemProgramID),
		},
		DataBytes: data,
	}
}

// FindVoteRecordPDA derives the PDA for a jury vote record.
// Seeds: [b"vote", disputePDA, jurorPubkey]
func FindVoteRecordPDA(programID, disputePDA, juror solana.PublicKey) (solana.PublicKey, uint8, error) {
	return solana.FindProgramAddress(
		[][]byte{[]byte("vote"), disputePDA[:], juror[:]},
		programID,
	)
}

// NewSponsorAgentInstruction builds the "sponsor_agent" instruction.
// An established agent (TrustScore >= 50, matured) vouches for a newcomer.
// Whitepaper Section 2.3: Anti-Sybil Layer B - Web-of-Trust, sponsor risks own stake.
// Args: bonusPoints u16 (2 bytes LE, capped at 5 by on-chain handler).
// Accounts: sponsor (Signer), sponsorIdentity (AgentIdentity, read),
//           agentIdentity (AgentIdentity, writable)
func NewSponsorAgentInstruction(
	programID solana.PublicKey,
	sponsor solana.PublicKey,
	sponsorIdentityPDA solana.PublicKey,
	agentIdentityPDA solana.PublicKey,
	bonusPoints uint16,
) solana.Instruction {
	disc := AnchorDiscriminator("sponsor_agent")
	data := make([]byte, 10) // 8 disc + 2 u16
	copy(data[:8], disc[:])
	binary.LittleEndian.PutUint16(data[8:10], bonusPoints)
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(sponsor).SIGNER(),
			solana.Meta(sponsorIdentityPDA),
			solana.Meta(agentIdentityPDA).WRITE(),
		},
		DataBytes: data,
	}
}

// NewFileInsuranceClaimInstruction builds the "file_insurance_claim" instruction.
// A requester who discovers a subtle defect within 90 days can file a retroactive claim.
// Whitepaper Section 6: Insurance Pool - max payout 80% of contract value.
// Args: amount u64 (8 bytes LE) + evidenceHash [32]byte = 40 bytes total.
// Accounts: claimant (Signer+Write), contract (read), insuranceClaim (Write, init PDA),
//           insurancePool (Write), claimantTokenAccount (Write), insuranceVault (Write),
//           protocolConfig (read), tokenProgram, systemProgram.
func NewFileInsuranceClaimInstruction(
	programID solana.PublicKey,
	claimant solana.PublicKey,
	contractPDA solana.PublicKey,
	insuranceClaimPDA solana.PublicKey,
	insurancePoolPDA solana.PublicKey,
	claimantTokenAccount solana.PublicKey,
	insuranceVault solana.PublicKey,
	configPDA solana.PublicKey,
	amount uint64,
	evidenceHash [32]byte,
) solana.Instruction {
	disc := AnchorDiscriminator("file_insurance_claim")
	data := make([]byte, 8+8+32) // 8 disc + 8 amount + 32 hash
	copy(data[:8], disc[:])
	binary.LittleEndian.PutUint64(data[8:16], amount)
	copy(data[16:48], evidenceHash[:])
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(claimant).SIGNER().WRITE(),
			solana.Meta(contractPDA),
			solana.Meta(insuranceClaimPDA).WRITE(),
			solana.Meta(insurancePoolPDA).WRITE(),
			solana.Meta(claimantTokenAccount).WRITE(),
			solana.Meta(insuranceVault).WRITE(),
			solana.Meta(configPDA),
			solana.Meta(TokenProgramID),
			solana.Meta(SystemProgramID),
		},
		DataBytes: data,
	}
}


// NewApproveInsuranceClaimInstruction builds the "approve_insurance_claim" instruction.
// Admin-only in Phase 0-2. Pays out claim amount to claimant, returns collateral.
// Whitepaper Section 6: Insurance Pool retroactive claims.
// Accounts: admin (Signer), insuranceClaim (Write), insurancePool (Write),
//           insuranceVault (Write), claimantTokenAccount (Write),
//           poolAuthority (read PDA), providerIdentity (Write, +fraud_flag),
//           protocolConfig (read), tokenProgram.
func NewApproveInsuranceClaimInstruction(
	programID solana.PublicKey,
	admin solana.PublicKey,
	insuranceClaimPDA solana.PublicKey,
	insurancePoolPDA solana.PublicKey,
	insuranceVault solana.PublicKey,
	claimantTokenAccount solana.PublicKey,
	poolAuthorityPDA solana.PublicKey,
	providerIdentityPDA solana.PublicKey,
	configPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("approve_insurance_claim")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(admin).SIGNER(),
			solana.Meta(insuranceClaimPDA).WRITE(),
			solana.Meta(insurancePoolPDA).WRITE(),
			solana.Meta(insuranceVault).WRITE(),
			solana.Meta(claimantTokenAccount).WRITE(),
			solana.Meta(poolAuthorityPDA),
			solana.Meta(providerIdentityPDA).WRITE(),
			solana.Meta(configPDA),
			solana.Meta(TokenProgramID),
		},
		DataBytes: disc[:],
	}
}

// NewDenyInsuranceClaimInstruction builds the "deny_insurance_claim" instruction.
// Admin-only in Phase 0-2. Forfeits collateral to insurance pool (anti-spam penalty).
// Whitepaper Section 6: Insurance Pool retroactive claims.
// Accounts: same as ApproveInsuranceClaim (shared Anchor Accounts struct).
func NewDenyInsuranceClaimInstruction(
	programID solana.PublicKey,
	admin solana.PublicKey,
	insuranceClaimPDA solana.PublicKey,
	insurancePoolPDA solana.PublicKey,
	insuranceVault solana.PublicKey,
	claimantTokenAccount solana.PublicKey,
	poolAuthorityPDA solana.PublicKey,
	providerIdentityPDA solana.PublicKey,
	configPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("deny_insurance_claim")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(admin).SIGNER(),
			solana.Meta(insuranceClaimPDA).WRITE(),
			solana.Meta(insurancePoolPDA).WRITE(),
			solana.Meta(insuranceVault).WRITE(),
			solana.Meta(claimantTokenAccount).WRITE(),
			solana.Meta(poolAuthorityPDA),
			solana.Meta(providerIdentityPDA).WRITE(),
			solana.Meta(configPDA),
			solana.Meta(TokenProgramID),
		},
		DataBytes: disc[:],
	}
}

// ---------------------------------------------------------------------------
// update_config — GAP-1/GAP-2 fix: admin updates min_bond and maturation_period
// ---------------------------------------------------------------------------

// UpdateConfigParams mirrors the Rust UpdateConfigParams struct (Borsh).
// Each field is Option<T>: 0-byte prefix = None, 1-byte prefix + value = Some(v).
type UpdateConfigParams struct {
	MinIdentityBond *uint64
	MaxIdentityBond *uint64
	MaturationPeriod *int64
}

func (p UpdateConfigParams) encode() []byte {
	buf := make([]byte, 0, 64)
	writeOptionU64 := func(v *uint64) {
		if v == nil {
			buf = append(buf, 0)
		} else {
			buf = append(buf, 1)
			b := make([]byte, 8)
			binary.LittleEndian.PutUint64(b, *v)
			buf = append(buf, b...)
		}
	}
	writeOptionI64 := func(v *int64) {
		if v == nil {
			buf = append(buf, 0)
		} else {
			buf = append(buf, 1)
			b := make([]byte, 8)
			binary.LittleEndian.PutUint64(b, uint64(*v))
			buf = append(buf, b...)
		}
	}
	writeOptionU64(p.MinIdentityBond)
	writeOptionU64(p.MaxIdentityBond)
	writeOptionI64(p.MaturationPeriod)
	return buf
}

// NewUpdateConfigInstruction builds the update_config instruction.
// programID: deployed program address. admin: protocol admin signer. configPDA: ProtocolConfig PDA.
func NewUpdateConfigInstruction(
	programID solana.PublicKey,
	admin solana.PublicKey,
	configPDA solana.PublicKey,
	params UpdateConfigParams,
) solana.Instruction {
	disc := AnchorDiscriminator("update_config")
	data := append(disc[:], params.encode()...)
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(admin).SIGNER().WRITE(),
			solana.Meta(configPDA).WRITE(),
		},
		DataBytes: data,
	}
}

// ---------------------------------------------------------------------------
// timeout_delivery — GAP-11: requester ghosting protection
// ---------------------------------------------------------------------------

// NewTimeoutDeliveryInstruction builds the timeout_delivery instruction.
// programID: deployed program address. All other accounts mirror TimeoutDelivery struct in contract.rs.
func NewTimeoutDeliveryInstruction(
	programID solana.PublicKey,
	caller solana.PublicKey,
	contractPDA solana.PublicKey,
	poePDA solana.PublicKey,
	providerIdentityPDA solana.PublicKey,
	providerTokenAccount solana.PublicKey,
	treasuryTokenAccount solana.PublicKey,
	insuranceVault solana.PublicKey,
	escrowVault solana.PublicKey,
	swornMint solana.PublicKey,
	configPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("timeout_delivery")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(caller).SIGNER().WRITE(),
			solana.Meta(contractPDA).WRITE(),
			solana.Meta(poePDA).WRITE(),
			solana.Meta(providerIdentityPDA).WRITE(),
			solana.Meta(providerTokenAccount).WRITE(),
			solana.Meta(treasuryTokenAccount).WRITE(),
			solana.Meta(insuranceVault).WRITE(),
			solana.Meta(escrowVault).WRITE(),
			solana.Meta(swornMint).WRITE(),
			solana.Meta(configPDA),
			solana.Meta(TokenProgramID),
		},
		DataBytes: disc[:],
	}
}


// ---------------------------------------------------------------------------
// hibernate_agent — Whitepaper §8.6: Declared hibernation
// ---------------------------------------------------------------------------

// NewHibernateAgentInstruction builds the hibernate_agent instruction.
// durationMonths: 1-12 months of hibernation. Reduced decay (0.5/month) during this period.
func NewHibernateAgentInstruction(
	programID solana.PublicKey,
	agent solana.PublicKey,
	agentIdentityPDA solana.PublicKey,
	durationMonths uint8,
) solana.Instruction {
	disc := AnchorDiscriminator("hibernate_agent")
	data := append(disc[:], durationMonths)
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(agent).SIGNER().WRITE(),
			solana.Meta(agentIdentityPDA).WRITE(),
		},
		DataBytes: data,
	}
}

// NewWakeAgentInstruction builds the wake_agent instruction (early exit from hibernation).
func NewWakeAgentInstruction(
	programID solana.PublicKey,
	agent solana.PublicKey,
	agentIdentityPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("wake_agent")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(agent).SIGNER().WRITE(),
			solana.Meta(agentIdentityPDA).WRITE(),
		},
		DataBytes: disc[:],
	}
}

// NewExpireHibernationInstruction builds the expire_hibernation instruction (permissionless).
// Can be called by anyone once hibernation max duration has passed.
func NewExpireHibernationInstruction(
	programID solana.PublicKey,
	caller solana.PublicKey,
	agentIdentityPDA solana.PublicKey,
) solana.Instruction {
	disc := AnchorDiscriminator("expire_hibernation")
	return &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(caller).SIGNER().WRITE(),
			solana.Meta(agentIdentityPDA).WRITE(),
		},
		DataBytes: disc[:],
	}
}
