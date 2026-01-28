# Language and Library Analysis: C++ vs Rust for Procedural Worlds

## Executive Summary

After comprehensive research, both C++ and Rust are viable options for building a voxel game engine. The decision primarily comes down to:

| Factor | C++ | Rust |
|--------|-----|------|
| Performance | Excellent | Excellent (within 5-10%) |
| Memory Safety | Manual (error-prone) | Compile-time guaranteed |
| Build System | Complex (CMake + vcpkg) | Unified (Cargo) |
| Game Libraries | Mature, extensive | Growing rapidly |
| Learning Curve | Familiar patterns | Steeper initially |
| Python Integration | pybind11 (mature) | PyO3 (excellent) |
| Voxel Examples | Many reference projects | Veloren, Rezcraft, vx_bevy |

**Recommendation**: Consider switching to **Rust** given:
1. Current C++ codebase is minimal (~400 lines)
2. Cargo eliminates build system complexity
3. Memory safety prevents common game engine bugs
4. Strong voxel engine ecosystem (Veloren, Bevy plugins)
5. wgpu provides cross-platform graphics (Vulkan/Metal/DX12/WebGPU)
6. PyO3 Python integration is mature

---

## Rust Ecosystem Analysis

### Graphics: wgpu

**What it is**: A safe, cross-platform graphics API based on WebGPU specification

**Backends**:
- Vulkan (Linux, Windows, Android)
- Metal (macOS, iOS)
- DirectX 12 (Windows)
- OpenGL ES (fallback)
- WebGPU (browsers via WASM)

