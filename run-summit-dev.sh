docker build -t summit-dev .

docker run -it \
  --privileged \
  -v $(pwd):/summit \
  -w /summit \
  summit-dev
