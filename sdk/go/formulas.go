package trustprotocol

import "math"

// ---------------------------------------------------------------------------
// Whitepaper-compliant formulas (Sections 3-4)
// ---------------------------------------------------------------------------

// CalculateTrustScore computes the whitepaper 5-factor TrustScore (0-100).
//
//	S = max(0, min(100, S_base - S_penalty - S_decay))
//	S_base = 30*task + 20*volume + 25*quality + 20*age + 5*sponsor
//
// solToSwornRate converts SOL lamports to SWORN lamports for the volume factor.
// Pass 0 to ignore SOL volume (conservative, safe default).
func CalculateTrustScore(a *AgentIdentity, monthsSinceCreation, monthsInactive, solToSwornRate float64) float64 {
	taskFactor := math.Min(1.0, math.Log10(1+float64(a.TasksCompleted))/3.0)
	// Volume: SWORN + SOL converted to SWORN equivalent (whitepaper §4.2)
	totalVolumeSworn := float64(a.VolumeProcessed) + float64(a.VolumeSol)*solToSwornRate
	volumeFactor := math.Min(1.0, math.Log10(1+totalVolumeSworn)/6.0)

	totalTasks := float64(a.TasksCompleted)
	disputeLossRatio := 0.0
	correctionRatio := 0.0
	if totalTasks > 0 {
		disputeLossRatio = float64(a.DisputesLost) / totalTasks
	}
	// Whitepaper §8.4: correction_ratio = corrected_deliveries / total_deliveries (not total_tasks)
	totalDeliveries := float64(a.TotalDeliveries)
	if totalDeliveries > 0 {
		correctionRatio = float64(a.CorrectionsReceived) / totalDeliveries
	}
	// Quality: Q = max(0, 1 - 2*C_r - 5*D_L) * min(1, N/20)  (whitepaper §8.1/§8.4)
	qualityFactor := math.Max(0, 1.0-2*correctionRatio-5*disputeLossRatio) * math.Min(1.0, totalTasks/20.0)

	ageFactor := math.Min(1.0, monthsSinceCreation/24.0)

	sponsorBonus := 0.0
	if a.SponsorBonus > 0 {
		sponsorBonus = 1.0
	}

	sBase := 30*taskFactor + 20*volumeFactor + 25*qualityFactor + 20*ageFactor + 5*sponsorBonus

	maxTasks := math.Max(1, totalTasks)
	// Whitepaper §8.1: S_penalty includes dispute_friction_total (0.5 pts per L2 round)
	frictionPts := float64(a.DisputeFrictionTotal) * 0.5
	sPenalty := 50*(float64(a.DisputesLost)/maxTasks) +
		150*(float64(a.TasksAbandoned)/maxTasks) +
		100*float64(a.FraudFlags) +
		frictionPts

	// Decay rate: 2.0/month normally, 0.5/month during hibernation (Whitepaper §8.6)
	decayRate := 2.0
	if a.IsHibernating {
		decayRate = 0.5
	}
	sDecay := math.Min(40, decayRate*monthsInactive)

	score := sBase - sPenalty - sDecay
	return math.Min(100, math.Max(0, score))
}

// CalculateStakeFactor returns the convex staking curve factor [0.05, 1.0].
//
//	f(ts) = max(0.05, 1.0 - 0.95 * (ts/100)^1.5)
func CalculateStakeFactor(trustScore float64) float64 {
	return math.Max(0.05, 1.0-0.95*math.Pow(trustScore/100.0, 1.5))
}

// CalculateStakeRequired returns the provider stake for a contract value.
func CalculateStakeRequired(contractValue uint64, trustScore float64) uint64 {
	factor := CalculateStakeFactor(trustScore)
	return uint64(float64(contractValue) * factor)
}

// MaxSimultaneousContracts returns floor(TrustScore/10) + 1.
func MaxSimultaneousContracts(trustScore float64) int {
	return int(math.Floor(trustScore/10.0)) + 1
}

// ExposureLimit returns 3x deposited capital.
func ExposureLimit(depositedCapital uint64) uint64 {
	return depositedCapital * 3
}

// FeeDistribution holds the protocol fee breakdown (70/20/10).\n// Fee rate: 0.5% for SWORN contracts, 1.0% for SOL contracts (Whitepaper §11.8).
type FeeDistribution struct {
	TotalFee      uint64 `json:"total_fee"`
	Treasury      uint64 `json:"treasury"`
	InsurancePool uint64 `json:"insurance_pool"`
	Burn          uint64 `json:"burn"`
}

// CalculateProtocolFee computes the fee with 70/20/10 split.
// Whitepaper §11.8: 0.5% for SWORN contracts, 1.0% for SOL contracts.
// isSworn=true => 0.5% (50 bps), isSworn=false => 1.0% (100 bps).
func CalculateProtocolFee(contractValue uint64, isSworn bool) FeeDistribution {
	var feeBps uint64 = 100 // 1.0% for SOL
	if isSworn {
		feeBps = 50 // 0.5% for SWORN (half-fee incentive)
	}
	totalFee := contractValue * feeBps / 10_000
	return FeeDistribution{
		TotalFee:      totalFee,
		Treasury:      totalFee * 70 / 100,
		InsurancePool: totalFee * 20 / 100,
		Burn:          totalFee * 10 / 100,
	}
}

// CalculateEscrowFactor returns the requester escrow factor [0.30, 1.00].
// Whitepaper §7.7: factor(ts) = max(0.30, 1.0 - 0.70*(ts/100)^1.5)
// New requesters (TS=0) deposit 100%. Experienced ones deposit as little as 30%.
func CalculateEscrowFactor(trustScore float64) float64 {
	return math.Max(0.30, 1.0-0.70*math.Pow(trustScore/100.0, 1.5))
}

// CalculateEscrowRequired returns the actual SWORN/SOL deposit for a requester.
func CalculateEscrowRequired(contractValue uint64, requesterTrustScore float64) uint64 {
	factor := CalculateEscrowFactor(requesterTrustScore)
	return uint64(float64(contractValue) * factor)
}

// ConfiscationSplit holds the split of confiscated stakes (15/60/25).
type ConfiscationSplit struct {
	Burned    uint64 `json:"burned"`
	Insurance uint64 `json:"insurance"`
	Winner    uint64 `json:"winner"`
}

// CalculateConfiscationSplit computes the 15/60/25 split.
func CalculateConfiscationSplit(amount uint64) ConfiscationSplit {
	burned := amount * 15 / 100
	insurance := amount * 60 / 100
	winner := amount - burned - insurance
	return ConfiscationSplit{
		Burned:    burned,
		Insurance: insurance,
		Winner:    winner,
	}
}
