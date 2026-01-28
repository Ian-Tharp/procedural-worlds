#include "Viewport.h"
#include <GL/glew.h>
#include <iostream>

// Helper function to check OpenGL errors
static void CheckGLError(const char* operation) {
    GLenum error;
    while ((error = glGetError()) != GL_NO_ERROR) {
        std::cerr << "OpenGL error after " << operation << ": 0x" << std::hex << error << std::dec << std::endl;
    }
}

// Helper function to clear any pending OpenGL errors
static void ClearGLErrors() {
    while (glGetError() != GL_NO_ERROR);
}

void Viewport::Init(int width, int height) {
    // Validate dimensions
    if (width <= 0 || height <= 0) {
        std::cerr << "Invalid viewport dimensions: " << width << "x" << height << std::endl;
        viewportWidth = 800;  // Default fallback
        viewportHeight = 600;
    } else {
        viewportWidth = width;
        viewportHeight = height;
    }
    
    // Clear any existing GL errors before we start
    ClearGLErrors();
    
    SetupFramebuffer();
}

void Viewport::SetupFramebuffer() {
    // Ensure we have valid dimensions
    if (viewportWidth <= 0 || viewportHeight <= 0) {
        std::cerr << "Cannot create framebuffer with invalid dimensions" << std::endl;
        return;
    }
    
    // Clear any pending errors
    ClearGLErrors();
    
    // Generate and bind the framebuffer
    glGenFramebuffers(1, &fbo);
    CheckGLError("glGenFramebuffers");
    
    if (fbo == 0) {
        std::cerr << "Failed to generate framebuffer" << std::endl;
        return;
    }
    
    glBindFramebuffer(GL_FRAMEBUFFER, fbo);
    CheckGLError("glBindFramebuffer");

    // Create a texture to attach to the framebuffer
    glGenTextures(1, &textureID);
    CheckGLError("glGenTextures");
    
    if (textureID == 0) {
        std::cerr << "Failed to generate texture" << std::endl;
        glDeleteFramebuffers(1, &fbo);
        fbo = 0;
        return;
    }
    
    glBindTexture(GL_TEXTURE_2D, textureID);
    CheckGLError("glBindTexture");
    
    // Allocate texture storage
    glTexImage2D(
        GL_TEXTURE_2D, 0, GL_RGB, viewportWidth, viewportHeight, 0,
        GL_RGB, GL_UNSIGNED_BYTE, NULL
    );
    CheckGLError("glTexImage2D");
    
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    CheckGLError("glTexParameteri");

    // Attach the texture to the framebuffer
    glFramebufferTexture2D(
        GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, textureID, 0
    );
    CheckGLError("glFramebufferTexture2D");

    // Create a renderbuffer object for depth and stencil attachment
    glGenRenderbuffers(1, &rbo);
    CheckGLError("glGenRenderbuffers");
    
    if (rbo == 0) {
        std::cerr << "Failed to generate renderbuffer" << std::endl;
        glDeleteTextures(1, &textureID);
        glDeleteFramebuffers(1, &fbo);
        textureID = 0;
        fbo = 0;
        return;
    }
    
    glBindRenderbuffer(GL_RENDERBUFFER, rbo);
    CheckGLError("glBindRenderbuffer");
    
    glRenderbufferStorage(
        GL_RENDERBUFFER, GL_DEPTH24_STENCIL8, viewportWidth, viewportHeight
    );
    CheckGLError("glRenderbufferStorage");
    
    glFramebufferRenderbuffer(
        GL_FRAMEBUFFER, GL_DEPTH_STENCIL_ATTACHMENT, GL_RENDERBUFFER, rbo
    );
    CheckGLError("glFramebufferRenderbuffer");

    // Check if framebuffer is complete
    GLenum status = glCheckFramebufferStatus(GL_FRAMEBUFFER);
    if (status != GL_FRAMEBUFFER_COMPLETE) {
        std::cerr << "Framebuffer is not complete! Status: 0x" << std::hex << status << std::dec << std::endl;
        
        // Clean up on failure
        glDeleteRenderbuffers(1, &rbo);
        glDeleteTextures(1, &textureID);
        glDeleteFramebuffers(1, &fbo);
        fbo = 0;
        textureID = 0;
        rbo = 0;
    } else {
        std::cout << "Framebuffer created successfully (" << viewportWidth << "x" << viewportHeight << ")" << std::endl;
    }

    // Unbind framebuffer
    glBindFramebuffer(GL_FRAMEBUFFER, 0);
    CheckGLError("glBindFramebuffer(0)");
}

void Viewport::Render() {
    // Don't try to render if framebuffer creation failed
    if (fbo == 0) {
        return;
    }
    
    // Bind framebuffer
    glBindFramebuffer(GL_FRAMEBUFFER, fbo);
    glViewport(0, 0, viewportWidth, viewportHeight);

    // Clear buffers with a gradient-like background to verify rendering
    glClearColor(0.2f, 0.3f, 0.4f, 1.0f);
    glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);

    // For now, just clear to a color to verify the framebuffer works
    // TODO: Add proper modern OpenGL rendering with shaders

    // Unbind framebuffer
    glBindFramebuffer(GL_FRAMEBUFFER, 0);
}

void Viewport::Resize(int width, int height) {
    // Validate new dimensions
    if (width <= 0 || height <= 0) {
        std::cerr << "Invalid resize dimensions: " << width << "x" << height << std::endl;
        return;
    }
    
    viewportWidth = width;
    viewportHeight = height;
    
    // Recreate framebuffer with new dimensions
    CleanUp();
    SetupFramebuffer();
}

void Viewport::CleanUp() {
    if (fbo != 0) {
        glDeleteFramebuffers(1, &fbo);
        fbo = 0;
    }
    if (textureID != 0) {
        glDeleteTextures(1, &textureID);
        textureID = 0;
    }
    if (rbo != 0) {
        glDeleteRenderbuffers(1, &rbo);
        rbo = 0;
    }
}