import json


monsters = json.load(open("ninesols/monsters.json", "r"))

new = []
for scene, monsters in monsters.items():
    for monster in monsters:
        name: str = monster.split("/")[-1]
        name, *rest = name.split(" (")

        namelower = name.lower()
        mini = "mini" in namelower
        boss = "boss" in namelower
        new.append(
            {
                "scene": scene,
                "name": name,
                "path": monster,
                "miniboss": mini and boss,
                "boss": not mini and boss,
                "flying": "flying" in namelower,
                "variant": "variant" in namelower,
                "shielded": "withshield" in namelower,
            }
        )

open("out.json", "w").write(json.dumps(new, indent=4))
