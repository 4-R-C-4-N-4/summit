#!/bin/bash
# Enter development container with source mounted
docker run -it --rm \
  --network host \
  --cap-add NET_ADMIN \
  --cap-add NET_RAW \
  --cap-add SYS_ADMIN \
  --ulimit memlock=-1:-1 \
  --device /dev/net/tun \
  -v "$(pwd):/summit" \
  summit bash
