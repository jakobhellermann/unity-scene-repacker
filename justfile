examplemod:
    uv run src/unity_scene_repacker/cli.py \
        --game-dir "C:/Program Files (x86)/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data" \
        --objects ninesols/bosses.json \
        --output "C:/Users/Jakob/Documents/dev/nine-sols/NineSols-ExampleMod/Resources/bundle.unity3d"

monsters:
    uv run src/unity_scene_repacker/cli.py \
        --game-dir "C:/Program Files (x86)/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data" \
        --objects ninesols/monsters.json \
        --output out/monsters.unity3d

