# ninesols := "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data"
ninesols := "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data"


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
        --game-dir "{{ ninesols }}" \
        --mode asset \
        --objects objects/ns-monsters.json \
        --output /tmp/testing/new.bundle \
        --bundle-name bundle \
        --compression none {{ args }}

bindgen:
    bindgen typetree-generator-api/bindings.h -o typetree-generator-api/src/generated.rs
