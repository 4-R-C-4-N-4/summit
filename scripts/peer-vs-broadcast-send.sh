#!/bin/bash
set -e
# Start both nodes
sudo ip netns exec summit-a env RUST_LOG=info ./target/debug/summitd a-veth &
PID_A=$!
sudo ip netns exec summit-b env RUST_LOG=info ./target/debug/summitd b-veth &
PID_B=$!
sleep 5

# Trust each other
PEER_B=$(sudo ip netns exec summit-a curl -s http://127.0.0.1:9001/peers | jq -r '.peers[0].public_key')
PEER_A=$(sudo ip netns exec summit-b curl -s http://127.0.0.1:9001/peers | jq -r '.peers[0].public_key')
sudo ip netns exec summit-a ./target/debug/summit-ctl trust add "$PEER_B"
sudo ip netns exec summit-b ./target/debug/summit-ctl trust add "$PEER_A"

# Test broadcast (default)
echo "Broadcast test" > /tmp/broadcast.txt
sudo ip netns exec summit-a ./target/debug/summit-ctl send /tmp/broadcast.txt

# Test peer targeting
echo "Peer-targeted test" > /tmp/peer.txt
sudo ip netns exec summit-a ./target/debug/summit-ctl send /tmp/peer.txt --peer "$PEER_B"

# Test session targeting
SESSION_ID=$(sudo ip netns exec summit-a curl -s http://127.0.0.1:9001/status | jq -r '.sessions[0].session_id')
echo "Session-targeted test" > /tmp/session.txt
sudo ip netns exec summit-a ./target/debug/summit-ctl send /tmp/session.txt --session "$SESSION_ID"

sleep 2

# Check what was received
sudo ip netns exec summit-b ./target/debug/summit-ctl files
ls -la /tmp/summit-received/

# Cleanup
kill $PID_A $PID_B 2>/dev/null
rm /tmp/test.txt 2>/dev/null
