#!/bin/bash
# Enter development container with source mounted
docker run -it --rm --privileged\
  -v "$(pwd):/summit" \
  --cap-add=NET_ADMIN \
  --cap-add=NET_RAW \
  summit bash
