package main

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/gagliardetto/solana-go"
	"github.com/gagliardetto/solana-go/rpc"
)

func anchorDisc(name string) [8]byte {
	h := sha256.Sum256([]byte("global:" + name))
	var d [8]byte
	copy(d[:], h[:8])
	return d
}

func main() {
	keypairPath := os.Getenv("SOLANA_KEYPAIR_PATH")
	if keypairPath == "" {
		keypairPath = "/tmp/deployer.json"
	}
	rpcURL := os.Getenv("SOLANA_RPC_URL")
	if rpcURL == "" {
		rpcURL = "https://api.devnet.solana.com"
	}
	programIDStr := os.Getenv("PROGRAM_ID")
	if programIDStr == "" {
		programIDStr = "CSBAc1SiMALr4rnuCoB17BsddzthB4RAhjibGvyt6p6S"
	}

	// Load keypair
	data, err := os.ReadFile(keypairPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "read keypair: %v\n", err)
		os.Exit(1)
	}
	var raw []byte
	if err := json.Unmarshal(data, &raw); err != nil {
		fmt.Fprintf(os.Stderr, "parse keypair: %v\n", err)
		os.Exit(1)
	}
	privKey := solana.PrivateKey(raw)
	adminKey := privKey.PublicKey()
	fmt.Printf("Admin: %s\n", adminKey)

	programID := solana.MustPublicKeyFromBase58(programIDStr)

	// Derive ProtocolConfig PDA
	configPDA, _, err := solana.FindProgramAddress([][]byte{[]byte("protocol-config")}, programID)
	if err != nil {
		fmt.Fprintf(os.Stderr, "PDA: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("Config PDA: %s\n", configPDA)

	// Build update_config instruction data:
	// discriminator (8) + UpdateConfigParams (Borsh):
	//   min_identity_bond: Option<u64> = Some(3_000_000)  -> [1, 0x40, 0x42, 0x0F, 0, 0, 0, 0, 0]
	//   max_identity_bond: Option<u64> = Some(5_000_000)  -> [1, 0x40, 0x4B, 0x4C, 0, 0, 0, 0, 0]
	//   maturation_period: Option<i64> = Some(1_209_600)  -> [1, 0x00, 0x70, 0x12, 0, 0, 0, 0, 0]
	disc := anchorDisc("update_config")
	buf := make([]byte, 0, 8+3+24)
	buf = append(buf, disc[:]...)

	minBond := uint64(3_000_000)
	maxBond := uint64(5_000_000)
	maturation := int64(1_209_600) // 14 days

	writeOptU64 := func(v uint64) {
		buf = append(buf, 1)
		b := make([]byte, 8)
		binary.LittleEndian.PutUint64(b, v)
		buf = append(buf, b...)
	}
	writeOptI64 := func(v int64) {
		buf = append(buf, 1)
		b := make([]byte, 8)
		binary.LittleEndian.PutUint64(b, uint64(v))
		buf = append(buf, b...)
	}
	writeOptU64(minBond)
	writeOptU64(maxBond)
	writeOptI64(maturation)

	inst := &solana.GenericInstruction{
		ProgID: programID,
		AccountValues: solana.AccountMetaSlice{
			solana.Meta(adminKey).SIGNER().WRITE(),
			solana.Meta(configPDA).WRITE(),
		},
		DataBytes: buf,
	}

	client := rpc.New(rpcURL)
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	recent, err := client.GetRecentBlockhash(ctx, rpc.CommitmentFinalized)
	if err != nil {
		fmt.Fprintf(os.Stderr, "blockhash: %v\n", err)
		os.Exit(1)
	}

	tx, err := solana.NewTransaction(
		[]solana.Instruction{inst},
		recent.Value.Blockhash,
		solana.TransactionPayer(adminKey),
	)
	if err != nil {
		fmt.Fprintf(os.Stderr, "new tx: %v\n", err)
		os.Exit(1)
	}

	_, err = tx.Sign(func(key solana.PublicKey) *solana.PrivateKey {
		if key == adminKey {
			return &privKey
		}
		return nil
	})
	if err != nil {
		fmt.Fprintf(os.Stderr, "sign: %v\n", err)
		os.Exit(1)
	}

	sig, err := client.SendTransaction(ctx, tx)
	if err != nil {
		fmt.Fprintf(os.Stderr, "send: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("update_config TX: %s\n", sig)
	fmt.Printf("min_bond=3_000_000 (3 SWORN/6dec), max_bond=5_000_000, maturation=1_209_600 (14d)\n")
}
