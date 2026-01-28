# Procedural Worlds Engine - Copilot Workspace

## Project Overview
Procedural Worlds is an innovative game engine that combines C++ performance with Python flexibility to create AI-driven, procedurally generated game worlds. The engine emphasizes emergent narratives, player empowerment, and collaborative world-building between developers, players, and AI.

## Repository Structure

### `/Engine` - Core Engine Code
- `/include` - Public header files
  - `EngineGUI.h` - Editor interface management
  - `Viewport.h` - 3D rendering viewport
  - `PythonInterface.h` - Python scripting integration
- `/src` - Implementation files
  - `main.cpp` - Application entry point and main loop
  - `EngineGUI.cpp` - ImGui-based editor implementation
  - `Viewport.cpp` - Framebuffer and rendering management
  - `PythonInterface.cpp` - Python interpreter embedding
- `/scripts` - Python scripts for procedural generation
- `/assets` - Game assets (models, textures, sounds)
- `/third_party` - External dependencies

### Root Files
- `CMakeLists.txt` - CMake build configuration
- `pyproject.toml` - Python project configuration (using uv)
- `.cursorrules` - AI assistant context rules
- `Engine/.cursorrules` - Engine-specific implementation rules

## Key Technologies

### C++ Stack
- **C++17** - Modern C++ features
- **OpenGL 3.3 Core** - Graphics rendering
- **SDL2** - Window management and input
- **GLEW** - OpenGL extension loading
- **ImGui** - Immediate mode GUI for editor

### Python Stack
- **Python 3.12+** - Scripting language
- **pybind11** - C++/Python bindings
- **FastAPI** - Web API framework (future use)
- **uvicorn** - ASGI server (future use)

### Build Tools
- **CMake** - Cross-platform build system
- **vcpkg** - C++ package manager
- **uv** - Python package manager

## Architecture Patterns

### Component Lifecycle
```cpp
class Component {
public:
    void Init();      // One-time initialization
    void Update();    // Per-frame update
    void Render();    // Rendering (if applicable)
    void Shutdown();  // Cleanup
private:
    void _internalMethod();  // Private methods prefixed with _
};
```

### Resource Management
- RAII pattern for all resources
- Explicit initialization and shutdown
- Smart pointers where appropriate

### Python Integration
- Embedded interpreter in C++ application
- Scripts in `/Engine/scripts`
- Future: Full engine API exposed to Python

## Current Development Status

### Completed ✅
- Window creation and OpenGL context
- Basic ImGui editor framework
- Viewport system for off-screen rendering
- Python interpreter integration
- Build system setup

### In Progress 🚧
- Basic 3D rendering implementation
- Shader system
- Camera controls

### Planned 📋
- Entity-Component System
- Procedural generation algorithms
- Asset loading pipeline
- Networking support
- LLM integration for content generation

## Code Standards

### C++ Guidelines
- Use `#pragma once` for header guards
- Explicit `public`/`private` declarations
- Private methods prefixed with underscore
- Opening braces on same line as statement
- RAII for resource management

### Python Guidelines
- Type hints for all functions
- Follow PEP 8 style guide
- Document all public APIs
- Use descriptive variable names

### Commit Messages
- Use conventional commits format
- Reference issue numbers when applicable
- Keep messages concise but descriptive

## Common Tasks

### Adding a New Component
1. Create header in `/Engine/include`
2. Create implementation in `/Engine/src`
3. Follow component lifecycle pattern
4. Update CMakeLists.txt if needed
5. Document in Obsidian vault

### Exposing C++ to Python
1. Add binding in PythonInterface
2. Use pybind11 macros
3. Create example Python script
4. Test error handling

### Adding ImGui Window
1. Add window function to EngineGUI
2. Add menu item for access
3. Follow ImGui best practices
4. Make window dockable

## Performance Targets
- 60 FPS minimum
- < 16ms frame time
- Efficient memory usage
- Minimal Python overhead in hot paths

## Future Vision
The engine will support:
- AI-driven content generation
- Real-time collaborative editing
- Procedural world streaming
- Advanced modding capabilities
- Cloud-based world persistence

## Documentation
Full documentation maintained in Obsidian vault:
`1. Current Projects/Procedural Worlds/`

Key documents:
- Project Vision.md - Overall vision and philosophy
- Technical Details.md - Architecture deep dive
- Development Roadmap.md - Feature planning
- Build Instructions.md - Setup guide 