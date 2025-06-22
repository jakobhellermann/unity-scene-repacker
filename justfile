# ninesols := "C:/Program Files (x86)/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data"
ninesols := "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols/NineSols_Data"


hollowknight *args:
    cargo run --release -- \
        --steam-game 'Hollow Knight' \
        --objects objects/hk-enemies.jsonc \
        --output out/hollowknight.unity3d {{ args }}

ninesols *args:
    cargo run --release -- \
        --game-dir "{{ ninesols }}" \
        --objects objects/ns-bosses.json \
        --output /tmp/testing/new.bundle {{ args }}
