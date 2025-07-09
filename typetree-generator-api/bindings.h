#define u8 __UINT8_TYPE__

typedef struct TypeTreeGeneratorHandle TypeTreeGeneratorHandle;

TypeTreeGeneratorHandle *TypeTreeGenerator_init(const char *unity_version, const char *generator_name);

int TypeTreeGenerator_loadDLL(TypeTreeGeneratorHandle *handle, const u8 *dll_ptr, int dll_len);

char *TypeTreeGenerator_getLoadedDLLNames(TypeTreeGeneratorHandle *handle);

int TypeTreeGenerator_generateTreeNodesJson(TypeTreeGeneratorHandle *handle, const char *assembly_name,
                                            const char *full_name, char **json_addr);

typedef struct TypeTreeNodeNative {
  char *m_Type;
  char *m_Name;
  int m_Level;
  int m_MetaFlag;
} TypeTreeNodeNative;

int TypeTreeGenerator_generateTreeNodesRaw(TypeTreeGeneratorHandle *handle, const char *assembly_name,
                                           const char *full_name, struct TypeTreeNodeNative **arr_addr,
                                           int *arr_length);

int TypeTreeGenerator_getMonoBehaviorDefinitions(TypeTreeGeneratorHandle *handle, char *(*(*arr_addr))[2],
                                                 int *arr_length);

int TypeTreeGenerator_freeMonoBehaviorDefinitions(char *(*arr_addr)[2], int arr_length);

int TypeTreeGenerator_del(TypeTreeGeneratorHandle *handle);

void FreeCoTaskMem(void *ptr);
