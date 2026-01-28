#include "EngineGUI.h"
#include <imgui.h>
#include <imgui_impl_sdl2.h>
#include <imgui_impl_opengl3.h>

#include "Viewport.h"

void EngineGUI::Init(SDL_Window* window, SDL_GLContext glContext) {
    // Setup Dear ImGui context
    IMGUI_CHECKVERSION();
    ImGui::CreateContext();

    // Setup ImGui style
    ImGui::StyleColorsDark();

    // Initialize ImGui SDL2 and OpenGL3 backends
    ImGui_ImplSDL2_InitForOpenGL(window, glContext);
    
    // Use appropriate GLSL version based on OpenGL context
    const char* glsl_version = "#version 130";  // OpenGL 3.0 compatible
    ImGui_ImplOpenGL3_Init(glsl_version);

    int width, height;
    SDL_GetWindowSize(window, &width, &height);
    
    // Ensure valid dimensions
    if (width <= 0 || height <= 0) {
        width = 800;
        height = 600;
    }
    
    viewport.Init(width, height);
}

void EngineGUI::Render() {
    // Start ImGui frame
    ImGui_ImplOpenGL3_NewFrame();
    ImGui_ImplSDL2_NewFrame();
    ImGui::NewFrame();

    // Main Menu Bar
    if (ImGui::BeginMainMenuBar()) {
        if (ImGui::BeginMenu("File")) {
            if (ImGui::MenuItem("New Project")) {
                // TODO: Implement new project logic
            }
            if (ImGui::MenuItem("Load Project")) {
                // TODO: Implement load project logic
            }
            if (ImGui::MenuItem("Exit")) {
                // TODO: Implement exit logic
            }
            ImGui::EndMenu();
        }
        ImGui::EndMainMenuBar();
    }

    if (ImGui::Begin("Viewport", nullptr, ImGuiWindowFlags_NoCollapse)) {
        // Viewport rendering placeholder
        ImVec2 viewportPanelPos = ImGui::GetCursorScreenPos();
        ImVec2 viewportPanelSize = ImGui::GetContentRegionAvail();
        
        // Ensure minimum size for viewport
        if (viewportPanelSize.x < 1.0f) viewportPanelSize.x = 1.0f;
        if (viewportPanelSize.y < 1.0f) viewportPanelSize.y = 1.0f;
        
        // Check if size has changed and resize viewport
        if (static_cast<int>(viewportPanelSize.x) != viewport.GetWidth() || 
            static_cast<int>(viewportPanelSize.y) != viewport.GetHeight()) {
            viewport.Resize(static_cast<int>(viewportPanelSize.x), static_cast<int>(viewportPanelSize.y));
        }
        
        // Render scene to texture
        viewport.Render();
        
        // Display the texture in ImGui only if we have a valid texture
        if (viewport.GetTextureID() != 0) {
            ImGui::Image((ImTextureID)(intptr_t)viewport.GetTextureID(), viewportPanelSize, ImVec2(0, 1), ImVec2(1, 0));
        } else {
            ImGui::Text("Viewport not initialized");
        }
        
        ImGui::End();
    }

    // Additional GUI elements can be added here

    // Rendering
    ImGui::Render();
    ImGui_ImplOpenGL3_RenderDrawData(ImGui::GetDrawData());
}

void EngineGUI::Shutdown() {
    // Cleanup viewport resources
    viewport.CleanUp();

    // Cleanup ImGui resources
    ImGui_ImplOpenGL3_Shutdown();
    ImGui_ImplSDL2_Shutdown();
    ImGui::DestroyContext();
}