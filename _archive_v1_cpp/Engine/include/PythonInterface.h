#pragma once

#include <pybind11/embed.h>
#include <memory>
#include <string>

namespace py = pybind11;

class PythonInterface {
public:
    PythonInterface();
    ~PythonInterface();

    void ExecuteScript(const std::string& script);
    bool IsInitialized() const { return m_initialized; }

private:
    std::unique_ptr<py::scoped_interpreter> m_guard;
    bool m_initialized;
};