**Advantages**:
- Single API targets all platforms including web
- Memory-safe by design
- No raw pointer management
- Active development (wgpu 25 as of late 2025)
- Excellent documentation with [Learn wgpu](https://sotrh.github.io/learn-wgpu/) tutorial

**Voxel Engine Examples**:
- **[wgpu-voxel-engine](https://github.com/Blatko1/wgpu-voxel-engine)**: Fast chunk loading, minimal FPS drops
- **[Rezcraft](https://github.com/Shapur1234/Rezcraft)**: Greedy meshing, colored lighting, WASM support

### Game Framework: Bevy

**Current Version**: 0.17.1 (as of late 2025)

**What it is**: Data-driven game engine with ECS architecture

**Key Features**:
- Entity-Component-System (perfect for voxel worlds)
- Uses wgpu internally
- Hot-reload support
- WebAssembly deployment
- 18K+ GitHub stars, very active community

**Voxel Plugins**:
- **[bevy_voxel_world](https://github.com/splashdust/bevy_voxel_world)**: Multithreaded meshing, LOD support, easy API
- **[vx_bevy](https://github.com/Game4all/vx_bevy)**: ~100fps on GTX 1060 with 16-chunk render distance
- **[projekto](https://github.com/afonsolage/projekto)**: 16x256x16 chunks, full voxel game

**Bevy vs Custom Engine**:
| Approach | Pros | Cons |
|----------|------|------|
| Bevy | Fast start, community plugins, battle-tested | Less control, breaking changes between versions |
| Custom wgpu | Full control, learn deeply | More work, reinventing wheels |

### Production Example: Veloren

**What it is**: Open-source multiplayer voxel RPG written entirely in Rust

**Technical Decisions**:
- Custom engine (not Bevy) for voxel-specific optimizations
- Voxels represented with minimal data (not generic "objects")
- Multi-threaded with Rust's safe concurrency
- Started 2018, engine rewrite in 2019
- Runs on Windows, macOS, Linux (x86_64 and ARM64)

**Why They Built Custom**:
> "Traditional game engines have a representation of what an 'object' is, and Veloren would need each voxel in the world to be represented by this 'object'. Internally, Veloren represents voxels with as little data as possible."

### Python Integration: PyO3

**What it is**: Rust bindings for Python interpreter

**Performance**: Rust functions called from Python have ~22μs overhead per call (vs ~60ns native). Solution: minimize cross-language calls, do bulk work per call.

**Build Tool**: maturin - compiles Rust to Python wheel packages

**Game Development Pattern**:
```
Rust: Core engine, physics, rendering (performance-critical)
Python: Game logic, AI, scripting, modding (rapid iteration)
```

**Requires**: Rust 1.83+

### Build System: Cargo

**Advantages over CMake + vcpkg**:
- Single command: `cargo build`
- Automatic dependency resolution via Cargo.toml
- Lockfile ensures reproducible builds
- Integrated testing: `cargo test`
- Integrated docs: `cargo doc`
- No separate package manager configuration
- Consistent project structure

**Example Cargo.toml**:
```toml
[package]
name = "procedural-worlds"
version = "0.1.0"
edition = "2024"

[dependencies]
wgpu = "25.0"
winit = "0.30"
glam = "0.29"     # Math library
noise = "0.9"     # Perlin/Simplex noise
pyo3 = { version = "0.23", features = ["auto-initialize"] }
```

---

## C++ Ecosystem Analysis

### Graphics Options

**OpenGL 3.3+ (Current)**:
- Pros: Familiar, well-documented, works everywhere
- Cons: Dated API, manual state management, no compute shaders in 3.3

**Vulkan**:
- Pros: Modern, explicit control, compute shaders
- Cons: Very verbose (~1000 lines for a triangle), steep learning curve

**Libraries**:
- **[Diligent Engine](https://github.com/DiligentGraphics/DiligentEngine)**: Abstracts Vulkan/DX12/Metal/OpenGL
- **bgfx**: Cross-platform rendering library
- **SDL2 + OpenGL**: What you currently have

### Voxel Engine References

**[SimpleVoxelEngine](https://github.com/JamesRandall/SimpleVoxelEngine)**: C++ + OpenGL from scratch, good learning resource

**[VoxelEngine-Cpp](https://github.com/eversinc33/OpenGL-Voxel-Engine)**: Modern OpenGL voxel engine

**[Luanti (Minetest)](https://www.minetest.net/)**: Production voxel platform, C++ + Lua

### Build System Reality

**CMake + vcpkg challenges**:
- Finding correct target names is difficult
- Toolchain file management
- Platform-specific configuration
- Multiple tools to learn (CMake, vcpkg, possibly Conan)

**Your current setup works** but adds friction for new dependencies.

### Python Integration: pybind11

**Status**: Mature, well-documented, widely used

**Your current implementation**: Basic but functional

---

## Current Codebase Analysis

### What You Have

```
Engine/
├── include/
│   ├── EngineGUI.h      # ImGui wrapper (17 lines)
│   ├── Viewport.h       # FBO management (23 lines)
│   └── PythonInterface.h # Python embedding (21 lines)
├── src/
│   ├── main.cpp         # Entry + loop (184 lines)
│   ├── EngineGUI.cpp    # ImGui rendering (101 lines)
│   ├── Viewport.cpp     # Framebuffer setup (184 lines)
│   └── PythonInterface.cpp # Python init (39 lines)
└── third_party/
    └── imgui_backends/   # SDL2 backend
```

**Total**: ~570 lines of C++ code

### Issues Identified

#### 1. Architecture Problems

**Tight Coupling**: `EngineGUI` owns `Viewport` directly as a member
```cpp
class EngineGUI {
private:
    Viewport viewport;  // Direct ownership, hard to test or swap
};
```

**No Abstraction Layers**: Missing:
- Renderer abstraction (locked to OpenGL)
- Window abstraction (locked to SDL2)
- Input system
- Asset management
- Scene/World representation

**No ECS**: Adding entities will require significant restructuring

#### 2. OpenGL Context Issues

**Compatibility Profile Used**:
```cpp
SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_COMPATIBILITY);
```
This limits modern OpenGL features and is deprecated on macOS.

**Version Fallback Logic**: Falls back to OpenGL 2.1 which won't support modern shaders well.

#### 3. Missing Core Systems

For a voxel engine, you need (none exist yet):
- [ ] Chunk data structure (16x16x16 block storage)
- [ ] Mesh generation (greedy meshing)
- [ ] Camera system
- [ ] Shader management
- [ ] Texture atlas
- [ ] World coordinate system
- [ ] Block type registry

#### 4. Build System

**Hardcoded Paths**:
```powershell
$VcpkgToolchain = "C:/vcpkg/scripts/buildsystems/vcpkg.cmake"
```
Won't work on other machines without modification.

**No Release Configuration Tested**: Only Debug builds shown in artifact.

#### 5. Python Integration

**Optional but Incomplete**: The feature flag system works, but no actual Python API exists yet - just `ExecuteScript()`.

---

## Recommendation: Rust Fresh Start

### Why Rust Makes Sense Here

1. **Minimal Rewrite Cost**: ~570 lines is negligible
2. **Build System**: Cargo eliminates CMake/vcpkg complexity
3. **Memory Safety**: Prevents entire bug categories (use-after-free, data races)
4. **wgpu**: Single API for Vulkan/Metal/DX12/WebGPU
5. **Bevy Ecosystem**: Voxel plugins already exist
6. **Veloren Precedent**: Proves Rust works for voxel games
7. **Modern Tooling**: cargo, rustfmt, clippy are unified

### Recommended Rust Stack

```
Core:
├── wgpu          # Graphics (Vulkan/Metal/DX12/WebGPU)
├── winit         # Windowing (cross-platform)
├── egui          # Immediate-mode GUI (like ImGui)
├── glam          # Math (vectors, matrices)
└── pyo3          # Python integration

Voxel-Specific:
├── noise         # Terrain generation
├── rayon         # Parallel meshing
└── parking_lot   # Fast mutexes

Optional:
├── bevy          # If you want full ECS framework
└── bevy_voxel_world  # Pre-built voxel terrain
```

### Recommended Project Structure

```
procedural-worlds/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── engine/
│   │   ├── mod.rs
│   │   ├── renderer.rs
│   │   ├── window.rs
│   │   └── input.rs
│   ├── world/
│   │   ├── mod.rs
│   │   ├── chunk.rs
│   │   ├── block.rs
│   │   └── meshing.rs
│   ├── editor/
│   │   ├── mod.rs
│   │   └── gui.rs
│   └── scripting/
│       ├── mod.rs
│       └── python.rs
├── assets/
├── scripts/        # Python scripts
└── Documentation/
```

---

## Alternative: Improve C++ Setup

If you prefer staying with C++, here's what needs fixing:

### Immediate Changes

1. **Switch to Core Profile**:
```cpp
SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_CORE);
SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 4);
SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 1);  // macOS max
```

2. **Add Abstraction Layer**:
```cpp
class IRenderer {
public:
    virtual void Init() = 0;
    virtual void BeginFrame() = 0;
    virtual void EndFrame() = 0;
    virtual void DrawMesh(const Mesh& mesh) = 0;
};
```

3. **Create Chunk System**:
```cpp
class Chunk {
    static constexpr int SIZE = 16;
    std::array<BlockID, SIZE*SIZE*SIZE> blocks;
    // ...
};
```

4. **Fix Build Portability**: Use environment variables or CMake presets

### Consider Vulkan

If staying C++, consider moving to Vulkan (or a wrapper like Diligent) for:
- Modern features
- Better performance potential
- Compute shaders for meshing

---

## Decision Matrix

| If You Value... | Choose |
|-----------------|--------|
| Fastest path to working voxels | Rust + Bevy + bevy_voxel_world |
| Full control, learn everything | Rust + wgpu (custom) |
| Familiar tooling | C++ (fix current issues) |
| Maximum ecosystem | C++ |
| Memory safety | Rust |
| Web deployment | Rust + wgpu (WebGPU support) |

---

## Sources

### Rust Resources
- [wgpu.rs](https://wgpu.rs/) - Official wgpu site
- [Learn Wgpu Tutorial](https://sotrh.github.io/learn-wgpu/)
- [Bevy Engine](https://bevy.org/)
- [Veloren](https://veloren.net/) - Production Rust voxel game
- [PyO3](https://pyo3.rs/) - Python integration
- [Bevy in 2025](https://medium.com/solo-devs/bevy-in-2025-rusts-game-engine-taking-over-indie-dev-caec2ae50c09)
- [Rust vs C++ 2026](https://blog.jetbrains.com/rust/2025/12/16/rust-vs-cpp-comparison-for-2026/)

### C++ Resources
- [SimpleVoxelEngine](https://github.com/JamesRandall/SimpleVoxelEngine)
- [Diligent Engine](https://github.com/DiligentGraphics/DiligentEngine)
- [Luanti/Minetest](https://www.minetest.net/)

### Voxel Rendering
- [Greedy Meshing](https://0fps.net/2012/06/30/meshing-in-a-minecraft-game/)
- [Vercidium Optimizations](https://vercidium.com/blog/voxel-world-optimisations/)
- [Voxel Meshing Explained](https://playspacefarer.com/voxel-meshing/)
