from UnityPy.classes.SpriteAtlas import SpriteAtlas
from UnityPy.files import SerializedFile, ObjectReader
from UnityPy.classes import Object, GameObject, Transform, RectTransform, PPtr
from UnityPy.enums import ClassIDType
from collections import deque
from collections.abc import Iterator
from utils import lookup_path, get_root_objects


def prune(scene: SerializedFile, keep_paths: list[str]) -> list[Transform]:
    root_objs = list(get_root_objects(scene))
    keep_new_root = [lookup_path(keep, root_objs) for keep in keep_paths]
    reachable = [keep.reader for keep in keep_new_root]
    prune_reachable(scene, reachable)

    for keep in keep_new_root:
        keep = keep.reader
        print(keep)
        tt = keep.read_typetree()
        tt["m_Father"] = {"m_FileID": 0, "m_PathID": 0}
        keep.save_typetree(tt)


    return keep_new_root

def prune_reachable(scene: SerializedFile, reachable: list):
    include: set[int] = set()
    queue: deque[ObjectReader] = deque(reachable)

    while queue:
        node = queue.popleft()
        include.add(node.path_id)

        for reachable in iterate_visible(node):
            if reachable.path_id not in include:
                queue.append(reachable)

    old_objects = scene.objects
    new_objects = {key: old_objects[key] for key in include}

    scene.objects = dict(sorted(new_objects.items()))

    # remove unused types
    type_index = 0
    type_mapping = {}
    new_types = []
    for used_type in set(obj.type_id for obj in scene.objects.values()):
        type_mapping[used_type] = type_index
        new_types.append(scene.types[used_type])
        type_index += 1
    for obj in scene.objects.values():
        obj.type_id = type_mapping[obj.type_id]
    scene.types = new_types


def iterate_visible(obj: ObjectReader) -> Iterator[Object]:
    if isinstance(obj, PPtr):
        obj = obj.get_obj()

    if obj.type == ClassIDType.GameObject:
        go: GameObject = obj.read()
        yield from go.m_Components
    elif obj.type == ClassIDType.Canvas:
        pass
    elif obj.type == ClassIDType.Transform:
        transform: Transform = obj.read()
        yield transform.m_GameObject
        yield from transform.m_Children
    elif obj.type == ClassIDType.RectTransform:
        rect: RectTransform = obj.read()
        yield rect.m_GameObject
        yield from rect.m_Children
    elif obj.type == ClassIDType.SpriteAtlas:
        atlas: SpriteAtlas = obj.read()
        yield from atlas.m_PackedSprites
