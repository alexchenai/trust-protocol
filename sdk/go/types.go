// Package trustprotocol provides a Go SDK for interacting with the
// Trust Protocol Anchor program on Solana.
//
// It includes on-chain account struct definitions with Borsh
// deserialization, PDA derivation helpers, whitepaper-compliant
// formulas, and Anchor instruction builders.
package trustprotocol

import (
	"crypto/sha256"
	"encoding/binary"
	"fmt"

	"github.com/gagliardetto/solana-go"
)

// AccountDiscriminator returns the 8-byte Anchor account discriminator
// for the given account name: sha256("account:<Name>")[0:8].
func AccountDiscriminator(accountName string) [8]byte {
	h := sha256.Sum256([]byte("account:" + accountName))
	var d [8]byte
	copy(d[:], h[:8])
	return d
}

// ---------------------------------------------------------------------------
// On-chain account structs (mirror Anchor state.rs exactly)
// ---------------------------------------------------------------------------

// AgentIdentity represents a soulbound agent identity on-chain.
// Anchor discriminator: 8 bytes, then 115 bytes of fields = 123 total.
// Field order mirrors state.rs exactly (Borsh serialization).
type AgentIdentity struct {
	Authority                    solana.PublicKey `json:"authority"`
	IdentityBond                 uint64           `json:"identity_bond"`
	RegisteredAt                 int64            `json:"registered_at"`
	Matured                      bool             `json:"matured"`
	TrustScore                   uint16           `json:"trust_score"`
	TasksCompleted               uint64           `json:"tasks_completed"`
	VolumeProcessed              uint64           `json:"volume_processed"` // SWORN lamports
	VolumeSol                    uint64           `json:"volume_sol"`       // SOL lamports
	DisputesLost                 uint32           `json:"disputes_lost"`
	DisputesWon                  uint32           `json:"disputes_won"`
	TasksAbandoned               uint32           `json:"tasks_abandoned"`
	FraudFlags                   uint32           `json:"fraud_flags"`
	TotalDeliveries              uint32           `json:"total_deliveries"`
	CorrectionsReceived          uint32           `json:"corrections_received"`
	ActiveContracts              uint32           `json:"active_contracts"`
	LastTaskCompletedAt          int64            `json:"last_task_completed_at"`
	SponsorBonus                 uint16           `json:"sponsor_bonus"`
	Banned                       bool             `json:"banned"`
	// Hibernation fields (Whitepaper §8.6)
	IsHibernating                bool             `json:"is_hibernating"`
	HibernationStartedAt         int64            `json:"hibernation_started_at"`
	HibernationEndsAt            int64            `json:"hibernation_ends_at"`
	TasksSinceLastHibernation    uint32           `json:"tasks_since_last_hibernation"`
	Bump                         uint8            `json:"bump"`
}

// AgentIdentitySize is the on-chain size including Anchor discriminator.
// 8 disc + 32(authority) + 8(bond) + 8(registered_at) + 1(matured) + 2(trust_score)
// + 8(tasks_completed) + 8(volume_processed) + 8(volume_sol)
// + 4(disputes_lost) + 4(disputes_won) + 4(tasks_abandoned) + 4(fraud_flags)
// + 4(total_deliveries) + 4(corrections_received) + 4(active_contracts)
// + 8(last_task_completed_at) + 2(sponsor_bonus) + 1(banned)
// + 1(is_hibernating) + 8(hibernation_started_at) + 8(hibernation_ends_at)
// + 4(tasks_since_last_hibernation) + 1(bump) = 144
const AgentIdentitySize = 144

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
	a.VolumeSol = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	a.DisputesLost = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
	a.DisputesWon = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
	a.TasksAbandoned = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
	a.FraudFlags = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
	a.TotalDeliveries = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
	a.CorrectionsReceived = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
	a.ActiveContracts = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
	a.LastTaskCompletedAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	a.SponsorBonus = binary.LittleEndian.Uint16(data[o : o+2])
	o += 2
	a.Banned = data[o] == 1
	o++
	// Hibernation fields (§8.6)
	a.IsHibernating = data[o] == 1
	o++
	a.HibernationStartedAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	a.HibernationEndsAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	a.TasksSinceLastHibernation = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
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
	ProtocolFeeSwornBps     uint16           `json:"protocol_fee_sworn_bps"`
	ProtocolFeeSolBps       uint16           `json:"protocol_fee_sol_bps"`
	MaxCorrections          uint8            `json:"max_corrections"`
	DeadlineValidation      int64            `json:"deadline_validation"`
	Bump                    uint8            `json:"bump"`
}

