examplemod:
    uv run src/unity_scene_repacker/cli.py \
        --preloads ninesols/bosses.json \
        --output "C:/Users/Jakob/Documents/dev/nine-sols/NineSols-ExampleMod/Resources/preloads.bundle" \
        --game-dir "C:/Program Files (x86)/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data"

monsters:
    uv run src/unity_scene_repacker/cli.py \
        --preloads ninesols/monsters.json \
        --output out/preloads.bundle \
        --game-dir "C:/Program Files (x86)/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data"

