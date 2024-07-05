from collections.abc import Iterator
from UnityPy.files import SerializedFile, ObjectReader
from UnityPy.classes import Object, GameObject, Transform, RectTransform, PPtr
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

    for id, obj in old_objects.items():
        if id not in new_objects:
            if obj.type in [ClassIDType.Material]:
                new_objects[id] = obj

    scene.objects = dict(sorted(new_objects.items()))

    # for keep in keep_new_root:
    #     invalid_parent = keep.m_Transform.get_obj()
    #     tt = invalid_parent.read_typetree()
    #     tt["m_Father"] = {"m_FileID": 0, "m_PathID": 0}
    #     invalid_parent.save_typetree(tt)


def iterate_visible(obj: ObjectReader):
    if isinstance(obj, PPtr):
        obj = obj.get_obj()

    if obj.type == ClassIDType.GameObject:
        obj: GameObject = obj.read()
        yield from obj.m_Components
    elif obj.type == ClassIDType.Canvas:
        pass
    elif obj.type == ClassIDType.Transform:
        obj: Transform = obj.read()
        yield obj.m_GameObject
        yield from obj.m_Children
    elif obj.type == ClassIDType.RectTransform:
        obj: RectTransform = obj.read()
        yield obj.m_GameObject
        yield from obj.m_Children


def flatten(nested):
    for item in nested:
        if item is None:
            pass
        elif isinstance(item, list):
            yield from nested
        else:
            yield item


def lookup_path_in(orig: str, path: list[str], current: Transform, report_errors=True):
    segment, *rest = path

    candidates = []
    for child in current.m_Children:
        child: Transform = child.read()

        if child.m_GameObject.read().name == segment:
            candidates.append(child)

    if len(candidates) == 0:
        if not report_errors:
            return None
        raise Exception("todo")
    elif len(candidates) > 1:
        if rest:
            candidates = list(flatten([lookup_path_in(orig, rest, candidate, False) for candidate in candidates]))

        if len(candidates) == 1:
            return candidates[0]
        if report_errors:
            print(f"found {len(candidates)} candidates for '{orig}', choosing first")
            return candidates[0]
        else:
            return candidates
    else:
        candidate = candidates[0]

        if not rest:
            return candidate
        return lookup_path_in(orig, rest, candidate, report_errors)


# def lookup_path_in(orig, path: list[str], go: GameObject):
#     current: Transform = go.m_Transform.read()
#
#     while path:
#         segment = path.pop(0)
#         print(segment)
#
#         found = None
#         for child in current.m_Children:
#             child: Transform = child.read()
#             if child.m_GameObject.read().name == segment:
#                 if found is not None:
#                     raise Exception(f"Ambiguous child '{segment}' in '{orig}'")
#
#                 found = child
#                 current = child
#
#         if found is None:
#             raise Exception(f"path '{orig}' not found at {segment}")
#         else:
#             current = found
#
#     return current.m_GameObject.read()


def lookup_path(path: str, root_objs: list[GameObject]):
    root, *rest = path.split("/")

    root_obj = next(filter(lambda obj: obj.name == root, root_objs), None)
    assert root_obj is not None

    if not rest:
        return root_obj.m_Transform.read()

    return lookup_path_in(path, rest, root_obj.m_Transform.read())


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
