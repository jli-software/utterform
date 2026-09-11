# Build whisper.cpp for the desktops Utterform ships to, not for the machine
# that happens to compile it.
#
# ggml's CMake defaults `GGML_NATIVE` to ON, which is `-march=native`: the
# build machine's entire instruction set, baked in. A GitHub Actions runner
# with AVX-512 and AMX therefore produced the 0.7.2 Linux binary, which died
# with SIGILL on an Intel N300 the moment whisper.cpp loaded a model — in
# plain C++ frame code, before ggml's own runtime dispatch could choose a
# kernel, which is why that dispatch could not save it.
#
# With it off, ggml enables its explicit defaults instead: SSE4.2, AVX, AVX2,
# FMA, F16C and BMI2 on x86-64, leaving out AVX-512, VNNI and AMX — the ones a
# laptop may lack and a server has. whisper.cpp's quantized kernels gain little
# from those, so this costs almost nothing and is what makes the binary
# portable. On Apple Silicon clang already targets apple-m1, the oldest Mac
# Utterform supports, so nothing extra is needed there.
#
# Deliberately not setting CMAKE_SYSTEM_NAME: that would mark the build as
# cross-compiling, and ggml then drops to plain x86-64 with no AVX2 at all.
#
# Reached through CMAKE_TOOLCHAIN_FILE in `.cargo/config.toml`, because
# whisper-rs-sys builds whisper.cpp itself and offers no way to pass a define.
set(GGML_NATIVE OFF CACHE BOOL "Build for every supported desktop, not for this one" FORCE)