// ProtocolConfigSize is the on-chain size including Anchor discriminator.
const ProtocolConfigSize = 146

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
	if o+2 <= len(data) {
		c.ProtocolFeeSwornBps = binary.LittleEndian.Uint16(data[o : o+2])
		o += 2
	}
	if o+2 <= len(data) {
		c.ProtocolFeeSolBps = binary.LittleEndian.Uint16(data[o : o+2])
		o += 2
	}
	if o < len(data) {
		c.MaxCorrections = data[o]
		o++
	}
	if o+8 <= len(data) {
		c.DeadlineValidation = int64(binary.LittleEndian.Uint64(data[o : o+8]))
		o += 8
	}
	if o < len(data) {
		c.Bump = data[o]
	}
	return c, nil
}

// Currency represents the denomination for a contract.
type Currency uint8

const (
	CurrencySworn Currency = 0
	CurrencySol   Currency = 1
)

// String returns the human-readable currency name.
func (cur Currency) String() string {
	switch cur {
	case CurrencySol:
		return "SOL"
	default:
		return "SWORN"
	}
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
	ContractStatusProposed          ContractStatus = 8
	ContractStatusCancelledExpired  ContractStatus = 9
)

// String returns the human-readable name.
func (s ContractStatus) String() string {
	names := [...]string{"Created", "Active", "Delivered", "Completed", "Disputed", "Cancelled", "ResolvedProvider", "ResolvedRequester", "Proposed", "Cancelled"}
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
	DisputeLevel          uint8            `json:"dispute_level"`
	Bump                  uint8            `json:"bump"`
	ProposalExpiresAt     int64            `json:"proposal_expires_at"`
	ProviderStakeRequired uint64           `json:"provider_stake_required"`
	Currency              Currency         `json:"currency"`
	EscrowFactorBps       uint16           `json:"escrow_factor_bps"`
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
		o++
	}
	if o+8 <= len(data) {
		c.ProposalExpiresAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
		o += 8
	}
	if o+8 <= len(data) {
		c.ProviderStakeRequired = binary.LittleEndian.Uint64(data[o : o+8])
		o += 8
	}
	if o < len(data) {
		c.Currency = Currency(data[o])
		o++
	}
	if o+2 <= len(data) {
		c.EscrowFactorBps = binary.LittleEndian.Uint16(data[o : o+2])
	}
	return c, nil
}

// DisputeLevel represents the 4-level dispute escalation path.
type DisputeLevel uint8

const (
	DisputeLevelDirectCorrection DisputeLevel = 0
	DisputeLevelPrivateRounds    DisputeLevel = 1
	DisputeLevelPublicJury       DisputeLevel = 2
	DisputeLevelAppeal           DisputeLevel = 3
)

func (d DisputeLevel) String() string {
	switch d {
	case DisputeLevelDirectCorrection:
		return "DirectCorrection"
	case DisputeLevelPrivateRounds:
		return "PrivateRounds"
	case DisputeLevelPublicJury:
		return "PublicJury"
	case DisputeLevelAppeal:
		return "Appeal"
	default:
		return fmt.Sprintf("Unknown(%d)", d)
	}
}

