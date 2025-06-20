from UnityPy import Environment
from UnityPy.files import BundleFile, SerializedFile

path = "/home/jakob/dev/unity/RustyAssetBundleEXtractor/rust.unity3d"
# path = "/home/jakob/dev/unity/unity-scene-repacker/out/hollowknight.unity3d"


env = Environment()
file: BundleFile = env.load_file(path)

for name, serialized in file.files.items():
    serialized: SerializedFile
    print(name)
    for obj in serialized.objects.values():
        if name.endswith("sharedAssets"):
            #print("-", obj.read())
            print("-", obj)
        else:
            #print("-", obj)
            pass
