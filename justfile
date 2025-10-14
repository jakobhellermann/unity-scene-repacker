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
        --compression none {{ args }}

ninesols *args:
    cargo run --release -- \
        --steam-game "nine sols" \
        --mode asset \
        --objects objects/ns-monsters.json \
        --output out/ninesols.bundle \
        --bundle-name test \
        --compression none {{ args }}

ninesols-fx *args:
    cargo run --release -- \
        --steam-game "nine sols" \
        --mode asset \
        --extra-objects objects/ns-by-type.json \
        --output out/ninesols-by-type.bundle \
        --bundle-name bundle \
        --compression none {{ args }}
