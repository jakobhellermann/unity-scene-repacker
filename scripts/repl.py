from UnityPy import Environment
from UnityPy.files import BundleFile

from unity_scene_repacker import utils

# path = "C:/Users/Jakob/Documents/dev/nine-sols/NineSols-ExampleMod/Resources/preloads.bundle"
path = "/home/jakob/dev/unity/RustyAssetBundleEXtractor/out/BuildPlayer-bundle_Dream_Final_Boss.sharedAssets"
path = "/home/jakob/dev/unity/RustyAssetBundleEXtractor/rust.unity3d"

env = Environment()
file = env.load_file(path)
print(file.files)
