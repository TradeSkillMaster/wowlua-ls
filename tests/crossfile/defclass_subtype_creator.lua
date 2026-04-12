-- Cross-file defclass subtype test: creates enum instances via @defclass factory method
EnumStore.MY_ENUM = EnumFactory.New("MY_ENUM", {})
EnumStore.OTHER_ENUM = EnumFactory.New("OTHER_ENUM", {})
