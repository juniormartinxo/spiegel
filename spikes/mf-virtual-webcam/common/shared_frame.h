// SPIKE — throwaway. Shared-memory contract between the feeder (user session)
// and the media source (runs inside the Frame Server service, session 0).
#pragma once
#include <windows.h>
#include <stdint.h>

#define SPIKE_VCAM_NAME L"Spiegel Spike Webcam"
// {6E2F4C1B-9A3D-4E57-B0C8-2D7A15F3E901}
static const GUID CLSID_SpiegelSpikeVCam = {0x6e2f4c1b, 0x9a3d, 0x4e57, {0xb0, 0xc8, 0x2d, 0x7a, 0x15, 0xf3, 0xe9, 0x01}};
#define SPIKE_VCAM_CLSID_STR L"{6E2F4C1B-9A3D-4E57-B0C8-2D7A15F3E901}"

// Global\ so both session 0 (Frame Server) and the user's session see the same objects.
// Creating a Global\ section needs SeCreateGlobalPrivilege, which a normal user does not
// have but LocalService (Frame Server) does. Both sides open-or-create.
#define SPIKE_SHM_NAME L"Global\\SpiegelSpikeVCamFrames"
#define SPIKE_EVT_NAME L"Global\\SpiegelSpikeVCamFrameReady"
// SYSTEM, LocalService, Administrators, Authenticated Users, Interactive: full access.
#define SPIKE_SDDL L"D:(A;;GA;;;SY)(A;;GA;;;LS)(A;;GA;;;BA)(A;;GA;;;AU)(A;;GA;;;IU)"

enum : uint32_t {
    SPIKE_W = 1280,
    SPIKE_H = 720,
    SPIKE_FPS = 30,
    SPIKE_NV12_SIZE = SPIKE_W * SPIKE_H * 3 / 2,
    SPIKE_MAGIC = 0x53504756, // 'SPGV'
    SPIKE_SLOTS = 2,
};

#pragma pack(push, 8)
struct SpikeSlot {
    volatile LONG64 seq;      // odd while being written (seqlock)
    volatile LONG64 frameQpc; // QPC when the feeder finished the frame
    volatile LONG64 frameNumber;
};

struct SpikeHeader {
    uint32_t magic;
    uint32_t width, height;
    volatile LONG feederPid;
    volatile LONG64 feederHeartbeatQpc; // updated every frame; stale => standby
    volatile LONG64 latestSlot;          // index of the newest complete slot
    volatile LONG64 publishCount;        // bumps once per published frame
    SpikeSlot slots[SPIKE_SLOTS];

    // Written back by the media source, read by the feeder for stats.
    volatile LONG sourcePid;
    volatile LONG sourceSubtype;          // FOURCC of the negotiated format
    volatile LONG64 sourceDelivered;      // samples delivered to Frame Server
    volatile LONG64 sourceLastFrameNumber;
    volatile LONG64 sourceLastLatency100ns; // feeder publish -> sample queued
    volatile LONG64 sourceStandbyDelivered;
    volatile LONG64 sourceRepeats;
};
#pragma pack(pop)

static_assert(sizeof(SpikeHeader) <= 4096, "header must fit its page");
static inline uint8_t* SpikeSlotData(SpikeHeader* h, int slot) {
    return reinterpret_cast<uint8_t*>(h) + 4096 + (size_t)slot * SPIKE_NV12_SIZE;
}
static const size_t SPIKE_SHM_SIZE = 4096 + (size_t)SPIKE_SLOTS * SPIKE_NV12_SIZE;

static inline LONG64 SpikeQpc() {
    LARGE_INTEGER t;
    QueryPerformanceCounter(&t);
    return t.QuadPart;
}
static inline double SpikeQpcToMs(LONG64 ticks) {
    static LONG64 freq = [] { LARGE_INTEGER f; QueryPerformanceFrequency(&f); return f.QuadPart; }();
    return (double)ticks * 1000.0 / (double)freq;
}

// ---- Machine-readable stamp burned into the top of each frame's Y plane ----
// 96 blocks of 12x16 px: 64 bits QPC, 24 bits frame number, 8 bits checksum.
enum : uint32_t { STAMP_BITS = 96, STAMP_BW = 12, STAMP_BH = 16, STAMP_X0 = 64, STAMP_Y0 = 8 };

static inline void SpikeWriteStamp(uint8_t* y, int pitch, uint64_t qpc, uint32_t frame) {
    uint8_t bits[STAMP_BITS];
    uint8_t sum = 0;
    for (int i = 0; i < 64; i++) bits[i] = (qpc >> i) & 1;
    for (int i = 0; i < 24; i++) bits[64 + i] = (frame >> i) & 1;
    for (int i = 0; i < 88; i++) sum = (uint8_t)(sum * 31 + bits[i] + 7);
    for (int i = 0; i < 8; i++) bits[88 + i] = (sum >> i) & 1;
    for (int b = 0; b < (int)STAMP_BITS; b++) {
        uint8_t v = bits[b] ? 235 : 16;
        for (int r = 0; r < (int)STAMP_BH; r++) {
            memset(y + (size_t)(STAMP_Y0 + r) * pitch + STAMP_X0 + b * STAMP_BW, v, STAMP_BW);
        }
    }
}

// Reads the stamp from a Y plane (or from YUY2 with bytesPerLuma=2). Returns false if the
// checksum fails (e.g. standby image, or frame was scaled).
static inline bool SpikeReadStamp(const uint8_t* y, int pitch, int bytesPerLuma, uint64_t* qpc, uint32_t* frame) {
    uint8_t bits[STAMP_BITS];
    for (int b = 0; b < (int)STAMP_BITS; b++) {
        int x = STAMP_X0 + b * STAMP_BW + STAMP_BW / 2;
        int row = STAMP_Y0 + STAMP_BH / 2;
        bits[b] = y[(size_t)row * pitch + (size_t)x * bytesPerLuma] > 128;
    }
    uint8_t sum = 0, got = 0;
    for (int i = 0; i < 88; i++) sum = (uint8_t)(sum * 31 + bits[i] + 7);
    for (int i = 0; i < 8; i++) got |= bits[88 + i] << i;
    if (sum != got) return false;
    uint64_t q = 0;
    uint32_t f = 0;
    for (int i = 0; i < 64; i++) q |= (uint64_t)bits[i] << i;
    for (int i = 0; i < 24; i++) f |= (uint32_t)bits[64 + i] << i;
    *qpc = q;
    *frame = f;
    return true;
}
