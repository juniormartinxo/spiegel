// SPIKE — throwaway. Control and test tool for the spike virtual camera.
//
//   vcamctl add    [--lifetime session|system] [--access user|all]   create + start the camera
//   vcamctl remove [--lifetime session|system] [--access user|all]   remove it
//   vcamctl list                                                      MF video capture devices
//   vcamctl dshow                                                     DirectShow devices + formats
//   vcamctl probe  [--format nv12|yuy2] [--seconds N]                 read frames, measure latency
#include <windows.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mferror.h>
#include <mfreadwrite.h>
#include <mfvirtualcamera.h>
#include <dshow.h>
#include <wrl/client.h>
#include <algorithm>
#include <string>
#include <vector>
#include <stdio.h>

#include "../common/shared_frame.h"

using Microsoft::WRL::ComPtr;

static int Fail(const char* what, HRESULT hr) {
    printf("%s falhou: hr=0x%08X\n", what, (unsigned)hr);
    return 1;
}

static bool Arg(int argc, char** argv, const char* name, const char** value) {
    for (int i = 2; i < argc - 1; i++)
        if (!strcmp(argv[i], name)) { *value = argv[i + 1]; return true; }
    return false;
}

static std::string Fourcc(const GUID& g) {
    char s[5] = {(char)(g.Data1 & 0xff), (char)((g.Data1 >> 8) & 0xff), (char)((g.Data1 >> 16) & 0xff),
                 (char)(g.Data1 >> 24), 0};
    return s;
}

static int CmdAddRemove(int argc, char** argv, bool add) {
    const char* lifetime = "session";
    const char* access = "user";
    Arg(argc, argv, "--lifetime", &lifetime);
    Arg(argc, argv, "--access", &access);
    auto lt = !strcmp(lifetime, "system") ? MFVirtualCameraLifetime_System : MFVirtualCameraLifetime_Session;
    auto ac = !strcmp(access, "all") ? MFVirtualCameraAccess_AllUsers : MFVirtualCameraAccess_CurrentUser;

    BOOL supported = FALSE;
    HRESULT hr = MFIsVirtualCameraTypeSupported(MFVirtualCameraType_SoftwareCameraSource, &supported);
    printf("MFIsVirtualCameraTypeSupported: hr=0x%08X supported=%d\n", (unsigned)hr, supported);

    ComPtr<IMFVirtualCamera> vcam;
    hr = MFCreateVirtualCamera(MFVirtualCameraType_SoftwareCameraSource, lt, ac, SPIKE_VCAM_NAME,
                               SPIKE_VCAM_CLSID_STR, nullptr, 0, &vcam);
    printf("MFCreateVirtualCamera(lifetime=%s, access=%s): hr=0x%08X\n", lifetime, access, (unsigned)hr);
    if (FAILED(hr)) return 1;

    if (!add) {
        hr = vcam->Remove();
        printf("Remove: hr=0x%08X\n", (unsigned)hr);
        return FAILED(hr);
    }

    hr = vcam->Start(nullptr);
    printf("Start: hr=0x%08X%s\n", (unsigned)hr,
           hr == E_ACCESSDENIED ? "  (Frame Server nao consegue ler a DLL? veja README)" : "");
    if (FAILED(hr)) return 1;

    if (lt == MFVirtualCameraLifetime_Session) {
        printf("Camera '%ls' ativa enquanto este processo viver. Enter para remover.\n", SPIKE_VCAM_NAME);
        getchar();
        hr = vcam->Remove();
        printf("Remove: hr=0x%08X\n", (unsigned)hr);
    } else {
        printf("Camera '%ls' registrada com lifetime=system; persiste depois que este processo sair.\n",
               SPIKE_VCAM_NAME);
    }
    return 0;
}

static const wchar_t* g_camName = L"Spiegel";
static wchar_t g_camNameBuf[128];

static HRESULT FindCamera(IMFActivate** out, bool print) {
    ComPtr<IMFAttributes> attrs;
    MFCreateAttributes(&attrs, 1);
    attrs->SetGUID(MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID);
    IMFActivate** devices = nullptr;
    UINT32 count = 0;
    HRESULT hr = MFEnumDeviceSources(attrs.Get(), &devices, &count);
    if (FAILED(hr)) return hr;
    hr = MF_E_NOT_FOUND;
    for (UINT32 i = 0; i < count; i++) {
        wchar_t* name = nullptr;
        wchar_t* link = nullptr;
        UINT32 len;
        devices[i]->GetAllocatedString(MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, &name, &len);
        devices[i]->GetAllocatedString(MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, &link, &len);
        if (print) printf("  [%u] %ls\n      %ls\n", i, name ? name : L"?", link ? link : L"?");
        if (out && !*out && name && wcsstr(name, g_camName)) {
            *out = devices[i];
            devices[i]->AddRef();
            hr = S_OK;
        }
        CoTaskMemFree(name);
        CoTaskMemFree(link);
        devices[i]->Release();
    }
    CoTaskMemFree(devices);
    if (print) printf("%u dispositivo(s) de captura de video no Media Foundation.\n", count);
    return hr;
}

