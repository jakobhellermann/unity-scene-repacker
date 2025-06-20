examplemod:
    uv run src/unity_scene_repacker/cli.py \
        --game-dir "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data " \
        --objects ninesols/bosses.json \
        --output "out/ examplemod.unity3d"

monsters:
    uv run src/unity_scene_repacker/cli.py \
        --game-dir "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data" \
        --objects ninesols/monsters.json \
        --output out/monsters.unity3d

hk:
    uv run src/unity_scene_repacker/cli.py \
        --game-dir "/home/jakob/.local/share/Steam/steamapps/common/Hollow Knight/hollow_knight_Data" \
        --objects hollowknight/hk.json \
        --output out/hollowknight.unity3d

