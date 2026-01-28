cmake_minimum_required(VERSION 3.15)
project(GameEngine VERSION 0.1.0 LANGUAGES CXX)

# Set C++ standard
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Enable multi-configuration generators (e.g., Visual Studio)
if(NOT CMAKE_CONFIGURATION_TYPES AND WIN32)
    set(CMAKE_CONFIGURATION_TYPES Debug Release CACHE STRING "" FORCE)
endif()

# Option to enable/disable Python support
option(ENABLE_PYTHON_SUPPORT "Enable Python scripting support" OFF)

# Find packages using vcpkg toolchain
# Assumes vcpkg is integrated; otherwise, specify toolchain file
# Example:
# set(CMAKE_TOOLCHAIN_FILE "${CMAKE_CURRENT_SOURCE_DIR}/vcpkg/scripts/buildsystems/vcpkg.cmake" CACHE STRING "Vcpkg toolchain file")

# Find SDL2
find_package(SDL2 CONFIG REQUIRED)
find_package(GLEW CONFIG REQUIRED)
find_package(OpenGL REQUIRED)
find_package(imgui CONFIG REQUIRED)

# Python support
if(ENABLE_PYTHON_SUPPORT)
    find_package(pybind11 CONFIG REQUIRED)
    find_package(Python3 3.12 REQUIRED COMPONENTS Interpreter Development)
endif()

# Include directories for include/
include_directories(Engine/include)
include_directories(Engine/third_party/imgui_backends)

# Add source files
file(GLOB_RECURSE SOURCES Engine/src/*.cpp)

# Exclude Python files if Python support is disabled
if(NOT ENABLE_PYTHON_SUPPORT)
    list(REMOVE_ITEM SOURCES ${CMAKE_CURRENT_SOURCE_DIR}/Engine/src/PythonInterface.cpp)
endif()

# Add ImGui SDL2 backend
list(APPEND SOURCES Engine/third_party/imgui_backends/imgui_impl_sdl2.cpp)

# Add executable
add_executable(GameEngine ${SOURCES})

# Define preprocessor macros
if(ENABLE_PYTHON_SUPPORT)
    target_compile_definitions(GameEngine PRIVATE ENABLE_PYTHON_SUPPORT)
endif()

# Link libraries
target_link_libraries(GameEngine PRIVATE 
    SDL2::SDL2 
    SDL2::SDL2main
    GLEW::GLEW 
    OpenGL::GL 
    imgui::imgui
)

# Add Python libraries if enabled
if(ENABLE_PYTHON_SUPPORT)
    target_link_libraries(GameEngine PRIVATE 
        pybind11::embed
        Python3::Python
    )
endif()

# For Windows, ensure proper subsystem
if(WIN32)
    set_target_properties(GameEngine PROPERTIES 
        WIN32_EXECUTABLE FALSE  # Keep as console app for debugging
    )
    
    # Copy Python DLL to output directory if Python is enabled
    if(ENABLE_PYTHON_SUPPORT)
        add_custom_command(TARGET GameEngine POST_BUILD
            COMMAND ${CMAKE_COMMAND} -E copy_if_different
            "${Python3_RUNTIME_LIBRARY}"
            $<TARGET_FILE_DIR:GameEngine>
        )
        
        # Also copy python3.dll if it exists
        get_filename_component(PYTHON_DIR "${Python3_RUNTIME_LIBRARY}" DIRECTORY)
        if(EXISTS "${PYTHON_DIR}/python3.dll")
            add_custom_command(TARGET GameEngine POST_BUILD
                COMMAND ${CMAKE_COMMAND} -E copy_if_different
                "${PYTHON_DIR}/python3.dll"
                $<TARGET_FILE_DIR:GameEngine>
            )
        endif()
    endif()
endif()

# Set output directories
set_target_properties(GameEngine PROPERTIES 
    RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin
    RUNTIME_OUTPUT_DIRECTORY_DEBUG ${CMAKE_BINARY_DIR}/bin
    RUNTIME_OUTPUT_DIRECTORY_RELEASE ${CMAKE_BINARY_DIR}/bin
)

# Print configuration summary
message(STATUS "========================================")
message(STATUS "GameEngine Configuration Summary:")
message(STATUS "  C++ Standard: ${CMAKE_CXX_STANDARD}")
message(STATUS "  Python Support: ${ENABLE_PYTHON_SUPPORT}")
if(ENABLE_PYTHON_SUPPORT)
    message(STATUS "  Python Version: ${Python3_VERSION}")
endif()
message(STATUS "========================================")
