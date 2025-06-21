# ninesols := "C:/Program Files (x86)/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data"
ninesols := "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols-Speedrunpatch"

hollowknight:
    cargo run --release -- \
        --steam-game 'Hollow Knight' \
        --objects preloads/hk-palecourt.json \
        --output out/hollowknight.unity3d

ninesols:
    cargo run --release -- \
        --game-dir "{{ninesols}}" \
        --objects preloads/ns-monsters.json \
        --output out/ninesols.unity3d
