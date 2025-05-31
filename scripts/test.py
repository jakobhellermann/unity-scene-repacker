from sys import platform

from UnityPy import Environment

from unity_scene_repacker import utils

path = "C:/Program Files (x86)/Steam/steamapps/common/Hollow Knight/hollow_knight_Data"
if platform == "linux" or platform == "linux2":
    path = "/mnt/c/Program Files (x86)/Steam/steamapps/common/Hollow Knight/hollow_knight_Data"

env = Environment()
town = env.load_file(path + "/level7")


for root in utils.get_root_objects(town):
    print(root.m_Name)