static int CmdList() {
    FindCamera(nullptr, true);
    return 0;
}

static int CmdDshow() {
    ComPtr<ICreateDevEnum> devEnum;
    HRESULT hr = CoCreateInstance(CLSID_SystemDeviceEnum, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&devEnum));
    if (FAILED(hr)) return Fail("CLSID_SystemDeviceEnum", hr);
    ComPtr<IEnumMoniker> monikers;
    hr = devEnum->CreateClassEnumerator(CLSID_VideoInputDeviceCategory, &monikers, 0);
    if (hr != S_OK) {
        printf("Nenhum dispositivo DirectShow de entrada de video.\n");
        return 0;
    }
    ComPtr<IMoniker> m;
    while (monikers->Next(1, &m, nullptr) == S_OK) {
        ComPtr<IPropertyBag> bag;
        m->BindToStorage(nullptr, nullptr, IID_PPV_ARGS(&bag));
        VARIANT v;
        VariantInit(&v);
        if (bag && SUCCEEDED(bag->Read(L"FriendlyName", &v, nullptr))) printf("* %ls\n", v.bstrVal);
        VariantClear(&v);

        ComPtr<IBaseFilter> filter;
        hr = m->BindToObject(nullptr, nullptr, IID_PPV_ARGS(&filter));
        if (FAILED(hr)) {
            printf("    BindToObject hr=0x%08X\n", (unsigned)hr);
            m.Reset();
            continue;
        }
        ComPtr<IEnumPins> pins;
        filter->EnumPins(&pins);
        ComPtr<IPin> pin;
        while (pins && pins->Next(1, &pin, nullptr) == S_OK) {
            ComPtr<IAMStreamConfig> cfg;
            if (SUCCEEDED(pin.As(&cfg))) {
                int n = 0, size = 0;
                cfg->GetNumberOfCapabilities(&n, &size);
                std::vector<BYTE> caps(size);
                for (int i = 0; i < n; i++) {
                    AM_MEDIA_TYPE* mt = nullptr;
                    if (SUCCEEDED(cfg->GetStreamCaps(i, &mt, caps.data())) && mt) {
                        if (mt->formattype == FORMAT_VideoInfo && mt->pbFormat) {
                            auto* vih = (VIDEOINFOHEADER*)mt->pbFormat;
                            printf("    %s %ldx%ld @ %.2f fps\n", Fourcc(mt->subtype).c_str(), vih->bmiHeader.biWidth,
                                   vih->bmiHeader.biHeight,
                                   vih->AvgTimePerFrame ? 1e7 / vih->AvgTimePerFrame : 0.0);
                        } else {
                            printf("    %s (formato nao-VideoInfo)\n", Fourcc(mt->subtype).c_str());
                        }
                        if (mt->pbFormat) CoTaskMemFree(mt->pbFormat);
                        if (mt->pUnk) mt->pUnk->Release();
                        CoTaskMemFree(mt);
                    }
                }
            }
            pin.Reset();
        }
        m.Reset();
    }
    return 0;
}

