// Package trustprotocol provides a Go SDK for interacting with the
// Trust Protocol Anchor program on Solana.
//
// It includes on-chain account struct definitions with Borsh
// deserialization, PDA derivation helpers, whitepaper-compliant
// formulas, and Anchor instruction builders.
package trustprotocol

import (
	"encoding/binary"
	"fmt"

	"github.com/gagliardetto/solana-go"
)

// ---------------------------------------------------------------------------
// On-chain account structs (mirror Anchor state.rs exactly)
// ---------------------------------------------------------------------------

// AgentIdentity represents a soulbound agent identity on-chain.
// Anchor discriminator: 8 bytes, then 87 bytes of fields = 95 total.
type AgentIdentity struct {
	Authority       solana.PublicKey `json:"authority"`
	IdentityBond    uint64           `json:"identity_bond"`
	RegisteredAt    int64            `json:"registered_at"`
	Matured         bool             `json:"matured"`
	TrustScore      uint16           `json:"trust_score"`
	TasksCompleted  uint64           `json:"tasks_completed"`
	VolumeProcessed uint64           `json:"volume_processed"`
	DisputesLost    uint32           `json:"disputes_lost"`
	DisputesWon     uint32           `json:"disputes_won"`
	TasksAbandoned  uint32           `json:"tasks_abandoned"`
	FraudFlags      uint32           `json:"fraud_flags"`
	SponsorBonus    uint16           `json:"sponsor_bonus"`
	Banned          bool             `json:"banned"`
	Bump            uint8            `json:"bump"`
}

// AgentIdentitySize is the on-chain size including Anchor discriminator.
const AgentIdentitySize = 8 + 32 + 8 + 8 + 1 + 2 + 8 + 8 + 4 + 4 + 4 + 4 + 2 + 1 + 1 // 95

// DecodeAgentIdentity parses raw account data (including 8-byte discriminator).
func DecodeAgentIdentity(data []byte) (*AgentIdentity, error) {
	if len(data) < AgentIdentitySize {
		return nil, fmt.Errorf("agent identity data too short: %d < %d", len(data), AgentIdentitySize)
	}
	o := 8 // skip discriminator
	a := &AgentIdentity{}
	a.Authority = solana.PublicKeyFromBytes(data[o : o+32])
	o += 32
	a.IdentityBond = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	a.RegisteredAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	a.Matured = data[o] == 1
	o++
	a.TrustScore = binary.LittleEndian.Uint16(data[o : o+2])
	o += 2
	a.TasksCompleted = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	a.VolumeProcessed = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	a.DisputesLost = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
	a.DisputesWon = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
	a.TasksAbandoned = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
	a.FraudFlags = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
	a.SponsorBonus = binary.LittleEndian.Uint16(data[o : o+2])
	o += 2
	a.Banned = data[o] == 1
	o++
	a.Bump = data[o]
	return a, nil
}

// DID returns the decentralized identifier for this agent.
func (a *AgentIdentity) DID() string {
	return "did:trust:" + a.Authority.String()
}

// ProtocolConfig represents the global protocol configuration on-chain.
// Anchor discriminator: 8 bytes, then 125 bytes = 133 total.
type ProtocolConfig struct {
	Admin                   solana.PublicKey `json:"admin"`
	SwornMint               solana.PublicKey `json:"sworn_mint"`
	MinIdentityBond         uint64           `json:"min_identity_bond"`
	MaxIdentityBond         uint64           `json:"max_identity_bond"`
	MaturationPeriod        int64            `json:"maturation_period"`
	MinStakeFactorBps       uint16           `json:"min_stake_factor_bps"`
	MaxStakeFactorBps       uint16           `json:"max_stake_factor_bps"`
	BurnRateBps             uint16           `json:"burn_rate_bps"`
	InsuranceRateBps        uint16           `json:"insurance_rate_bps"`
	ClaimWindow             int64            `json:"claim_window"`
	MaxClaimPayoutBps       uint16           `json:"max_claim_payout_bps"`
	ExposureLimitMultiplier uint8            `json:"exposure_limit_multiplier"`
	GovernancePhase         uint8            `json:"governance_phase"`
	TotalContracts          uint64           `json:"total_contracts"`
	TotalAgents             uint64           `json:"total_agents"`
	Bump                    uint8            `json:"bump"`
}

// ProtocolConfigSize is the on-chain size including Anchor discriminator.
const ProtocolConfigSize = 133

