from pathlib import Path
from UnityPy.environment import Environment


class Fake(object):
    def __init__(self, _class, **kwargs):
        self.__class__ = _class
        self.__dict__.update(kwargs)


def load_bundle(path):
    if isinstance(path, Path):
        path = str(path)
    return Environment(path).file
