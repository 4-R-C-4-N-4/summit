# Start both nodes
sudo ip netns exec summit-a env RUST_LOG=info ./target/debug/summitd a-veth &
sudo ip netns exec summit-b env RUST_LOG=info ./target/debug/summitd b-veth &
sleep 5

# Create a test file
echo "Hello from Summit P2P file transfer!" > /tmp/test.txt

# Send from node A to all peers
sudo ip netns exec summit-a ./target/debug/summit-ctl send /tmp/test.txt

# Wait a moment for transfer
sleep 2

# Check received files on node B
sudo ip netns exec summit-b ./target/debug/summit-ctl files

# Verify the file content
sudo ip netns exec summit-b cat /tmp/summit-received/test.txt

# Cleanup
kill %1 %2
