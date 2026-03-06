#!/bin/bash
# Deploy Trust Protocol to Solana Devnet
# Prerequisites: solana CLI, anchor CLI, funded wallet

set -e

echo "=== Trust Protocol Devnet Deployment ==="

# Configure for devnet
solana config set --url devnet

# Check wallet balance
BALANCE=$(solana balance | awk '{print $1}')
echo "Wallet balance: $BALANCE SOL"

if (( $(echo "$BALANCE < 2" | bc -l) )); then
    echo "ERROR: Need at least 2 SOL for deployment. Request airdrop:"
    echo "  solana airdrop 2"
    exit 1
fi

# Build
echo "Building program..."
anchor build

# Get program ID from build
PROGRAM_ID=$(solana-keygen pubkey target/deploy/trust_protocol-keypair.json)
echo "Program ID: $PROGRAM_ID"

# Update program ID in lib.rs and Anchor.toml
sed -i "s/declare_id!(\".*\")/declare_id!(\"$PROGRAM_ID\")/" programs/trust-protocol/src/lib.rs
sed -i "s/trust_protocol = \".*\"/trust_protocol = \"$PROGRAM_ID\"/" Anchor.toml

# Rebuild with correct program ID
echo "Rebuilding with correct program ID..."
anchor build

# Deploy
echo "Deploying to devnet..."
anchor deploy --provider.cluster devnet

echo ""
echo "=== Deployment Complete ==="
echo "Program ID: $PROGRAM_ID"
echo "Explorer: https://explorer.solana.com/address/$PROGRAM_ID?cluster=devnet"
echo ""
echo "Save this program ID in your configuration!"
