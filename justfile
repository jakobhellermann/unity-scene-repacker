# ninesols := "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data"
ninesols := "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data"


hollowknight *args:
    cargo run --release -- \
        --steam-game 'Hollow Knight' \
        --mode asset \
        --objects objects/hk-realworld.jsonc \
        --output out/all.bundle {{ args }} \
        --bundle-name bundle --compression none

ninesols *args:
    cargo run --release -- \
        --game-dir "{{ ninesols }}" \
        --mode asset \
        --objects objects/ns-monsters.json \
        --output /tmp/testing/new.bundle {{ args }}