// DisputeStatus represents the current state of a dispute.
type DisputeStatus uint8

const (
	DisputeStatusOpen             DisputeStatus = 0
	DisputeStatusResponded        DisputeStatus = 1
	DisputeStatusVoting           DisputeStatus = 2
	DisputeStatusResolvedProvider DisputeStatus = 3
	DisputeStatusResolvedRequester DisputeStatus = 4
	DisputeStatusEscalated        DisputeStatus = 5
	// JuryDecided: quorum reached, pending permissionless settlement (whitepaper §5.3)
	DisputeStatusJuryDecided      DisputeStatus = 6
)

func (d DisputeStatus) String() string {
	switch d {
	case DisputeStatusOpen:
		return "Open"
	case DisputeStatusResponded:
		return "Responded"
	case DisputeStatusVoting:
		return "Voting"
	case DisputeStatusResolvedProvider:
		return "ResolvedProvider"
	case DisputeStatusResolvedRequester:
		return "ResolvedRequester"
	case DisputeStatusEscalated:
		return "Escalated"
	case DisputeStatusJuryDecided:
		return "JuryDecided"
	default:
		return fmt.Sprintf("Unknown(%d)", d)
	}
}

// Dispute represents an on-chain dispute account.
type Dispute struct {
	Contract         solana.PublicKey `json:"contract"`
	Initiator        solana.PublicKey `json:"initiator"`
	Level            DisputeLevel     `json:"level"`
	Status           DisputeStatus    `json:"status"`
	EvidenceHash     [32]byte         `json:"evidence_hash"`
	ResponseHash     [32]byte         `json:"response_hash"`
	VotesProvider    uint16           `json:"votes_provider"`
	VotesRequester   uint16           `json:"votes_requester"`
	JurySize         uint16           `json:"jury_size"`
	Deadline         int64            `json:"deadline"`
	CreatedAt        int64            `json:"created_at"`
	ResolvedAt       int64            `json:"resolved_at"`
	Bump             uint8            `json:"bump"`
	CorrectionsCount uint8            `json:"corrections_count"`
	// AppealStake is deposited by the escalating party at Level 4 (Whitepaper Section 5.4).
	// On loss, forfeited: 60% insurance, 25% winner, 15% burn (double-or-nothing).
	// Zero for all levels below Appeal. Added in migrate_dispute_appeal_stake migration.
	AppealStake uint64 `json:"appeal_stake"`
}

// DisputeSize is the on-chain size including Anchor discriminator.
// v0.1.6: 8 + 32 + 32 + 1 + 1 + 32 + 32 + 2 + 2 + 2 + 8 + 8 + 8 + 1 = 169
// v0.1.7: + 1 (corrections_count) = 170
// v0.1.12: + 8 (appeal_stake) = 178
const DisputeSize = 178

// DecodeDispute parses raw account data (including 8-byte discriminator).
// Backward compatible: corrections_count and appeal_stake are zero if data is shorter.
func DecodeDispute(data []byte) (*Dispute, error) {
	const minSize = 169 // v0.1.6 minimum
	if len(data) < minSize {
		return nil, fmt.Errorf("dispute data too short: %d < %d", len(data), minSize)
	}
	o := 8 // skip discriminator
	d := &Dispute{}
	d.Contract = solana.PublicKeyFromBytes(data[o : o+32])
	o += 32
	d.Initiator = solana.PublicKeyFromBytes(data[o : o+32])
	o += 32
	d.Level = DisputeLevel(data[o])
	o++
	d.Status = DisputeStatus(data[o])
	o++
	copy(d.EvidenceHash[:], data[o:o+32])
	o += 32
	copy(d.ResponseHash[:], data[o:o+32])
	o += 32
	d.VotesProvider = binary.LittleEndian.Uint16(data[o : o+2])
	o += 2
	d.VotesRequester = binary.LittleEndian.Uint16(data[o : o+2])
	o += 2
	d.JurySize = binary.LittleEndian.Uint16(data[o : o+2])
	o += 2
	d.Deadline = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	d.CreatedAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	d.ResolvedAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	d.Bump = data[o]
	o++
	if o < len(data) {
		d.CorrectionsCount = data[o]
		o++
	}
	if o+8 <= len(data) {
		d.AppealStake = binary.LittleEndian.Uint64(data[o : o+8])
	}
	return d, nil
}

