#!/bin/bash
# Download and install MSYS2, needed primarily for the test suite (run-make) but
# also used by the MinGW toolchain for assembling things.

set -euo pipefail
IFS=$'\n\t'

source "$(cd "$(dirname "$0")" && pwd)/../shared.sh"
# should try to remove windows git from the path.
# There are two different windows gits at C:\Program Files\Git\mingw64\bin\git.exe
# and C:\Program Files\Git\bin\git.exe ?!
if isWindows; then
    echo "PATH: $PATH"
    echo "MAJAHA PWD: $(pwd) | $(cygpath -w $(pwd))"
    echo "MSYSTEM: ${MSYSTEM-unset}"
    echo "MAJAHA 1: $(cygpath -w $(which git))"
    msys2Path="c:/msys64"
    mkdir -p "${msys2Path}/home/${USERNAME}"
    ciCommandAddPath "${msys2Path}/usr/bin" # This is what rotates the CI shell from Git bash to msys bash i think
    echo "MAJAHA 2: $(cygpath -w $(which git))"

    # Detect the native Python version installed on the agent. On GitHub
    # Actions, the C:\hostedtoolcache\windows\Python directory contains a
    # subdirectory for each installed Python version.
    #
    # The -V flag of the sort command sorts the input by version number.
    native_python_version="$(ls /c/hostedtoolcache/windows/Python | sort -Vr | head -n 1)"

    # Make sure we use the native python interpreter instead of some msys equivalent
    # one way or another. The msys interpreters seem to have weird path conversions
    # baked in which break LLVM's build system one way or another, so let's use the
    # native version which keeps everything as native as possible.
    python_home="/c/hostedtoolcache/windows/Python/${native_python_version}/x64"
    if ! [[ -f "${python_home}/python3.exe" ]]; then
        cp "${python_home}/python.exe" "${python_home}/python3.exe"
    fi
    echo "MAJAHA 1: $(cygpath -w $(which python))"
    #ciCommandAddPath "C:\\hostedtoolcache\\windows\\Python\\${native_python_version}\\x64"
    #ciCommandAddPath "C:\\hostedtoolcache\\windows\\Python\\${native_python_version}\\x64\\Scripts"
    echo "MAJAHA 2: $(cygpath -w $(which python))"
fi
