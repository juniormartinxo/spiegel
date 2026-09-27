// SPIKE — throwaway, NOT the production Virtual webcam DLL.
// Media Foundation virtual camera media source that reads NV12 frames from shared memory
// written by a separate user-mode process (feeder), and falls back to a static standby
// image when the feeder is not running. Offers NV12 and YUY2 at 1280x720 @ 30 fps.
#include <windows.h>
#include <initguid.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mferror.h>
#include <mfvirtualcamera.h>
#include <ks.h>
#include <ksmedia.h>
#include <ksproxy.h>
#include <sddl.h>
#include <shlobj.h>
#include <wrl/client.h>
#include <atomic>
#include <deque>
#include <mutex>
#include <string>
#include <thread>
#include <vector>
#include <stdio.h>
#include <stdarg.h>

#include "../common/shared_frame.h"
#include "../common/nv12_draw.h"

using Microsoft::WRL::ComPtr;

static HMODULE g_module;
static std::atomic<long> g_objects{0};

// ---------------------------------------------------------------------------------------
// Logging: one file per host process under C:\ProgramData\SpiegelVCamSpike\logs, because
// the interesting process (Frame Server) runs in session 0 where nobody sees debug output.
// ---------------------------------------------------------------------------------------
static std::mutex g_logLock;
static HANDLE g_logFile = INVALID_HANDLE_VALUE;

static void Log(const wchar_t* fmt, ...) {
    wchar_t msg[1024];
    va_list ap;
    va_start(ap, fmt);
    _vsnwprintf_s(msg, _TRUNCATE, fmt, ap);
    va_end(ap);

    wchar_t line[1200];
    SYSTEMTIME st;
    GetLocalTime(&st);
    _snwprintf_s(line, _TRUNCATE, L"%02d:%02d:%02d.%03d [%lu] %s\r\n", st.wHour, st.wMinute, st.wSecond,
                 st.wMilliseconds, GetCurrentThreadId(), msg);
    OutputDebugStringW(line);

    std::lock_guard<std::mutex> g(g_logLock);
    if (g_logFile == INVALID_HANDLE_VALUE) {
        wchar_t exe[MAX_PATH] = L"?";
        GetModuleFileNameW(nullptr, exe, MAX_PATH);
        const wchar_t* base = wcsrchr(exe, L'\\');
        base = base ? base + 1 : exe;
        wchar_t path[MAX_PATH];
        _snwprintf_s(path, _TRUNCATE, L"C:\\ProgramData\\SpiegelVCamSpike\\logs\\%s-%lu.log", base,
                     GetCurrentProcessId());
        g_logFile = CreateFileW(path, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_ALWAYS,
                                FILE_ATTRIBUTE_NORMAL, nullptr);
        if (g_logFile == INVALID_HANDLE_VALUE) g_logFile = nullptr; // don't retry every line
    }
    if (g_logFile) {
        char utf8[2400];
        int n = WideCharToMultiByte(CP_UTF8, 0, line, -1, utf8, sizeof(utf8), nullptr, nullptr);
        DWORD written;
        if (n > 1) WriteFile(g_logFile, utf8, n - 1, &written, nullptr);
    }
}

