// SPIKE — throwaway. User-mode "feeder": writes a moving 1280x720 NV12 test pattern at
// 30 fps into the shared memory the virtual camera's media source reads.
//
// It also opens a small window with a live millisecond clock. Put that window next to an
// app showing the camera and take a screenshot: latency = window clock - clock in the image.
//
//   vcam-feeder.exe [--no-window] [--seconds N]
#include <windows.h>
#include <sddl.h>
#include <timeapi.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <atomic>
#include <thread>
#include <vector>

#include "../common/shared_frame.h"
#include "../common/nv12_draw.h"

static std::atomic<bool> g_quit{false};
static LONG64 g_startQpc;

static BOOL WINAPI OnCtrl(DWORD) {
    g_quit = true;
    return TRUE;
}

static void DrawFrame(uint8_t* buf, uint32_t frame, LONG64 qpc) {
    Nv12 f{buf, buf + SPIKE_W * SPIKE_H, (int)SPIKE_W, (int)SPIKE_W, (int)SPIKE_H};
    // Scrolling colour bars.
    static const int bars[8][3] = {{235, 235, 235}, {235, 235, 16}, {16, 235, 235}, {16, 235, 16},
                                   {235, 16, 235}, {235, 16, 16}, {16, 16, 235}, {16, 16, 16}};
    int barW = SPIKE_W / 8;
    int shift = (int)(frame * 4) % SPIKE_W;
    for (int i = 0; i < 9; i++) {
        int x = i * barW - shift % barW;
        const int* c = bars[(i + shift / barW) % 8];
        Nv12Fill(f, x, 0, barW, SPIKE_H, RgbToYuv(c[0], c[1], c[2]));
    }
    // Bouncing box.
    int period = 120;
    int t = frame % (2 * period);
    int bx = 40 + (t < period ? t : 2 * period - t) * (SPIKE_W - 200) / period;
    Nv12Fill(f, bx, 540, 120, 120, RgbToYuv(255, 128, 0));

    double ms = SpikeQpcToMs(qpc - g_startQpc);
    char line[64];
    Yuv fg = RgbToYuv(255, 255, 255), bg = RgbToYuv(0, 0, 0);
    snprintf(line, sizeof line, "T=%010.1f MS", ms);
    Nv12Text(f, 64, 120, 10, line, fg, bg);
    snprintf(line, sizeof line, "FRAME %07u", frame);
    Nv12Text(f, 64, 260, 8, line, fg, bg);
    Nv12Text(f, 64, 400, 4, "SPIEGEL SPIKE - ALIMENTADOR", fg, bg);
    // Neutral chroma under the stamp so it survives YUV->RGB conversion in browsers.
    Nv12Fill(f, STAMP_X0 - 8, STAMP_Y0 - 4, STAMP_BITS * STAMP_BW + 16, STAMP_BH + 8, Yuv{16, 128, 128});
    SpikeWriteStamp(f.y, f.pitch, (uint64_t)qpc, frame);
}

static HANDLE OpenShared(SpikeHeader** out, HANDLE* evt) {
    SECURITY_ATTRIBUTES sa{sizeof(sa), nullptr, FALSE};
    ConvertStringSecurityDescriptorToSecurityDescriptorW(SPIKE_SDDL, SDDL_REVISION_1, &sa.lpSecurityDescriptor, nullptr);
    HANDLE map = CreateFileMappingW(INVALID_HANDLE_VALUE, &sa, PAGE_READWRITE, 0, (DWORD)SPIKE_SHM_SIZE, SPIKE_SHM_NAME);
    DWORD createErr = GetLastError();
    bool created = map && createErr != ERROR_ALREADY_EXISTS;
    if (!map) map = OpenFileMappingW(FILE_MAP_READ | FILE_MAP_WRITE, FALSE, SPIKE_SHM_NAME);
    if (!map) {
        LocalFree(sa.lpSecurityDescriptor);
        static bool told = false;
        if (!told) {
            printf("Nao consegui criar %ls (erro %lu) nem abrir (erro %lu).\n"
                   "Sem SeCreateGlobalPrivilege, so quem cria a secao Global\\ e a media source dentro do Frame "
                   "Server.\nAbra a camera em algum app uma vez; vou tentando a cada 500 ms...\n",
                   SPIKE_SHM_NAME, createErr, GetLastError());
            told = true;
        }
        return nullptr;
    }
    *evt = CreateEventW(&sa, FALSE, FALSE, SPIKE_EVT_NAME);
    if (!*evt) *evt = OpenEventW(SYNCHRONIZE | EVENT_MODIFY_STATE, FALSE, SPIKE_EVT_NAME);
    LocalFree(sa.lpSecurityDescriptor);
    auto* h = (SpikeHeader*)MapViewOfFile(map, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, SPIKE_SHM_SIZE);
    if (created || h->magic != SPIKE_MAGIC) {
        h->magic = SPIKE_MAGIC;
        h->width = SPIKE_W;
        h->height = SPIKE_H;
    }
    printf("Memoria compartilhada %s (%s), evento %s.\n", created ? "criada pelo alimentador" : "aberta (ja existia)",
           created ? "o usuario tem SeCreateGlobalPrivilege" : "criada pela media source ou por outro processo",
           *evt ? "ok" : "INDISPONIVEL");
    *out = h;
    return map;
}

