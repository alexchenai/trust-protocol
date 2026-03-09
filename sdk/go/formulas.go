package trustprotocol

import "math"

// ---------------------------------------------------------------------------
// Whitepaper-compliant formulas (Sections 3-4)
// ---------------------------------------------------------------------------

// CalculateTrustScore computes the whitepaper 5-factor TrustScore (0-100).
//
//	S = max(0, min(100, S_base - S_penalty - S_decay))
//	S_base = 30*task + 20*volume + 25*quality + 20*age + 5*sponsor
func CalculateTrustScore(a *AgentIdentity, monthsSinceCreation, monthsInactive float64) float64 {
	taskFactor := math.Min(1.0, math.Log10(1+float64(a.TasksCompleted))/3.0)
	volumeFactor := math.Min(1.0, math.Log10(1+float64(a.VolumeProcessed))/6.0)

	totalTasks := float64(a.TasksCompleted)
	disputeLossRatio := 0.0
	if totalTasks > 0 {
		disputeLossRatio = float64(a.DisputesLost) / totalTasks
	}
	qualityFactor := math.Max(0, 1.0-5*disputeLossRatio) * math.Min(1.0, totalTasks/20.0)

	ageFactor := math.Min(1.0, monthsSinceCreation/24.0)

	sponsorBonus := 0.0
	if a.SponsorBonus > 0 {
		sponsorBonus = 1.0
	}

	sBase := 30*taskFactor + 20*volumeFactor + 25*qualityFactor + 20*ageFactor + 5*sponsorBonus

	maxTasks := math.Max(1, totalTasks)
	sPenalty := 50*(float64(a.DisputesLost)/maxTasks) +
		150*(float64(a.TasksAbandoned)/maxTasks) +
		100*float64(a.FraudFlags)

	sDecay := math.Min(40, 2.0*monthsInactive)

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

// FeeDistribution holds the 1% protocol fee breakdown (70/20/10).
type FeeDistribution struct {
	TotalFee      uint64 `json:"total_fee"`
	Treasury      uint64 `json:"treasury"`
	InsurancePool uint64 `json:"insurance_pool"`
	Burn          uint64 `json:"burn"`
}

// CalculateProtocolFee computes the 1.0% fee with 70/20/10 split.
func CalculateProtocolFee(contractValue uint64) FeeDistribution {
	totalFee := contractValue / 100
	return FeeDistribution{
		TotalFee:      totalFee,
		Treasury:      totalFee * 70 / 100,
		InsurancePool: totalFee * 20 / 100,
		Burn:          totalFee * 10 / 100,
	}
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
