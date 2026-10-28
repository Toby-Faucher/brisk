# Machine hub image: a PREBUILT brisk-hostd (cargo --features embed, web page inside). Nothing is
# compiled here. Build steps: docs/hosting-runbook.md, Machine hub.
# Context: this directory with the binary copied in as ./brisk-hostd.
# The binary is glibc: build the image from a base at least as new as the box that built it
# (override with --build-arg BASE=...).
ARG BASE=debian:trixie-slim
FROM ${BASE}
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --uid 10001 --home-dir /data --no-create-home --shell /usr/sbin/nologin brisk \
 && install -d -o brisk -g brisk -m 700 /data
COPY --chmod=0755 brisk-hostd /usr/local/bin/brisk-hostd
USER brisk
ENV HOME=/data BRISK_STATIC=embed BRISK_DATA_DIR=/data
VOLUME /data
ENTRYPOINT ["/usr/local/bin/brisk-hostd"]
