from collections.abc import Iterator
from UnityPy.files import SerializedFile, ObjectReader
from UnityPy.classes import Object, GameObject, Transform, PPtr
from UnityPy.enums import ClassIDType
from collections import deque


def prune(scene: SerializedFile, keep_paths: list[str]):
    root_objs = list(get_root_objs(scene))
    keep_new_root = [lookup_path(keep, root_objs) for keep in keep_paths]

    include: set[int] = set()
    queue: deque[Object] = deque([keep.reader for keep in keep_new_root])

    while queue:
        node = queue.popleft()

        include.add(node.path_id)

        # if isinstance(node, GameObject):
        # print(node.m_Components)
        for reachable in iterate_visible(node):
            if reachable.path_id not in include:
                queue.append(reachable)

    old_objects = scene.objects
    new_objects = {key: old_objects[key] for key in include}
    scene.objects = new_objects

    # scene.objects = {}

    for keep in keep_new_root:
        invalid_parent = keep.m_Transform.get_obj()
        tt = invalid_parent.read_typetree()
        tt["m_Father"] = {"m_FileID": 0, "m_PathID": 0}
        invalid_parent.save_typetree(tt)


def iterate_visible(obj: ObjectReader):
    if isinstance(obj, PPtr):
        obj = obj.get_obj()

    if obj.type == ClassIDType.GameObject:
        obj: GameObject = obj.read()
        yield from obj.m_Components
    elif obj.type == ClassIDType.Transform:
        obj: Transform = obj.read()
        yield obj.m_GameObject
        yield from obj.m_Children


def lookup_path_in(path: list[str], go: GameObject):
    current: Transform = go.m_Transform.read()

    while path:
        segment = path.pop(0)

        for child in current.m_Children:
            child: Transform = child.read()
            if child.m_GameObject.read().name == segment:
                current = child
                break
        else:
            raise Exception(f"path '{'/'.join(path)}' not found at {segment}")

    return current.m_GameObject.read()


def lookup_path(path: str, root_objs: list[GameObject]):
    root, *rest = path.split("/")

    root_obj = next(filter(lambda obj: obj.name == root, root_objs), None)
    assert root_obj is not None

    return lookup_path_in(rest, root_obj)


def get_root_objs(file: SerializedFile) -> Iterator[GameObject]:
    for obj in file.objects.values():
        if obj.class_id == 4:
            transform: Transform = obj.read()
            parent = transform.m_Father.get_obj()
            if parent is None:
                yield transform.m_GameObject.read()


def get_root_objs2(objects: dict) -> Iterator[GameObject]:
    for obj in objects.values():
        if obj.class_id == 4:
            transform: Transform = obj.read()
            parent = transform.m_Father.get_obj()
            if parent is None:
                yield transform.m_GameObject.read()
