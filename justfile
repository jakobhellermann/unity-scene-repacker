h:
    cargo run -- -h
help:
    cargo run -- --help

hollowknight *args:
    cargo run --release -- \
        --steam-game 'Hollow Knight' \
        --mode asset \
        --objects objects/hk-representative.jsonc \
        --output out/all.bundle {{ args }} \
        --bundle-name bundle \
        {{ args }}

hollowknight-dependencies *args:
    cargo run --release -- \
        --steam-game 'Hollow Knight' \
        --mode asset \
        --objects objects/hk-in-silksong.jsonc \
        --output out/hk-in-silksong.bundle {{ args }} \
        --emit-dependency-addressables \
        {{ args }}

ninesols *args:
    cargo run --release -- \
        --steam-game "nine sols" \
        --mode asset \
        --objects objects/ns-monsters.json \
        --output out/ninesols.bundle \
        --bundle-name test \
        {{ args }}

ninesols-fx *args:
    cargo run --release -- \
        --steam-game "nine sols" \
        --mode asset \
        --extra-objects objects/ns-by-type.json \
        --output out/ninesols-by-type.bundle \
        --bundle-name bundle \
        {{ args }}

# pack the nuget package with the native library for this platform into ~/.nuget/local-feed
nuget-local:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release -p bindings
    metadata=$(cargo metadata --format-version 1 --no-deps)
    target=$(jq -r .target_directory <<< "$metadata")
    version=$(jq -r '.packages[] | select(.name == "bindings") | .version' <<< "$metadata")
    native=$(mktemp -d)
    mkdir -p "$native/bindings-linux-x64"
    cp "$target/release/libunityscenerepacker.so" "$native/bindings-linux-x64/"
    dotnet pack bindings/dotnet/UnitySceneRepacker.Bindings -c Release -o ~/.nuget/local-feed \
        -p:NativeDir="$native" -p:NativeRids=linux-x64 \
        -p:Version="$version-dev.$(date +%Y%m%d%H%M%S)"
    rm -r "$native"
