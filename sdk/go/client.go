package trustprotocol

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/gagliardetto/solana-go"
	"github.com/gagliardetto/solana-go/rpc"
)

// Client is a high-level interface to the Trust Protocol on Solana.
type Client struct {
	ProgramID solana.PublicKey
	RPC       *rpc.Client
}

// NewClient creates a new Trust Protocol client.
func NewClient(programID solana.PublicKey, rpcURL string) *Client {
	return &Client{
		ProgramID: programID,
		RPC:       rpc.New(rpcURL),
	}
}

// NewDevnetClient creates a client pointed at Solana devnet.
func NewDevnetClient(programID solana.PublicKey) *Client {
	return NewClient(programID, rpc.DevNet_RPC)
}

// NewMainnetClient creates a client pointed at Solana mainnet-beta.
func NewMainnetClient(programID solana.PublicKey) *Client {
	return NewClient(programID, rpc.MainNetBeta_RPC)
}

// ---------------------------------------------------------------------------
// Read helpers — fetch + decode on-chain accounts
// ---------------------------------------------------------------------------

// GetProtocolConfig fetches and decodes the ProtocolConfig PDA.
func (c *Client) GetProtocolConfig(ctx context.Context) (*ProtocolConfig, error) {
	pda, _ := FindProtocolConfigPDA(c.ProgramID)
	info, err := c.RPC.GetAccountInfoWithOpts(ctx, pda, &rpc.GetAccountInfoOpts{
		Commitment: rpc.CommitmentConfirmed,
	})
	if err != nil {
		return nil, fmt.Errorf("rpc error: %w", err)
	}
	if info == nil || info.Value == nil {
		return nil, fmt.Errorf("protocol config PDA not found at %s", pda)
	}
	return DecodeProtocolConfig(info.Value.Data.GetBinary())
}

// GetAgentIdentity fetches and decodes an AgentIdentity for the given wallet.
func (c *Client) GetAgentIdentity(ctx context.Context, agent solana.PublicKey) (*AgentIdentity, error) {
	pda, _ := FindAgentIdentityPDA(c.ProgramID, agent)
	info, err := c.RPC.GetAccountInfoWithOpts(ctx, pda, &rpc.GetAccountInfoOpts{
		Commitment: rpc.CommitmentConfirmed,
	})
	if err != nil {
		return nil, fmt.Errorf("rpc error: %w", err)
	}
	if info == nil || info.Value == nil {
		return nil, fmt.Errorf("agent identity not found for %s (PDA: %s)", agent, pda)
	}
	return DecodeAgentIdentity(info.Value.Data.GetBinary())
}

// GetContract fetches and decodes a Contract by its sequential ID.
func (c *Client) GetContract(ctx context.Context, contractID uint64) (*Contract, error) {
	pda, _ := FindContractPDA(c.ProgramID, contractID)
	info, err := c.RPC.GetAccountInfoWithOpts(ctx, pda, &rpc.GetAccountInfoOpts{
		Commitment: rpc.CommitmentConfirmed,
	})
	if err != nil {
		return nil, fmt.Errorf("rpc error: %w", err)
	}
	if info == nil || info.Value == nil {
		return nil, fmt.Errorf("contract #%d not found (PDA: %s)", contractID, pda)
	}
	return DecodeContract(info.Value.Data.GetBinary())
}

// ---------------------------------------------------------------------------
// Transaction helpers — build, sign, send
// ---------------------------------------------------------------------------

// SendAndConfirm signs a transaction with the given keypair and sends it.
func (c *Client) SendAndConfirm(
	ctx context.Context,
	instructions []solana.Instruction,
	payer solana.PublicKey,
	signers ...solana.PrivateKey,
) (solana.Signature, error) {
	recent, err := c.RPC.GetLatestBlockhash(ctx, rpc.CommitmentFinalized)
	if err != nil {
		return solana.Signature{}, fmt.Errorf("get blockhash: %w", err)
	}

	tx, err := solana.NewTransaction(instructions, recent.Value.Blockhash, solana.TransactionPayer(payer))
	if err != nil {
		return solana.Signature{}, fmt.Errorf("build tx: %w", err)
	}

	_, err = tx.Sign(func(key solana.PublicKey) *solana.PrivateKey {
		for i := range signers {
			if signers[i].PublicKey() == key {
				return &signers[i]
			}
		}
		return nil
	})
	if err != nil {
		return solana.Signature{}, fmt.Errorf("sign tx: %w", err)
	}

	sig, err := c.RPC.SendTransactionWithOpts(ctx, tx, rpc.TransactionOpts{
		SkipPreflight:       false,
		PreflightCommitment: rpc.CommitmentConfirmed,
	})
	if err != nil {
		return solana.Signature{}, fmt.Errorf("send tx: %w", err)
	}
	return sig, nil
}

// LoadKeypairFromJSON parses a JSON byte-array keypair (e.g. Solana CLI format).
func LoadKeypairFromJSON(jsonBytes string) (solana.PrivateKey, error) {
	var keyBytes []byte
	if err := json.Unmarshal([]byte(jsonBytes), &keyBytes); err != nil {
		return nil, fmt.Errorf("failed to parse keypair JSON: %w", err)
	}
	return solana.PrivateKey(keyBytes), nil
}