// ---- live clock window --------------------------------------------------------------
static LRESULT CALLBACK WndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    switch (msg) {
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(hwnd, &ps);
        RECT rc;
        GetClientRect(hwnd, &rc);
        HDC mem = CreateCompatibleDC(dc);
        HBITMAP bmp = CreateCompatibleBitmap(dc, rc.right, rc.bottom);
        SelectObject(mem, bmp);
        FillRect(mem, &rc, (HBRUSH)GetStockObject(BLACK_BRUSH));
        HFONT font = CreateFontW(72, 0, 0, 0, FW_BOLD, 0, 0, 0, DEFAULT_CHARSET, 0, 0, ANTIALIASED_QUALITY,
                                 FIXED_PITCH, L"Consolas");
        SelectObject(mem, font);
        SetTextColor(mem, RGB(80, 255, 80));
        SetBkMode(mem, TRANSPARENT);
        wchar_t s[64];
        swprintf_s(s, L"T=%010.1f MS", SpikeQpcToMs(SpikeQpc() - g_startQpc));
        TextOutW(mem, 16, 16, s, (int)wcslen(s));
        BitBlt(dc, 0, 0, rc.right, rc.bottom, mem, 0, 0, SRCCOPY);
        DeleteObject(font);
        DeleteObject(bmp);
        DeleteDC(mem);
        EndPaint(hwnd, &ps);
        return 0;
    }
    case WM_DESTROY:
        g_quit = true;
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

static void ClockWindow() {
    WNDCLASSW wc{};
    wc.lpfnWndProc = WndProc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.lpszClassName = L"SpiegelSpikeClock";
    wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
    RegisterClassW(&wc);
    HWND hwnd = CreateWindowExW(WS_EX_TOPMOST, wc.lpszClassName, L"Spiegel spike - relogio do alimentador",
                                WS_OVERLAPPEDWINDOW | WS_VISIBLE, 40, 40, 640, 150, nullptr, nullptr, wc.hInstance, nullptr);
    // Repaint as fast as the compositor presents, so the clock is at most one refresh old.
    MSG msg;
    while (!g_quit) {
        while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        InvalidateRect(hwnd, nullptr, FALSE);
        UpdateWindow(hwnd);
        Sleep(1);
    }
}

int main(int argc, char** argv) {
    bool window = true;
    int seconds = 0;
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--no-window")) window = false;
        else if (!strcmp(argv[i], "--seconds") && i + 1 < argc) seconds = atoi(argv[++i]);
    }
    setvbuf(stdout, nullptr, _IONBF, 0);
    SetConsoleCtrlHandler(OnCtrl, TRUE);
    timeBeginPeriod(1);
    g_startQpc = SpikeQpc();

    std::thread ui;
    if (window) ui = std::thread(ClockWindow);

    SpikeHeader* h = nullptr;
    HANDLE evt = nullptr, map = nullptr;
    while (!g_quit && !(map = OpenShared(&h, &evt))) Sleep(500);
    if (!map) return 1;
    h->feederPid = (LONG)GetCurrentProcessId();

    HANDLE timer = CreateWaitableTimerExW(nullptr, nullptr, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS);
    LARGE_INTEGER due{};
    due.QuadPart = -1;
    SetWaitableTimer(timer, &due, 1000 / SPIKE_FPS, nullptr, nullptr, FALSE);

    std::vector<uint8_t> frame(SPIKE_NV12_SIZE);
    LONG64 lastPrint = SpikeQpc();
    uint32_t n = 0;
    printf("Publicando 1280x720 NV12 @ 30 fps. Ctrl+C para parar.\n");
    while (!g_quit) {
        WaitForSingleObject(timer, INFINITE);
        LONG64 qpc = SpikeQpc();
        DrawFrame(frame.data(), n, qpc);

        int slot = (int)((h->latestSlot + 1) & 1);
        SpikeSlot& s = h->slots[slot];
        InterlockedIncrement64(&s.seq); // odd: writing
        memcpy(SpikeSlotData(h, slot), frame.data(), SPIKE_NV12_SIZE);
        s.frameNumber = n;
        s.frameQpc = SpikeQpc();
        InterlockedIncrement64(&s.seq); // even: complete
        InterlockedExchange64(&h->latestSlot, slot);
        InterlockedIncrement64(&h->publishCount);
        h->feederHeartbeatQpc = SpikeQpc();
        if (evt) SetEvent(evt);
        n++;

        LONG64 now = SpikeQpc();
        if (SpikeQpcToMs(now - lastPrint) >= 1000) {
            lastPrint = now;
            LONG st = h->sourceSubtype;
            char fcc[5] = {(char)(st & 0xff), (char)((st >> 8) & 0xff), (char)((st >> 16) & 0xff), (char)(st >> 24), 0};
            printf("quadro %6u | media source pid %5ld fmt %s entregues %lld (standby %lld, repeticoes %lld) | "
                   "ultimo quadro %lld, latencia alimentador->source %.2f ms\n",
                   n, h->sourcePid, st ? fcc : "----", h->sourceDelivered, h->sourceStandbyDelivered,
                   h->sourceRepeats, h->sourceLastFrameNumber, h->sourceLastLatency100ns / 10000.0);
        }
        if (seconds && SpikeQpcToMs(now - g_startQpc) >= seconds * 1000.0) break;
    }
    h->feederPid = 0;
    g_quit = true;
    if (ui.joinable()) ui.join();
    printf("Alimentador parado; a camera deve voltar para a imagem de espera.\n");
    return 0;
}
