#include <SDL2/SDL.h>
#include <GL/glew.h>
#include <iostream>

#include "EngineGUI.h"
#include "imgui_impl_sdl2.h"
// Python support is now controlled by CMake
// Use -DENABLE_PYTHON_SUPPORT=ON or OFF when configuring

#ifdef ENABLE_PYTHON_SUPPORT
#include "PythonInterface.h"
#include <pybind11/embed.h>
#include <Python.h>
namespace py = pybind11;
#endif

#ifdef _WIN32
#include <windows.h>

// Force high-performance GPU on Windows (NVIDIA/AMD)
extern "C" {
    __declspec(dllexport) DWORD NvOptimusEnablement = 1;
    __declspec(dllexport) int AmdPowerXpressRequestHighPerformance = 1;
}
#endif

// Global flag to track if Python is available
static bool g_pythonAvailable = false;

int main(int argc, char* argv[]) {
    std::cout << "Starting Game Engine..." << std::endl;
    
    // Initialize SDL with video support first
    if (SDL_Init(SDL_INIT_VIDEO) < 0) {
        std::cerr << "SDL Initialization Failed: " << SDL_GetError() << std::endl;
        return EXIT_FAILURE;
    }
    std::cout << "SDL initialized successfully." << std::endl;

    // Try OpenGL 3.3 Core first, fallback to 3.0 or 2.1 if needed
    SDL_GL_SetAttribute(SDL_GL_DOUBLEBUFFER, 1);
    SDL_GL_SetAttribute(SDL_GL_DEPTH_SIZE, 24);
    SDL_GL_SetAttribute(SDL_GL_STENCIL_SIZE, 8);
    
    // Start with compatibility profile for better support
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 3);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 0);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_COMPATIBILITY);

    // Create the SDL window
    SDL_Window* window = SDL_CreateWindow(
        "Game Engine",
        SDL_WINDOWPOS_CENTERED,
        SDL_WINDOWPOS_CENTERED,
        1280, // Width
        720,  // Height
        SDL_WINDOW_OPENGL | SDL_WINDOW_RESIZABLE | SDL_WINDOW_ALLOW_HIGHDPI
    );

    if (!window) {
        std::cerr << "Window Creation Failed: " << SDL_GetError() << std::endl;
        SDL_Quit();
        return EXIT_FAILURE;
    }

    // Create the OpenGL context with fallback
    SDL_GLContext glContext = SDL_GL_CreateContext(window);
    if (!glContext) {
        std::cerr << "OpenGL Context Creation Failed: " << SDL_GetError() << std::endl;
        
        // Try OpenGL 2.1 as fallback
        SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 2);
        SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 1);
        SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, 0);
        
        glContext = SDL_GL_CreateContext(window);
        if (!glContext) {
            std::cerr << "OpenGL 2.1 Context Creation Failed: " << SDL_GetError() << std::endl;
            SDL_DestroyWindow(window);
            SDL_Quit();
            return EXIT_FAILURE;
        }
    }

    // Make context current
    SDL_GL_MakeCurrent(window, glContext);

    // Initialize GLEW to manage OpenGL extensions
    glewExperimental = GL_TRUE;
    GLenum glewStatus = glewInit();
    if (GLEW_OK != glewStatus) {
        std::cerr << "GLEW Initialization Failed: " << glewGetErrorString(glewStatus) << std::endl;
        SDL_GL_DeleteContext(glContext);
        SDL_DestroyWindow(window);
        SDL_Quit();
        return EXIT_FAILURE;
    }

    // Clear any GL errors that GLEW might have generated
    while (glGetError() != GL_NO_ERROR);

    // Print OpenGL information
    std::cout << "OpenGL Vendor: " << glGetString(GL_VENDOR) << std::endl;
    std::cout << "OpenGL Renderer: " << glGetString(GL_RENDERER) << std::endl;
    std::cout << "OpenGL Version: " << glGetString(GL_VERSION) << std::endl;
    std::cout << "GLSL Version: " << glGetString(GL_SHADING_LANGUAGE_VERSION) << std::endl;

    // Enable VSync
    SDL_GL_SetSwapInterval(1);

    // Set up OpenGL options
    glViewport(0, 0, 1280, 720);
    glEnable(GL_DEPTH_TEST);

    // Initialize the GUI
    EngineGUI engineGUI;
    engineGUI.Init(window, glContext);

    // Initialize Python after graphics context is set up (optional)
#ifdef ENABLE_PYTHON_SUPPORT
    PythonInterface* pythonInterface = nullptr;
    
    // Try to initialize Python, but don't fail if it doesn't work
    try {
        // Create PythonInterface which handles Python initialization
        pythonInterface = new PythonInterface();
        g_pythonAvailable = true;
        
        // Test Python is working
        pythonInterface->ExecuteScript("print('Python initialized successfully!')");
        pythonInterface->ExecuteScript("print('Hello from embedded Python!')");
    } catch (const std::exception& e) {
        std::cerr << "Python initialization disabled: " << e.what() << std::endl;
        std::cerr << "Continuing without Python support..." << std::endl;
        g_pythonAvailable = false;
        if (pythonInterface) {
            delete pythonInterface;
            pythonInterface = nullptr;
        }
    }
#else
    std::cout << "Python support is disabled at compile time." << std::endl;
#endif

    // Main application loop -------------------------------------------------------------------------------------------------
    bool isRunning = true;
    SDL_Event event;

    while (isRunning) {
        // Event processing
        while (SDL_PollEvent(&event)) {
            // Handle GUI events - Pass events to ImGui
            ImGui_ImplSDL2_ProcessEvent(&event);

            if (event.type == SDL_QUIT) {
                isRunning = false;
            }
            // Handle additional events here
        }

        // Clear the screen first
        glClearColor(0.1f, 0.1f, 0.1f, 1.0f);
        glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);

        // Start the ImGui frame and render GUI
        engineGUI.Render();

        // Swap buffers
        SDL_GL_SwapWindow(window);
    }

    // Clean up resources
#ifdef ENABLE_PYTHON_SUPPORT
    if (pythonInterface) {
        delete pythonInterface;
    }
#endif
    engineGUI.Shutdown();
    SDL_GL_DeleteContext(glContext);
    SDL_DestroyWindow(window);
    SDL_Quit();

    return EXIT_SUCCESS;
}