#define LOG_IF_FAILED(expr)                                                         \
    do {                                                                            \
        HRESULT _hr = (expr);                                                       \
        if (FAILED(_hr)) Log(L"%S failed hr=0x%08X (line %d)", #expr, _hr, __LINE__); \
    } while (0)
#define RETURN_IF_FAILED(expr)                                                      \
    do {                                                                            \
        HRESULT _hr = (expr);                                                       \
        if (FAILED(_hr)) {                                                          \
            Log(L"%S failed hr=0x%08X (line %d)", #expr, _hr, __LINE__);            \
            return _hr;                                                             \
        }                                                                           \
    } while (0)

static std::wstring GuidStr(REFGUID g) {
    wchar_t s[64];
    StringFromGUID2(g, s, 64);
    return s;
}

// ---------------------------------------------------------------------------------------
// Shared memory (consumer side)
// ---------------------------------------------------------------------------------------
struct SharedFrames {
    HANDLE map = nullptr;
    HANDLE evt = nullptr;
    SpikeHeader* h = nullptr;
    LONG64 lastAttemptQpc = 0;

    bool EnsureOpen() {
        if (h) return true;
        LONG64 now = SpikeQpc();
        if (lastAttemptQpc && SpikeQpcToMs(now - lastAttemptQpc) < 500) return false;
        lastAttemptQpc = now;

        SECURITY_ATTRIBUTES sa{sizeof(sa), nullptr, FALSE};
        ConvertStringSecurityDescriptorToSecurityDescriptorW(SPIKE_SDDL, SDDL_REVISION_1, &sa.lpSecurityDescriptor,
                                                             nullptr);
        bool created = false;
        map = CreateFileMappingW(INVALID_HANDLE_VALUE, &sa, PAGE_READWRITE, 0, (DWORD)SPIKE_SHM_SIZE, SPIKE_SHM_NAME);
        DWORD err = GetLastError();
        if (map) {
            created = err != ERROR_ALREADY_EXISTS;
        } else {
            map = OpenFileMappingW(FILE_MAP_READ | FILE_MAP_WRITE, FALSE, SPIKE_SHM_NAME);
            if (!map) {
                Log(L"shm: create failed (%lu) and open failed (%lu)", err, GetLastError());
                LocalFree(sa.lpSecurityDescriptor);
                return false;
            }
        }
        evt = CreateEventW(&sa, FALSE, FALSE, SPIKE_EVT_NAME);
        if (!evt) evt = OpenEventW(SYNCHRONIZE | EVENT_MODIFY_STATE, FALSE, SPIKE_EVT_NAME);
        LocalFree(sa.lpSecurityDescriptor);

        h = (SpikeHeader*)MapViewOfFile(map, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, SPIKE_SHM_SIZE);
        if (!h) {
            Log(L"shm: MapViewOfFile failed (%lu)", GetLastError());
            CloseHandle(map);
            map = nullptr;
            return false;
        }
        if (created) {
            h->magic = SPIKE_MAGIC;
            h->width = SPIKE_W;
            h->height = SPIKE_H;
        }
        Log(L"shm: %s section, event=%p", created ? L"created" : L"opened existing", evt);
        return true;
    }

    bool FeederAlive(LONG64 now) const {
        return h && h->magic == SPIKE_MAGIC && h->feederPid != 0 && h->publishCount > 0 &&
               SpikeQpcToMs(now - h->feederHeartbeatQpc) < 500;
    }

    // Copies the newest complete frame into dst (tightly packed NV12). Seqlock read.
    bool ReadLatest(uint8_t* dst, LONG64* frameNumber, LONG64* frameQpc) {
        for (int attempt = 0; attempt < 4; attempt++) {
            int slot = (int)(h->latestSlot & 1);
            LONG64 s1 = h->slots[slot].seq;
            if (s1 & 1) continue;
            MemoryBarrier();
            memcpy(dst, SpikeSlotData(h, slot), SPIKE_NV12_SIZE);
            *frameNumber = h->slots[slot].frameNumber;
            *frameQpc = h->slots[slot].frameQpc;
            MemoryBarrier();
            if (h->slots[slot].seq == s1) return true;
        }
        return false;
    }

    ~SharedFrames() {
        if (h) UnmapViewOfFile(h);
        if (map) CloseHandle(map);
        if (evt) CloseHandle(evt);
    }
};

static void BuildStandby(std::vector<uint8_t>& buf) {
    buf.resize(SPIKE_NV12_SIZE);
    Nv12 f{buf.data(), buf.data() + SPIKE_W * SPIKE_H, (int)SPIKE_W, (int)SPIKE_W, (int)SPIKE_H};
    Nv12Fill(f, 0, 0, SPIKE_W, SPIKE_H, RgbToYuv(24, 32, 48));
    Nv12Fill(f, 0, SPIKE_H - 24, SPIKE_W, 24, RgbToYuv(200, 120, 0));
    Yuv fg = RgbToYuv(230, 230, 230), bg = RgbToYuv(24, 32, 48);
    Nv12Text(f, 160, 260, 8, "SPIEGEL SPIKE", fg, bg);
    Nv12Text(f, 160, 360, 5, "STANDBY - SEM SINAL", RgbToYuv(255, 170, 40), bg);
    Nv12Text(f, 160, 430, 3, "O ALIMENTADOR NAO ESTA RODANDO", fg, bg);
}

static void CopyNv12(const uint8_t* src, uint8_t* dst, LONG pitch) {
    for (UINT r = 0; r < SPIKE_H; r++) memcpy(dst + (size_t)r * pitch, src + (size_t)r * SPIKE_W, SPIKE_W);
    const uint8_t* suv = src + SPIKE_W * SPIKE_H;
    uint8_t* duv = dst + (size_t)pitch * SPIKE_H;
    for (UINT r = 0; r < SPIKE_H / 2; r++) memcpy(duv + (size_t)r * pitch, suv + (size_t)r * SPIKE_W, SPIKE_W);
}

static void Nv12ToYuy2(const uint8_t* src, uint8_t* dst, LONG pitch) {
    const uint8_t* uvPlane = src + SPIKE_W * SPIKE_H;
    for (UINT r = 0; r < SPIKE_H; r++) {
        const uint8_t* y = src + (size_t)r * SPIKE_W;
        const uint8_t* uv = uvPlane + (size_t)(r / 2) * SPIKE_W;
        uint8_t* o = dst + (size_t)r * pitch;
        for (UINT x = 0; x < SPIKE_W; x += 2) {
            o[0] = y[x];
            o[1] = uv[x];
            o[2] = y[x + 1];
            o[3] = uv[x + 1];
            o += 4;
        }
    }
}

// ---------------------------------------------------------------------------------------
// Media stream
// ---------------------------------------------------------------------------------------
class MediaStream final : public IMFMediaStream2, public IKsControl {
public:
    MediaStream() { g_objects++; }
    ~MediaStream() {
        StopWorker();
        g_objects--;
    }

    HRESULT Initialize(IMFMediaSource* parent, DWORD id) {
        _parent = parent;
        _id = id;
        RETURN_IF_FAILED(MFCreateEventQueue(&_queue));
        RETURN_IF_FAILED(MFCreateAttributes(&_attrs, 8));
        SetStreamAttrs(_attrs.Get());

        ComPtr<IMFMediaType> types[2];
        const GUID subtypes[2] = {MFVideoFormat_NV12, MFVideoFormat_YUY2};
        for (int i = 0; i < 2; i++) {
            RETURN_IF_FAILED(MFCreateMediaType(&types[i]));
            types[i]->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
            types[i]->SetGUID(MF_MT_SUBTYPE, subtypes[i]);
            types[i]->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
            types[i]->SetUINT32(MF_MT_ALL_SAMPLES_INDEPENDENT, TRUE);
            MFSetAttributeSize(types[i].Get(), MF_MT_FRAME_SIZE, SPIKE_W, SPIKE_H);
            MFSetAttributeRatio(types[i].Get(), MF_MT_FRAME_RATE, SPIKE_FPS, 1);
            MFSetAttributeRatio(types[i].Get(), MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
            UINT32 stride = i == 0 ? SPIKE_W : SPIKE_W * 2;
            UINT32 size = i == 0 ? SPIKE_NV12_SIZE : SPIKE_W * SPIKE_H * 2;
            types[i]->SetUINT32(MF_MT_DEFAULT_STRIDE, stride);
            types[i]->SetUINT32(MF_MT_SAMPLE_SIZE, size);
            types[i]->SetUINT32(MF_MT_FIXED_SIZE_SAMPLES, TRUE);
            types[i]->SetUINT32(MF_MT_AVG_BITRATE, size * 8 * SPIKE_FPS);
            types[i]->SetUINT32(MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235);
            types[i]->SetUINT32(MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT601);
        }
        IMFMediaType* raw[2] = {types[0].Get(), types[1].Get()};
        // Experiment switch: does the DirectShow bridge synthesise YUY2 on its own?
        bool nv12Only = GetFileAttributesW(LR"(C:\ProgramData\SpiegelVCamSpike\logs\nv12-only.flag)") != INVALID_FILE_ATTRIBUTES;
        Log(L"stream: offering %s", nv12Only ? L"NV12 only" : L"NV12 + YUY2");
        RETURN_IF_FAILED(MFCreateStreamDescriptor(_id, nv12Only ? 1 : 2, raw, &_desc));
        ComPtr<IMFMediaTypeHandler> handler;
        RETURN_IF_FAILED(_desc->GetMediaTypeHandler(&handler));
        RETURN_IF_FAILED(handler->SetCurrentMediaType(raw[0]));
        SetStreamAttrs(_desc.Get());

        BuildStandby(_standby);
        _scratch.resize(SPIKE_NV12_SIZE);
        _wake = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        _stop = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        _worker = std::thread([this] { WorkerLoop(); });
        return S_OK;
    }

    static void SetStreamAttrs(IMFAttributes* a) {
        a->SetGUID(MF_DEVICESTREAM_STREAM_CATEGORY, PINNAME_VIDEO_CAPTURE);
        a->SetUINT32(MF_DEVICESTREAM_STREAM_ID, 0);
        a->SetUINT32(MF_DEVICESTREAM_FRAMESERVER_SHARED, 1);
        a->SetUINT32(MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES, MFFrameSourceTypes_Color);
    }

    IMFAttributes* Attributes() { return _attrs.Get(); }

    HRESULT SetAllocator(IUnknown* unk) {
        std::lock_guard<std::mutex> g(_lock);
        _allocator.Reset();
        HRESULT hr = unk->QueryInterface(IID_PPV_ARGS(&_allocator));
        Log(L"stream: SetAllocator hr=0x%08X", hr);
        return hr;
    }

    HRESULT Start(IMFMediaType* type) {
        std::lock_guard<std::mutex> g(_lock);
        if (_shutdown) return MF_E_SHUTDOWN;
        return StartLocked(type, true);
    }

    HRESULT Stop(bool sendEvent) {
        std::lock_guard<std::mutex> g(_lock);
        if (_shutdown) return MF_E_SHUTDOWN;
        _state = MF_STREAM_STATE_STOPPED;
        _tokens.clear();
        Log(L"stream: stopped");
        if (sendEvent) RETURN_IF_FAILED(_queue->QueueEventParamVar(MEStreamStopped, GUID_NULL, S_OK, nullptr));
        return S_OK;
    }

    void Shutdown() {
        StopWorker();
        std::lock_guard<std::mutex> g(_lock);
        _shutdown = true;
        _tokens.clear();
        if (_queue) _queue->Shutdown();
        _queue.Reset();
        _parent.Reset();
        _allocator.Reset();
        Log(L"stream: shutdown (delivered=%lld standby=%lld repeats=%lld)", _delivered, _standbyCount, _repeats);
    }

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER;
        if (riid == __uuidof(IUnknown) || riid == __uuidof(IMFMediaEventGenerator) || riid == __uuidof(IMFMediaStream) ||
            riid == __uuidof(IMFMediaStream2))
            *ppv = static_cast<IMFMediaStream2*>(this);
        else if (riid == __uuidof(IKsControl))
            *ppv = static_cast<IKsControl*>(this);
        else {
            *ppv = nullptr;
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return ++_ref; }
    STDMETHODIMP_(ULONG) Release() override {
        ULONG r = --_ref;
        if (!r) delete this;
        return r;
    }

    // IMFMediaEventGenerator
    STDMETHODIMP BeginGetEvent(IMFAsyncCallback* cb, IUnknown* state) override {
        auto q = Queue();
        return q ? q->BeginGetEvent(cb, state) : MF_E_SHUTDOWN;
    }
    STDMETHODIMP EndGetEvent(IMFAsyncResult* res, IMFMediaEvent** ev) override {
        auto q = Queue();
        return q ? q->EndGetEvent(res, ev) : MF_E_SHUTDOWN;
    }
    STDMETHODIMP GetEvent(DWORD flags, IMFMediaEvent** ev) override {
        auto q = Queue();
        return q ? q->GetEvent(flags, ev) : MF_E_SHUTDOWN;
    }
    STDMETHODIMP QueueEvent(MediaEventType met, REFGUID ext, HRESULT hr, const PROPVARIANT* v) override {
        auto q = Queue();
        return q ? q->QueueEventParamVar(met, ext, hr, v) : MF_E_SHUTDOWN;
    }

    // IMFMediaStream
    STDMETHODIMP GetMediaSource(IMFMediaSource** src) override {
        std::lock_guard<std::mutex> g(_lock);
        if (!src) return E_POINTER;
        if (!_parent) return MF_E_SHUTDOWN;
        return _parent.CopyTo(src);
    }
    STDMETHODIMP GetStreamDescriptor(IMFStreamDescriptor** sd) override {
        std::lock_guard<std::mutex> g(_lock);
        if (!sd) return E_POINTER;
        if (!_desc) return MF_E_SHUTDOWN;
        return _desc.CopyTo(sd);
    }
    STDMETHODIMP RequestSample(IUnknown* token) override {
        std::lock_guard<std::mutex> g(_lock);
        if (_shutdown) return MF_E_SHUTDOWN;
        if (_state != MF_STREAM_STATE_RUNNING) return MF_E_INVALIDREQUEST;
        _tokens.emplace_back(token);
        _requests++;
        SetEvent(_wake);
        return S_OK;
    }

    // IMFMediaStream2
    STDMETHODIMP SetStreamState(MF_STREAM_STATE value) override {
        Log(L"stream: SetStreamState %d -> %d", _state, value);
        std::lock_guard<std::mutex> g(_lock);
        if (_shutdown) return MF_E_SHUTDOWN;
        if (value == _state) return S_OK;
        switch (value) {
        case MF_STREAM_STATE_PAUSED:
            if (_state != MF_STREAM_STATE_RUNNING) return MF_E_INVALID_STATE_TRANSITION;
            _state = value;
            return S_OK;
        case MF_STREAM_STATE_RUNNING:
            return StartLocked(nullptr, false);
        case MF_STREAM_STATE_STOPPED:
            _state = value;
            _tokens.clear();
            return S_OK;
        default:
            return MF_E_INVALID_STATE_TRANSITION;
        }
    }
    STDMETHODIMP GetStreamState(MF_STREAM_STATE* value) override {
        if (!value) return E_POINTER;
        *value = _state;
        return S_OK;
    }

    // IKsControl
    STDMETHODIMP KsProperty(PKSPROPERTY, ULONG, LPVOID, ULONG, ULONG*) override {
        return HRESULT_FROM_WIN32(ERROR_SET_NOT_FOUND);
    }
    STDMETHODIMP KsMethod(PKSMETHOD, ULONG, LPVOID, ULONG, ULONG*) override {
        return HRESULT_FROM_WIN32(ERROR_SET_NOT_FOUND);
    }
    STDMETHODIMP KsEvent(PKSEVENT, ULONG, LPVOID, ULONG, ULONG*) override {
        return HRESULT_FROM_WIN32(ERROR_SET_NOT_FOUND);
    }

private:
    ComPtr<IMFMediaEventQueue> Queue() {
        std::lock_guard<std::mutex> g(_lock);
        return _queue;
    }

    HRESULT StartLocked(IMFMediaType* type, bool sendEvent) {
        if (type) {
            _type = type;
            type->GetGUID(MF_MT_SUBTYPE, &_subtype);
        }
        if (!_type) return MF_E_NOT_INITIALIZED;
        if (_allocator) {
            HRESULT hr = _allocator->InitializeSampleAllocator(10, _type.Get());
            Log(L"stream: InitializeSampleAllocator hr=0x%08X", hr);
            if (FAILED(hr)) _allocator.Reset(); // fall back to our own memory samples
        }
        _yuy2 = _subtype == MFVideoFormat_YUY2;
        Log(L"stream: start format=%s allocator=%s", _yuy2 ? L"YUY2" : L"NV12", _allocator ? L"provided" : L"own");
        _state = MF_STREAM_STATE_RUNNING;
        if (sendEvent) RETURN_IF_FAILED(_queue->QueueEventParamVar(MEStreamStarted, GUID_NULL, S_OK, nullptr));
        return S_OK;
    }

    void StopWorker() {
        if (_worker.joinable()) {
            SetEvent(_stop);
            _worker.join();
        }
        if (_wake) { CloseHandle(_wake); _wake = nullptr; }
        if (_stop) { CloseHandle(_stop); _stop = nullptr; }
    }

    enum class Kind { Live, Repeat, Standby };

    void WorkerLoop() {
        // Pacing for standby/repeat frames. A plain timeout would be quantised to the 15.6 ms
        // default timer resolution inside the service, so use a high-resolution timer.
        HANDLE tick = CreateWaitableTimerExW(nullptr, nullptr, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS);
        LARGE_INTEGER due{};
        due.QuadPart = -1;
        SetWaitableTimer(tick, &due, 1000 / SPIKE_FPS, nullptr, nullptr, FALSE);
        LONG64 lastDeliverQpc = 0;
        for (;;) {
            HANDLE hs[4] = {_stop, _wake, tick, _shm.evt};
            DWORD n = _shm.evt ? 4 : 3;
            DWORD w = WaitForMultipleObjects(n, hs, FALSE, 100);
            if (w == WAIT_OBJECT_0) break;
            bool ticked = w == WAIT_OBJECT_0 + 2;

            {
                std::lock_guard<std::mutex> g(_lock);
                if (_tokens.empty() || _state != MF_STREAM_STATE_RUNNING) continue;
            }
            _shm.EnsureOpen();
            LONG64 now = SpikeQpc();
            if (_shm.FeederAlive(now)) {
                LONG64 pc = _shm.h->publishCount;
                if (pc != _lastPublish) {
                    if (Deliver(Kind::Live)) { _lastPublish = pc; lastDeliverQpc = now; }
                } else if (ticked && SpikeQpcToMs(now - lastDeliverQpc) >= 100) {
                    if (Deliver(Kind::Repeat)) lastDeliverQpc = now;
                }
            } else if (ticked) {
                if (Deliver(Kind::Standby)) lastDeliverQpc = now;
            }
        }
        CloseHandle(tick);
    }

    bool Deliver(Kind kind) {
        LONG64 frameNumber = -1, frameQpc = 0;
        const uint8_t* src = _standby.data();
        if (kind != Kind::Standby) {
            if (_shm.ReadLatest(_scratch.data(), &frameNumber, &frameQpc)) src = _scratch.data();
            else kind = Kind::Standby;
        }

        std::lock_guard<std::mutex> g(_lock);
        if (_tokens.empty() || _state != MF_STREAM_STATE_RUNNING || !_queue) return false;

        ComPtr<IMFSample> sample;
        ComPtr<IMFMediaBuffer> buffer;
        const DWORD size = _yuy2 ? SPIKE_W * SPIKE_H * 2 : SPIKE_NV12_SIZE;
        if (_allocator) {
            HRESULT hr = _allocator->AllocateSample(&sample);
            if (FAILED(hr)) {
                if (!_allocFailLogged) Log(L"stream: AllocateSample hr=0x%08X (consumer holding samples?)", hr);
                _allocFailLogged = true;
                return false;
            }
            if (FAILED(sample->GetBufferByIndex(0, &buffer))) return false;
        } else {
            if (FAILED(MFCreateSample(&sample)) || FAILED(MFCreateMemoryBuffer(size, &buffer))) return false;
            buffer->SetCurrentLength(size);
            sample->AddBuffer(buffer.Get());
        }

        BYTE* dst = nullptr;
        LONG pitch = 0;
        ComPtr<IMF2DBuffer2> b2;
        ComPtr<IMF2DBuffer> b1;
        bool locked2d = false;
        if (SUCCEEDED(buffer.As(&b2))) {
            BYTE* start;
            DWORD len;
            locked2d = SUCCEEDED(b2->Lock2DSize(MF2DBuffer_LockFlags_Write, &dst, &pitch, &start, &len));
        } else if (SUCCEEDED(buffer.As(&b1))) {
            locked2d = SUCCEEDED(b1->Lock2D(&dst, &pitch));
        }
        if (!locked2d) {
            DWORD maxLen;
            if (FAILED(buffer->Lock(&dst, &maxLen, nullptr))) return false;
            pitch = _yuy2 ? SPIKE_W * 2 : SPIKE_W;
        }
        if (!_formatLogged) {
            Log(L"stream: first sample pitch=%ld locked2d=%d", pitch, locked2d);
            _formatLogged = true;
        }
        if (_yuy2) Nv12ToYuy2(src, dst, pitch);
        else CopyNv12(src, dst, pitch);
        if (locked2d) (b2 ? b2->Unlock2D() : b1->Unlock2D());
        else buffer->Unlock();

        sample->SetSampleTime(MFGetSystemTime());
        sample->SetSampleDuration(10'000'000 / SPIKE_FPS);
        ComPtr<IUnknown> token = _tokens.front();
        _tokens.pop_front();
        if (token) sample->SetUnknown(MFSampleExtension_Token, token.Get());
        HRESULT hr = _queue->QueueEventParamUnk(MEMediaSample, GUID_NULL, S_OK, sample.Get());
        if (FAILED(hr)) {
            Log(L"stream: QueueEvent(MEMediaSample) hr=0x%08X", hr);
            return false;
        }

        _delivered++;
        if (kind == Kind::Standby) _standbyCount++;
        if (kind == Kind::Repeat) _repeats++;
        if (_shm.h) {
            SpikeHeader* h = _shm.h;
            h->sourcePid = (LONG)GetCurrentProcessId();
            h->sourceSubtype = _yuy2 ? (LONG)'2YUY' : (LONG)'21VN';
            h->sourceDelivered = _delivered;
            h->sourceStandbyDelivered = _standbyCount;
            h->sourceRepeats = _repeats;
            if (kind == Kind::Live) {
                h->sourceLastFrameNumber = frameNumber;
                h->sourceLastLatency100ns = (LONG64)(SpikeQpcToMs(SpikeQpc() - frameQpc) * 10000.0);
            }
        }
        if (_delivered == 1 || _delivered % 300 == 0)
            Log(L"stream: delivered=%lld requests=%lld standby=%lld repeats=%lld", _delivered, _requests,
                _standbyCount, _repeats);
        return true;
    }

    std::atomic<ULONG> _ref{1};
    std::mutex _lock;
    DWORD _id = 0;
    bool _shutdown = false;
    MF_STREAM_STATE _state = MF_STREAM_STATE_STOPPED;
    ComPtr<IMFMediaSource> _parent;
    ComPtr<IMFMediaEventQueue> _queue;
    ComPtr<IMFAttributes> _attrs;
    ComPtr<IMFStreamDescriptor> _desc;
    ComPtr<IMFVideoSampleAllocator> _allocator;
    ComPtr<IMFMediaType> _type;
    GUID _subtype = MFVideoFormat_NV12;
    bool _yuy2 = false;
    std::deque<ComPtr<IUnknown>> _tokens;

    HANDLE _wake = nullptr, _stop = nullptr;
    std::thread _worker;
    SharedFrames _shm;
    std::vector<uint8_t> _standby, _scratch;
    LONG64 _lastPublish = -1;
    LONG64 _delivered = 0, _requests = 0, _standbyCount = 0, _repeats = 0;
    bool _allocFailLogged = false, _formatLogged = false;
};

// ---------------------------------------------------------------------------------------
// Media source
// ---------------------------------------------------------------------------------------
class MediaSource final : public IMFMediaSourceEx, public IMFGetService, public IKsControl, public IMFSampleAllocatorControl {
public:
    MediaSource() { g_objects++; }
    ~MediaSource() { g_objects--; }

    HRESULT Initialize(IMFAttributes* activateAttrs) {
        RETURN_IF_FAILED(MFCreateAttributes(&_attrs, 4));
        if (activateAttrs) activateAttrs->CopyAllItems(_attrs.Get());

        ComPtr<IMFSensorProfileCollection> collection;
        ComPtr<IMFSensorProfile> profile;
        RETURN_IF_FAILED(MFCreateSensorProfileCollection(&collection));
        RETURN_IF_FAILED(MFCreateSensorProfile(KSCAMERAPROFILE_Legacy, 0, nullptr, &profile));
        RETURN_IF_FAILED(profile->AddProfileFilter(0, L"((RES==;FRT<=30,1;SUT==))"));
        RETURN_IF_FAILED(collection->AddProfile(profile.Get()));
        RETURN_IF_FAILED(_attrs->SetUnknown(MF_DEVICEMFT_SENSORPROFILE_COLLECTION, collection.Get()));

        RETURN_IF_FAILED(MFCreateEventQueue(&_queue));
        _stream.Attach(new MediaStream());
        RETURN_IF_FAILED(_stream->Initialize(this, 0));
        ComPtr<IMFStreamDescriptor> sd;
        RETURN_IF_FAILED(_stream->GetStreamDescriptor(&sd));
        IMFStreamDescriptor* raw = sd.Get();
        RETURN_IF_FAILED(MFCreatePresentationDescriptor(1, &raw, &_pd));
        return S_OK;
    }

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER;
        if (riid == __uuidof(IUnknown) || riid == __uuidof(IMFMediaEventGenerator) || riid == __uuidof(IMFMediaSource) ||
            riid == __uuidof(IMFMediaSourceEx))
            *ppv = static_cast<IMFMediaSourceEx*>(this);
        else if (riid == __uuidof(IMFGetService))
            *ppv = static_cast<IMFGetService*>(this);
        else if (riid == __uuidof(IKsControl))
            *ppv = static_cast<IKsControl*>(this);
        else if (riid == __uuidof(IMFSampleAllocatorControl))
            *ppv = static_cast<IMFSampleAllocatorControl*>(this);
        else {
            *ppv = nullptr;
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return ++_ref; }
    STDMETHODIMP_(ULONG) Release() override {
        ULONG r = --_ref;
        if (!r) delete this;
        return r;
    }

    // IMFMediaEventGenerator
    STDMETHODIMP BeginGetEvent(IMFAsyncCallback* cb, IUnknown* state) override {
        auto q = Queue();
        return q ? q->BeginGetEvent(cb, state) : MF_E_SHUTDOWN;
    }
    STDMETHODIMP EndGetEvent(IMFAsyncResult* res, IMFMediaEvent** ev) override {
        auto q = Queue();
        return q ? q->EndGetEvent(res, ev) : MF_E_SHUTDOWN;
    }
    STDMETHODIMP GetEvent(DWORD flags, IMFMediaEvent** ev) override {
        auto q = Queue();
        return q ? q->GetEvent(flags, ev) : MF_E_SHUTDOWN;
    }
    STDMETHODIMP QueueEvent(MediaEventType met, REFGUID ext, HRESULT hr, const PROPVARIANT* v) override {
        auto q = Queue();
        return q ? q->QueueEventParamVar(met, ext, hr, v) : MF_E_SHUTDOWN;
    }

    // IMFMediaSource
    STDMETHODIMP CreatePresentationDescriptor(IMFPresentationDescriptor** pd) override {
        std::lock_guard<std::mutex> g(_lock);
        if (!pd) return E_POINTER;
        if (!_pd) return MF_E_SHUTDOWN;
        return _pd->Clone(pd);
    }
    STDMETHODIMP GetCharacteristics(DWORD* c) override {
        if (!c) return E_POINTER;
        *c = MFMEDIASOURCE_IS_LIVE;
        return S_OK;
    }
    STDMETHODIMP Pause() override { return MF_E_INVALID_STATE_TRANSITION; }
    STDMETHODIMP Start(IMFPresentationDescriptor* pd, const GUID* timeFormat, const PROPVARIANT* startPos) override {
        std::lock_guard<std::mutex> g(_lock);
        if (!_queue) return MF_E_SHUTDOWN;
        if (!pd || !startPos) return E_INVALIDARG;
        if (timeFormat && *timeFormat != GUID_NULL) return MF_E_UNSUPPORTED_TIME_FORMAT;

        BOOL selected = FALSE;
        ComPtr<IMFStreamDescriptor> sd;
        RETURN_IF_FAILED(pd->GetStreamDescriptorByIndex(0, &selected, &sd));
        BOOL wasSelected = FALSE;
        ComPtr<IMFStreamDescriptor> ours;
        RETURN_IF_FAILED(_pd->GetStreamDescriptorByIndex(0, &wasSelected, &ours));
        Log(L"source: Start selected=%d wasSelected=%d", selected, wasSelected);

        if (selected) {
            _pd->SelectStream(0);
            ComPtr<IMFMediaTypeHandler> handler;
            ComPtr<IMFMediaType> type;
            RETURN_IF_FAILED(sd->GetMediaTypeHandler(&handler));
            RETURN_IF_FAILED(handler->GetCurrentMediaType(&type));
            RETURN_IF_FAILED(_queue->QueueEventParamUnk(wasSelected ? MEUpdatedStream : MENewStream, GUID_NULL, S_OK,
                                                        static_cast<IMFMediaStream2*>(_stream.Get())));
            RETURN_IF_FAILED(_stream->Start(type.Get()));
        } else if (wasSelected) {
            _pd->DeselectStream(0);
            _stream->Stop(false);
        }
        PROPVARIANT t;
        PropVariantInit(&t);
        t.vt = VT_I8;
        t.hVal.QuadPart = MFGetSystemTime();
        RETURN_IF_FAILED(_queue->QueueEventParamVar(MESourceStarted, GUID_NULL, S_OK, &t));
        return S_OK;
    }
    STDMETHODIMP Stop() override {
        std::lock_guard<std::mutex> g(_lock);
        if (!_queue) return MF_E_SHUTDOWN;
        Log(L"source: Stop");
        _stream->Stop(true);
        _pd->DeselectStream(0);
        PROPVARIANT t;
        PropVariantInit(&t);
        t.vt = VT_I8;
        t.hVal.QuadPart = MFGetSystemTime();
        return _queue->QueueEventParamVar(MESourceStopped, GUID_NULL, S_OK, &t);
    }
    STDMETHODIMP Shutdown() override {
        ComPtr<MediaStream> stream;
        {
            std::lock_guard<std::mutex> g(_lock);
            if (!_queue) return MF_E_SHUTDOWN;
            Log(L"source: Shutdown");
            _queue->Shutdown();
            _queue.Reset();
            _pd.Reset();
            stream = _stream;
            _stream.Reset();
        }
        if (stream) stream->Shutdown();
        return S_OK;
    }

    // IMFMediaSourceEx
    STDMETHODIMP GetSourceAttributes(IMFAttributes** a) override {
        if (!a) return E_POINTER;
        return _attrs.CopyTo(a);
    }
    STDMETHODIMP GetStreamAttributes(DWORD id, IMFAttributes** a) override {
        std::lock_guard<std::mutex> g(_lock);
        if (!a) return E_POINTER;
        if (id != 0) return MF_E_INVALIDSTREAMNUMBER;
        if (!_stream) return MF_E_SHUTDOWN;
        IMFAttributes* sa = _stream->Attributes();
        sa->AddRef();
        *a = sa;
        return S_OK;
    }
    STDMETHODIMP SetD3DManager(IUnknown* m) override {
        // Keep CPU samples: the frames come from system memory anyway.
        Log(L"source: SetD3DManager(%p) ignored", m);
        return E_NOTIMPL;
    }

    // IMFGetService
    STDMETHODIMP GetService(REFGUID, REFIID, LPVOID* ppv) override {
        if (ppv) *ppv = nullptr;
        return MF_E_UNSUPPORTED_SERVICE;
    }

    // IKsControl
    STDMETHODIMP KsProperty(PKSPROPERTY, ULONG, LPVOID, ULONG, ULONG*) override {
        return HRESULT_FROM_WIN32(ERROR_SET_NOT_FOUND);
    }
    STDMETHODIMP KsMethod(PKSMETHOD, ULONG, LPVOID, ULONG, ULONG*) override {
        return HRESULT_FROM_WIN32(ERROR_SET_NOT_FOUND);
    }
    STDMETHODIMP KsEvent(PKSEVENT, ULONG, LPVOID, ULONG, ULONG*) override {
        return HRESULT_FROM_WIN32(ERROR_SET_NOT_FOUND);
    }

    // IMFSampleAllocatorControl
    STDMETHODIMP SetDefaultAllocator(DWORD id, IUnknown* allocator) override {
        if (!allocator) return E_POINTER;
        if (id != 0) return MF_E_INVALIDSTREAMNUMBER;
        std::lock_guard<std::mutex> g(_lock);
        if (!_stream) return MF_E_SHUTDOWN;
        return _stream->SetAllocator(allocator);
    }
    STDMETHODIMP GetAllocatorUsage(DWORD id, DWORD* inputId, MFSampleAllocatorUsage* usage) override {
        if (!inputId || !usage) return E_POINTER;
        if (id != 0) return MF_E_INVALIDSTREAMNUMBER;
        *inputId = id;
        *usage = MFSampleAllocatorUsage_UsesProvidedAllocator;
        return S_OK;
    }

private:
    ComPtr<IMFMediaEventQueue> Queue() {
        std::lock_guard<std::mutex> g(_lock);
        return _queue;
    }

    std::atomic<ULONG> _ref{1};
    std::mutex _lock;
    ComPtr<IMFAttributes> _attrs;
    ComPtr<IMFMediaEventQueue> _queue;
    ComPtr<IMFPresentationDescriptor> _pd;
    ComPtr<MediaStream> _stream;
};

// ---------------------------------------------------------------------------------------
// Activator: the COM object Frame Server creates from our CLSID.
// ---------------------------------------------------------------------------------------
#define FWD(name, params, args) \
    STDMETHODIMP name params override { return _attrs->name args; }

class Activator final : public IMFActivate {
public:
    Activator() { g_objects++; }
    ~Activator() { g_objects--; }

    HRESULT Initialize() {
        RETURN_IF_FAILED(MFCreateAttributes(&_attrs, 4));
        _attrs->SetUINT32(MF_VIRTUALCAMERA_PROVIDE_ASSOCIATED_CAMERA_SOURCES, 1);
        _attrs->SetGUID(MFT_TRANSFORM_CLSID_Attribute, CLSID_SpiegelSpikeVCam);
        return S_OK;
    }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER;
        if (riid == __uuidof(IUnknown) || riid == __uuidof(IMFAttributes) || riid == __uuidof(IMFActivate)) {
            *ppv = static_cast<IMFActivate*>(this);
            AddRef();
            return S_OK;
        }
        *ppv = nullptr;
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return ++_ref; }
    STDMETHODIMP_(ULONG) Release() override {
        ULONG r = --_ref;
        if (!r) delete this;
        return r;
    }

    STDMETHODIMP ActivateObject(REFIID riid, void** ppv) override {
        Log(L"activator: ActivateObject %s", GuidStr(riid).c_str());
        std::lock_guard<std::mutex> g(_lock);
        if (!_source) {
            ComPtr<MediaSource> src;
            src.Attach(new MediaSource());
            RETURN_IF_FAILED(src->Initialize(this));
            _source = src;
        }
        return _source->QueryInterface(riid, ppv);
    }
    STDMETHODIMP ShutdownObject() override {
        Log(L"activator: ShutdownObject");
        std::lock_guard<std::mutex> g(_lock);
        if (_source) _source->Shutdown();
        _source.Reset();
        return S_OK;
    }
    STDMETHODIMP DetachObject() override {
        Log(L"activator: DetachObject");
        std::lock_guard<std::mutex> g(_lock);
        _source.Reset();
        return S_OK;
    }

    // IMFAttributes, forwarded to an inner store.
    FWD(GetItem, (REFGUID k, PROPVARIANT* v), (k, v))
    FWD(GetItemType, (REFGUID k, MF_ATTRIBUTE_TYPE* t), (k, t))
    FWD(CompareItem, (REFGUID k, REFPROPVARIANT v, BOOL* r), (k, v, r))
    FWD(Compare, (IMFAttributes* a, MF_ATTRIBUTES_MATCH_TYPE m, BOOL* r), (a, m, r))
    FWD(GetUINT32, (REFGUID k, UINT32* v), (k, v))
    FWD(GetUINT64, (REFGUID k, UINT64* v), (k, v))
    FWD(GetDouble, (REFGUID k, double* v), (k, v))
    FWD(GetGUID, (REFGUID k, GUID* v), (k, v))
    FWD(GetStringLength, (REFGUID k, UINT32* v), (k, v))
    FWD(GetString, (REFGUID k, LPWSTR s, UINT32 n, UINT32* l), (k, s, n, l))
    FWD(GetAllocatedString, (REFGUID k, LPWSTR* s, UINT32* l), (k, s, l))
    FWD(GetBlobSize, (REFGUID k, UINT32* v), (k, v))
    FWD(GetBlob, (REFGUID k, UINT8* b, UINT32 n, UINT32* l), (k, b, n, l))
    FWD(GetAllocatedBlob, (REFGUID k, UINT8** b, UINT32* l), (k, b, l))
    FWD(GetUnknown, (REFGUID k, REFIID i, LPVOID* p), (k, i, p))
    FWD(SetItem, (REFGUID k, REFPROPVARIANT v), (k, v))
    FWD(DeleteItem, (REFGUID k), (k))
    FWD(DeleteAllItems, (), ())
    FWD(SetUINT32, (REFGUID k, UINT32 v), (k, v))
    FWD(SetUINT64, (REFGUID k, UINT64 v), (k, v))
    FWD(SetDouble, (REFGUID k, double v), (k, v))
    FWD(SetGUID, (REFGUID k, REFGUID v), (k, v))
    FWD(SetString, (REFGUID k, LPCWSTR v), (k, v))
    FWD(SetBlob, (REFGUID k, const UINT8* b, UINT32 n), (k, b, n))
    FWD(SetUnknown, (REFGUID k, IUnknown* u), (k, u))
    FWD(LockStore, (), ())
    FWD(UnlockStore, (), ())
    FWD(GetCount, (UINT32* n), (n))
    FWD(GetItemByIndex, (UINT32 i, GUID* k, PROPVARIANT* v), (i, k, v))
    FWD(CopyAllItems, (IMFAttributes* d), (d))

private:
    std::atomic<ULONG> _ref{1};
    std::mutex _lock;
    ComPtr<IMFAttributes> _attrs;
    ComPtr<MediaSource> _source;
};

// ---------------------------------------------------------------------------------------
// COM plumbing
// ---------------------------------------------------------------------------------------
class ClassFactory final : public IClassFactory {
public:
    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER;
        if (riid == __uuidof(IUnknown) || riid == __uuidof(IClassFactory)) {
            *ppv = static_cast<IClassFactory*>(this);
            AddRef();
            return S_OK;
        }
        *ppv = nullptr;
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return 2; }
    STDMETHODIMP_(ULONG) Release() override { return 1; }
    STDMETHODIMP CreateInstance(IUnknown* outer, REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (outer) return CLASS_E_NOAGGREGATION;
        ComPtr<Activator> a;
        a.Attach(new Activator());
        RETURN_IF_FAILED(a->Initialize());
        HRESULT hr = a->QueryInterface(riid, ppv);
        Log(L"factory: CreateInstance %s hr=0x%08X", GuidStr(riid).c_str(), hr);
        return hr;
    }
    STDMETHODIMP LockServer(BOOL lock) override {
        lock ? g_objects++ : g_objects--;
        return S_OK;
    }
};
static ClassFactory g_factory;

BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) {
        g_module = module;
        DisableThreadLibraryCalls(module);
    }
    return TRUE;
}

