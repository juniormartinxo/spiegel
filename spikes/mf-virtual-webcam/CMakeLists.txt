# SPIKE — throwaway build for the Media Foundation virtual camera prototype.
cmake_minimum_required(VERSION 3.20)
project(spiegel_vcam_spike CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>") # static CRT: Frame Server needs no VC++ redist
add_compile_definitions(UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX _WIN32_WINNT=0x0A00 NTDDI_VERSION=0x0A00000C)
add_compile_options(/W3 /utf-8 /wd4066 /wd4838)

add_library(spiegel_vcam_spike SHARED source/vcam_source.cpp source/vcam_source.def)
target_link_libraries(spiegel_vcam_spike PRIVATE mfplat mfuuid mfsensorgroup advapi32 ole32)

add_executable(vcam-feeder feeder/feeder.cpp)
target_link_libraries(vcam-feeder PRIVATE advapi32 winmm gdi32 user32)

add_executable(vcamctl vcamctl/vcamctl.cpp)
target_link_libraries(vcamctl PRIVATE mfplat mf mfuuid mfreadwrite mfsensorgroup strmiids ole32 oleaut32)
