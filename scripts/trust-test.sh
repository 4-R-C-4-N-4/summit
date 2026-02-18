#!/bin/bash
set -e

# Start both nodes
sudo ip netns exec summit-a env RUST_LOG=info ./target/debug/summitd a-veth &
PID_A=$!
sudo ip netns exec summit-b env RUST_LOG=info ./target/debug/summitd b-veth &
PID_B=$!

sleep 5

echo "=== Node A Status ==="
sudo ip netns exec summit-a ./target/debug/summit-ctl status

echo ""
echo "=== Node A: Get B's public key ==="
PEERS_A_JSON=$(sudo ip netns exec summit-a curl -s http://127.0.0.1:9001/peers)
PEER_B_PUBKEY=$(echo "$PEERS_A_JSON" | jq -r '.peers[0].public_key')
echo "Node B pubkey: $PEER_B_PUBKEY"

echo ""
echo "=== Node B: Get A's public key ==="
PEERS_B_JSON=$(sudo ip netns exec summit-b curl -s http://127.0.0.1:9001/peers)
PEER_A_PUBKEY=$(echo "$PEERS_B_JSON" | jq -r '.peers[0].public_key')
echo "Node A pubkey: $PEER_A_PUBKEY"

echo ""
echo "=== Node A: Trust Node B ==="
sudo ip netns exec summit-a curl -s -X POST http://127.0.0.1:9001/trust/add \
  -H "Content-Type: application/json" \
  -d "{\"public_key\":\"$PEER_B_PUBKEY\"}" | jq .

echo ""
echo "=== Node B: Trust Node A ==="
sudo ip netns exec summit-b curl -s -X POST http://127.0.0.1:9001/trust/add \
  -H "Content-Type: application/json" \
  -d "{\"public_key\":\"$PEER_A_PUBKEY\"}" | jq .

# Create test file
echo "Hello from trusted Summit P2P!" > /tmp/test.txt

echo ""
echo "=== Sending File from A to B ==="
sudo ip netns exec summit-a ./target/debug/summit-ctl send /tmp/test.txt

sleep 2

echo ""
echo "=== Files on Node B ==="
sudo ip netns exec summit-b ./target/debug/summit-ctl files

echo ""
echo "=== Verify File Content ==="
if [ -f /tmp/summit-received/test.txt ]; then
    cat /tmp/summit-received/test.txt
    echo ""
    echo "✅ File transfer successful!"
else
    echo "❌ File not received"
fi

# Cleanup
kill $PID_A $PID_B 2>/dev/null
rm /tmp/test.txt 2>/dev/null
