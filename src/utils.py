from pathlib import Path
from UnityPy.environment import Environment
from UnityPy.files import SerializedFile, ObjectReader
from UnityPy.classes import GameObject, Transform, Object
from UnityPy.enums import ClassIDType
from collections.abc import Iterator


class Fake(object):
    def __init__(self, _class, **kwargs):
        self.__class__ = _class
        self.__dict__.update(kwargs)


def load_bundle(path):
    if isinstance(path, Path):
        path = str(path)
    return Environment(path).file


def load_scenes(project: Path, level_names: list[str], scene_map: dict[str, str]) -> list[SerializedFile]:
    paths = [str(project.joinpath(f"level{scene_map[name]}")) for name in level_names]

    env = Environment()
    env.load_file(str(project.joinpath("globalgamemanagers.assets")))
    for i, (path, name) in enumerate(zip(paths, level_names)):
        print(f"Loading {i+1}/{len(paths)} [{name}]                     ", end="\r")
        env.load_file(path)
    print()
    return [env.files[path] for path in paths]


def get_root_objects(file: SerializedFile) -> Iterator[GameObject]:
    for reader in get_root_object_readers(file):
        yield reader.read()


def get_root_object_readers(file: SerializedFile) -> Iterator[ObjectReader]:
    for obj in file.objects.values():
        if obj.class_id == 4:
            transform: Transform = obj.read()
            parent = transform.m_Father.get_obj()
            if parent is None:
                yield transform.m_GameObject.get_obj()


def components_in_children(object: GameObject, *, type: ClassIDType = None) -> Iterator[Object]:
    for c in object.m_Components:
        c: ObjectReader = c.get_obj()
        if type is None or c.type == type:
            yield c

    for child in object.m_Transform.read().m_Children:
        yield from components_in_children(child.read().m_GameObject.read(), type=type)


def path(object: GameObject) -> str:
    path = []

    current = object

    while True:
        path.append(current.name)
        father = current.m_Transform.read().m_Father.get_obj()
        if father is None:
            break
        current = father.read().m_GameObject.read()

    path.reverse()
    return "/".join(path)


def lookup_path(path: str, root_objs: list[GameObject]) -> Transform:
    root, *rest = path.split("/")

    root_obj = next(filter(lambda obj: obj.name == root, root_objs), None)
    assert root_obj is not None

    if not rest:
        return root_obj.m_Transform.read()

    return lookup_path_in(path, rest, root_obj.m_Transform.read())


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


def flatten(nested):
    for item in nested:
        if item is None:
            pass
        elif isinstance(item, list):
            yield from nested
        else:
            yield item