// DecodeProtocolConfig parses raw account data (including 8-byte discriminator).
func DecodeProtocolConfig(data []byte) (*ProtocolConfig, error) {
	if len(data) < ProtocolConfigSize {
		return nil, fmt.Errorf("protocol config data too short: %d < %d", len(data), ProtocolConfigSize)
	}
	o := 8
	c := &ProtocolConfig{}
	c.Admin = solana.PublicKeyFromBytes(data[o : o+32])
	o += 32
	c.SwornMint = solana.PublicKeyFromBytes(data[o : o+32])
	o += 32
	c.MinIdentityBond = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	c.MaxIdentityBond = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	c.MaturationPeriod = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	c.MinStakeFactorBps = binary.LittleEndian.Uint16(data[o : o+2])
	o += 2
	c.MaxStakeFactorBps = binary.LittleEndian.Uint16(data[o : o+2])
	o += 2
	c.BurnRateBps = binary.LittleEndian.Uint16(data[o : o+2])
	o += 2
	c.InsuranceRateBps = binary.LittleEndian.Uint16(data[o : o+2])
	o += 2
	c.ClaimWindow = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	c.MaxClaimPayoutBps = binary.LittleEndian.Uint16(data[o : o+2])
	o += 2
	c.ExposureLimitMultiplier = data[o]
	o++
	c.GovernancePhase = data[o]
	o++
	c.TotalContracts = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	c.TotalAgents = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	c.Bump = data[o]
	return c, nil
}

// ContractStatus represents the lifecycle state of a contract.
type ContractStatus uint8

const (
	ContractStatusCreated           ContractStatus = 0
	ContractStatusActive            ContractStatus = 1
	ContractStatusDelivered         ContractStatus = 2
	ContractStatusCompleted         ContractStatus = 3
	ContractStatusDisputed          ContractStatus = 4
	ContractStatusCancelled         ContractStatus = 5
	ContractStatusResolvedProvider  ContractStatus = 6
	ContractStatusResolvedRequester ContractStatus = 7
)

// String returns the human-readable name.
func (s ContractStatus) String() string {
	names := [...]string{"Created", "Active", "Delivered", "Completed", "Disputed", "Cancelled", "ResolvedProvider", "ResolvedRequester"}
	if int(s) < len(names) {
		return names[s]
	}
	return fmt.Sprintf("Unknown(%d)", s)
}

// Contract represents an on-chain contract between two agents.
type Contract struct {
	ID             uint64           `json:"id"`
	Requester      solana.PublicKey `json:"requester"`
	Provider       solana.PublicKey `json:"provider"`
	Value          uint64           `json:"value"`
	ProviderStake  uint64           `json:"provider_stake"`
	RequesterStake uint64           `json:"requester_stake"`
	Status         ContractStatus   `json:"status"`
	CreatedAt      int64            `json:"created_at"`
	ResolvedAt     int64            `json:"resolved_at"`
	PoeHash        [32]byte         `json:"poe_hash"`
	PoeArweaveTx   string           `json:"poe_arweave_tx"`
	DisputeLevel   uint8            `json:"dispute_level"`
	Bump           uint8            `json:"bump"`
}

// DecodeContract parses raw account data (including 8-byte discriminator).
// Note: the poe_arweave_tx is a Borsh string (4-byte LE length prefix + bytes).
func DecodeContract(data []byte) (*Contract, error) {
	minSize := 8 + 8 + 32 + 32 + 8 + 8 + 8 + 1 + 8 + 8 + 32 + 4 // 157 minimum
	if len(data) < minSize {
		return nil, fmt.Errorf("contract data too short: %d < %d", len(data), minSize)
	}
	o := 8
	c := &Contract{}
	c.ID = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	c.Requester = solana.PublicKeyFromBytes(data[o : o+32])
	o += 32
	c.Provider = solana.PublicKeyFromBytes(data[o : o+32])
	o += 32
	c.Value = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	c.ProviderStake = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	c.RequesterStake = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	c.Status = ContractStatus(data[o])
	o++
	c.CreatedAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	c.ResolvedAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	copy(c.PoeHash[:], data[o:o+32])
	o += 32
	// Borsh string: 4-byte LE length + bytes
	if o+4 <= len(data) {
		strLen := int(binary.LittleEndian.Uint32(data[o : o+4]))
		o += 4
		if o+strLen <= len(data) {
			c.PoeArweaveTx = string(data[o : o+strLen])
			o += strLen
		}
	}
	if o < len(data) {
		c.DisputeLevel = data[o]
		o++
	}
	if o < len(data) {
		c.Bump = data[o]
	}
	return c, nil
}
