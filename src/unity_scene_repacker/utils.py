from pathlib import Path
from UnityPy.environment import Environment
from UnityPy.files import SerializedFile, ObjectReader
from UnityPy.classes import GameObject, Transform, Object, BuildSettings
from UnityPy.enums import ClassIDType
from collections.abc import Iterator


class Fake(object):
    def __init__(self, _class, **kwargs):
        self.__class__ = _class
        self.__dict__.update(kwargs)


def load_bundle(path: Path | str):
    if isinstance(path, Path):
        path = str(path)
    return Environment(path).file


def load_scenes(project: Path, level_names: list[str], scene_map: dict[str, str]) -> list[SerializedFile]:
    paths = [str(project.joinpath(f"level{scene_map[name]}")) for name in level_names]

    env = Environment()
    env.load_file(str(project.joinpath("globalgamemanagers.assets")))
    for i, (path, name) in enumerate(zip(paths, level_names)):
        print(f"Loading {i + 1}/{len(paths)} [{name}]                     ", end="\r")
        env.load_file(path)
    print()
    return [env.files[path] for path in paths]


def get_scene_names(globalgamemanagers: SerializedFile) -> list[str] | None:
    for obj in globalgamemanagers.objects.values():
        if obj.type == ClassIDType.BuildSettings:
            settings: BuildSettings = obj.read()
            return [Path(scene).stem for scene in settings.scenes]
    return None


def get_root_object(file: SerializedFile, name: str) -> GameObject | None:
    for root in get_root_objects(file):
        if root.m_Name == name:
            return root
    return None


def get_child_names(obj: GameObject) -> list[str]:
    return [child.read().m_GameObject.read().m_Name for child in obj.m_Transform.read().m_Children]


def get_root_objects(file: SerializedFile) -> Iterator[GameObject]:
    for reader in get_root_object_readers(file):
        yield reader.read()


def get_root_object_readers(file: SerializedFile) -> Iterator[ObjectReader]:
    for obj in file.objects.values():
        if obj.class_id == ClassIDType.Transform:
            transform: Transform = obj.read()
            if transform.m_Father.m_PathID != 0 and transform.m_Father.m_PathID not in transform.assets_file.objects:
                # save_typetree only changes the underlying file, no what is accessible through python apparently
                # so these are the newly unparented filtered objects
                yield transform.m_GameObject.deref()
            elif transform.m_Father.m_PathID == 0 or transform.m_Father.read() is None:
                yield transform.m_GameObject.deref()


def components_in_children(obj: GameObject, *, ty: ClassIDType | None = None) -> Iterator[Object]:
    for c in obj.m_Components:
        c: ObjectReader = c.deref()
        if ty is None or c.type == ty:
            yield c.read()

    for child in obj.m_Transform.read().m_Children:
        yield from components_in_children(child.read().m_GameObject.read(), ty=ty)


def get_path(obj: GameObject) -> str:
    path = []

    current = obj

    while True:
        path.append(current.m_Name)
        father = current.m_Transform.read().m_Father
        if father is None or father.path_id == 0:
            break
        current = father.read().m_GameObject.read()

    path.reverse()
    return "/".join(path)


def lookup_path(path: str, root_objs: list[GameObject]) -> Transform:
    root, *rest = path.split("/")

    root_obj = next(filter(lambda obj: obj.m_Name == root, root_objs), None)
    assert root_obj is not None, (
        f"Root object not found: {root} of {path} in {list(map(lambda x: x.m_Name, root_objs))}"
    )

    if not rest:
        return root_obj.m_Transform.read()

    return lookup_path_in(path, rest, root_obj.m_Transform.read())


def lookup_path_in(orig: str, path: list[str], current: Transform, report_errors=True):
    segment, *rest = path

    candidates = []
    for c in current.m_Children:
        child: Transform = c.read()

        if child.m_GameObject.read().m_Name == segment:
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

def format_size(bytes_size):
    if bytes_size < 1024:
        return f"{bytes_size} B"
    elif bytes_size < 1024 ** 2:
        return f"{bytes_size / 1024:.2f} KiB"
    elif bytes_size < 1024 ** 3:
        return f"{bytes_size / 1024 ** 2:.2f} MiB"
    else:
        return f"{bytes_size / 1024 ** 3:.2f} GiB"