// ProofOfExecution represents an on-chain PoE record with input/output hashes.
type ProofOfExecution struct {
	Contract    solana.PublicKey `json:"contract"`
	Provider    solana.PublicKey `json:"provider"`
	InputHash   [32]byte        `json:"input_hash"`
	OutputHash  [32]byte        `json:"output_hash"`
	SubmittedAt int64           `json:"submitted_at"`
	Validated   bool            `json:"validated"`
	ArweaveTx   string          `json:"arweave_tx"`
	Bump        uint8           `json:"bump"`
}

// ProofOfExecutionMinSize is the minimum account size (empty arweave_tx string).
const ProofOfExecutionMinSize = 8 + 32 + 32 + 32 + 32 + 8 + 1 + 4 + 1 // 150

// DecodeProofOfExecution parses raw account data (including 8-byte discriminator).
func DecodeProofOfExecution(data []byte) (*ProofOfExecution, error) {
	if len(data) < ProofOfExecutionMinSize {
		return nil, fmt.Errorf("poe data too short: %d < %d", len(data), ProofOfExecutionMinSize)
	}
	o := 8 // skip discriminator
	p := &ProofOfExecution{}
	p.Contract = solana.PublicKeyFromBytes(data[o : o+32])
	o += 32
	p.Provider = solana.PublicKeyFromBytes(data[o : o+32])
	o += 32
	copy(p.InputHash[:], data[o:o+32])
	o += 32
	copy(p.OutputHash[:], data[o:o+32])
	o += 32
	p.SubmittedAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	p.Validated = data[o] != 0
	o++
	if o+4 <= len(data) {
		strLen := int(binary.LittleEndian.Uint32(data[o : o+4]))
		o += 4
		if o+strLen <= len(data) {
			p.ArweaveTx = string(data[o : o+strLen])
			o += strLen
		}
	}
	if o < len(data) {
		p.Bump = data[o]
	}
	return p, nil
}

// VoteRecord represents an on-chain jury vote record.
// One per juror per dispute. Prevents double-voting via PDA uniqueness.
// Seeds: [b"vote", dispute_key, juror_key]
// Size: 8 (discriminator) + 32 + 32 + 1 + 8 + 1 = 82 bytes total
type VoteRecord struct {
	Dispute         solana.PublicKey `json:"dispute"`
	Juror           solana.PublicKey `json:"juror"`
	VoteForProvider bool             `json:"vote_for_provider"`
	VotedAt         int64            `json:"voted_at"`
	Bump            uint8            `json:"bump"`
}

// VoteRecordSize is the on-chain account size including 8-byte discriminator.
const VoteRecordSize = 8 + 32 + 32 + 1 + 8 + 1 // = 82

// DecodeVoteRecord parses raw account data.
func DecodeVoteRecord(data []byte) (*VoteRecord, error) {
	const minSize = 73 // 8 disc + 32 + 32 + 1 (dispute+juror+vote)
	if len(data) < minSize {
		return nil, fmt.Errorf("vote_record data too short: %d < %d", len(data), minSize)
	}
	o := 8 // skip discriminator
	v := &VoteRecord{}
	copy(v.Dispute[:], data[o:o+32])
	o += 32
	copy(v.Juror[:], data[o:o+32])
	o += 32
	v.VoteForProvider = data[o] != 0
	o++
	if o+8 <= len(data) {
		v.VotedAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
		o += 8
	}
	if o < len(data) {
		v.Bump = data[o]
	}
	return v, nil
}

