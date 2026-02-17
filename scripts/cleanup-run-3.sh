sudo ip netns del summit-a 2>/dev/null || true
sudo ip netns del summit-b 2>/dev/null || true
sudo ip netns del summit-c 2>/dev/null || true
sudo ip link del br-summit 2>/dev/null || true
