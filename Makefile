
LUA_LIB=/opt/homebrew/lib

UNAME_S := $(shell uname -s)



ifeq (${UNAME_S}, Linux)
	LIB_NAME = libdevinfo.so
else ifeq (${UNAME_S}, Darwin)
	LIB_NAME = libdevinfo.dylib
else
	FLAG = --release --target i686-pc-windows-gnu
	OS = WIN32
endif


all:
	LUA_LIB=${LUA_LIB} cargo build $(FLAG)
ifneq (${OS}, WIN32)
	mv target/debug/${LIB_NAME}  target/debug/devinfo.so
endif
	

test:
ifeq (${OS}, WIN32)
	cd target/i686-pc-windows-gnu/release/ && lua ../../../test.lua
else
	cd target/debug/ && lua ../../test.lua
endif
	