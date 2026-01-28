#pragma once

class Viewport {
public:
    void Init(int width, int height);
    void Render();
    void Resize(int width, int height);
    void CleanUp();
    
    // Getter methods
    unsigned int GetTextureID() const { return textureID; }
    int GetWidth() const { return viewportWidth; }
    int GetHeight() const { return viewportHeight; }

private:
    unsigned int fbo = 0;         // Framebuffer object
    unsigned int textureID = 0;   // Texture attached to FBO
    unsigned int rbo = 0;         // Renderbuffer object for depth and stencil
    int viewportWidth = 0;
    int viewportHeight = 0;

    void SetupFramebuffer();
};