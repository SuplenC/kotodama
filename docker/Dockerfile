# The dev environment for machines without Nix. It installs nothing itself:
# every tool comes from the flake's dev shell, fetched into /nix on first use.
# Keep /nix in a named volume so the store survives between runs.
FROM nixos/nix:2.35.3

LABEL org.opencontainers.image.source=https://github.com/SuplenC/kotodama \
      org.opencontainers.image.description="Kotodama dev environment: enters the flake's dev shell"

# /etc/nix/nix.conf links into /nix, which the volume covers, so the config lives outside it.
COPY nix.conf /etc/kotodama/nix.conf
COPY entrypoint.sh /usr/local/bin/kotodama-dev
COPY gitconfig /root/.gitconfig

ENV NIX_CONF_DIR=/etc/kotodama \
    CARGO_HOME=/cargo \
    CARGO_TARGET_DIR=/cargo/target

WORKDIR /work
ENTRYPOINT ["/usr/local/bin/kotodama-dev"]
