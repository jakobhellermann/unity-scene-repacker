from UnityPy import Environment
from UnityPy.files import BundleFile, SerializedFile

path_a = "/home/jakob/dev/unity/unity-scene-repacker/out/hollowknight.unity3d"
path_b = "/home/jakob/dev/unity/RustyAssetBundleEXtractor/rust.unity3d"


env = Environment()
a: BundleFile = env.load_file(path_a)
b: BundleFile = env.load_file(path_b)

assert a.files.keys() == b.files.keys(), f"{a.files.keys()} !=  f{b.files.keys()}"

for name, serialized_a in a.files.items():
    serialized_a: SerializedFile
    serialized_b: SerializedFile = b.files[name]

    print(name)

    if len(serialized_a.objects) != len(serialized_b.objects):
        print(name)

        print(" <", len(serialized_a.objects))
        print(" >", len(serialized_b.objects))

        new_objects = [serialized_b.objects[path_id] for path_id in serialized_b.objects if path_id not in serialized_a.objects]
        removed_objects = [serialized_a.objects[path_id] for path_id in serialized_a.objects if path_id not in serialized_b.objects]
        print("New:", set(x.class_id for x in new_objects))
        if removed_objects: print("Removed:", set(x.class_id for x in removed_objects))

        print("OLD OBJECTS")
        for obj in serialized_a.objects.values():
            print(obj)
            pass
        print("NEW OBJECTS")
        for obj in serialized_b.objects.values():
            print(obj)
            pass
