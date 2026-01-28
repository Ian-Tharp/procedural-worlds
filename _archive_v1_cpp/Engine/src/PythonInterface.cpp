#include "PythonInterface.h"
#include <iostream>

PythonInterface::PythonInterface() : m_initialized(false) {
    try {
        // Initialize Python interpreter
        m_guard = std::make_unique<py::scoped_interpreter>();
        m_initialized = true;
        
        // Initialize any required Python modules here
        py::exec("import sys");
        py::exec("print(f'Python {sys.version} initialized successfully')");
    } catch (const std::exception& e) {
        std::cerr << "Failed to initialize Python: " << e.what() << std::endl;
        m_initialized = false;
        m_guard.reset();
    }
}

PythonInterface::~PythonInterface() {
    // Python interpreter will be finalized automatically when m_guard is destroyed
    if (m_guard) {
        m_guard.reset();
    }
}

void PythonInterface::ExecuteScript(const std::string& script) {
    if (!m_initialized) {
        std::cerr << "Python is not initialized. Cannot execute script." << std::endl;
        return;
    }
    
    try {
        py::exec(script);
    } catch (const py::error_already_set& e) {
        std::cerr << "Python Error: " << e.what() << std::endl;
    }
}