STDAPI DllGetClassObject(REFCLSID clsid, REFIID riid, void** ppv) {
    if (!ppv) return E_POINTER;
    *ppv = nullptr;
    if (clsid != CLSID_SpiegelSpikeVCam) return CLASS_E_CLASSNOTAVAILABLE;
    static bool once = [] {
        wchar_t exe[MAX_PATH];
        GetModuleFileNameW(nullptr, exe, MAX_PATH);
        Log(L"DllGetClassObject: first load in %s (pid %lu)", exe, GetCurrentProcessId());
        return true;
    }();
    (void)once;
    return g_factory.QueryInterface(riid, ppv);
}

STDAPI DllCanUnloadNow() { return g_objects == 0 ? S_OK : S_FALSE; }

STDAPI DllRegisterServer() {
    wchar_t path[MAX_PATH];
    GetModuleFileNameW(g_module, path, MAX_PATH);
    std::wstring key = L"Software\\Classes\\CLSID\\" SPIKE_VCAM_CLSID_STR;
    HKEY k;
    LSTATUS s = RegCreateKeyExW(HKEY_LOCAL_MACHINE, key.c_str(), 0, nullptr, 0, KEY_WRITE, nullptr, &k, nullptr);
    if (s != ERROR_SUCCESS) return HRESULT_FROM_WIN32(s);
    RegSetValueExW(k, nullptr, 0, REG_SZ, (const BYTE*)SPIKE_VCAM_NAME, sizeof(SPIKE_VCAM_NAME));
    RegCloseKey(k);
    s = RegCreateKeyExW(HKEY_LOCAL_MACHINE, (key + L"\\InprocServer32").c_str(), 0, nullptr, 0, KEY_WRITE, nullptr, &k,
                        nullptr);
    if (s != ERROR_SUCCESS) return HRESULT_FROM_WIN32(s);
    RegSetValueExW(k, nullptr, 0, REG_SZ, (const BYTE*)path, (DWORD)((wcslen(path) + 1) * sizeof(wchar_t)));
    RegSetValueExW(k, L"ThreadingModel", 0, REG_SZ, (const BYTE*)L"Both", sizeof(L"Both"));
    RegCloseKey(k);
    return S_OK;
}

STDAPI DllUnregisterServer() {
    LSTATUS s = RegDeleteTreeW(HKEY_LOCAL_MACHINE, L"Software\\Classes\\CLSID\\" SPIKE_VCAM_CLSID_STR);
    return s == ERROR_SUCCESS || s == ERROR_FILE_NOT_FOUND ? S_OK : HRESULT_FROM_WIN32(s);
}
