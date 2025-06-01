from UnityPy import Environment
from UnityPy.files import BundleFile

from unity_scene_repacker import utils

# path = "C:/Users/Jakob/Documents/dev/nine-sols/NineSols-ExampleMod/Resources/preloads.bundle"
path = "C:/Users/Jakob/Documents/dev/nine-sols/NineSols-ExampleMod/Resources/preloads.bundle"
# path = "C:/Program Files (x86)/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data/level10"

env = Environment()
file = env.load_file(path)
