from UnityPy.files import SerializedFile


def prune(scene: SerializedFile):
    print(scene)


# def get_root_objects(file: SerializedFile) -> Iterator[GameObject]:
#     for obj in file.objects.values():
#         if obj.class_id == 4:
#             transform: Transform = obj.read()
#             parent = transform.m_Father.get_obj()
#             if parent is None:
#                 yield transform.m_GameObject.read()
