# The dev environment for machines without Nix. It installs nothing itself:
# every tool comes from the flake's dev shell, fetched into /nix on first use.
# Keep /nix in a named volume so the store survives between runs.
FROM nixos/nix:2.35.3

COPY nix.conf /etc/nix/nix.conf
COPY entrypoint.sh /usr/local/bin/kotodama-dev
COPY gitconfig /root/.gitconfig

ENV CARGO_HOME=/cargo \
    CARGO_TARGET_DIR=/cargo/target

WORKDIR /work
ENTRYPOINT ["/usr/local/bin/kotodama-dev"]
