# Tooling image for the timelib port experiments (re2c/re2rust + a C toolchain to build PHP's timelib copy).
#   docker build -t twig-timelib-tools research/timelib-port/docker
FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends re2c gcc libc6-dev make ca-certificates \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /work
