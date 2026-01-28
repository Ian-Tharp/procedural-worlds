#pragma once

#include <SDL2/SDL.h>
#include <SDL2/SDL_opengl.h>
#include <imgui.h>
#include "Viewport.h"

class EngineGUI {
public:
    void Init(SDL_Window* window, SDL_GLContext glContext);
    void Render();
    void Shutdown();

private:
    // Additional private members can be added here
    Viewport viewport;
};