// InsuranceClaim represents an on-chain retroactive insurance claim.
// Whitepaper Section 6: filed within 90 days of contract completion.
// Size: 8 disc + 32 + 32 + 8 + 8 + 32 + 1 + 8 + 8 + 1 = 138 bytes total.
type InsuranceClaim struct {
	Claimant             solana.PublicKey `json:"claimant"`
	Contract             solana.PublicKey `json:"contract"`
	Amount               uint64           `json:"amount"`
	Collateral           uint64           `json:"collateral"`
	EvidenceHash         [32]byte         `json:"evidence_hash"`
	Status               uint8            `json:"status"` // 0=Filed,1=UnderReview,2=Approved,3=Denied
	FiledAt              int64            `json:"filed_at"`
	ContractCompletedAt  int64            `json:"contract_completed_at"`
	Bump                 uint8            `json:"bump"`
}

// InsuranceClaimSize is the on-chain account size including 8-byte discriminator.
const InsuranceClaimSize = 8 + 32 + 32 + 8 + 8 + 32 + 1 + 8 + 8 + 1 // = 138

// ClaimStatusStrings maps the on-chain ClaimStatus u8 to a readable string.
var ClaimStatusStrings = map[uint8]string{
	0: "Filed",
	1: "UnderReview",
	2: "Approved",
	3: "Denied",
}

// DecodeInsuranceClaim parses raw account data (including 8-byte discriminator).
func DecodeInsuranceClaim(data []byte) (*InsuranceClaim, error) {
	if len(data) < InsuranceClaimSize {
		return nil, fmt.Errorf("insurance_claim data too short: %d < %d", len(data), InsuranceClaimSize)
	}
	o := 8 // skip discriminator
	c := &InsuranceClaim{}
	c.Claimant = solana.PublicKeyFromBytes(data[o : o+32])
	o += 32
	c.Contract = solana.PublicKeyFromBytes(data[o : o+32])
	o += 32
	c.Amount = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	c.Collateral = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	copy(c.EvidenceHash[:], data[o:o+32])
	o += 32
	c.Status = data[o]
	o++
	c.FiledAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	c.ContractCompletedAt = int64(binary.LittleEndian.Uint64(data[o : o+8]))
	o += 8
	if o < len(data) {
		c.Bump = data[o]
	}
	return c, nil
}

// InsurancePool represents the on-chain Insurance Pool account.
// Whitepaper Section 6: Accumulates 60% of confiscated stakes.
// Size: 8 disc + 8 + 8 + 4 + 32 + 1 = 61 bytes total.
type InsurancePool struct {
	TotalBalance      uint64           `json:"total_balance"`
	TotalClaimsPaid   uint64           `json:"total_claims_paid"`
	ActiveClaims      uint32           `json:"active_claims"`
	Authority         solana.PublicKey `json:"authority"`
	Bump              uint8            `json:"bump"`
}

// InsurancePoolSize is the on-chain account size including 8-byte discriminator.
const InsurancePoolSize = 8 + 8 + 8 + 4 + 32 + 1 // = 61

// DecodeInsurancePool parses raw account data (including 8-byte discriminator).
func DecodeInsurancePool(data []byte) (*InsurancePool, error) {
	if len(data) < InsurancePoolSize {
		return nil, fmt.Errorf("insurance_pool data too short: %d < %d", len(data), InsurancePoolSize)
	}
	o := 8 // skip discriminator
	p := &InsurancePool{}
	p.TotalBalance = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	p.TotalClaimsPaid = binary.LittleEndian.Uint64(data[o : o+8])
	o += 8
	p.ActiveClaims = binary.LittleEndian.Uint32(data[o : o+4])
	o += 4
	p.Authority = solana.PublicKeyFromBytes(data[o : o+32])
	o += 32
	if o < len(data) {
		p.Bump = data[o]
	}
	return p, nil
}
