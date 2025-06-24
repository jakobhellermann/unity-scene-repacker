ninesols := "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data"
# ninesols := "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data"


hollowknight *args:
    cargo run --release -- \
        --steam-game 'Hollow Knight' \
        --objects /tmp/preloads.json \
        --output out/hollowknight.unity3d {{ args }} \
        --bundle-name bundle --disable --compression none

ninesols *args:
    cargo run --release -- \
        --game-dir "{{ ninesols }}" \
        --objects objects/ns-monsters.json \
        --output /tmp/testing/new.bundle {{ args }}
