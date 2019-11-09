# CMakeLists.txt for WebSockets
#
# © 2018 by Richard Walters

cmake_minimum_required(VERSION 3.8)
set(This WebSockets)

set(Headers
    include/WebSockets/MakeConnection.hpp
    include/WebSockets/WebSocket.hpp
)

set(Sources
    src/MakeConnection.cpp
    src/WebSocket.cpp
)

add_library(${This} STATIC ${Sources} ${Headers})
set_target_properties(${This} PROPERTIES
    FOLDER Libraries
)

target_include_directories(${This} PUBLIC include)

target_link_libraries(${This} PUBLIC
    Base64
    Hash
    Http
    StringExtensions
    SystemAbstractions
    Utf8
)

add_subdirectory(test)
