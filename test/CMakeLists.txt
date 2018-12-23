# CMakeLists.txt for WebSocketsTests
#
# © 2018 by Richard Walters

cmake_minimum_required(VERSION 3.8)
set(This WebSocketsTests)

set(Sources
    src/MakeConnectionTests.cpp
    src/WebSocketTests.cpp
)

add_executable(${This} ${Sources})
set_target_properties(${This} PROPERTIES
    FOLDER Tests
)

target_include_directories(${This} PRIVATE ..)

target_link_libraries(${This} PUBLIC
    Base64
    gtest_main
    Hash
    Http
    SystemAbstractions
    WebSockets
)

add_test(
    NAME ${This}
    COMMAND ${This}
)
