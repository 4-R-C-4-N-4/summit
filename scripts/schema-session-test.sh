# Start the daemon
sudo ip netns exec summit-a env RUST_LOG=info ./target/debug/summitd a-veth &
sudo ip netns exec summit-b env RUST_LOG=info ./target/debug/summitd b-veth &
sleep 5

# Test 1: Schema list
echo "=== Schema List ==="
sudo ip netns exec summit-a ./target/debug/summit-ctl schema list

# Test 2: Session inspect
SESSION_ID=$(sudo ip netns exec summit-a curl -s http://127.0.0.1:9001/status | jq -r '.sessions[0].session_id')
echo ""
echo "=== Session Inspect ==="
sudo ip netns exec summit-a ./target/debug/summit-ctl sessions inspect "$SESSION_ID"

# Test 3: Session drop
echo ""
echo "=== Before Drop ==="
sudo ip netns exec summit-a ./target/debug/summit-ctl status | grep "Active sessions"

echo ""
echo "=== Drop Session ==="
sudo ip netns exec summit-a ./target/debug/summit-ctl sessions drop "$SESSION_ID"

echo ""
echo "=== After Drop ==="
sudo ip netns exec summit-a ./target/debug/summit-ctl status | grep "Active sessions"

# Cleanup
pkill -f summitd