static int CmdProbe(int argc, char** argv) {
    const char* fmt = "nv12";
    const char* secs = "10";
    Arg(argc, argv, "--format", &fmt);
    Arg(argc, argv, "--seconds", &secs);
    GUID want = !_stricmp(fmt, "yuy2") ? MFVideoFormat_YUY2 : MFVideoFormat_NV12;
    const char* camName;
    if (Arg(argc, argv, "--name", &camName)) {
        MultiByteToWideChar(CP_UTF8, 0, camName, -1, g_camNameBuf, 128);
        g_camName = g_camNameBuf;
    }
    double seconds = atof(secs);

    ComPtr<IMFActivate> act;
    HRESULT hr = FindCamera(&act, false);
    if (FAILED(hr)) return Fail("Encontrar a camera 'Spiegel' no Media Foundation", hr);
    ComPtr<IMFMediaSource> source;
    LONG64 t0 = SpikeQpc();
    hr = act->ActivateObject(IID_PPV_ARGS(&source));
    if (FAILED(hr)) return Fail("ActivateObject", hr);
    printf("ActivateObject: %.1f ms\n", SpikeQpcToMs(SpikeQpc() - t0));

    ComPtr<IMFSourceReader> reader;
    hr = MFCreateSourceReaderFromMediaSource(source.Get(), nullptr, &reader);
    if (FAILED(hr)) return Fail("MFCreateSourceReaderFromMediaSource", hr);

    ComPtr<IMFMediaType> chosen;
    for (DWORD i = 0;; i++) {
        ComPtr<IMFMediaType> t;
        if (FAILED(reader->GetNativeMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM, i, &t))) break;
        GUID sub;
        UINT32 w, h, num, den;
        t->GetGUID(MF_MT_SUBTYPE, &sub);
        MFGetAttributeSize(t.Get(), MF_MT_FRAME_SIZE, &w, &h);
        MFGetAttributeRatio(t.Get(), MF_MT_FRAME_RATE, &num, &den);
        printf("  tipo nativo %lu: %s %ux%u @ %u/%u\n", i, Fourcc(sub).c_str(), w, h, num, den);
        if (sub == want && !chosen) chosen = t;
    }
    if (!chosen) return Fail("Achar o formato pedido", MF_E_INVALIDMEDIATYPE);
    const char* dummy;
    if (Arg(argc, argv, "--no-set", &dummy) || (argc > 2 && !strcmp(argv[argc - 1], "--no-set"))) {
        // Second-client mode: don't ask for control, read whatever type is current.
        ComPtr<IMFMediaType> cur;
        reader->GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM, &cur);
        cur->GetGUID(MF_MT_SUBTYPE, &want);
        printf("--no-set: lendo no tipo atual %s\n", Fourcc(want).c_str());
    } else {
        hr = reader->SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM, nullptr, chosen.Get());
        if (FAILED(hr)) return Fail("SetCurrentMediaType", hr);
    }

    bool yuy2 = want == MFVideoFormat_YUY2;
    std::vector<double> lat;
    int frames = 0, standby = 0, dup = 0, gaps = 0;
    uint32_t lastFrame = 0xffffffff;
    LONG64 start = SpikeQpc(), firstQpc = 0;
    while (SpikeQpcToMs(SpikeQpc() - start) < seconds * 1000) {
        DWORD stream, flags;
        LONGLONG ts;
        ComPtr<IMFSample> sample;
        hr = reader->ReadSample(MF_SOURCE_READER_FIRST_VIDEO_STREAM, 0, &stream, &flags, &ts, &sample);
        LONG64 now = SpikeQpc();
        if (FAILED(hr)) return Fail("ReadSample", hr);
        if (!sample) continue;
        if (!firstQpc) {
            firstQpc = now;
            printf("Primeiro quadro apos %.1f ms\n", SpikeQpcToMs(now - start));
        }
        frames++;
        ComPtr<IMFMediaBuffer> buf;
        sample->ConvertToContiguousBuffer(&buf);
        BYTE* p;
        DWORD len;
        buf->Lock(&p, nullptr, &len);
        uint64_t qpc;
        uint32_t fn;
        if (SpikeReadStamp(p, yuy2 ? SPIKE_W * 2 : SPIKE_W, yuy2 ? 2 : 1, &qpc, &fn)) {
            lat.push_back(SpikeQpcToMs(now - (LONG64)qpc));
            if (fn == lastFrame) dup++;
            else if (lastFrame != 0xffffffff && fn != lastFrame + 1) gaps++;
            lastFrame = fn;
        } else {
            standby++;
        }
        buf->Unlock();
    }
    double elapsed = SpikeQpcToMs(SpikeQpc() - (firstQpc ? firstQpc : start)) / 1000.0;
    printf("\nFormato %s: %d quadros em %.1f s = %.1f fps | espera (sem carimbo): %d | repetidos: %d | saltos: %d\n",
           yuy2 ? "YUY2" : "NV12", frames, elapsed, frames / elapsed, standby, dup, gaps);
    if (!lat.empty()) {
        std::sort(lat.begin(), lat.end());
        double sum = 0;
        for (double v : lat) sum += v;
        printf("Latencia alimentador -> app (ReadSample): min %.1f | media %.1f | p50 %.1f | p95 %.1f | max %.1f ms\n",
               lat.front(), sum / lat.size(), lat[lat.size() / 2], lat[lat.size() * 95 / 100], lat.back());
    }
    source->Shutdown();
    return 0;
}

int main(int argc, char** argv) {
    if (argc < 2) {
        printf("uso: vcamctl add|remove|list|dshow|probe [opcoes]\n");
        return 2;
    }
    setvbuf(stdout, nullptr, _IONBF, 0);
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    MFStartup(MF_VERSION);
    int rc = 2;
    if (!strcmp(argv[1], "add")) rc = CmdAddRemove(argc, argv, true);
    else if (!strcmp(argv[1], "remove")) rc = CmdAddRemove(argc, argv, false);
    else if (!strcmp(argv[1], "list")) rc = CmdList();
    else if (!strcmp(argv[1], "dshow")) rc = CmdDshow();
    else if (!strcmp(argv[1], "probe")) rc = CmdProbe(argc, argv);
    else printf("comando desconhecido: %s\n", argv[1]);
    MFShutdown();
    CoUninitialize();
    return rc;
}
